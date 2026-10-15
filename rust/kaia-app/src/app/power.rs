//! Suspend timers and frontlight ramp steps run on the app thread. The device
//! plans (Wi-Fi scripts, suspend, resume) run on the worker in `network.rs`, and
//! the sleep picture is prepared in `sleep_picture.rs`.

use kaia_i18n::tr;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use kaia_events::{Ctx, Event, Refresh, RefreshMode, ShowOpts, TaskId, WindowId};
use kaia_gfx::Pixmap;
use kaia_platform::frontlight::RampStep;
use kaia_platform::plan::Outcome;
use kaia_platform::power::{PowerContext, ResumeContext};
use kaia_platform::suspend::{Effect, PrepareResult, SuspendFlow, Timer, WakeupCause, SUSPEND_WAIT_TIMEOUT};
use kaia_platform::sysfs::SystemIo;
use kaia_ui::Window;

use super::sleep_picture::{self, SleepPicture};
use super::App;
use crate::backend::Backend;
use crate::surfaces::{settings, Route};

/// Jobs on the plan worker. Prepare jobs count up from 1, so these never collide with them.
const WIFI_JOB: u64 = 0;
const FINISH_JOB: u64 = u64::MAX;
const RESUME_JOB: u64 = u64::MAX - 1;
const POWER_OFF_JOB: u64 = u64::MAX - 2;

/// How often the loop polls a worker while one of its jobs runs.
const POLL: Duration = Duration::from_millis(50);

pub(super) struct State {
    flow: SuspendFlow,
    pub(super) scripts: super::network::Scripts,
    /// Wi-Fi jobs in flight. Suspend turns Wi-Fi off when it is on or a job is turning it on.
    wifi_jobs: usize,
    /// Wi-Fi jobs submitted since start, as (turn on, turn off). Tests read it.
    wifi_requests: (u64, u64),
    prepare_generation: u64,
    /// The finish plan is running: `sync`, then the suspend write. Power and Resume wait for it.
    finishing: bool,
    /// A Power press or Resume came while `finishing`. The resume runs when the finish plan returns.
    resume_queued: bool,
    last_activity: Duration,
    suspended_time: Duration,
    suspend_started: Option<(Duration, std::time::SystemTime)>,
    power_press: Option<(Duration, u64, bool)>,
    off_requested: bool,
    off_started: bool,
    off_screen: Option<WindowId>,
    battery_due: Duration,
    battery_warned: bool,
    charge_screen: bool,
    timers: Vec<(Timer, TaskId)>,
    queue: Rc<RefCell<VecDeque<Due>>>,
    screen: Option<WindowId>,
    /// The book that the sleep screen belongs to.
    sleep_book: Option<i64>,
    /// Whether the sleep screen shows its final picture. It is false while the cover is still being prepared.
    sleep_covered: bool,
    picture: SleepPicture,
    restore_light: bool,
    ramp_generation: u64,
}

enum Due {
    Timer(Timer),
    Light(u64, RampStep),
}

impl State {
    /// `io` writes device files for the plan worker. It must share the platform's root.
    /// `library` is the library database that the sleep picture reads covers from.
    pub(super) fn new(io: SystemIo, library: PathBuf) -> Self {
        Self {
            flow: SuspendFlow::default(),
            scripts: super::network::Scripts::new(io),
            wifi_jobs: 0,
            wifi_requests: (0, 0),
            prepare_generation: 0,
            finishing: false,
            resume_queued: false,
            last_activity: Duration::ZERO,
            suspended_time: Duration::ZERO,
            suspend_started: None,
            power_press: None,
            off_requested: false,
            off_started: false,
            off_screen: None,
            battery_due: Duration::ZERO,
            battery_warned: false,
            charge_screen: false,
            timers: Vec::new(),
            queue: Rc::default(),
            screen: None,
            sleep_book: None,
            sleep_covered: true,
            picture: SleepPicture::new(library),
            restore_light: false,
            ramp_generation: 0,
        }
    }
}

impl<B: Backend> App<B> {
    pub fn sleeping(&self) -> bool {
        self.power.screen.is_some()
    }

    /// Whether the sleep picture of the book being read is ready. Tests wait for it.
    pub fn sleep_picture_ready(&self) -> bool {
        self.reader_book()
            .is_some_and(|book| self.power.picture.ready(book, self.screen).is_some())
    }

    /// Starts the sleep picture of `book` on the worker, so that Power can draw it at once.
    pub(super) fn prepare_sleep_picture(&mut self, book: i64) {
        if settings::sleep::shows_cover(&self.services) {
            self.power.picture.request(book, self.screen);
        }
    }

    pub(super) fn sleep(&mut self) {
        if self.sleeping() {
            return;
        }
        self.broadcast_app(Event::new("Suspend"));
        if let Err(e) = self.services.settings.flush(&self.services.settings_path()) {
            self.notices.push(format!("Cannot save settings: {e}"));
        }
        let book = self.reader_book();
        // None: the picture is not ready yet, so the screen shows the label on a blank page.
        // Some(None): the screen has no cover, because the book has none or the setting is blank.
        let picture = match book {
            Some(book) if settings::sleep::shows_cover(&self.services) => {
                self.power.picture.request(book, self.screen);
                self.power
                    .picture
                    .ready(book, self.screen)
                    .map(|picture| picture.cloned())
            }
            _ => Some(None),
        };
        self.power.sleep_book = book;
        self.power.sleep_covered = picture.is_some();
        let id = self.show_sleep_screen(picture.flatten());
        self.power.screen = Some(id);
        self.power.restore_light = self.services.platform.frontlight.target_is_on();
        if self.power.restore_light {
            self.toggle_light();
        }
        let effects = self.power.flow.reschedule_suspend(SUSPEND_WAIT_TIMEOUT);
        self.power_effects(effects);
    }

    /// Shows the PIN pad when a PIN is set and the pad is not already on top. The pad
    /// covers the screens below it. Navigation that was waiting for the reader is dropped.
    pub(super) fn lock_screen(&mut self) {
        if !crate::surfaces::pin::store::is_on(&self.services.paths.data_dir) {
            return;
        }
        if self.top_route().is_some_and(crate::surfaces::pin::is_lock_route) {
            return;
        }
        self.pending_navigation = None;
        let dir = self.services.paths.data_dir.clone();
        self.push(Box::new(crate::surfaces::pin::LockSurface::new(&dir)));
    }

    pub(super) fn resume(&mut self) {
        if self.power.finishing {
            // KOReader handles a Power press after the suspend write returns. This one waits too.
            self.power.resume_queued = true;
            return;
        }
        self.power.resume_queued = false;
        self.note_activity();
        self.power.charge_screen = false;
        self.power.prepare_generation += 1;
        let effects = self.power.flow.resume();
        self.power_effects(effects);
        let Some(id) = self.power.screen.take() else {
            return;
        };
        self.stack
            .close(id, Some(Refresh::new(RefreshMode::Full)), &mut self.ctx);
        self.lock_screen();
        let plan = self
            .services
            .platform
            .power
            .clear_wakeup()
            .then(self.services.platform.power.resume(ResumeContext {
                sync_system_clock: true,
                ..ResumeContext::default()
            }));
        self.power.scripts.submit(RESUME_JOB, plan);
        if self.power.restore_light && !self.services.platform.frontlight.target_is_on() {
            self.toggle_light();
        }
        self.broadcast_app(Event::new("Resume"));
    }

    pub(super) fn toggle_light(&mut self) {
        self.power.ramp_generation += 1;
        let generation = self.power.ramp_generation;
        let Some(ramp) = self.services.platform.frontlight.begin_toggle() else {
            return;
        };
        // Keep the platform's brightness steps and use Nickel's full fade durations.
        let duration = match ramp.direction {
            kaia_platform::frontlight::RampDirection::On => Duration::from_millis(1500),
            kaia_platform::frontlight::RampDirection::Off => Duration::from_millis(750),
        };
        let count = ramp.steps.len() as u32;
        for (index, step) in ramp.steps.into_iter().enumerate() {
            let delay = duration.mul_f64((index + 1) as f64 / f64::from(count));
            let queue = self.power.queue.clone();
            self.ctx.scheduler.schedule_in(
                delay.as_secs_f64(),
                Box::new(move |_: &mut Ctx| {
                    queue.borrow_mut().push_back(Due::Light(generation, step));
                }),
            );
        }
    }

    pub(super) fn wifi(&mut self, on: bool) {
        use kaia_platform::plan::{Action, OnError, Plan};
        let mut plan = self.services.platform.power.prepare_suspend(PowerContext {
            wifi_on: true,
            charging: false,
        });
        plan.steps.retain(|s| matches!(s.action, Action::Run { .. }));
        if on {
            plan.steps.truncate(1);
            if let Some(step) = plan.steps.first_mut() {
                if let Action::Run { program, .. } = &mut step.action {
                    *program = self
                        .services
                        .platform
                        .paths
                        .install_dir
                        .join("enable-wifi.sh")
                        .to_string_lossy()
                        .into_owned();
                }
                step.on_error = OnError::Abort;
            }
        }
        self.power.wifi_jobs += 1;
        if on {
            self.power.wifi_requests.0 += 1;
        } else {
            self.power.wifi_requests.1 += 1;
        }
        self.power.scripts.submit(WIFI_JOB, Plan::new(plan.steps));
    }

    /// Wi-Fi jobs submitted since start, as (turn on, turn off). The jobs run the
    /// enable and disable scripts on the script worker.
    pub fn wifi_requests(&self) -> (u64, u64) {
        self.power.wifi_requests
    }

    pub(super) fn note_activity(&mut self) {
        self.power.last_activity = self.idle_now();
    }

    fn idle_now(&self) -> Duration {
        self.ctx.scheduler.now() + self.power.suspended_time
    }

    pub(super) fn power_wait(&self) -> Duration {
        let wait = self.idle_wait();
        if self.power.picture.busy() {
            wait.min(POLL)
        } else {
            wait
        }
    }

    /// How long the idle timers allow the loop to wait.
    fn idle_wait(&self) -> Duration {
        let now = self.ctx.scheduler.now();
        let battery = self.power.battery_due.saturating_sub(now);
        let mut wait = battery;
        if let Some((pressed, _, _)) = self.power.power_press {
            wait = wait.min((pressed + kaia_platform::events::POWER_OFF_HOLD).saturating_sub(now));
        }
        #[cfg(feature = "lua")]
        if self.services.plugins.idle_paused() {
            return wait;
        }
        if self.usb_mass_storage || self.power.off_started {
            return wait;
        }
        for (key, default, enabled) in [
            (settings::keys::DEVICE_SLEEP_MINUTES, 15.0, !self.sleeping()),
            (settings::keys::DEVICE_POWER_OFF_MINUTES, 0.0, true),
        ] {
            if enabled {
                if let Some(timeout) = self.idle_timeout(key, default) {
                    wait = wait.min((self.power.last_activity + timeout).saturating_sub(self.idle_now()));
                }
            }
        }
        wait
    }

    fn idle_timeout(&self, key: &str, default: f64) -> Option<Duration> {
        let minutes = settings::keys::number_or(&self.services.settings, key, default);
        (minutes.is_finite() && minutes > 0.0).then(|| Duration::from_secs_f64((minutes * 60.0).min(86400.0)))
    }

    fn check_idle_power(&mut self) {
        let now = self.ctx.scheduler.now();
        if now >= self.power.battery_due {
            self.power.battery_due = now + Duration::from_secs(60);
            let level = self.services.platform.battery.capacity();
            let charging = self.services.platform.battery.is_charging().unwrap_or(false);
            if charging || level.is_some_and(|n| n > 10) {
                self.power.battery_warned = false;
            } else if level.is_some_and(|n| n <= 3) && !self.sleeping() {
                self.power.charge_screen = true;
                self.sleep();
            } else if level.is_some_and(|n| n <= 10) && !self.power.battery_warned && !self.sleeping() {
                self.power.battery_warned = true;
                crate::surface::SurfaceEnv::new(
                    &mut self.services,
                    &mut self.ctx,
                    self.screen,
                    &mut self.notices,
                    &mut self.transient,
                )
                .toast(tr("Battery low. Please charge your reader."), 5.0);
            }
        }
        if let Some((pressed, _, _)) = self.power.power_press {
            if now.saturating_sub(pressed) >= kaia_platform::events::POWER_OFF_HOLD {
                self.power.power_press = None;
                self.request_power_off();
            }
        }
        #[cfg(not(feature = "lua"))]
        let paused = false;
        #[cfg(feature = "lua")]
        let paused = self.services.plugins.idle_paused();
        if paused || self.usb_mass_storage || self.power.off_requested || self.power.off_started {
            return;
        }
        let idle = self.idle_now().saturating_sub(self.power.last_activity);
        if self
            .idle_timeout(settings::keys::DEVICE_POWER_OFF_MINUTES, 0.0)
            .is_some_and(|timeout| idle >= timeout)
        {
            self.request_power_off();
        } else if !self.sleeping()
            && self
                .idle_timeout(settings::keys::DEVICE_SLEEP_MINUTES, 15.0)
                .is_some_and(|timeout| idle >= timeout)
        {
            self.sleep();
        }
    }

    pub(super) fn power_key(&mut self, event: kaia_input::KeyEvent) {
        if self.power.off_requested || self.power.off_started {
            return;
        }
        if event.pressed {
            if self.power.power_press.is_some() {
                return;
            }
            let woke = self.sleeping();
            self.power.power_press = Some((self.ctx.scheduler.now(), event.time_us, woke));
            if woke {
                self.resume();
            }
        } else if let Some((pressed, time_us, woke)) = self.power.power_press.take() {
            let held = self
                .ctx
                .scheduler
                .now()
                .saturating_sub(pressed)
                .max(Duration::from_micros(event.time_us.saturating_sub(time_us)));
            if held >= kaia_platform::events::POWER_OFF_HOLD {
                self.request_power_off();
            } else if !woke {
                self.sleep();
            }
        }
    }

    fn request_power_off(&mut self) {
        if self.power.off_requested || self.power.off_started {
            return;
        }
        self.power.off_requested = true;
        self.power.prepare_generation += 1;
        self.power.resume_queued = false;
        self.quitting = true;
        let effects = self.power.flow.resume();
        self.power_effects(effects);
    }

    pub(super) fn power_off_requested(&self) -> bool {
        self.power.off_requested && !self.power.off_started
    }

    pub(super) fn cancel_power_off(&mut self) {
        self.power.off_requested = false;
        self.power.off_started = false;
        if let Some(id) = self.power.off_screen.take() {
            self.stack
                .close(id, Some(Refresh::new(RefreshMode::Full)), &mut self.ctx);
        }
        self.note_activity();
    }

    pub(super) fn finish_power_off(&mut self) {
        if self.power.finishing {
            self.quitting = false;
            return;
        }
        if let Err(error) = self.services.settings.flush(&self.services.settings_path()) {
            self.notices.push(kaia_i18n::format(
                &tr("Cannot save settings: %1"),
                &[&error.to_string()],
            ));
            self.cancel_power_off();
            self.quitting = false;
            return;
        }
        self.power.off_started = true;
        self.quitting = false;
        self.power.off_screen = Some(self.show_sleep_screen(None));
        self.stack.apply_requests(&mut self.ctx);
        self.router.register(&mut self.transient);
        if let Err(error) = self.repaint() {
            self.notices.push(kaia_i18n::format(
                &tr("Could not draw the power-off screen: %1"),
                &[&error.to_string()],
            ));
            self.cancel_power_off();
            return;
        }
        self.backend.wait_idle();
        self.power
            .scripts
            .submit(POWER_OFF_JOB, self.services.platform.power.power_off());
    }

    pub(super) fn power_due(&mut self) {
        if self.power.off_requested && !self.power.off_started && !self.power.finishing {
            self.quitting = true;
        }
        self.check_idle_power();
        self.sleep_picture_due();
        while let Some((job, outcome)) = self.power.scripts.poll() {
            self.plan_done(job, outcome);
        }
        loop {
            let next = self.power.queue.borrow_mut().pop_front();
            let Some(next) = next else {
                break;
            };
            match next {
                Due::Timer(timer) => {
                    if !self.power.timers.iter().any(|(t, _)| *t == timer) {
                        continue;
                    }
                    self.power.timers.retain(|(t, _)| *t != timer);
                    let effects = match timer {
                        Timer::Suspend => self.power.flow.suspend_timer_fired(),
                        Timer::FinishSuspend => self.power.flow.finish_timer_fired(),
                        Timer::UnexpectedWakeupCheck => {
                            self.power.flow.check_timer_fired(WakeupCause::Unexpected)
                        }
                    };
                    self.power_effects(effects);
                }
                Due::Light(generation, step)
                    if generation == self.power.ramp_generation
                        && self.services.platform.frontlight.is_ramping() =>
                {
                    if let Err(e) = self.services.platform.frontlight.apply_ramp_step(&step) {
                        self.services.platform.frontlight.cancel_ramp();
                        self.power.ramp_generation += 1;
                        self.notices.push(format!("Cannot change the light: {e}"));
                    }
                }
                _ => {}
            }
        }
    }

    /// Acts on a plan that the worker finished.
    fn plan_done(&mut self, job: u64, outcome: Outcome) {
        match job {
            POWER_OFF_JOB => {
                if matches!(outcome, Outcome::Completed | Outcome::Skipped(_)) {
                    self.quitting = true;
                } else {
                    self.cancel_power_off();
                    self.notices.push(kaia_i18n::format(
                        &tr("Could not power off: %1"),
                        &[&format!("{outcome:?}")],
                    ));
                }
            }
            WIFI_JOB => {
                self.power.wifi_jobs = self.power.wifi_jobs.saturating_sub(1);
                if !matches!(outcome, Outcome::Completed) {
                    self.notices.push(format!("Wi-Fi did not change: {outcome:?}"));
                }
            }
            FINISH_JOB => {
                if let Some((clock, wall)) = self.power.suspend_started.take() {
                    let elapsed = wall.elapsed().unwrap_or_default();
                    let awake = self.ctx.scheduler.now().saturating_sub(clock);
                    self.power.suspended_time += elapsed.saturating_sub(awake);
                }
                // The suspend write returned. The device woke up, or the kernel refused to suspend.
                self.power.finishing = false;
                if !self.power.off_requested {
                    let effects = self.power.flow.finish_done();
                    self.power_effects(effects);
                }
                if std::mem::take(&mut self.power.resume_queued) {
                    self.resume();
                }
            }
            RESUME_JOB => {}
            super::clock::CLOCK_JOB => self.clock_done(outcome),
            generation if generation == self.power.prepare_generation && self.sleeping() => {
                let effects = self
                    .power
                    .flow
                    .prepare_done(PrepareResult::from_outcome(&outcome));
                self.power_effects(effects);
            }
            _ => {}
        }
    }

    /// Swaps the blank sleep screen for the cover once the picture is ready.
    fn sleep_picture_due(&mut self) {
        self.power.picture.poll();
        if !self.sleeping() || self.power.sleep_covered {
            return;
        }
        let Some(book) = self.power.sleep_book else {
            return;
        };
        let Some(ready) = self.power.picture.ready(book, self.screen) else {
            return;
        };
        let picture = ready.cloned();
        self.power.sleep_covered = true;
        // A book without a cover keeps the blank sleep screen.
        let Some(picture) = picture else {
            return;
        };
        if let Some(old) = self.power.screen.take() {
            self.stack.close(old, None, &mut self.ctx);
        }
        let id = self.show_sleep_screen(Some(picture));
        self.power.screen = Some(id);
    }

    /// Builds the sleep screen and queues it as a full-screen modal window.
    fn show_sleep_screen(&mut self, picture: Option<Pixmap>) -> WindowId {
        let label = if self.power.off_started {
            tr("Kaia, powered off")
        } else if self.power.charge_screen {
            tr("Please charge your reader")
        } else {
            tr("Kaia, sleeping")
        };
        let stack = sleep_picture::sleep_screen(&self.services, self.screen, picture, &label);
        let window = Window::new(
            stack,
            self.services.theme.clone(),
            self.services.assets.clone(),
            self.screen,
        );
        let id = window.show(
            &mut self.ctx,
            ShowOpts {
                modal: true,
                covers_fullscreen: true,
                refresh: Some(Refresh::new(RefreshMode::Full)),
                ..ShowOpts::default()
            },
        );
        self.transient.push(window);
        id
    }

    /// The book of the reader screen, when one is open.
    fn reader_book(&self) -> Option<i64> {
        self.router.routes().iter().find_map(|r| match r {
            Route::Reader { book } => Some(*book),
            _ => None,
        })
    }

    fn power_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Cancel(timer) => {
                    for (_, task) in self.power.timers.iter().filter(|(t, _)| *t == timer) {
                        self.ctx.scheduler.unschedule(*task);
                    }
                    self.power.timers.retain(|(t, _)| *t != timer);
                }
                Effect::Schedule { timer, delay } => {
                    let queue = self.power.queue.clone();
                    let task = self.ctx.scheduler.schedule_in(
                        delay.as_secs_f64(),
                        Box::new(move |_: &mut Ctx| {
                            queue.borrow_mut().push_back(Due::Timer(timer));
                        }),
                    );
                    self.power.timers.push((timer, task));
                }
                Effect::PrepareSuspend => {
                    if self.reader_busy() {
                        let effects = self.power.flow.reschedule_suspend(Duration::from_millis(50));
                        self.power_effects(effects);
                        continue;
                    }
                    self.broadcast_app(Event::new("SaveState"));
                    let platform = &self.services.platform;
                    let plan = platform.power.prepare_suspend(PowerContext {
                        wifi_on: platform.network.is_wifi_on() || self.power.wifi_jobs > 0,
                        charging: platform.battery.is_charging().unwrap_or(false),
                    });
                    self.power.prepare_generation += 1;
                    self.power.scripts.submit(self.power.prepare_generation, plan);
                }
                Effect::RunFinishSuspend => {
                    // The panel finishes its last update first. Only the sync and the
                    // suspend write run on the worker.
                    self.backend.wait_idle();
                    let mut plan = self.services.platform.power.finish_suspend();
                    if let Some(timeout) = self.idle_timeout(settings::keys::DEVICE_POWER_OFF_MINUTES, 0.0) {
                        let remaining = (self.power.last_activity + timeout).saturating_sub(self.idle_now());
                        let epoch = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let alarm = self
                            .services
                            .platform
                            .power
                            .schedule_wakeup(remaining.as_secs().max(1), epoch);
                        plan = alarm.then(plan);
                    }
                    self.power.suspend_started =
                        Some((self.ctx.scheduler.now(), std::time::SystemTime::now()));
                    self.power.finishing = true;
                    self.power.scripts.submit(FINISH_JOB, plan);
                }
                Effect::GiveUp => self.broadcast_app(Event::new("UnexpectedWakeupLimit")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppOptions, HeadlessBackend, ScriptInput, Services};
    use kaia_events::ManualClock;
    use kaia_platform::{Env, Platform};

    #[test]
    fn power_press_cancels_the_two_second_finish_timer() {
        let dir = std::env::temp_dir().join(format!("kaia-power-timer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let platform = Platform::detect_in(
            &dir,
            &Env {
                home: Some(dir.clone()),
                ..Env::default()
            },
        )
        .unwrap();
        let panel = kaia_display::PanelSpec::LIBRA_COLOUR;
        let services = Services::open(platform, panel, Some(dir.join("data"))).unwrap();
        let clock = ManualClock::new();
        let mut app = App::new(
            HeadlessBackend::new(panel, ScriptInput::idle()),
            services,
            Box::new(clock.clone()),
            AppOptions::default(),
        );
        app.start().unwrap();
        app.sleep();
        let cancel = app.power.flow.resume();
        app.power_effects(cancel);
        let effects = app.power.flow.prepare_done(PrepareResult::Done);
        app.power_effects(effects);
        assert!(app.power.flow.finish_pending());
        clock.advance(Duration::from_secs(1));
        app.key(kaia_input::Key::Power).unwrap();
        assert!(!app.power.flow.finish_pending());
        clock.advance(Duration::from_secs(2));
        app.process(Vec::new()).unwrap();
        assert!(!app.sleeping());
        assert!(app.power.timers.is_empty());
    }
}

#[cfg(test)]
mod fence_tests {
    use super::*;
    use crate::{AppOptions, HeadlessBackend, ScriptInput, Services};
    use kaia_display::{Display, DisplayError, InputEvent, InputSource, Refresh, Rotation};
    use kaia_events::ManualClock;
    use kaia_gfx::{PixelFormat, Pixmap, Size};
    use kaia_platform::{Env, Platform};
    struct Backend {
        inner: HeadlessBackend,
        waits: usize,
    }
    impl Display for Backend {
        fn size(&self) -> Size {
            self.inner.size()
        }
        fn format(&self) -> PixelFormat {
            self.inner.format()
        }
        fn dpi(&self) -> u32 {
            self.inner.dpi()
        }
        fn present(&mut self, frame: &Pixmap, refresh: &Refresh) -> Result<(), DisplayError> {
            self.inner.present(frame, refresh)
        }
        fn set_rotation(&mut self, rotation: Rotation) -> Result<(), DisplayError> {
            self.inner.set_rotation(rotation)
        }
        fn wait_idle(&mut self) {
            self.waits += 1;
        }
    }
    impl InputSource for Backend {
        fn poll(&mut self, timeout: Option<Duration>) -> Vec<InputEvent> {
            self.inner.poll(timeout)
        }
    }
    #[test]
    fn completion_fence_is_at_suspend_boundary() {
        let dir = std::env::temp_dir().join(format!("kaia-suspend-fence-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let platform = Platform::detect_in(
            &dir,
            &Env {
                home: Some(dir.clone()),
                ..Env::default()
            },
        )
        .unwrap();
        let panel = kaia_display::PanelSpec::LIBRA_COLOUR;
        let services = Services::open(platform, panel, Some(dir.join("data"))).unwrap();
        let backend = Backend {
            inner: HeadlessBackend::new(panel, ScriptInput::idle()),
            waits: 0,
        };
        let mut app = App::new(
            backend,
            services,
            Box::new(ManualClock::new()),
            AppOptions::default(),
        );
        app.start().unwrap();
        app.sleep();
        app.process(Vec::new()).unwrap();
        assert_eq!(app.backend.waits, 0);
        app.power_effects(vec![Effect::RunFinishSuspend]);
        assert_eq!(app.backend.waits, 1);
    }
    #[test]
    fn power_cancels_suspend_while_scripts_run() {
        let dir = std::env::temp_dir().join(format!("kaia-suspend-scripts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let platform = Platform::detect_in(
            &dir,
            &Env {
                home: Some(dir.clone()),
                ..Env::default()
            },
        )
        .unwrap();
        let panel = kaia_display::PanelSpec::LIBRA_COLOUR;
        let services = Services::open(platform, panel, Some(dir.join("data"))).unwrap();
        let mut app = App::new(
            HeadlessBackend::new(panel, ScriptInput::idle()),
            services,
            Box::new(ManualClock::new()),
            AppOptions::default(),
        );
        app.start().unwrap();
        app.sleep();
        // A prepare job is on the worker when Power comes.
        app.power.prepare_generation = 1;
        app.power.scripts.submit(
            1,
            kaia_platform::plan::Plan::new(vec![kaia_platform::plan::Step::sleep(
                Duration::from_millis(200),
                "test",
            )]),
        );
        let start = std::time::Instant::now();
        app.key(kaia_input::Key::Power).unwrap();
        assert!(start.elapsed() < Duration::from_millis(100));
        assert!(!app.sleeping());
        while app.power.scripts.busy() {
            assert!(start.elapsed() < Duration::from_secs(2));
            app.process(Vec::new()).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.power.flow.finish_pending());
    }
}

#[cfg(test)]
mod latency_tests {
    use super::*;
    use crate::{AppOptions, HeadlessBackend, ScriptInput, Services};
    use kaia_events::ManualClock;
    use kaia_platform::battery::MONZA_BATTERY_DIR;
    use kaia_platform::power::PRE_SUSPEND_DELAY;
    use kaia_platform::{Env, Platform};
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Instant;

    fn put(root: &Path, device_path: &str, text: &str) {
        let path = root.join(device_path.trim_start_matches('/'));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// Runs the loop until `done` holds. Fails after a minute. The suspend plan runs
    /// a real `sync`, which can take many seconds on a CI runner after a large build.
    fn wait_until(app: &mut App<HeadlessBackend>, what: &str, done: impl Fn(&App<HeadlessBackend>) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !done(app) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            app.process(Vec::new()).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// A Libra Colour tree where `/sys/power/state` is a FIFO with no reader. The
    /// suspend write blocks until a reader opens the FIFO, as it blocks until wakeup.
    #[test]
    fn power_is_fast_while_the_suspend_plan_runs() {
        let dir = std::env::temp_dir().join(format!("kaia-suspend-latency-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        put(
            &dir,
            "/mnt/onboard/.kobo/version",
            "N3-0-0,4.38.23171,4.38.23171,4.38.23171,x,y,z,320\n",
        );
        put(&dir, &format!("{MONZA_BATTERY_DIR}/status"), "Discharging\n");
        put(&dir, &format!("{MONZA_BATTERY_DIR}/capacity"), "60\n");
        put(&dir, "/sys/power/state", "mem\n");
        put(&dir, "/sys/power/state-extended", "0");
        let env = Env {
            product: Some("monza".to_string()),
            ..Env::default()
        };
        let platform = Platform::detect_in(&dir, &env).unwrap();
        let state = dir.join("sys/power/state");
        std::fs::remove_file(&state).unwrap();
        assert!(std::process::Command::new("mkfifo")
            .arg(&state)
            .status()
            .unwrap()
            .success());

        // The Kobo reads its fonts from the installed copy. The tree links the repository copy.
        let install = dir.join("mnt/onboard/.adds/kaia");
        std::fs::create_dir_all(&install).unwrap();
        std::os::unix::fs::symlink(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources"),
            install.join("resources"),
        )
        .unwrap();

        let panel = kaia_display::PanelSpec::LIBRA_COLOUR;
        let services = Services::open(platform, panel, Some(dir.join("data"))).unwrap();
        let clock = ManualClock::new();
        let mut app = App::new(
            HeadlessBackend::new(panel, ScriptInput::idle()),
            services,
            Box::new(clock.clone()),
            AppOptions::default(),
        );
        app.start().unwrap();
        app.sleep();
        clock.advance(SUSPEND_WAIT_TIMEOUT);
        // The suspend timer starts the prepare plan. Its result starts the 2 s wait.
        app.process(Vec::new()).unwrap();
        wait_until(&mut app, "the prepare plan", |a| a.power.flow.finish_pending());

        // The reader opens the FIFO once the test releases it. The helper also
        // gives up after a minute, so a blocked app thread fails the test instead
        // of hanging it.
        let (release, released) = mpsc::channel::<()>();
        let reader = std::thread::spawn({
            let state = state.clone();
            move || {
                let _ = released.recv_timeout(Duration::from_secs(60));
                std::fs::File::open(&state).unwrap()
            }
        });

        clock.advance(PRE_SUSPEND_DELAY);
        let start = Instant::now();
        app.process(Vec::new()).unwrap();
        let finish_step = start.elapsed();
        eprintln!("Finish step on the app thread: {finish_step:?}");
        assert!(finish_step < Duration::from_millis(100), "{finish_step:?}");
        assert!(app.power.finishing);

        let start = Instant::now();
        app.key(kaia_input::Key::Power).unwrap();
        let power = start.elapsed();
        eprintln!("Power dispatch while the suspend plan runs: {power:?}");
        assert!(power < Duration::from_millis(100), "{power:?}");
        assert!(app.sleeping());

        release.send(()).unwrap();
        wait_until(&mut app, "the suspend write to return", |a| !a.power.finishing);
        wait_until(&mut app, "the queued resume", |a| !a.sleeping());
        drop(reader.join().unwrap());
    }
}

#[cfg(test)]
mod idle_tests {
    use super::*;
    use crate::{AppOptions, HeadlessBackend, ScriptInput, Services};
    use kaia_events::{Clock, ManualClock, Value};
    use kaia_platform::{Env, Platform};

    fn app() -> (tempfile::TempDir, App<HeadlessBackend>, ManualClock) {
        let dir = tempfile::tempdir().unwrap();
        let platform = Platform::detect_in(
            dir.path(),
            &Env {
                home: Some(dir.path().into()),
                ..Env::default()
            },
        )
        .unwrap();
        let panel = kaia_display::PanelSpec::LIBRA_COLOUR;
        let services = Services::open(platform, panel, Some(dir.path().join("data"))).unwrap();
        let clock = ManualClock::new();
        let mut app = App::new(
            HeadlessBackend::new(panel, ScriptInput::idle()),
            services,
            Box::new(clock.clone()),
            AppOptions::default(),
        );
        app.start().unwrap();
        (dir, app, clock)
    }

    #[test]
    fn pending_orientation_runs_on_the_next_loop_without_an_input_wait() {
        let (_dir, mut app, _) = app();
        app.command("settings:pick:display.rotation", &[Value::Number(1.0)]);
        assert_eq!(app.wait_budget(None), Some(Duration::ZERO));
    }

    #[test]
    fn suspended_wall_time_counts_toward_idle_power_off() {
        let (_dir, mut app, clock) = app();
        app.services
            .settings
            .save(settings::keys::DEVICE_POWER_OFF_MINUTES, Value::Number(5.0));
        app.sleep();
        // Model a kernel sleep where the monotonic clock advances by only one second.
        app.power.finishing = true;
        app.power.suspend_started = Some((
            clock.now(),
            std::time::SystemTime::now() - Duration::from_secs(301),
        ));
        clock.advance(Duration::from_secs(1));
        app.plan_done(FINISH_JOB, Outcome::Completed);
        app.check_idle_power();
        assert!(app.power.off_requested);
        assert!(app.power.timers.is_empty());
    }

    #[test]
    fn power_off_drops_a_late_suspend_prepare_result() {
        let (_dir, mut app, _) = app();
        app.sleep();
        app.power.prepare_generation = 5;
        let generation = app.power.prepare_generation;
        app.request_power_off();
        app.plan_done(generation, Outcome::Completed);
        assert!(app.power.timers.is_empty());
        assert!(!app.power.flow.finish_pending());
    }

    #[test]
    fn failed_power_off_closes_its_screen_and_resets_idle() {
        let (_dir, mut app, clock) = app();
        app.power.off_requested = true;
        app.power.off_started = true;
        let id = app.show_sleep_screen(None);
        app.power.off_screen = Some(id);
        app.stack.apply_requests(&mut app.ctx);
        clock.advance(Duration::from_secs(42));
        app.plan_done(
            POWER_OFF_JOB,
            Outcome::Failed {
                step: 2,
                recovered: false,
            },
        );
        assert!(!app.stack.contains(id));
        assert!(!app.power.off_requested);
        assert!(!app.power.off_started);
        assert_eq!(app.power.last_activity, clock.now());
    }
}
