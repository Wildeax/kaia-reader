//! The main loop. It follows the "App loop contract" in `docs/kaia/specs/UI_TOOLKIT.md`:
//!
//! 1. Wait for input. The wait ends at the next scheduled task, the next gesture
//!    timer, the exit time, or when the scan thread has news.
//! 2. Feed touches through the gesture recognizer and dispatch gestures and keys
//!    to the top window. Unhandled input may close an overlay or go Back.
//! 3. Run the actions and events the windows queued. Each action goes to the top
//!    surface, then the root surface, then the global commands.
//! 4. Paint the dirty windows into one frame, bottom up, and present each refresh.
//! 5. Run the due tasks (button releases among them), apply the window requests
//!    they made, and paint and present again.
//!
//! This file knows no surface. It asks the surfaces in [`crate::surfaces`] for
//! their windows and applies the [`Outcome`]s they return.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use kaia_display::{DisplayError, InputEvent};
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

/// Counters for tests and for the desktop run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Frames that reached the display: a paint that produced at least one refresh.
    pub frames: u64,
    /// Refreshes sent to the display.
    pub refreshes: u64,
}

/// What the loop does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Exit,
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
    services: Services,
    ctx: Ctx,
    stack: WindowStack,
    router: Router,
    frame: Pixmap,
    screen: Size,
    gestures: GestureDetector,
    options: AppOptions,
    scan: Option<LibraryScan>,
    scan_progress: Option<(usize, usize)>,
    stats: FrameStats,
    modes: Vec<RefreshMode>,
    unknown: Vec<String>,
    notices: Vec<String>,
    /// Windows that surfaces showed outside the router (dialogs, sheets). Their
    /// actions are collected with the router's.
    transient: Vec<Window>,
    last_input_us: u64,
    exit_at: Option<Duration>,
    quitting: bool,
    /// Set by `KAIA_TRACE_INPUT`. Logs every gesture and the top route to stderr.
    trace_input: bool,
}

impl<B: Backend> App<B> {
    /// Builds the app on `backend`. Call [`App::start`] to show Home.
    pub fn new(backend: B, services: Services, clock: Box<dyn Clock>, options: AppOptions) -> Self {
        let screen = backend.size();
        let format = services.theme.pixel_format();
        let mut frame = Pixmap::new(screen.w, screen.h, format);
        frame.fill_rect(Rect::from_size(screen), services.theme.color(Tone::Surface));
        let mut ctx = Ctx::new(clock, EventRect::new(0, 0, screen.w as i32, screen.h as i32));
        ctx.dirty.configure_from(&services.settings);
        let gestures = GestureDetector::new(GestureConfig {
            dpi: backend.dpi(),
            ..GestureConfig::default()
        });
        App {
            backend,
            services,
            ctx,
            stack: WindowStack::new(),
            router: Router::new(),
            frame,
            screen,
            gestures,
            options,
            scan: None,
            scan_progress: None,
            stats: FrameStats::default(),
            modes: Vec::new(),
            unknown: Vec::new(),
            notices: Vec::new(),
            transient: Vec::new(),
            last_input_us: 0,
            exit_at: None,
            quitting: false,
            trace_input: std::env::var_os("KAIA_TRACE_INPUT").is_some(),
        }
    }

    /// Shows Home with a full refresh, starts the library scan, and paints the first frame.
    pub fn start(&mut self) -> Result<(), DisplayError> {
        self.switch_root_to(Route::Home, RefreshMode::Full);
        if !self.options.library_roots.is_empty() {
            let db = self.services.library_path();
            self.scan = Some(LibraryScan::spawn(db, self.options.library_roots.clone()));
        }
        self.step(Vec::new()).map(|_| ())
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
    /// time set by [`App::exit_after`] passes. Then it saves the settings.
    pub fn run(&mut self) -> Result<(), DisplayError> {
        loop {
            if self.iterate(None)? == Flow::Exit || self.quitting {
                break;
            }
            if self.exit_due() {
                break;
            }
        }
        self.shutdown();
        Ok(())
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

    /// Presses a key, then runs one iteration.
    pub fn key(&mut self, key: Key) -> Result<Flow, DisplayError> {
        let ev = KeyEvent {
            key,
            pressed: true,
            time_us: self.gesture_now_us(),
        };
        self.process(vec![InputEvent::Key(ev)])
    }

    /// Stops the loop after the current iteration.
    pub fn quit(&mut self) {
        self.quitting = true;
    }

    /// Stops [`App::run`] once `d` has passed on the app clock.
    pub fn exit_after(&mut self, d: Duration) {
        self.exit_at = Some(self.ctx.scheduler.now() + d);
    }

    /// Saves the settings and stops the library scan. [`App::run`] calls it on exit.
    pub fn shutdown(&mut self) {
        if let Some(scan) = self.scan.take() {
            scan.cancel();
        }
        if let Err(e) = self.services.settings.flush(&self.services.settings_path()) {
            eprintln!("kaia: cannot save settings: {e}");
        }
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

    pub fn scan_running(&self) -> bool {
        self.scan.is_some()
    }

    /// The absolute bounds of the widget with `key` on the top window, from its last layout.
    pub fn find_rect(&self, key: &str) -> Option<Rect> {
        self.router.top_window()?.find_rect(key)
    }

    /// Runs `f` on the widget with `key` on the top window. Returns false when the
    /// widget is missing or has another type.
    pub fn update_top<T, F>(&mut self, key: &str, f: F) -> bool
    where
        T: kaia_ui::Widget + 'static,
        F: FnOnce(&mut T, &mut kaia_ui::UpdateCx<'_>),
    {
        match self.router.top_window() {
            Some(window) => window.update::<T, F>(&mut self.ctx, key, f),
            None => false,
        }
    }

    /// The loop body after input: the touches, keys and events, gestures, actions,
    /// paint, tasks and paint again.
    fn step(&mut self, input: Vec<InputEvent>) -> Result<Flow, DisplayError> {
        self.pump_scan();
        // Each event is dispatched, painted and settled before the next one, so a
        // later event sees the screen that the earlier one produced.
        for event in input {
            match event {
                InputEvent::Touch(t) => self.touch(t),
                InputEvent::Key(k) => self.key_event(k),
                InputEvent::Quit => self.quitting = true,
                InputEvent::Resize(_) => {}
            }
            self.show_input_result()?;
        }
        self.poll_gestures();
        self.show_input_result()?;

        // Tasks due now (button releases, for one) run here. Their paint goes out
        // in the same iteration.
        let now = self.ctx.scheduler.now();
        self.ctx.run_due(now);
        self.stack.apply_requests(&mut self.ctx);
        self.settle();
        self.repaint()?;

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
        self.last_input_us = self.last_input_us.max(t.time_us);
        for gesture in self.gestures.feed(t) {
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
        let Some(top) = self.router.top_window() else {
            return;
        };
        let handled = top.dispatch(UiEvent::Gesture(gesture.clone()), &mut self.ctx) == Handled::Yes;
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

    fn key_event(&mut self, ev: KeyEvent) {
        if !ev.pressed {
            return;
        }
        match ev.key {
            Key::LPgFwd | Key::RPgFwd => {
                self.send(Event::new("PageForward"));
            }
            Key::LPgBack | Key::RPgBack => {
                self.send(Event::new("PageBack"));
            }
            key => {
                let Some(top) = self.router.top_window() else {
                    return;
                };
                let handled = top.dispatch(UiEvent::Key(ev), &mut self.ctx) == Handled::Yes;
                if !handled && key == Key::Back {
                    self.back();
                }
            }
        }
    }

    /// Sends a named event through the window stack. Returns true when a window used it.
    fn send(&mut self, ev: Event) -> bool {
        self.stack.send(&ev, &mut self.ctx)
    }

    fn broadcast(&mut self, ev: Event) {
        self.stack.broadcast(&ev, &mut self.ctx);
    }

    /// Delivers an app event to the windows and to every surface, bottom up. The
    /// outcomes are applied after all surfaces have seen the event.
    fn broadcast_app(&mut self, ev: Event) {
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
        if self.ctx.pending_window_requests() == 0 {
            let stack = &self.stack;
            self.transient
                .retain(|w| w.id().is_none_or(|id| stack.contains(id)));
        }
        let mut outs = self.router.drain_outboxes();
        for window in &self.transient {
            outs.extend(window.drain_outbox());
        }
        outs
    }

    /// Routes an action id: the top surface first, then the root, then the global
    /// commands. An id nobody knows is recorded.
    fn command(&mut self, id: &str, args: &[Value]) {
        let depth = self.router.depth();
        if let Some(top) = depth.checked_sub(1) {
            if self.route_command(top, id, args) {
                return;
            }
        }
        if depth > 1 && self.route_command(0, id, args) {
            return;
        }
        match Command::parse(id) {
            Some(command) => self.global(command),
            None => self.unknown.push(id.to_string()),
        }
    }

    /// Asks the surface at `index` about a command. True when it took the command.
    fn route_command(&mut self, index: usize, id: &str, args: &[Value]) -> bool {
        let outcome = {
            let mut env = env!(self);
            match self.router.surface_mut(index) {
                Some(surface) => surface.on_command(id, args, &mut env),
                None => Outcome::NotMine,
            }
        };
        self.apply(index, outcome)
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

    fn global(&mut self, command: Command) {
        match command {
            Command::Navigate(route) => self.switch_root_to(route, RefreshMode::Ui),
            Command::OpenBook(book) => self.open_book(book),
            Command::Back => self.back(),
            Command::Quit => self.quitting = true,
        }
    }

    fn navigate(&mut self, nav: Nav) {
        match nav {
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
        let Some(window) = self.build_surface(&mut *surface) else {
            return;
        };
        self.hide_all();
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
        self.hide(top);
        self.router.pop(mode, &mut self.stack, &mut self.ctx);
        self.uncover_top();
        self.reveal_top();
    }

    /// `back`: closes the top pushed screen (the reader saves its progress on the
    /// way out). From a root that is not Home, it goes to Home.
    fn back(&mut self) {
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

    fn open_book(&mut self, book: BookRef) {
        let found = match &book {
            BookRef::Id(id) => self.services.library.book(*id),
            BookRef::Path(path) => self.services.library.book_by_path(path),
        };
        match found {
            Ok(Some(row)) => {
                if let Some(surface) = surfaces::create(&Route::Reader { book: row.id }) {
                    self.push(surface);
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
        self.router
            .swap_top_window(window, RefreshMode::Ui, &mut self.stack, &mut self.ctx);
        self.uncover_top();
    }

    /// Builds `surface` for the screen. None when it has nothing to show.
    fn build_surface(&mut self, surface: &mut dyn Surface) -> Option<Window> {
        let mut env = env!(self);
        surface.build(&mut env)
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
        if let Some(window) = self.router.top_window() {
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

    /// Paints the dirty windows bottom up into the frame and presents each refresh.
    /// Returns the number of refreshes presented.
    fn repaint(&mut self) -> Result<usize, DisplayError> {
        let plan = self.stack.paint_plan(&mut self.ctx.dirty);
        let mut painted = false;
        for id in plan {
            if let Some(window) = self.router.window(id) {
                painted |= window.paint(&mut self.frame, &mut self.ctx.dirty);
            }
        }
        let refreshes = self.ctx.dirty.take_frame(painted);
        let color = self.services.theme.color_panel;
        let mut sent = 0;
        for refresh in refreshes {
            // Surfaces say which refreshes dither their pictures (covers, pages). Plain UI is not dithered.
            let content_dither = self
                .router
                .top_surface()
                .is_some_and(|surface| surface.content_dither(refresh.mode));
            let Some(panel) = refresh::to_panel(&refresh, self.screen, color, content_dither) else {
                continue;
            };
            self.backend.present(&self.frame, &panel)?;
            self.modes.push(refresh.mode);
            self.stats.refreshes += 1;
            sent += 1;
        }
        if sent > 0 {
            self.stats.frames += 1;
        }
        Ok(sent)
    }

    /// The time the gesture recognizer and the scheduler share, in microseconds.
    /// Input timestamps can run ahead of the app clock, so the larger one wins.
    fn gesture_now_us(&self) -> u64 {
        let clock = self.ctx.scheduler.now().as_micros() as u64;
        clock.max(self.last_input_us)
    }

    /// How long the backend may wait: the earliest of the timeout the caller gave,
    /// the next scheduled task, the next gesture timer, the exit time, and the scan
    /// poll interval while a scan runs.
    fn wait_budget(&self, timeout: Option<Duration>) -> Option<Duration> {
        let now = self.ctx.scheduler.now();
        let mut budget = timeout;
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
        if self.scan.is_some() {
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
