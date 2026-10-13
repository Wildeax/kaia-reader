//! The scheduler runs suspend timers and frontlight ramp steps on the app thread.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use kaia_events::{Ctx, Event, Refresh, RefreshMode, ShowOpts, TaskId, WindowId};
use kaia_platform::frontlight::RampStep;
use kaia_platform::power::{PowerContext, ResumeContext};
use kaia_platform::suspend::{Effect, PrepareResult, SuspendFlow, Timer, WakeupCause, SUSPEND_WAIT_TIMEOUT};
use kaia_ui::Window;

use super::App;
use crate::backend::Backend;
use crate::surfaces::{settings, Route};

#[derive(Default)]
pub(super) struct State {
    flow: SuspendFlow,
    pub(super) scripts: super::network::Scripts,
    prepare_generation: u64,
    prepare_rest: Option<kaia_platform::plan::Plan>,
    last_activity: Duration,
    battery_due: Duration,
    battery_warned: bool,
    charge_screen: bool,
    timers: Vec<(Timer, TaskId)>,
    queue: Rc<RefCell<VecDeque<Due>>>,
    screen: Option<WindowId>,
    restore_light: bool,
    ramp_generation: u64,
}

enum Due {
    Timer(Timer),
    Light(u64, RampStep),
    Resume(kaia_platform::plan::Plan),
}

impl<B: Backend> App<B> {
    pub fn sleeping(&self) -> bool {
        self.power.screen.is_some()
    }

    pub(super) fn sleep(&mut self) {
        if self.sleeping() {
            return;
        }
        self.broadcast_app(Event::new("Suspend"));
        if let Err(e) = self.services.settings.flush(&self.services.settings_path()) {
            self.notices.push(format!("Cannot save settings: {e}"));
        }
        let book = self.router.routes().iter().find_map(|r| match r {
            Route::Reader { book } => Some(*book),
            _ => None,
        });
        let window = Window::new(
            settings::sleep::sleep_screen_with_label(
                &self.services,
                self.screen,
                book,
                if self.power.charge_screen {
                    "Please charge your reader"
                } else {
                    settings::sleep::SLEEP_LABEL
                },
            ),
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
        self.power.screen = Some(id);
        self.power.restore_light = self.services.platform.frontlight.is_on();
        if self.power.restore_light {
            self.toggle_light();
        }
        let effects = self.power.flow.reschedule_suspend(SUSPEND_WAIT_TIMEOUT);
        self.power_effects(effects);
    }

    pub(super) fn resume(&mut self) {
        self.note_activity();
        self.power.charge_screen = false;
        self.power.prepare_generation += 1;
        self.power.prepare_rest = None;
        let effects = self.power.flow.resume();
        self.power_effects(effects);
        let Some(id) = self.power.screen.take() else {
            return;
        };
        self.stack
            .close(id, Some(Refresh::new(RefreshMode::Full)), &mut self.ctx);
        let mut plan = self.services.platform.power.resume(ResumeContext {
            sync_system_clock: true,
            ..ResumeContext::default()
        });
        // Split the plan's short delay as well, so input remains responsive on resume.
        if let Some(index) = plan
            .steps
            .iter()
            .position(|s| matches!(s.action, kaia_platform::plan::Action::Sleep(_)))
        {
            let rest = plan.steps.split_off(index + 1);
            let delay = match plan.steps.pop().unwrap().action {
                kaia_platform::plan::Action::Sleep(d) => d,
                _ => unreachable!(),
            };
            self.services.platform.run_plan(&plan);
            let queue = self.power.queue.clone();
            self.ctx.scheduler.schedule_in(
                delay.as_secs_f64(),
                Box::new(move |_: &mut Ctx| {
                    queue
                        .borrow_mut()
                        .push_back(Due::Resume(kaia_platform::plan::Plan::new(rest)));
                }),
            );
        } else {
            self.services.platform.run_plan(&plan);
        }
        if self.power.restore_light {
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
        let mut delay = Duration::ZERO;
        for step in ramp.steps {
            delay += step.delay;
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
        self.power.scripts.submit(0, Plan::new(plan.steps));
    }

    pub(super) fn note_activity(&mut self) {
        self.power.last_activity = self.ctx.scheduler.now();
    }

    pub(super) fn power_wait(&self) -> Duration {
        let now = self.ctx.scheduler.now();
        let battery = self.power.battery_due.saturating_sub(now);
        #[cfg(feature = "lua")]
        if self.services.plugins.idle_paused() {
            return battery;
        }
        let minutes = self
            .services
            .settings
            .read("device.sleep_minutes")
            .as_f64()
            .unwrap_or(15.0);
        if self.sleeping() || !minutes.is_finite() || minutes <= 0.0 {
            return battery;
        }
        battery.min(
            (self.power.last_activity + Duration::from_secs_f64((minutes * 60.0).min(86400.0)))
                .saturating_sub(now),
        )
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
                .toast("Battery low. Please charge your reader.", 5.0);
            }
        }
        let minutes = self
            .services
            .settings
            .read("device.sleep_minutes")
            .as_f64()
            .unwrap_or(15.0);
        #[cfg(not(feature = "lua"))]
        let paused = false;
        #[cfg(feature = "lua")]
        let paused = self.services.plugins.idle_paused();
        if !paused
            && !self.sleeping()
            && minutes.is_finite()
            && minutes > 0.0
            && now.saturating_sub(self.power.last_activity).as_secs_f64() >= minutes * 60.0
        {
            self.sleep();
        }
    }

    pub(super) fn power_due(&mut self) {
        self.check_idle_power();
        while let Some((generation, outcome)) = self.power.scripts.poll() {
            if generation == 0 {
                if !matches!(outcome, kaia_platform::plan::Outcome::Completed) {
                    self.notices.push(format!("Wi-Fi did not change: {outcome:?}"));
                }
            } else if generation == self.power.prepare_generation && self.sleeping() {
                if let Some(rest) = self.power.prepare_rest.take() {
                    let result = self.services.platform.run_plan(&rest);
                    let effects = self.power.flow.prepare_done(PrepareResult::from_outcome(&result));
                    self.power_effects(effects);
                }
            }
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
                Due::Light(generation, step) if generation == self.power.ramp_generation => {
                    if let Err(e) = self.services.platform.frontlight.apply_ramp_step(&step) {
                        self.services.platform.frontlight.cancel_ramp();
                        self.power.ramp_generation += 1;
                        self.notices.push(format!("Cannot change the light: {e}"));
                    }
                }
                Due::Resume(plan) => {
                    self.services.platform.run_plan(&plan);
                }
                _ => {}
            }
        }
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
                    let platform = &mut self.services.platform;
                    let mut plan = platform.power.prepare_suspend(PowerContext {
                        wifi_on: platform.network.is_wifi_on() || self.power.scripts.busy(),
                        charging: platform.battery.is_charging().unwrap_or(false),
                    });
                    let count = plan
                        .steps
                        .iter()
                        .take_while(|s| matches!(s.action, kaia_platform::plan::Action::Run { .. }))
                        .count();
                    if count > 0 {
                        let rest = plan.steps.split_off(count);
                        self.power.prepare_generation += 1;
                        self.power.prepare_rest = Some(kaia_platform::plan::Plan::new(rest));
                        self.power.scripts.submit(self.power.prepare_generation, plan);
                        continue;
                    }
                    let result = platform.run_plan(&plan);
                    let next = self.power.flow.prepare_done(PrepareResult::from_outcome(&result));
                    self.power_effects(next);
                }
                Effect::RunFinishSuspend => {
                    self.backend.wait_idle();
                    let plan = self.services.platform.power.finish_suspend();
                    self.services.platform.run_plan(&plan);
                    let next = self.power.flow.finish_done();
                    self.power_effects(next);
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
        app.power.prepare_generation = 1;
        app.power.prepare_rest = Some(kaia_platform::plan::Plan::new(vec![]));
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
        assert!(app.power.prepare_rest.is_none());
    }
}
