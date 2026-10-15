//! The main loop. It follows the "App loop contract" in `docs/kaia/specs/UI_TOOLKIT.md`:
//!
//! 1. Wait for input. The wait ends at the next scheduled task, the next gesture
//!    timer, the exit time, or when the scan thread has news.
//! 2. Feed touches through the gesture recognizer and dispatch gestures and keys
//!    to the top window. Unhandled input may close an overlay or go Back.
//! 3. Run the actions and events the windows queued. Each action goes to the top
//!    surface and each surface below it, then the global commands.
//! 4. Paint the dirty windows into one frame, bottom up, and present each refresh.
//! 5. Run the due tasks (button releases among them), apply the window requests
//!    they made, and paint and present again.
//!
//! This file knows no surface. It asks the surfaces in [`crate::surfaces`] for
//! their windows and applies the [`Outcome`]s they return.

mod clock;
mod natural_light;
mod network;
#[cfg(feature = "lua")]
mod plugins;
mod power;
mod sleep_picture;

use kaia_i18n::{format as tr_format, tr};
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use kaia_display::{DisplayError, InputEvent, Waveform};
use kaia_events::{Clock, Ctx, Event, Rect as EventRect, RefreshMode, Value, WindowStack};
use kaia_gfx::{Pixmap, Point, Rect, Size};
use kaia_input::{Direction, Gesture, GestureConfig, GestureDetector, Key, KeyEvent, Phase, TouchEvent};
use kaia_ui::widget::{Handled, UiEvent};
use kaia_ui::{Out, Tone, Window};

use crate::backend::Backend;
use crate::command::{BookRef, Command};
use crate::refresh;
use crate::router::{Route, Router};
use crate::scan::{LibraryScan, ScanMessage};
use crate::services::Services;
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces;

/// The longest the loop waits while a library scan runs, so it can show progress.
const SCAN_POLL: Duration = Duration::from_millis(50);

/// Upper bound on rounds of action handling per iteration. A widget that keeps
/// queuing actions cannot stall the loop.
const MAX_SETTLE_ROUNDS: usize = 16;

/// Time between the press and the release of a synthetic tap, for [`App::tap`].
const TAP_HOLD: Duration = Duration::from_millis(50);

/// Builds a [`SurfaceEnv`] from the app's fields. The fields are borrowed one by
/// one, so the router can be borrowed at the same time.
macro_rules! env {
    ($app:expr) => {
        SurfaceEnv::new(
            &mut $app.services,
            &mut $app.ctx,
            $app.screen,
            &mut $app.notices,
            &mut $app.transient,
        )
    };
}

mod device_input;
mod screenshot;

/// Counters for tests and for the desktop run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Frames that reached the display: a paint that produced at least one refresh.
    pub frames: u64,
    /// Refreshes sent to the display.
    pub refreshes: u64,
}

/// What the loop does next. `Exit` means the exit was accepted: the reader and the
/// queued sync updates are saved. The caller then calls [`App::shutdown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
}

/// Why a request to quit was refused. The loop keeps running and shows the reason.
enum Refusal {
    /// The reader could not save. It shows its own message.
    Reader,
    /// The sync queue could not be written. The text is the error, for the log.
    SyncQueue(String),
}

/// Settings for [`App::new`].
#[derive(Debug, Clone, Default)]
pub struct AppOptions {
    /// Folders the library scan reads. Empty means no scan.
    pub library_roots: Vec<PathBuf>,
}

/// The app. It owns the backend, the services, the window stack and the router.
pub struct App<B: Backend> {
    backend: B,
    power: power::State,
    natural_light: natural_light::State,
    clock: clock::State,
    services: Services,
    ctx: Ctx,
    stack: WindowStack,
    router: Router,
    frame: Pixmap,
    presented_color: Option<bool>,
    presented_night: Option<bool>,
    screen: Size,
    rotation: kaia_display::Rotation,
    pending_rotation: Option<kaia_display::Rotation>,
    usb_mass_storage: bool,
    scan_again: bool,
    gestures: GestureDetector,
    held_page_keys: Vec<Key>,
    options: AppOptions,
    scan: Option<LibraryScan>,
    scan_progress: Option<(usize, usize)>,
    stats: FrameStats,
    modes: Vec<RefreshMode>,
    unknown: Vec<String>,
    notices: Vec<String>,
    /// Windows waiting for registration with the router.
    transient: Vec<Window>,
    last_input_us: u64,
    /// Input event time minus scheduler time at the latest touch. Evdev
    /// timestamps use the kernel clock, the scheduler counts from app start, so
    /// gesture timers (hold, double tap) convert through this offset.
    input_clock_offset_us: i64,
    exit_at: Option<Duration>,
    quitting: bool,
    /// A request to quit was refused because the sync queue could not be written. Later
    /// refusals of that kind show the prompt instead of the toast.
    sync_save_failed: bool,
    /// The prompt that offers to exit without the sync update, while it is open.
    sync_prompt: Option<kaia_events::WindowId>,
    /// The user chose to exit without the queued sync update.
    sync_drop_confirmed: bool,
    pending_navigation: Option<(String, Vec<Value>)>,
    /// Screenshot saves that are still running.
    screenshots: screenshot::Saves,
    /// Set by `KAIA_TRACE_INPUT`. Logs every gesture and the top route to stderr.
    trace_input: bool,
}

impl<B: Backend> App<B> {
    /// Builds the app on `backend`. Call [`App::start`] to show Home.
    pub fn new(backend: B, mut services: Services, clock: Box<dyn Clock>, mut options: AppOptions) -> Self {
        for root in &mut options.library_roots {
            if let Ok(absolute) = std::path::absolute(&*root) {
                *root = absolute;
            }
        }
        services.library_roots = options.library_roots.clone();
        #[cfg(feature = "lua")]
        if let Some(root) = options.library_roots.first() {
            services.plugins.library.borrow_mut().folder = root.clone();
        }
        let rotation = device_input::rotation(services.platform.device.startup_rotation_degrees() as i64);
        let screen = backend.size();
        let format = services.theme.pixel_format();
        let mut frame = Pixmap::new(screen.w, screen.h, format);
        frame.fill_rect(Rect::from_size(screen), services.theme.color(Tone::Surface));
        let mut ctx = Ctx::new(clock, EventRect::new(0, 0, screen.w as i32, screen.h as i32));
        ctx.dirty.configure_from(&services.settings);
        // The reader counts page turns. UI paints and selection changes do not count.
        ctx.dirty.full_refresh_count = 0;
        let gestures = GestureDetector::new(GestureConfig {
            dpi: backend.dpi(),
            ..GestureConfig::default()
        });
        App {
            backend,
            power: power::State::new(services.platform.io_handle(), services.library_path()),
            natural_light: natural_light::State::new(services.platform.frontlight.warmth_revision()),
            clock: clock::State::new(),
            services,
            ctx,
            stack: WindowStack::new(),
            router: Router::new(),
            frame,
            presented_color: None,
            presented_night: None,
            screen,
            rotation,
            pending_rotation: None,
            usb_mass_storage: false,
            scan_again: false,
            gestures,
            held_page_keys: Vec::new(),
            options,
            scan: None,
            scan_progress: None,
            stats: FrameStats::default(),
            modes: Vec::new(),
            unknown: Vec::new(),
            notices: Vec::new(),
            transient: Vec::new(),
            last_input_us: 0,
            input_clock_offset_us: 0,
            exit_at: None,
            quitting: false,
            sync_save_failed: false,
            sync_prompt: None,
            sync_drop_confirmed: false,
            pending_navigation: None,
            screenshots: screenshot::Saves::default(),
            trace_input: std::env::var_os("KAIA_TRACE_INPUT").is_some(),
        }
    }

    /// Shows Home with a full refresh, starts the library scan, and paints the first frame.
    pub fn start(&mut self) -> Result<(), DisplayError> {
        let degrees = self
            .services
            .settings
            .read(surfaces::settings::keys::DISPLAY_ROTATION)
            .as_i64();
        if let Some(degrees) = degrees {
            self.rotate(device_input::rotation(degrees))?;
        }
        self.switch_root_to(Route::Home, RefreshMode::Full);
        // A PIN set in the data folder locks the app before any book shows.
        self.lock_screen();
        for notice in std::mem::take(&mut self.services.startup_notices) {
            env!(self).notice(notice);
        }
        if !self.options.library_roots.is_empty() {
            let db = self.services.library_path();
            self.scan = Some(LibraryScan::spawn(db, self.options.library_roots.clone()));
        }
        self.step(Vec::new())?;
        // The first screen is up. The start is healthy once it has stayed up for a while.
        if self.stats.frames > 0 {
            let data = self.services.paths.data_dir.clone();
            self.ctx.scheduler.schedule_in(
                crate::safe_mode::HEALTHY_AFTER.as_secs_f64(),
                Box::new(move |_| crate::safe_mode::mark_healthy(&data)),
            );
        }
        Ok(())
    }

    /// Runs one iteration, waiting for input at most `timeout` (or until something
    /// is due, whichever comes first). `None` waits for input only when nothing is
    /// scheduled.
    pub fn iterate(&mut self, timeout: Option<Duration>) -> Result<Flow, DisplayError> {
        let budget = self.wait_budget(timeout);
        let input = self.backend.poll(budget);
        self.step(input)
    }

    /// Runs the loop until the backend quits, [`App::quit`] is called, or the exit
    /// time set by [`App::exit_after`] passes, and the exit is accepted. Then it shuts
    /// down. A refused exit keeps the loop running.
    pub fn run(&mut self) -> Result<(), DisplayError> {
        let result = loop {
            match self.iterate(None) {
                Err(error) => break Err(error),
                Ok(Flow::Exit) => break Ok(()),
                Ok(Flow::Continue) => {}
            }
        };
        if result.is_err() {
            // The loop stopped on an error, so no exit check ran. Save the reader here.
            self.broadcast_app(Event::new("SaveState"));
        }
        self.shutdown();
        result
    }

    /// Handles input that the caller supplies, in place of the backend's. Runs one
    /// iteration. Tests use this to drive the app.
    pub fn process(&mut self, input: Vec<InputEvent>) -> Result<Flow, DisplayError> {
        self.step(input)
    }

    /// Taps `p` with a touch down and up, then runs one iteration.
    pub fn tap(&mut self, p: Point) -> Result<Flow, DisplayError> {
        let t0 = self.gesture_now_us();
        let t1 = t0 + TAP_HOLD.as_micros() as u64;
        let down = InputEvent::Touch(TouchEvent {
            slot: 0,
            phase: Phase::Down,
            pos: p,
            time_us: t0,
        });
        let up = InputEvent::Touch(TouchEvent {
            slot: 0,
            phase: Phase::Up,
            pos: p,
            time_us: t1,
        });
        self.process(vec![down, up])
    }

    /// Taps a key with both edges, then runs one iteration.
    pub fn key(&mut self, key: Key) -> Result<Flow, DisplayError> {
        let ev = KeyEvent {
            key,
            pressed: true,
            time_us: self.gesture_now_us(),
        };
        self.process(vec![
            InputEvent::Key(ev),
            InputEvent::Key(KeyEvent { pressed: false, ..ev }),
        ])
    }

    /// Asks the loop to stop after the current iteration. A refused exit keeps it running.
    pub fn quit(&mut self) {
        self.quitting = true;
    }

    /// Stops [`App::run`] once `d` has passed on the app clock.
    pub fn exit_after(&mut self, d: Duration) {
        self.exit_at = Some(self.ctx.scheduler.now() + d);
    }

    /// Hides the screens, writes the sync queue once more, stops the sync worker and
    /// the library scan, and saves the settings. The caller calls it after
    /// [`App::iterate`] returns [`Flow::Exit`]. [`App::run`] does too.
    ///
    /// It cannot refuse. The loop wrote the queued sync updates before it returned
    /// `Exit`, so a failure here only reaches the log.
    pub fn shutdown(&mut self) {
        self.hide_all();
        #[cfg(feature = "lua")]
        self.pump_plugins();
        // Hiding the reader pushes its position once more. The loop already wrote
        // that position, so a failed write here only loses the repeat. It is logged.
        if let Err(error) = self.services.sync.persist_pending(&self.services.settings) {
            eprintln!("kaia: dropping sync updates that could not be written: {error}");
        }
        self.services.sync.close();
        self.services.webdav.stop();
        if let Some(scan) = self.scan.take() {
            scan.cancel();
        }
        if let Err(e) = self.services.settings.flush(&self.services.settings_path()) {
            eprintln!("kaia: cannot save settings: {e}");
        }
    }

    /// The checks that can keep the app open. The loop runs them on each request to
    /// stop, before any screen is hidden. A refusal leaves the reader and the pending
    /// sync updates in place, so the next request runs the checks again.
    fn prepare_exit(&mut self) -> Result<(), Refusal> {
        if !self.can_hide_all() {
            return Err(Refusal::Reader);
        }
        self.broadcast_app(Event::new("SaveState"));
        if let Err(error) = self.services.sync.persist_pending(&self.services.settings) {
            if !self.sync_drop_confirmed {
                return Err(Refusal::SyncQueue(error));
            }
            eprintln!("kaia: exiting without saving a sync update: {error}");
        }
        Ok(())
    }

    /// Tells the user why an exit was refused. The first refused sync write shows a
    /// toast. The next request to stop shows a prompt that offers to exit without the
    /// update, which is the way out when the queue stays unwritable. The prompt is not
    /// shown twice while it is open.
    fn refuse_exit(&mut self, refusal: Refusal) {
        self.sync_drop_confirmed = false;
        let Refusal::SyncQueue(error) = refusal else {
            return;
        };
        eprintln!("kaia: the sync queue could not be written, the app stays open: {error}");
        if !self.sync_save_failed {
            self.sync_save_failed = true;
            env!(self).notice(tr("Could not save the reading position for sync. Try again."));
            return;
        }
        if self
            .sync_prompt
            .is_some_and(|id| self.stack.options(id).is_some())
        {
            return;
        }
        let mut env = env!(self);
        let prompt = kaia_ui::composite::Dialog::new(
            env.theme(),
            tr("Sync position not saved"),
            tr(
                "The reading position could not be saved for sync. Exiting now drops that update. The book keeps its own saved position.",
            ),
            vec![
                kaia_ui::composite::DialogButton::new(tr("Keep reading"), "dialog:dismiss"),
                kaia_ui::composite::DialogButton::new(tr("Exit anyway"), "sync:exit-anyway"),
            ],
        );
        self.sync_prompt = Some(env.dialog(prompt));
    }

    pub fn services(&self) -> &Services {
        &self.services
    }

    pub fn services_mut(&mut self) -> &mut Services {
        &mut self.services
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }

    pub fn router(&self) -> &Router {
        &self.router
    }

    /// The route on top of the stack.
    pub fn top_route(&self) -> Option<&Route> {
        self.router.top_route()
    }

    /// The frame as the app last painted it. Screenshots come from here.
    pub fn frame(&self) -> &Pixmap {
        &self.frame
    }

    pub fn screen(&self) -> Size {
        self.screen
    }

    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    /// The refresh modes sent to the display, in order.
    pub fn refresh_modes(&self) -> &[RefreshMode] {
        &self.modes
    }

    /// Action ids that no surface and no global command recognized.
    pub fn unknown_commands(&self) -> &[String] {
        &self.unknown
    }

    /// Notes about requests that could not be done, such as a missing book.
    pub fn notices(&self) -> &[String] {
        &self.notices
    }

    /// `(done, total)` of the running library scan, or None when no scan is running.
    pub fn scan_progress(&self) -> Option<(usize, usize)> {
        self.scan_progress
    }

    pub fn reader_busy(&self) -> bool {
        self.router.busy()
    }

    pub fn scan_running(&self) -> bool {
        self.scan.is_some()
    }

    /// The absolute bounds of the widget with `key` on the top window, from its last layout.
    pub fn find_rect(&self, key: &str) -> Option<Rect> {
        self.input_window()?.find_rect(key)
    }

    /// Runs `f` on the widget with `key` on the top window. Returns false when the
    /// widget is missing or has another type.
    pub fn update_top<T, F>(&mut self, key: &str, f: F) -> bool
    where
        T: kaia_ui::Widget + 'static,
        F: FnOnce(&mut T, &mut kaia_ui::UpdateCx<'_>),
    {
        match self.input_window() {
            Some(window) => window.update::<T, F>(&mut self.ctx, key, f),
            None => false,
        }
    }

    /// The loop body after input: the touches, keys and events, gestures, actions,
    /// paint, tasks and paint again.
    fn step(&mut self, input: Vec<InputEvent>) -> Result<Flow, DisplayError> {
        if !self.reader_busy() {
            if let Some(rotation) = self.pending_rotation.take() {
                self.rotate(rotation)?;
            }
        }
        self.pump_scan();
        if let Some(result) = self.services.dictionary_downloads.poll(&self.services.settings) {
            match result {
                Ok(_) => {
                    self.services.dictionary = None;
                    crate::surfaces::reader::dict::load(&mut self.services);
                    env!(self).notice(tr("Dictionary installed."));
                }
                Err(error) => env!(self).notice(tr_format(&tr("Dictionary download failed: %1"), &[&error])),
            }
            self.broadcast_app(Event::new("DictionariesChanged"));
        }
        let connected = self.services.settings.is_true(crate::webdav::ENABLED)
            && !crate::surfaces::settings::keys::airplane_mode(&self.services.settings)
            && self.services.platform.network.is_connected();
        for message in self
            .services
            .webdav
            .tick(&self.services.settings, connected, crate::sync::now_unix())
        {
            env!(self).toast(message, 5.0);
        }
        self.poll_screenshots();
        if crate::covers::poll() || crate::covers::busy() {
            self.stack.broadcast(&Event::new("CoversReady"), &mut self.ctx);
        }
        self.broadcast_app(Event::new("CatalogTick"));
        self.broadcast_app(Event::new("ReaderTick"));
        if !self.reader_busy() {
            if let Some((id, args)) = self.pending_navigation.take() {
                self.command(&id, &args);
            }
        }
        for reply in self
            .services
            .sync
            .tick(&self.services.settings, self.ctx.scheduler.now(), false)
        {
            match reply {
                crate::sync::Reply::Remote(book, p) => self.broadcast_app(Event::with_args(
                    "SyncProgress",
                    vec![
                        Value::Number(book as f64),
                        Value::Str(p.progress),
                        Value::Number(p.percentage),
                        Value::Str(p.device),
                        Value::Str(p.device_id),
                        p.timestamp.map(|t| Value::Number(t as f64)).unwrap_or_default(),
                    ],
                )),
                crate::sync::Reply::Pushed(document) => {
                    self.broadcast_app(Event::with_args("SyncPushed", vec![Value::Str(document)]))
                }
                crate::sync::Reply::ServerAnswered => {
                    env!(self).toast(tr("The server answered."), 4.0);
                }
                crate::sync::Reply::Error(e) => {
                    env!(self).toast(tr_format(&tr("Progress sync failed: %1"), &[e.as_str()]), 5.0);
                }
                crate::sync::Reply::Done => {}
            }
        }
        // Each event is dispatched, painted and settled before the next one, so a
        // later event sees the screen that the earlier one produced.
        for event in input {
            if self.counts_as_activity(&event) {
                self.note_activity();
            }
            #[cfg(feature = "lua")]
            if !self.reader_busy() {
                if let Some(ui) = self.services.plugins.ui.clone() {
                    ui.borrow_mut()
                        .in_app(&mut self.stack, &mut self.ctx, |lua| lua.input_hook());
                }
            }
            match event {
                InputEvent::Pen(pen) => {
                    if !self.sleeping() && !self.reader_busy() {
                        self.dispatch_input(UiEvent::Pen(pen));
                    }
                }
                InputEvent::Touch(t) => self.touch(t),
                InputEvent::TouchFrame(frame) => self.touch_frame(frame),
                InputEvent::Key(k) => self.key_event(k),
                InputEvent::Quit => self.quitting = true,
                InputEvent::Light if !self.sleeping() => self.toggle_light(),
                InputEvent::Light => {}
                InputEvent::Orientation(rotation) => {
                    if !self
                        .services
                        .settings
                        .is_true(surfaces::settings::keys::DISPLAY_ROTATION_LOCK)
                    {
                        self.pending_rotation = Some(rotation);
                    }
                }
                InputEvent::Resize(size) => self.resize(size),
            }
            self.show_input_result()?;
        }
        self.poll_gestures();
        self.show_input_result()?;

        // Tasks due now (button releases, for one) run here. Their paint goes out
        // in the same iteration.
        let now = self.ctx.scheduler.now();
        self.ctx.run_due(now);
        self.power_due();
        self.natural_light_due();
        self.clock_due();
        self.stack.apply_requests(&mut self.ctx);
        self.settle();
        self.repaint()?;

        if self.quitting || self.exit_due() {
            if self.reader_busy() {
                return Ok(Flow::Continue);
            }
            match self.prepare_exit() {
                Ok(()) => {
                    if self.power_off_requested() {
                        self.finish_power_off();
                    } else {
                        self.quitting = true;
                    }
                }
                Err(refusal) => {
                    self.refuse_exit(refusal);
                    self.cancel_power_off();
                    self.quitting = false;
                    self.exit_at = None;
                    // Paint the toast or the prompt now, not at the next input.
                    self.show_input_result()?;
                    return Ok(Flow::Continue);
                }
            }
        }
        Ok(if self.quitting { Flow::Exit } else { Flow::Continue })
    }

    /// Paints what an input event changed, then runs the actions it queued and
    /// paints their result. The paint before the actions shows a pressed button
    /// on the panel before a tap that navigates replaces the screen.
    fn show_input_result(&mut self) -> Result<(), DisplayError> {
        self.repaint()?;
        self.settle();
        self.repaint()?;
        Ok(())
    }

    fn touch(&mut self, t: TouchEvent) {
        self.touch_frame(vec![t]);
    }

    fn touch_frame(&mut self, frame: Vec<TouchEvent>) {
        let Some(latest) = frame.iter().map(|t| t.time_us).max() else {
            return;
        };
        self.last_input_us = self.last_input_us.max(latest);
        let clock = self.ctx.scheduler.now().as_micros() as i64;
        self.input_clock_offset_us = latest as i64 - clock;
        for gesture in self.gestures.feed_frame(&frame) {
            self.gesture(gesture);
        }
    }

    fn poll_gestures(&mut self) {
        let now = self.gesture_now_us();
        for gesture in self.gestures.poll(now) {
            self.gesture(gesture);
        }
    }

    fn gesture(&mut self, gesture: Gesture) {
        if self.trace_input {
            eprintln!(
                "kaia: gesture {} on {:?}",
                gesture.name(),
                self.router.top_route()
            );
        }
        if self.sleeping() {
            return;
        }
        if screenshot::is_gesture(&gesture, self.screen, &self.services.theme.scale) {
            self.save_screenshot();
            return;
        }
        let edge = self.services.theme.scale.dp(40) as i32;
        let quick = matches!(&gesture, Gesture::Swipe { pos, direction: Some(Direction::South), .. } if pos.y < edge)
            || matches!(&gesture, Gesture::Tap { pos, .. } if pos.y < edge && self.router.top_route().is_some_and(Route::is_top_level));
        #[cfg(feature = "lua")]
        let quick = quick && self.services.plugins.foreign_window_count() == 0;
        // The PIN pad takes no quick settings sheet, so input cannot reach the settings under it.
        let quick = quick && !self.top_route().is_some_and(surfaces::pin::is_lock_route);
        if quick {
            // The page has already seen the pan. Let it finish a claimed
            // brightness swipe before opening a sheet over its release.
            let reader_edge = matches!(self.top_route(), Some(Route::Reader { .. }))
                && matches!(&gesture, Gesture::Swipe { pos, .. }
                    if pos.x >= 0 && i64::from(pos.x) * 100 < i64::from(self.screen.w) * 13);
            if reader_edge && self.dispatch_input(UiEvent::Gesture(gesture.clone())) {
                return;
            }
            self.quick_settings();
            return;
        }
        #[cfg(feature = "lua")]
        if matches!(self.top_route(), Some(Route::Reader { .. })) && !self.reader_busy() {
            if let Some(ui) = self.services.plugins.ui.clone() {
                let used = ui.borrow_mut().in_app(&mut self.stack, &mut self.ctx, |lua| {
                    if lua.foreign_window_count() == 0 {
                        lua.reader_input(&gesture)
                    } else {
                        false
                    }
                });
                if used {
                    return;
                }
            }
        }
        let handled = self.dispatch_input(UiEvent::Gesture(gesture.clone()));
        if handled {
            return;
        }
        // Taps that the screen does not use close it, when it says so. A swipe from
        // the left edge goes Back.
        let edge = self.services.theme.scale.dp(40) as i32;
        let dismissed = matches!(gesture, Gesture::Tap { .. })
            && self
                .router
                .top_surface()
                .is_some_and(|surface| surface.closes_on_unused_tap());
        let edge_back = matches!(
            &gesture,
            Gesture::Swipe { pos, direction: Some(Direction::East), .. } if pos.x < edge
        );
        if dismissed || edge_back {
            self.back();
        }
    }

    fn key_event(&mut self, mut ev: KeyEvent) {
        if ev.key == Key::Power {
            self.power_key(ev);
            return;
        }
        let page_key = device_input::is_page_key(ev.key);
        let repeat = page_key && self.held_page_keys.contains(&ev.key);
        if page_key {
            if !ev.pressed {
                self.held_page_keys.retain(|key| *key != ev.key);
            } else if !repeat {
                self.held_page_keys.push(ev.key);
            }
        }
        let reader = !self.sleeping() && matches!(self.top_route(), Some(Route::Reader { .. }));
        if !ev.pressed && !(page_key && reader) {
            return;
        }
        match ev.key {
            Key::SleepCoverClosed | Key::SleepCoverOpened => {
                if !self.usb_mass_storage
                    && surfaces::settings::keys::bool_or(
                        &self.services.settings,
                        surfaces::settings::keys::DEVICE_SLEEP_COVER,
                        true,
                    )
                {
                    if ev.key == Key::SleepCoverClosed {
                        self.sleep();
                    } else {
                        self.resume();
                    }
                }
                return;
            }
            _ if self.sleeping() => {
                if device_input::is_page_key(ev.key)
                    && self.services.platform.supports_page_button_wake()
                    && self
                        .services
                        .settings
                        .is_true(surfaces::settings::keys::DEVICE_PAGE_BUTTON_WAKE)
                {
                    self.resume();
                }
                return;
            }
            _ => {}
        }
        ev.key = device_input::page_key(
            ev.key,
            self.rotation,
            self.services
                .settings
                .is_true(surfaces::settings::keys::READING_INVERT_PAGE_BUTTONS),
        );
        if page_key {
            if reader && self.dispatch_input(UiEvent::Named(device_input::reader_page_key(ev))) {
                return;
            }
            // Sheets and other surfaces keep their single turn on the first press.
            if !ev.pressed || repeat {
                return;
            }
        }
        let handled = self.dispatch_input(UiEvent::Key(ev));
        if !handled {
            match ev.key {
                Key::LPgFwd | Key::RPgFwd => {
                    self.send(Event::new("PageForward"));
                }
                Key::LPgBack | Key::RPgBack => {
                    self.send(Event::new("PageBack"));
                }
                Key::Back => self.back(),
                _ => {}
            }
        }
    }

    fn dispatch_input(&mut self, event: UiEvent) -> bool {
        let mut top_seen = false;
        for id in self.stack.ids_top_down() {
            let Some(opts) = self.stack.options(id) else {
                continue;
            };
            if top_seen && !opts.always_active {
                continue;
            }
            let used = self
                .app_window(id)
                .is_some_and(|w| w.dispatch(event.clone(), &mut self.ctx) == Handled::Yes);
            if opts.toast && !top_seen {
                continue;
            }
            top_seen = true;
            if used {
                return true;
            }
        }
        false
    }

    fn app_window(&self, id: kaia_events::WindowId) -> Option<Window> {
        #[cfg(feature = "lua")]
        if let Some(window) = self.plugin_window(id) {
            return Some(window);
        }
        self.router.window(id)
    }

    fn input_window(&self) -> Option<Window> {
        self.stack
            .ids_top_down()
            .into_iter()
            .find(|id| self.stack.options(*id).is_some_and(|o| !o.toast))
            .and_then(|id| self.app_window(id))
    }

    fn reconcile_windows(&mut self) {
        self.router.register(&mut self.transient);
        self.stack.apply_requests(&mut self.ctx);
        let closed = self.router.closed_screens(&self.stack);
        for index in &closed {
            self.hide(*index);
        }
        self.router.remove_closed(&self.stack);
        if !closed.is_empty() {
            self.reveal_top();
        }
    }

    /// Sends a named event through the window stack. Returns true when a window used it.
    pub fn send(&mut self, ev: Event) -> bool {
        if matches!(ev.name.as_str(), "PageForward" | "PageBack") {
            for id in self.stack.ids_top_down() {
                if let Some(window) = self.app_window(id) {
                    if window.dispatch(UiEvent::Named(ev.clone()), &mut self.ctx) == Handled::Yes {
                        return true;
                    }
                }
                if self.stack.options(id).is_some_and(|o| o.modal) {
                    break;
                }
            }
            false
        } else {
            self.stack.send(&ev, &mut self.ctx)
        }
    }

    fn broadcast(&mut self, ev: Event) {
        self.stack.broadcast(&ev, &mut self.ctx);
    }

    /// Delivers an app event to the windows and to every surface, bottom up. The
    /// outcomes are applied after all surfaces have seen the event.
    pub fn broadcast_app(&mut self, ev: Event) {
        #[cfg(feature = "lua")]
        if matches!(ev.name.as_str(), "Suspend" | "Resume" | "SaveState") {
            self.services.plugins.event(
                if ev.name == "SaveState" {
                    "FlushSettings"
                } else {
                    &ev.name
                },
                ev.args.clone(),
            );
        }
        self.broadcast(ev.clone());
        let mut outcomes = Vec::new();
        for index in 0..self.router.depth() {
            let outcome = {
                let mut env = env!(self);
                match self.router.surface_mut(index) {
                    Some(surface) => surface.on_event(&ev, &mut env),
                    None => Outcome::NotMine,
                }
            };
            outcomes.push((index, outcome));
        }
        for (index, outcome) in outcomes {
            self.apply(index, outcome);
        }
    }

    /// Runs the actions and events that windows queued, until none are left.
    /// Events a window did not use become commands with the same name.
    fn settle(&mut self) {
        for _ in 0..MAX_SETTLE_ROUNDS {
            let outs = self.collect_outboxes();
            if outs.is_empty() {
                return;
            }
            for out in outs {
                match out {
                    Out::Action(action) => self.command(&action.id, &action.args),
                    Out::Event(ev) => {
                        if !self.send(ev.clone()) {
                            self.command(&ev.name, &ev.args);
                        }
                    }
                }
            }
        }
    }

    /// The queued actions of the router's windows and of the transient windows.
    /// Transient windows are dropped once they are closed. The check waits until
    /// no window request is pending, so a window that is about to show is kept.
    fn collect_outboxes(&mut self) -> Vec<Out> {
        // Drain actions before removing windows that closed themselves in the same event.
        self.router.register(&mut self.transient);
        let outs = self.router.drain_outboxes();
        self.reconcile_windows();
        outs
    }

    /// Routes an action id through surfaces from top to bottom, then global commands. An id nobody knows is recorded.
    pub fn command(&mut self, id: &str, args: &[Value]) {
        #[cfg(not(feature = "lua"))]
        if id == "reader:plugins" {
            self.push(Box::new(
                surfaces::settings::section::SettingsSectionSurface::new("plugin-menus".into()),
            ));
            return;
        }
        #[cfg(feature = "lua")]
        if self.plugin_command(id, args) {
            return;
        }
        if self.reader_busy()
            && matches!(
                Command::parse(id),
                Some(Command::Navigate(_) | Command::OpenBook(_) | Command::Back | Command::Quit)
            )
        {
            self.pending_navigation = Some((id.to_string(), args.to_vec()));
            return;
        }
        // Airplane mode turns Wi-Fi off on the script worker. The quick settings
        // surface still gets the id and saves the switch.
        if (id == "settings:quick:airplane" && args.first().and_then(Value::as_bool) == Some(true))
            || (id == "settings:quick:wifi" && args.first().and_then(Value::as_bool) == Some(false))
        {
            self.services.webdav.update_gate(&self.services.settings, false);
        }
        if id == "settings:quick:airplane"
            && args.first().and_then(Value::as_bool) == Some(true)
            && self.services.platform.network.is_wifi_on()
        {
            self.wifi(false);
        }
        if self.device_command(id, args) {
            return;
        }
        match id {
            "resume" => {
                self.resume();
                return;
            }
            "settings:quick:wifi" => {
                if let Some(on) = args.first().and_then(Value::as_bool) {
                    // A sheet built before airplane mode went on still sends this id.
                    if on && crate::surfaces::settings::keys::airplane_mode(&self.services.settings) {
                        self.notices
                            .push(tr("Airplane mode is on. Turn it off to use Wi-Fi."));
                    } else {
                        self.wifi(on);
                    }
                }
                return;
            }
            "settings:quick:open" => {
                self.quick_settings();
                return;
            }
            "settings:quick:toggle-light" => {
                self.toggle_light();
                return;
            }
            _ => {}
        }
        if id.starts_with("status:") {
            if matches!(
                id,
                "status:power" | "status:light-settings" | "status:network-settings"
            ) {
                self.back();
            }
            let outcome = surfaces::status::command(id, args, &mut env!(self));
            if self.apply(0, outcome) {
                return;
            }
        }
        if id == "dialog:dismiss" {
            self.back();
            return;
        }
        // The prompt from `refuse_exit`. The user accepts losing the update. The next
        // exit check tries the write once more and exits without it if the write fails.
        if id == "sync:exit-anyway" {
            self.back();
            self.sync_drop_confirmed = true;
            self.quitting = true;
            return;
        }
        if id == "notes:export:text"
            || id == "notes:export:pdf"
            || id == "reader:acknowledge-change"
            || id == "reader:sync:accept"
            || id == "reader:ink:delete-confirm"
            || id.starts_with("library:collections:delete-confirm:")
            || id == "library:history:clear-confirm"
        {
            self.back();
        }
        if id.starts_with("settings:quick:") {
            let outcome = surfaces::settings::quick::command(id, args, &mut env!(self));
            if self.apply(0, outcome) {
                return;
            }
        }
        for index in (0..self.router.depth()).rev() {
            if self.route_command(index, id, args) {
                return;
            }
        }
        match Command::parse(id) {
            Some(Command::OpenBook(book)) => self.open_book(book, args.first().cloned()),
            Some(command) => self.global(command),
            None => self.unknown.push(id.to_string()),
        }
    }

    /// Asks the surface at `index` about a command. True when it took the command.
    fn route_command(&mut self, index: usize, id: &str, args: &[Value]) -> bool {
        let language = self.services.language_epoch;
        let outcome = {
            let mut env = env!(self);
            match self.router.surface_mut(index) {
                Some(surface) => surface.on_command(id, args, &mut env),
                None => Outcome::NotMine,
            }
        };
        if !matches!(outcome, Outcome::NotMine) {
            self.broadcast_app(Event::new("ReaderTick"));
        }
        let taken = self.apply(index, outcome);
        if self.services.language_epoch != language {
            self.rebuild_all();
        }
        taken
    }

    /// Builds every screen of the stack again, after the language changed. The top
    /// screen is swapped now. A covered one is marked and rebuilt when it becomes the top.
    fn rebuild_all(&mut self) {
        for index in 0..self.router.depth() {
            self.rebuild(index);
        }
    }

    /// Applies what a surface returned. False when the surface did not take the command.
    fn apply(&mut self, index: usize, outcome: Outcome) -> bool {
        match outcome {
            Outcome::NotMine => return false,
            Outcome::Handled => {}
            Outcome::Rebuild => self.rebuild(index),
            Outcome::Navigate(nav) => self.navigate(nav),
        }
        true
    }

    fn start_scan(&mut self) {
        if self.scan.is_some() {
            self.scan_again = true;
            return;
        }
        let roots = if self.options.library_roots.is_empty() {
            vec![self.services.paths.books_dir.clone()]
        } else {
            self.options.library_roots.clone()
        };
        self.scan = Some(LibraryScan::spawn(self.services.library_path(), roots));
    }

    fn quick_settings(&mut self) {
        let sheet = surfaces::settings::quick::quick_settings(&self.services);
        env!(self).sheet(sheet);
    }

    fn global(&mut self, command: Command) {
        match command {
            Command::Navigate(route) => self.switch_root_to(route, RefreshMode::Ui),
            Command::OpenBook(book) => self.open_book(book, None),
            Command::Back => self.back(),
            Command::Quit => self.quitting = true,
            Command::Sleep => self.sleep(),
            Command::Scan => self.start_scan(),
        }
    }

    fn navigate(&mut self, nav: Nav) {
        match nav {
            Nav::OpenBook { book, location } => self.open_book(BookRef::Id(book), location),
            Nav::Push(surface) => self.push(surface),
            Nav::Pop => self.pop(RefreshMode::Ui),
            Nav::SwitchRoot(route) => self.switch_root_to(route, RefreshMode::Ui),
            Nav::Replace(surface) => {
                if self.router.depth() < 2 {
                    self.switch_root(surface, RefreshMode::Ui);
                } else {
                    self.pop(RefreshMode::Ui);
                    self.push(surface);
                }
            }
        }
    }

    /// Makes the surface for `route` the root. Choosing the root that is already
    /// shown does nothing.
    fn switch_root_to(&mut self, route: Route, mode: RefreshMode) {
        if self.router.depth() == 1 && self.router.root_route() == Some(&route) {
            return;
        }
        if let Some(surface) = surfaces::create(&route) {
            self.switch_root(surface, mode);
        }
    }

    fn switch_root(&mut self, mut surface: Box<dyn Surface>, mode: RefreshMode) {
        if !self.can_hide_all() {
            return;
        }
        self.hide_all();
        let Some(window) = self.build_surface(&mut *surface) else {
            return;
        };
        self.router
            .switch_root(surface, window, mode, &mut self.stack, &mut self.ctx);
        self.uncover_top();
        self.reveal_top();
    }

    /// Shows `surface` above the top screen. A surface that covers the screen
    /// repaints the frame under it.
    fn push(&mut self, mut surface: Box<dyn Surface>) {
        let opts = surface.show_opts();
        let Some(window) = self.build_surface(&mut *surface) else {
            return;
        };
        self.router
            .push(surface, window, opts, &mut self.stack, &mut self.ctx);
        if opts.covers_fullscreen {
            self.uncover_top();
        }
        self.reveal_top();
    }

    /// Closes the top screen, which runs its `on_hide` first. Does nothing on the root.
    fn pop(&mut self, mode: RefreshMode) {
        let Some(top) = self.router.depth().checked_sub(1).filter(|&top| top > 0) else {
            return;
        };
        if !self.can_hide(top) {
            return;
        }
        let reader_closed = matches!(self.router.top_route(), Some(Route::Reader { .. }));
        self.hide(top);
        self.router.pop(mode, &mut self.stack, &mut self.ctx);
        if reader_closed {
            self.broadcast_app(Event::new("LibraryUpdated"));
        }
        self.reveal_top();
    }

    /// `back`: closes the top pushed screen (the reader saves its progress on the
    /// way out). From a root that is not Home, it goes to Home.
    fn back(&mut self) {
        // Back does not close the PIN pad. Only the right PIN does. Power off still
        // closes the app, since the exit check does not go through this path.
        if self.top_route().is_some_and(surfaces::pin::is_lock_route) {
            return;
        }
        if self.reader_busy() && matches!(self.top_route(), Some(Route::Reader { .. })) {
            self.pending_navigation = Some(("back".to_string(), Vec::new()));
            return;
        }
        if let Some(id) = self.input_window().and_then(|w| w.id()) {
            if self.router.top_window().and_then(|w| w.id()) != Some(id) {
                self.stack.close(id, None, &mut self.ctx);
                self.reconcile_windows();
                return;
            }
        }
        if self.router.depth() > 0 && self.route_command(self.router.depth() - 1, "surface:back", &[]) {
            return;
        }
        match self.router.depth() {
            0 => {}
            1 => {
                if self.router.root_route() != Some(&Route::Home) {
                    self.switch_root_to(Route::Home, RefreshMode::Ui);
                }
            }
            _ => self.pop(RefreshMode::Ui),
        }
    }

    fn open_book(&mut self, book: BookRef, location: Option<Value>) {
        let found = match &book {
            BookRef::Id(id) => self.services.library.book(*id),
            BookRef::Path(path) => self.services.library.book_by_path(path),
        };
        match found {
            Ok(Some(row)) => {
                self.prepare_sleep_picture(row.id);
                let existing = self.router.routes().iter().position(|route| match route {
                    Route::Reader { book } => {
                        *book == row.id
                            || self
                                .services
                                .library
                                .book(*book)
                                .ok()
                                .flatten()
                                .is_some_and(|old| old.partial_md5 == row.partial_md5)
                    }
                    _ => false,
                });
                if let Some(index) = existing {
                    while self.router.depth() > index + 1 {
                        self.pop(RefreshMode::Ui);
                    }
                    if let Some(location) = location {
                        self.route_command(index, "reader:location", &[location]);
                    }
                } else {
                    self.push(Box::new(surfaces::reader::ReaderSurface::at(row.id, location)));
                }
            }
            Ok(None) => self.notices.push(format!("no book for {book:?}")),
            Err(e) => self.notices.push(format!("cannot read the library: {e}")),
        }
    }

    /// Builds the surface at `index` again. The top screen is swapped at once. A
    /// covered one is marked and rebuilt when it becomes the top screen.
    fn rebuild(&mut self, index: usize) {
        if index + 1 != self.router.depth() {
            self.router.mark_stale(index);
            return;
        }
        let window = {
            let mut env = env!(self);
            self.router
                .surface_mut(index)
                .and_then(|surface| surface.build(&mut env))
        };
        let Some(window) = window else {
            return;
        };
        let pending = self.ctx.dirty.clone();
        let before = self.frame.clone();
        self.router
            .swap_top_window(window, RefreshMode::Ui, &mut self.stack, &mut self.ctx);
        self.frame.fill_rect(
            Rect::from_size(self.screen),
            self.services.theme.color(Tone::Surface),
        );
        let windows = Window::visible_stack(&self.stack, |id| self.app_window(id));
        for window in &windows {
            window.invalidate();
        }
        Window::paint_stack(&windows, &mut self.frame, &mut self.ctx.dirty);
        self.ctx.dirty = pending;
        if let Some(region) = changed_region(&before, &self.frame) {
            self.ctx.dirty.refresh(RefreshMode::Ui, Some(region), false);
        }
    }

    /// Builds `surface` for the screen. None when it has nothing to show.
    fn build_surface(&mut self, surface: &mut dyn Surface) -> Option<Window> {
        let mut env = env!(self);
        surface.build(&mut env)
    }

    fn can_hide(&mut self, index: usize) -> bool {
        let mut env = env!(self);
        self.router
            .surface_mut(index)
            .is_none_or(|surface| surface.can_hide(&mut env))
    }

    fn can_hide_all(&mut self) -> bool {
        (0..self.router.depth()).rev().all(|index| self.can_hide(index))
    }

    /// Runs `on_hide` on the screen at `index`.
    fn hide(&mut self, index: usize) {
        let mut env = env!(self);
        if let Some(surface) = self.router.surface_mut(index) {
            surface.on_hide(&mut env);
        }
    }

    /// Runs `on_hide` on every screen, top first, before the stack is replaced.
    fn hide_all(&mut self) {
        for index in (0..self.router.depth()).rev() {
            self.hide(index);
        }
    }

    /// Runs `on_show` on the top screen. A screen that went stale while it was
    /// covered is rebuilt now.
    fn reveal_top(&mut self) {
        #[cfg(feature = "lua")]
        {
            self.services.plugins.active_book =
                self.router.routes().iter().rev().find_map(|route| match route {
                    Route::Reader { book } => Some(*book),
                    _ => None,
                });
        }
        let Some(top) = self.router.depth().checked_sub(1) else {
            return;
        };
        {
            let mut env = env!(self);
            if let Some(surface) = self.router.surface_mut(top) {
                surface.on_show(&mut env);
            }
        }
        if self.router.take_stale(top) {
            self.rebuild(top);
        }
    }

    /// Makes the top window repaint everything on a clean frame. Widgets only paint
    /// their own bounds, so the frame needs the surface color first, or the pixels
    /// of the screen that was there before would show through. Use after a
    /// fullscreen window is shown or when one is uncovered.
    fn uncover_top(&mut self) {
        let surface = self.services.theme.color(Tone::Surface);
        self.frame.fill_rect(Rect::from_size(self.screen), surface);
        if let Some(window) = self.input_window() {
            window.invalidate();
        }
    }

    /// Turns the scan thread's messages into events. Progress and the end of the
    /// scan become `LibraryScanProgress`, `LibraryUpdated` and `LibraryScanFailed`.
    /// The surfaces that answer `LibraryUpdated` with `Rebuild` refresh themselves.
    fn pump_scan(&mut self) {
        loop {
            let Some(message) = self.scan.as_ref().and_then(|s| s.try_next()) else {
                return;
            };
            match message {
                ScanMessage::Progress { done, total } => {
                    self.scan_progress = Some((done, total));
                    self.broadcast_app(Event::with_args(
                        "LibraryScanProgress",
                        vec![Value::Number(done as f64), Value::Number(total as f64)],
                    ));
                }
                ScanMessage::Finished(result) => {
                    self.scan = None;
                    self.scan_progress = None;
                    if std::mem::take(&mut self.scan_again) {
                        self.start_scan();
                    }
                    match result {
                        Ok(report) => {
                            self.broadcast_app(Event::with_args(
                                "LibraryUpdated",
                                vec![
                                    Value::Number(report.added as f64),
                                    Value::Number(report.changed as f64),
                                    Value::Number(report.removed as f64),
                                ],
                            ));
                        }
                        Err(message) => {
                            self.notices.push(format!("library scan failed: {message}"));
                            self.broadcast_app(Event::with_args(
                                "LibraryScanFailed",
                                vec![Value::Str(message)],
                            ));
                        }
                    }
                    return;
                }
            }
        }
    }

    /// The waveform for the reader's partial refreshes, from the page turn setting.
    /// None on other screens, which keep the table's waveform.
    fn reader_partial_waveform(&self) -> Option<Waveform> {
        if !matches!(self.router.top_route(), Some(Route::Reader { .. })) {
            return None;
        }
        Some(surfaces::settings::keys::page_turn(&self.services.settings).waveform())
    }

    /// Paints the dirty windows bottom up into the frame and presents each refresh.
    /// Returns the number of refreshes presented.
    fn repaint(&mut self) -> Result<usize, DisplayError> {
        #[cfg(feature = "lua")]
        self.pump_plugins();
        let night = self.services.settings.is_true("display.night_mode");
        if self.presented_night != Some(night) {
            self.backend.set_night_mode(night);
            self.presented_night = Some(night);
            self.ctx.dirty.refresh(RefreshMode::Full, None, false);
        }
        let color = self.services.theme.color_panel
            && self
                .services
                .settings
                .read("display.color_rendering")
                .as_bool()
                .unwrap_or(true);
        if self.presented_color.is_some_and(|previous| previous != color) {
            self.ctx.dirty.refresh(RefreshMode::Full, None, false);
        }
        self.router.register(&mut self.transient);
        self.stack.apply_requests(&mut self.ctx);
        let plan = self.stack.paint_plan(&mut self.ctx.dirty);
        let painted = if plan.is_empty() {
            false
        } else {
            let windows = Window::visible_stack(&self.stack, |id| self.app_window(id));
            #[cfg(feature = "lua")]
            if self.services.plugins.running() {
                for window in &windows {
                    window.invalidate();
                }
            }
            // Header and footer status follow an existing redraw and never schedule one.
            let battery = self.services.platform.battery.capacity();
            for window in &windows {
                window.update::<crate::surfaces::reader::page::PageView, _>(
                    &mut self.ctx,
                    crate::surfaces::reader::page::PAGE_KEY,
                    |view, _| view.footer_battery = battery,
                );
            }
            Window::paint_stack(&windows, &mut self.frame, &mut self.ctx.dirty)
        };
        #[cfg(feature = "lua")]
        if let Some(ui) = self.services.plugins.ui.clone() {
            ui.borrow_mut()
                .in_app(&mut self.stack, &mut self.ctx, |lua| lua.after_app_paint());
        }
        let refreshes = self.ctx.dirty.take_frame(painted);
        let page_turn = self.reader_partial_waveform();
        let mut sent = 0;
        for refresh in refreshes {
            // Surfaces say which refreshes dither their pictures (covers, pages). Plain UI is not dithered.
            let content_dither = self
                .router
                .top_surface()
                .is_some_and(|surface| surface.content_dither(refresh.mode));
            let Some(mut panel) = refresh::to_panel(&refresh, self.screen, color, content_dither) else {
                continue;
            };
            // The reader's partial refreshes take the page turn choice. That covers page turns
            // and the small repaints for a selection or a bookmark. Full refreshes and the
            // other modes keep the waveform of the table.
            if let (RefreshMode::Partial, Some(waveform)) = (refresh.mode, page_turn) {
                panel.waveform = waveform;
            }
            if color || self.frame.format() != kaia_gfx::PixelFormat::Rgb24 {
                self.backend.present(&self.frame, &panel)?;
            } else {
                let mut gray = self.frame.clone();
                for pixel in gray.data_mut().as_chunks_mut::<3>().0 {
                    let luma =
                        ((u32::from(pixel[0]) * 77 + u32::from(pixel[1]) * 150 + u32::from(pixel[2]) * 29)
                            >> 8) as u8;
                    pixel.fill(luma);
                }
                self.backend.present(&gray, &panel.with_dither(false))?;
            }
            self.modes.push(refresh.mode);
            self.stats.refreshes += 1;
            sent += 1;
        }
        if sent > 0 {
            self.presented_color = Some(color);
            self.stats.frames += 1;
        }
        Ok(sent)
    }

    /// The time the gesture recognizer and the scheduler share, in microseconds.
    /// Input timestamps can run ahead of the app clock, so the larger one wins.
    /// Now in the input clock: the scheduler time shifted by the offset seen at
    /// the latest touch. It keeps advancing while a finger rests, so a hold fires
    /// after its interval instead of at lift.
    fn gesture_now_us(&self) -> u64 {
        let clock = self.ctx.scheduler.now().as_micros() as i64;
        let now = clock.saturating_add(self.input_clock_offset_us).max(0) as u64;
        now.max(self.last_input_us)
    }

    /// How long the backend may wait: the earliest of the timeout the caller gave,
    /// the next scheduled task, the next gesture timer, the exit time, and the scan
    /// poll interval while a scan runs.
    fn wait_budget(&self, timeout: Option<Duration>) -> Option<Duration> {
        if self.pending_rotation.is_some() && !self.sleeping() && !self.reader_busy() {
            return Some(Duration::ZERO);
        }
        let now = self.ctx.scheduler.now();
        let mut budget = min_opt(timeout, self.power_wait());
        if !self.sleeping() && matches!(self.top_route(), Some(Route::Home | Route::More)) {
            let until_minute = 60_000 - chrono::Local::now().timestamp_millis().rem_euclid(60_000);
            budget = min_opt(budget, Duration::from_millis(until_minute as u64));
        }
        budget = min_opt(budget, self.natural_light.wait(now));
        if let Some(wait) = self.services.webdav.wait(&self.services.settings) {
            budget = min_opt(budget, wait);
        }
        if let Some(due) = self.ctx.scheduler.next_due() {
            budget = min_opt(budget, due.saturating_sub(now));
        }
        if let Some(deadline) = self.gestures.next_deadline() {
            let until = deadline.saturating_sub(self.gesture_now_us());
            budget = min_opt(budget, Duration::from_micros(until));
        }
        if let Some(exit) = self.exit_at {
            budget = min_opt(budget, exit.saturating_sub(now));
        }
        if self.scan.is_some()
            || self.services.sync.busy()
            || self.services.webdav.busy()
            || self.services.dictionary_downloads.busy()
            || crate::covers::busy()
            || self.power.scripts.busy()
            || self.pending_navigation.is_some()
        {
            budget = min_opt(budget, SCAN_POLL);
        }
        budget
    }

    fn exit_due(&self) -> bool {
        self.exit_at.is_some_and(|at| self.ctx.scheduler.now() >= at)
    }
}

fn min_opt(a: Option<Duration>, b: Duration) -> Option<Duration> {
    Some(a.map_or(b, |a| a.min(b)))
}

/// Writes the frame as a PNG. Used by the binary for `--screenshot`.
pub fn save_frame(frame: &Pixmap, path: &std::path::Path) -> io::Result<()> {
    kaia_gfx::png::write_pixmap(frame, path).map_err(|e| io::Error::other(e.to_string()))
}

/// Bounds of pixels changed by a rebuilt screen.
fn changed_region(before: &Pixmap, after: &Pixmap) -> Option<EventRect> {
    let mut left = after.width();
    let mut top = after.height();
    let mut right = 0;
    let mut bottom = 0;
    let bytes = after.stride() / after.width() as usize;
    for (y, (a, b)) in before
        .data()
        .chunks(before.stride())
        .zip(after.data().chunks(after.stride()))
        .enumerate()
    {
        if a == b {
            continue;
        }
        for (x, (a, b)) in a.chunks(bytes).zip(b.chunks(bytes)).enumerate() {
            if a != b {
                left = left.min(x as u32);
                right = right.max(x as u32 + 1);
                top = top.min(y as u32);
                bottom = bottom.max(y as u32 + 1);
            }
        }
    }
    (left < right).then(|| {
        EventRect::new(
            left as i32,
            top as i32,
            (right - left) as i32,
            (bottom - top) as i32,
        )
    })
}
