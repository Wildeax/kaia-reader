//! The main loop. It follows the "App loop contract" in `docs/kaia/specs/UI_TOOLKIT.md`:
//!
//! 1. Wait for input. The wait ends at the next scheduled task, the next gesture
//!    timer, the exit time, or when the scan thread has news.
//! 2. Feed touches through the gesture recognizer and dispatch gestures and keys
//!    to the top window. Unhandled input may close an overlay or go Back.
//! 3. Run the actions and events the windows queued.
//! 4. Paint the dirty windows into one frame, bottom up, and present each refresh.
//! 5. Run the due tasks (button releases among them), apply the window requests
//!    they made, and paint and present again.

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use kaia_display::{DisplayError, InputEvent};
use kaia_events::{Clock, Ctx, Event, Rect as EventRect, Refresh, RefreshMode, ShowOpts, Value, WindowStack};
use kaia_gfx::{Pixmap, Point, Rect, Size};
use kaia_input::{Direction, Gesture, GestureConfig, GestureDetector, Key, KeyEvent, Phase, TouchEvent};
use kaia_ui::widget::{Handled, UiEvent};
use kaia_ui::{Out, Tone, Window};

use crate::backend::Backend;
use crate::command::{BookRef, Command, PageStep};
use crate::refresh;
use crate::router::{Route, Router};
use crate::scan::{LibraryScan, ScanMessage};
use crate::services::Services;
use crate::surfaces;

/// The longest the loop waits while a library scan runs, so it can show progress.
const SCAN_POLL: Duration = Duration::from_millis(50);

/// Upper bound on rounds of action handling per iteration. A widget that keeps
/// queuing actions cannot stall the loop.
const MAX_SETTLE_ROUNDS: usize = 16;

/// Time between the press and the release of a synthetic tap, for [`App::tap`].
const TAP_HOLD: Duration = Duration::from_millis(50);

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
    /// Set when the library changed while a pushed screen was on top. The root is
    /// rebuilt when the stack returns to it.
    library_stale: bool,
    stats: FrameStats,
    modes: Vec<RefreshMode>,
    unknown: Vec<String>,
    notices: Vec<String>,
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
            library_stale: false,
            stats: FrameStats::default(),
            modes: Vec::new(),
            unknown: Vec::new(),
            notices: Vec::new(),
            last_input_us: 0,
            exit_at: None,
            quitting: false,
            trace_input: std::env::var_os("KAIA_TRACE_INPUT").is_some(),
        }
    }

    /// Shows Home with a full refresh, starts the library scan, and paints the first frame.
    pub fn start(&mut self) -> Result<(), DisplayError> {
        let home = surfaces::home::build(&self.services, self.screen);
        self.router.switch_root(
            Route::Home,
            home,
            RefreshMode::Full,
            &mut self.stack,
            &mut self.ctx,
        );
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

    /// Action ids that the app did not recognize.
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
        // Taps that the overlay does not use close it. A swipe from the left edge goes Back.
        let edge = self.services.theme.scale.dp(40) as i32;
        let overlay_dismissed =
            matches!(self.router.top_route(), Some(Route::Overlay)) && matches!(gesture, Gesture::Tap { .. });
        let edge_back = matches!(
            &gesture,
            Gesture::Swipe { pos, direction: Some(Direction::East), .. } if pos.x < edge
        );
        if overlay_dismissed || edge_back {
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

    /// Runs the actions and events that windows queued, until none are left.
    /// Events a window did not use become commands with the same name.
    fn settle(&mut self) {
        for _ in 0..MAX_SETTLE_ROUNDS {
            let outs = self.router.drain_outboxes();
            if outs.is_empty() {
                return;
            }
            for out in outs {
                match out {
                    Out::Action(action) => self.command(&action.id),
                    Out::Event(ev) => {
                        if !self.send(ev.clone()) {
                            self.command(&ev.name);
                        }
                    }
                }
            }
        }
    }

    fn command(&mut self, id: &str) {
        match Command::parse(id) {
            Some(Command::Navigate(route)) => self.navigate(route),
            Some(Command::OpenBook(book)) => self.open_book(book),
            Some(Command::Back) => self.back(),
            Some(Command::LibraryPage(step)) => self.turn_library_page(step),
            Some(Command::ReaderOverlay) => self.show_overlay(),
            Some(Command::BookDetails(id)) => self.show_details(id),
            Some(Command::SettingsSection(name)) => self.show_section(name),
            Some(Command::Quit) => self.quitting = true,
            None => self.unknown.push(id.to_string()),
        }
    }

    /// `nav:*`: replaces the root. Choosing the root that is already shown does nothing.
    fn navigate(&mut self, route: Route) {
        if self.router.depth() == 1 && self.router.root_route() == Some(&route) {
            return;
        }
        self.show_root(route, RefreshMode::Ui);
    }

    fn show_root(&mut self, route: Route, mode: RefreshMode) {
        let Some(window) = self.build_window(&route) else {
            return;
        };
        self.router
            .switch_root(route, window, mode, &mut self.stack, &mut self.ctx);
        self.library_stale = false;
        self.uncover_top();
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

    fn open_book(&mut self, book: BookRef) {
        let found = match &book {
            BookRef::Id(id) => self.services.library.book(*id),
            BookRef::Path(path) => self.services.library.book_by_path(path),
        };
        match found {
            Ok(Some(row)) => self.push_reader(row.id),
            Ok(None) => self.notices.push(format!("no book for {book:?}")),
            Err(e) => self.notices.push(format!("cannot read the library: {e}")),
        }
    }

    fn push_reader(&mut self, book_id: i64) {
        let Some(window) = self.build_window(&Route::Reader { book: book_id }) else {
            return;
        };
        let opts = ShowOpts {
            covers_fullscreen: true,
            refresh: Some(Refresh::new(RefreshMode::Full)),
            ..ShowOpts::default()
        };
        self.router.push(
            Route::Reader { book: book_id },
            window,
            opts,
            &mut self.stack,
            &mut self.ctx,
        );
        self.uncover_top();
    }

    /// `back`: closes the overlay, or the reader (saving its progress), or the top
    /// pushed screen. From a root that is not Home, it goes to Home.
    fn back(&mut self) {
        match self.router.top_route().cloned() {
            None => return,
            Some(Route::Reader { book }) => self.save_progress(book),
            Some(_) => {}
        }
        if self.router.depth() > 1 {
            self.router.pop(RefreshMode::Ui, &mut self.stack, &mut self.ctx);
            self.uncover_top();
            if self.router.depth() == 1 && std::mem::take(&mut self.library_stale) {
                self.refresh_root();
            }
        } else if self.router.root_route() != Some(&Route::Home) {
            self.show_root(Route::Home, RefreshMode::Ui);
        }
    }

    /// Stores the reader's progress in the library. The library API has no
    /// progress-only call, so this uses `record_open`, which also sets the
    /// last-opened time.
    fn save_progress(&mut self, book_id: i64) {
        let mut progress = None;
        self.update_top::<surfaces::reader::ReaderPage, _>(surfaces::reader::READER_KEY, |reader, _| {
            progress = Some(reader.progress());
        });
        if let Some(percent) = progress {
            if let Err(e) = self.services.library.record_open(book_id, percent) {
                self.notices.push(format!("cannot save progress: {e}"));
            }
        }
    }

    fn turn_library_page(&mut self, step: PageStep) {
        let Some(Route::Library { page }) = self.router.top_route().cloned() else {
            return;
        };
        let next = match step {
            PageStep::Next => page + 1,
            PageStep::Previous => page.saturating_sub(1),
        };
        if next != page {
            self.show_root(Route::Library { page: next }, RefreshMode::Ui);
        }
    }

    fn show_overlay(&mut self) {
        if !matches!(self.router.top_route(), Some(Route::Reader { .. })) {
            return;
        }
        let mut state = None;
        self.update_top::<surfaces::reader::ReaderPage, _>(surfaces::reader::READER_KEY, |reader, _| {
            state = Some((reader.title().to_string(), reader.page(), reader.page_count()));
        });
        let Some((title, page, count)) = state else {
            return;
        };
        let window = surfaces::reader::overlay(&self.services, self.screen, &title, page, count);
        let opts = ShowOpts {
            covers_fullscreen: false,
            refresh: Some(Refresh::new(RefreshMode::Ui)),
            ..ShowOpts::default()
        };
        self.router
            .push(Route::Overlay, window, opts, &mut self.stack, &mut self.ctx);
    }

    fn show_details(&mut self, book_id: i64) {
        let book = match self.services.library.book(book_id) {
            Ok(Some(book)) => book,
            _ => {
                self.notices.push(format!("no book {book_id}"));
                return;
            }
        };
        let window = surfaces::placeholder::book_details(&self.services, self.screen, &book);
        self.push_plain(Route::BookDetails { book: book_id }, window);
    }

    fn show_section(&mut self, name: String) {
        let window = surfaces::placeholder::settings_section(&self.services, self.screen, &name);
        self.push_plain(Route::SettingsSection { name }, window);
    }

    /// Pushes a full-screen placeholder with the ordinary refresh for a page change.
    fn push_plain(&mut self, route: Route, window: Window) {
        let opts = ShowOpts {
            covers_fullscreen: true,
            refresh: Some(Refresh::new(RefreshMode::Ui)),
            ..ShowOpts::default()
        };
        self.router
            .push(route, window, opts, &mut self.stack, &mut self.ctx);
        self.uncover_top();
    }

    /// Builds the window for a route. None when the route has no data behind it.
    fn build_window(&mut self, route: &Route) -> Option<Window> {
        Some(match route {
            Route::Home => surfaces::home::build(&self.services, self.screen),
            Route::Library { page } => surfaces::library::build(&self.services, self.screen, *page),
            Route::Notes => surfaces::placeholder::notes(&self.services, self.screen),
            Route::Settings => surfaces::placeholder::settings(&self.services, self.screen),
            Route::Reader { book } => {
                let row = match self.services.library.book(*book) {
                    Ok(Some(row)) => row,
                    _ => {
                        self.notices.push(format!("no book {book}"));
                        return None;
                    }
                };
                let doc = kaia_doc::open(&row.path).map_err(|e| e.to_string());
                let page = match &doc {
                    Ok(doc) => surfaces::reader::page_for_progress(row.percent, doc.page_count() as u32),
                    Err(_) => 0,
                };
                // Opening a book is a reading event, so the library records it now.
                if let Err(e) = self.services.library.record_open(row.id, row.percent) {
                    self.notices.push(format!("cannot record the open: {e}"));
                }
                let title = surfaces::display_title(&row);
                surfaces::reader::build(&self.services, self.screen, title, doc, page)
            }
            Route::Overlay => return None,
            Route::BookDetails { book } => {
                let row = self.services.library.book(*book).ok().flatten()?;
                surfaces::placeholder::book_details(&self.services, self.screen, &row)
            }
            Route::SettingsSection { name } => {
                surfaces::placeholder::settings_section(&self.services, self.screen, name)
            }
        })
    }

    /// Rebuilds the root when it is Home or Library, as the library changed.
    /// Otherwise it waits until the stack returns to the root.
    fn library_changed(&mut self) {
        if self.router.depth() == 1 {
            self.refresh_root();
        } else {
            self.library_stale = true;
        }
    }

    fn refresh_root(&mut self) {
        if let Some(route @ (Route::Home | Route::Library { .. })) = self.router.root_route().cloned() {
            self.show_root(route, RefreshMode::Ui);
        }
    }

    /// Turns the scan thread's messages into events. Progress and the end of the
    /// scan become `LibraryScanProgress`, `LibraryUpdated` and `LibraryScanFailed`.
    fn pump_scan(&mut self) {
        loop {
            let Some(message) = self.scan.as_ref().and_then(|s| s.try_next()) else {
                return;
            };
            match message {
                ScanMessage::Progress { done, total } => {
                    self.scan_progress = Some((done, total));
                    self.broadcast(Event::with_args(
                        "LibraryScanProgress",
                        vec![Value::Number(done as f64), Value::Number(total as f64)],
                    ));
                }
                ScanMessage::Finished(result) => {
                    self.scan = None;
                    self.scan_progress = None;
                    match result {
                        Ok(report) => {
                            self.broadcast(Event::with_args(
                                "LibraryUpdated",
                                vec![
                                    Value::Number(report.added as f64),
                                    Value::Number(report.changed as f64),
                                    Value::Number(report.removed as f64),
                                ],
                            ));
                            self.library_changed();
                        }
                        Err(message) => {
                            self.notices.push(format!("library scan failed: {message}"));
                            self.broadcast(Event::with_args("LibraryScanFailed", vec![Value::Str(message)]));
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
            // Pictures (covers on Home, pages in the reader) are dithered. Plain UI is not.
            let content_dither = match self.router.top_route() {
                Some(Route::Home) => true,
                Some(Route::Reader { .. }) => refresh.mode == RefreshMode::Full,
                _ => false,
            };
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
