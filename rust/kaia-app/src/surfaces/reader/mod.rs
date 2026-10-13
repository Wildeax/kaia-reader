//! The reader: one book, full screen.
//!
//! The surface owns the book session through a shared [`state::Core`]. The page
//! widget ([`page::PageView`]) paints what the core gives it and sends touches as
//! commands. Tap zones: the left third goes back a page, the centre square opens
//! the overlay, the rest goes forward. A tap on a selection clears it. Holding a
//! word selects it, and releasing opens the selection popover.
//!
//! The overlay ([`overlay`]) and the panels ([`panels`]) are pushed above the
//! reader as surfaces. Their commands that the reader owns come back here, since
//! they answer [`Outcome::NotMine`].
//!
//! The reader saves its position in `on_hide`. A page turn that crosses a chapter
//! start, or the sixth turn since the last full refresh, is a full refresh.

mod commands;
mod dict;
mod overlay;
pub mod page;
pub mod panels;
mod state;

use std::cell::RefCell;
use std::rc::Rc;

use kaia_book::{Drawer, Margins, OpenOptions, Point, TextLayout, ZoomMode};
use kaia_events::{Ctx, Event, Refresh, RefreshMode, ShowOpts, Value};
use kaia_ui::components::Stack;
use kaia_ui::widget::PaintHint;
use kaia_ui::{WidgetPod, Window};

use commands::{Command, LayoutChange, PanelName};
use overlay::Overlay;
use page::{PageView, PAGE_KEY};
use panels::ContentItem;
use state::{font_face, read_field, schedule_if_pending, Core, Layer, Turn};

use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{self, display_title, window};
use crate::surfaces::Route;

/// KOReader's `scaleBySize` for `units` of its font sizes and margins, in pixels on
/// this screen. KOReader uses `min(width, height) / 600` as the scale when no DPI
/// override is set, which is the case on the Libra Colour. So 22 is 47 px and 10 is
/// 22 px there.
pub(crate) fn koreader_px(screen: kaia_gfx::Size, units: f64) -> f32 {
    (units * f64::from(screen.w.min(screen.h)) / 600.0).ceil() as f32
}

/// The text layout for a book with no saved layout: KOReader's defaults, scaled
/// the way KOReader scales them. The font is 22 units, the side margins 10 units
/// and the top and bottom margins 15 units. Line spacing (100 %) and the other
/// options keep the defaults of `kaia_book`. A book with a saved layout keeps it.
fn koreader_default_layout(screen: kaia_gfx::Size) -> TextLayout {
    let base = TextLayout::default();
    TextLayout {
        font_size: koreader_px(screen, 22.0),
        margins: Margins {
            top: koreader_px(screen, 15.0),
            right: koreader_px(screen, 10.0),
            bottom: koreader_px(screen, 15.0),
            left: koreader_px(screen, 10.0),
        },
        ..base
    }
}

/// The reader screen for one book.
pub struct ReaderSurface {
    book: i64,
    location: Option<Value>,
    reported: bool,
    remote: Option<kaia_book::Location>,
    core: Rc<RefCell<Core>>,
}

impl ReaderSurface {
    pub fn new(book: i64) -> Self {
        Self::at(book, None)
    }

    pub fn at(book: i64, location: Option<Value>) -> Self {
        Self {
            book,
            location,
            reported: false,
            remote: None,
            core: Rc::new(RefCell::new(Core::new(book))),
        }
    }
    fn receive_sync(&mut self, args: &[Value], env: &mut SurfaceEnv<'_>) {
        use kaia_sync::{Decision, LocalState, Policy, Progress, Strategy};
        if !env.services.settings.is_true("sync.enabled") || args.len() < 6 {
            return;
        }
        let mut core = self.core.borrow_mut();
        let Some(session) = core.session.as_ref() else {
            return;
        };
        let text = |i: usize| args[i].as_str().unwrap_or("").to_string();
        let remote = Progress {
            document: session.identity().partial_md5.clone(),
            progress: text(1),
            percentage: args[2].as_f64().unwrap_or(0.0),
            device: text(3),
            device_id: text(4),
            timestamp: args[5].as_i64(),
            metadata: None,
        };
        let local = LocalState {
            progress: crate::sync::position(session),
            percentage: session.current().percent,
            device: "Kaia Reader".into(),
            device_id: env
                .services
                .settings
                .read("sync.device_id")
                .as_str()
                .unwrap_or("")
                .into(),
            last_page_turn: core.last_page_turn,
        };
        let strategy = |key: &str, default| match env.services.settings.read(key).as_str() {
            Some("silent") => Strategy::Silent,
            Some("never") => Strategy::Never,
            Some("prompt") => Strategy::Prompt,
            _ => default,
        };
        let policy = Policy {
            auto_sync: true,
            sync_forward: strategy("sync.forward", Strategy::Prompt),
            sync_backward: strategy("sync.backward", Strategy::Never),
        };
        let location = if session.is_reflowable() {
            kaia_book::Location::Engine(remote.progress.clone())
        } else {
            let Ok(page) = remote.progress.parse::<u32>() else {
                return;
            };
            kaia_book::Location::Page(page.saturating_sub(1))
        };
        match kaia_sync::decide(&local, &remote, &policy) {
            Decision::PullRemote => {
                core.go_to(env.ctx, location);
            }
            Decision::AskUser => {
                self.remote = Some(location);
                env.dialog(kaia_ui::composite::Dialog::new(
                    env.theme(),
                    "Use synced position?",
                    format!("Another device is at {:.0} percent.", remote.percentage * 100.0),
                    vec![
                        kaia_ui::composite::DialogButton::new("Keep this position", "dialog:dismiss"),
                        kaia_ui::composite::DialogButton::new("Go there", "reader:sync:accept"),
                    ],
                ));
            }
            _ => {}
        }
    }

    fn report_state(&mut self, env: &mut SurfaceEnv<'_>) {
        if self.reported {
            return;
        }
        self.reported = true;
        let c = self.core.borrow();
        let Some(session) = c.session.as_ref() else {
            return;
        };
        if let Some(issue) = session.state_issue() {
            let message = match issue {
                kaia_book::StateIssue::NewerVersion(_) => {
                    "This book's saved state needs a newer Kaia. Changes cannot be saved."
                }
                kaia_book::StateIssue::Corrupt => {
                    "The saved state was damaged. Kaia will save edits in a recovered file."
                }
                kaia_book::StateIssue::Locked => {
                    "Another process has this book open. Changes cannot be saved."
                }
            };
            env.toast(message, 6.0);
        }
        if let Some(error) = session.save_error() {
            env.toast(format!("Cannot save this book: {error}"), 6.0);
        }
        if session.state_is_stale() || session.migration_report().is_some() {
            let count = session
                .migration_report()
                .map_or(0, |r| r.annotations_need_reanchoring);
            env.dialog(kaia_ui::composite::Dialog::new(
                env.theme(),
                "Book file changed",
                format!("Saved progress was kept. Check the reading position and {count} annotations."),
                vec![
                    kaia_ui::composite::DialogButton::new("Review later", "dialog:dismiss"),
                    kaia_ui::composite::DialogButton::new("Reviewed", "reader:acknowledge-change"),
                ],
            ));
        }
    }
}

impl Surface for ReaderSurface {
    fn route(&self) -> Route {
        Route::Reader { book: self.book }
    }

    /// Full screen, with a full refresh: the page is a picture.
    fn show_opts(&self) -> ShowOpts {
        ShowOpts {
            covers_fullscreen: true,
            refresh: Some(Refresh::new(RefreshMode::Full)),
            ..ShowOpts::default()
        }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        let row = match env.services.library.book(self.book) {
            Ok(Some(row)) => row,
            _ => {
                env.notice(format!("no book {}", self.book));
                return None;
            }
        };
        let screen = env.screen;
        let mut opts = OpenOptions::new(env.services.paths.data_dir.clone());
        opts.viewport = screen;
        opts.default_layout = koreader_default_layout(screen);
        opts.pixel_format = env.services.theme.pixel_format();
        let color_panel = env.services.theme.color_panel;
        let page = WidgetPod::new(PageView::new()).with_key(PAGE_KEY);
        let window = window(env.services, screen, Stack::new(vec![page]));

        let progress = {
            let mut c = self.core.borrow_mut();
            c.title = display_title(&row);
            c.seconds_per_page = common::seconds_per_page(
                &env.services.paths.data_dir,
                &c.title,
                row.authors.as_deref().unwrap_or("N/A"),
            );
            c.viewport = screen;
            c.color_panel = color_panel;
            c.refresh_every = env
                .services
                .settings
                .read("reading.full_refresh_pages")
                .as_i64()
                .unwrap_or(6)
                .max(0) as u32;
            c.page_window = Some(window.clone());
            if c.session.is_none() {
                c.open(&row.path, opts);
            }
            if let Some(location) = self.location.take().and_then(location_value) {
                c.go_to(env.ctx, location);
            }
            c.repaint_page(env.ctx, PaintHint::Full);
            c.percent()
        };
        // Opening a book is a reading event, so the library records it now.
        if let Some(percent) = progress {
            if let Err(e) = env.services.library.record_open(row.id, percent) {
                env.notice(format!("cannot record the open: {e}"));
            }
        }
        Some(window)
    }

    fn on_show(&mut self, env: &mut SurfaceEnv<'_>) {
        self.report_state(env);
        if let Some(session) = self.core.borrow().session.as_ref() {
            env.services.sync.pull(
                &env.services.settings,
                self.book,
                session,
                env.ctx.scheduler.now(),
            );
        }
    }

    fn on_event(&mut self, ev: &Event, env: &mut SurfaceEnv<'_>) -> Outcome {
        if ev.name == "SyncPushed" {
            let mut core = self.core.borrow_mut();
            if core.session.as_ref().is_some_and(|s| {
                ev.args.first().and_then(Value::as_str) == Some(s.identity().partial_md5.as_str())
            }) {
                core.periodic_sync.reset();
            }
            return Outcome::Handled;
        }
        if ev.name == "Resume" {
            if let Some(session) = self.core.borrow().session.as_ref() {
                env.services.sync.pull(
                    &env.services.settings,
                    self.book,
                    session,
                    env.ctx.scheduler.now(),
                );
            }
            return Outcome::Handled;
        }
        if ev.name == "SyncProgress" && ev.args.first().and_then(Value::as_i64) == Some(self.book) {
            self.receive_sync(&ev.args, env);
            return Outcome::Handled;
        }
        if matches!(ev.name.as_str(), "SaveState" | "Suspend") {
            if let Some(session) = self.core.borrow_mut().session.as_mut() {
                if let Err(e) = session.save() {
                    env.toast(format!("Cannot save reading progress: {e}"), 5.0);
                }
                env.services.sync.push(
                    &env.services.settings,
                    session,
                    env.services.paths.data_dir.join("sync-queue.json"),
                    env.ctx.scheduler.now(),
                );
            }
            return Outcome::Handled;
        }
        Outcome::NotMine
    }

    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        if cmd == "reader:sync:accept" {
            if let Some(location) = self.remote.take() {
                self.core.borrow_mut().go_to(env.ctx, location);
            }
            return Outcome::Handled;
        }
        if cmd == "reader:acknowledge-change" {
            if let Some(session) = self.core.borrow_mut().session.as_mut() {
                if let Err(e) = session.acknowledge_file_change() {
                    env.toast(format!("Cannot save confirmation: {e}"), 5.0);
                }
            }
            return Outcome::Handled;
        }
        if cmd == "reader:location" {
            if let Some(location) = args.first().cloned().and_then(location_value) {
                self.core.borrow_mut().go_to(env.ctx, location);
            }
            return Outcome::Handled;
        }
        command(&self.core, cmd, args, env)
    }

    fn on_hide(&mut self, env: &mut SurfaceEnv<'_>) {
        let mut c = self.core.borrow_mut();
        if let Some(session) = c.session.as_mut() {
            let percent = session.current().percent;
            env.services.sync.push(
                &env.services.settings,
                session,
                env.services.paths.data_dir.join("sync-queue.json"),
                env.ctx.scheduler.now(),
            );
            if let Err(e) = session.save() {
                env.notice(format!("cannot save the reading position: {e}"));
            }
            if let Err(e) = env.services.library.record_open(self.book, percent) {
                env.notice(format!("cannot save progress: {e}"));
            }
        }
        c.closed = true;
        c.overlay = None;
        c.panel = None;
        c.session = None;
    }

    /// A picture is dithered on a full refresh only (see `kaia_display`).
    fn content_dither(&self, mode: RefreshMode) -> bool {
        mode == RefreshMode::Full
    }
}

/// Runs a command owned by the reader.
pub(super) fn command(
    rc: &Rc<RefCell<Core>>,
    cmd: &str,
    args: &[Value],
    env: &mut SurfaceEnv<'_>,
) -> Outcome {
    let Some(command) = commands::parse(cmd, args) else {
        return Outcome::NotMine;
    };
    let outcome = handle(rc, command, env);
    schedule_if_pending(rc, env.ctx);
    outcome
}

fn handle(rc: &Rc<RefCell<Core>>, command: Command, env: &mut SurfaceEnv<'_>) -> Outcome {
    {
        let mut c = rc.borrow_mut();
        match command {
            Command::Tap(x, y) => {
                if c.selection.is_some() {
                    c.clear_selection(env.ctx);
                    return Outcome::Handled;
                }
                let w = f64::from(env.screen.w);
                let h = f64::from(env.screen.h);
                let centre = x >= w / 3.0 && x < 2.0 * w / 3.0 && y >= h / 3.0 && y < 2.0 * h / 3.0;
                if centre {
                    return open_overlay(rc, &mut c, env);
                }
                step(rc, &mut c, env, x >= w / 3.0)
            }
            Command::Next => step(rc, &mut c, env, true),
            Command::Prev => step(rc, &mut c, env, false),
            Command::Hold(x, y) => {
                if c.panel.is_none() && c.overlay.is_none() {
                    c.hold(env.ctx, Point::new(x as i32, y as i32));
                }
                Outcome::Handled
            }
            Command::Drag(x, y) => {
                c.drag(env.ctx, Point::new(x as i32, y as i32));
                Outcome::Handled
            }
            Command::Release => {
                if c.release() && c.panel.is_none() {
                    let word = c.selection.as_ref().map(|s| s.text.clone()).unwrap_or_default();
                    let window = panels::popover(env.services, env.screen, &word);
                    return push_panel(rc, &mut c, env.ctx, window);
                }
                Outcome::Handled
            }
            Command::Overlay => open_overlay(rc, &mut c, env),
            Command::Bookmark => {
                let result = c.session.as_mut().map(|s| s.toggle_bookmark());
                match result {
                    Some(Ok(added)) => {
                        let message = if added {
                            "Bookmark added"
                        } else {
                            "Bookmark removed"
                        };
                        env.toast(message, 2.0);
                        c.refresh_overlay(env.ctx);
                    }
                    Some(Err(e)) => env.notice(format!("cannot change the bookmark: {e}")),
                    None => {}
                }
                Outcome::Handled
            }
            Command::Slider(value) => {
                let count = c.session.as_ref().map_or(0, |s| s.page_count());
                if count > 0 && c.panel.is_none() {
                    goto_percent(&mut c, env, value / count as f64, PaintHint::Fast);
                }
                Outcome::Handled
            }
            Command::SearchOpen => {
                if c.panel.is_some() {
                    return Outcome::Handled;
                }
                let rows = c
                    .session
                    .as_ref()
                    .map(|s| panels::search_rows(s.search_results()))
                    .unwrap_or_default();
                let status = search_status(&c);
                let window = panels::search(env.services, env.screen, &c.search_text, rows, &status);
                push_panel(rc, &mut c, env.ctx, window)
            }
            Command::SearchGo => {
                let Some(layer) = c.panel.clone() else {
                    return Outcome::Handled;
                };
                let text = read_field(&layer.window, env.ctx, "search-field").unwrap_or_default();
                let text = text.trim().to_string();
                c.search_text = text.clone();
                if text.is_empty() {
                    env.toast("Type a word to search.", 2.0);
                    return Outcome::Handled;
                }
                let started = c.session.as_mut().map(|s| s.start_search(&text));
                match started {
                    Some(Ok(())) => c.update_search(env.ctx),
                    Some(Err(e)) => env.notice(format!("cannot search: {e}")),
                    None => {}
                }
                Outcome::Handled
            }
            Command::SearchHit(index) => {
                let location = c
                    .session
                    .as_ref()
                    .and_then(|s| s.search_results().get(index).map(|hit| hit.location.clone()));
                match location {
                    Some(location) => {
                        c.go_to(env.ctx, location);
                        Outcome::Navigate(Nav::Pop)
                    }
                    None => Outcome::Handled,
                }
            }
            Command::Panel(name) => {
                if c.panel.is_some() {
                    return Outcome::Handled;
                }
                let window = match name {
                    PanelName::Contents => {
                        let tab = c.contents_tab;
                        contents_window(&mut c, env, tab)
                    }
                    PanelName::Layout => layout_window(&c, env),
                    PanelName::More => panels::more(env.services, env.screen),
                };
                push_panel(rc, &mut c, env.ctx, window)
            }
            Command::PanelClose => Outcome::Navigate(Nav::Pop),
            Command::ContentsTab(tab) => {
                c.contents_tab = tab;
                let window = contents_window(&mut c, env, tab);
                show_panel(rc, &mut c, env.ctx, window, true)
            }
            Command::ContentsGoto(index) => {
                let moved = c
                    .session
                    .as_mut()
                    .map(|s| s.go_to_toc_entry(index))
                    .unwrap_or(Ok(false));
                match moved {
                    Ok(true) => {
                        c.repaint_page(env.ctx, PaintHint::Full);
                        Outcome::Navigate(Nav::Pop)
                    }
                    Ok(false) => {
                        env.toast("This entry has no page.", 2.0);
                        Outcome::Handled
                    }
                    Err(e) => {
                        env.notice(format!("cannot open that entry: {e}"));
                        Outcome::Handled
                    }
                }
            }
            Command::ContentsAnnotation(id) => {
                let location = c.session.as_ref().and_then(|s| s.annotation(id).map(|a| a.page));
                match location {
                    Some(location) => {
                        c.go_to(env.ctx, location);
                        Outcome::Navigate(Nav::Pop)
                    }
                    None => Outcome::Handled,
                }
            }
            Command::Layout(change) => {
                let result = match change {
                    LayoutChange::Font(index) => {
                        let face = font_face(index);
                        c.change_layout(|l| l.font_family = face)
                    }
                    LayoutChange::Size(size) => c.change_layout(|l| l.font_size = size as f32),
                    LayoutChange::Spacing(spacing) => c.change_layout(|l| l.line_spacing = spacing as f32),
                    LayoutChange::Margins(px) => {
                        let m = px as f32;
                        c.change_layout(|l| {
                            l.margins = Margins {
                                top: m,
                                right: m,
                                bottom: m,
                                left: m,
                            }
                        })
                    }
                    LayoutChange::Justify(on) => c.change_layout(|l| l.justify = on),
                    LayoutChange::Hyphenation(on) => c.change_layout(|l| l.hyphenation = on),
                    LayoutChange::Styles(on) => c.change_layout(|l| l.embedded_styles = on),
                    LayoutChange::Zoom(index) => {
                        let zoom = if index == 1 {
                            ZoomMode::FitWidth
                        } else {
                            ZoomMode::FitPage
                        };
                        c.set_zoom(env.ctx, zoom)
                    }
                };
                if let Err(message) = result {
                    env.toast(message, 3.0);
                }
                Outcome::Handled
            }
            Command::SelHighlight => {
                let Some(selection) = c.selection.clone() else {
                    return Outcome::Handled;
                };
                let color = highlight_color(&c);
                let result = c
                    .session
                    .as_mut()
                    .map(|s| s.add_highlight(&selection, Drawer::Lighten, color, None));
                finish_highlight(&mut c, env, result);
                Outcome::Navigate(Nav::Pop)
            }
            Command::SelNote => {
                let Some(selection) = c.selection.clone() else {
                    return Outcome::Handled;
                };
                let window = panels::note(env.services, env.screen, &selection.text);
                show_panel(rc, &mut c, env.ctx, window, true)
            }
            Command::SelCopy => {
                if let Some(selection) = c.selection.clone() {
                    env.services.clipboard = Some(selection.text.clone());
                    env.toast("Copied", 2.0);
                    c.clear_selection(env.ctx);
                }
                Outcome::Navigate(Nav::Pop)
            }
            Command::SelDict => {
                let Some(selection) = c.selection.clone() else {
                    return Outcome::Handled;
                };
                let word = kaia_dict::clean_selection(&selection.text);
                let text = dict::lookup(&word, env.services);
                c.clear_selection(env.ctx);
                let window = panels::dictionary(env.services, env.screen, &word, &text);
                show_panel(rc, &mut c, env.ctx, window, true)
            }
            Command::SelSearch => {
                let Some(selection) = c.selection.clone() else {
                    return Outcome::Handled;
                };
                let text = selection.text.trim().to_string();
                c.search_text = text.clone();
                c.clear_selection(env.ctx);
                let started = c.session.as_mut().map(|s| s.start_search(&text));
                if let Some(Err(e)) = started {
                    env.notice(format!("cannot search: {e}"));
                }
                let rows = c
                    .session
                    .as_ref()
                    .map(|s| panels::search_rows(s.search_results()))
                    .unwrap_or_default();
                let status = search_status(&c);
                let window = panels::search(env.services, env.screen, &text, rows, &status);
                show_panel(rc, &mut c, env.ctx, window, true)
            }
            Command::NoteSave => {
                let note = c
                    .panel
                    .clone()
                    .and_then(|layer| read_field(&layer.window, env.ctx, "note-field"))
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty());
                if let Some(selection) = c.selection.clone() {
                    let color = highlight_color(&c);
                    let result = c
                        .session
                        .as_mut()
                        .map(|s| s.add_highlight(&selection, Drawer::Lighten, color, note.as_deref()));
                    finish_highlight(&mut c, env, result);
                }
                Outcome::Navigate(Nav::Pop)
            }
            Command::MoreStart => {
                goto_percent(&mut c, env, 0.0, PaintHint::Full);
                Outcome::Navigate(Nav::Pop)
            }
            Command::MoreEnd => {
                goto_percent(&mut c, env, 1.0, PaintHint::Full);
                Outcome::Navigate(Nav::Pop)
            }
            Command::Finish => {
                let result = c.session.as_mut().map(|s| s.set_status(Some("complete")));
                match result {
                    Some(Err(e)) => {
                        env.toast(format!("Cannot mark the book as finished: {e}"), 5.0);
                    }
                    _ => {
                        env.toast("Marked as finished", 2.0);
                    }
                }
                Outcome::Navigate(Nav::Pop)
            }
        }
    }
}

/// Turns the page. At the last page a forward turn offers the end of book sheet.
fn step(rc: &Rc<RefCell<Core>>, c: &mut Core, env: &mut SurfaceEnv<'_>, forward: bool) -> Outcome {
    if c.panel.is_some() {
        return Outcome::Handled;
    }
    match c.turn(env.ctx, forward) {
        Turn::AtEnd => {
            let window = panels::end_of_book(env.services, env.screen);
            push_panel(rc, c, env.ctx, window)
        }
        Turn::Moved => {
            if env.services.settings.is_true("sync.enabled") {
                if let Some(session) = c.session.as_ref() {
                    if let Some(due) = c.periodic_sync.page_update(
                        &crate::sync::position(session),
                        env.ctx.scheduler.now(),
                        Some(10),
                    ) {
                        env.services.sync.periodic(
                            &env.services.settings,
                            session,
                            env.services.paths.data_dir.join("sync-queue.json"),
                            due,
                        );
                    }
                }
            }
            if let Some(session) = c.session.as_mut() {
                if let Err(e) = session.maybe_save(std::time::Instant::now()) {
                    env.toast(format!("Cannot save reading progress: {e}"), 5.0);
                }
            }
            Outcome::Handled
        }
        Turn::Blocked => Outcome::Handled,
    }
}

fn open_overlay(rc: &Rc<RefCell<Core>>, c: &mut Core, env: &mut SurfaceEnv<'_>) -> Outcome {
    if c.overlay.is_some() || c.panel.is_some() {
        return Outcome::Handled;
    }
    let Some(model) = c.overlay_model() else {
        return Outcome::Handled;
    };
    let window = overlay::build(env.services, env.screen, &model);
    let serial = next_serial(c);
    c.overlay = Some(Layer {
        serial,
        window: window.clone(),
    });
    Outcome::Navigate(Nav::Push(Box::new(Overlay::new(window, serial, rc.clone()))))
}

/// Pushes a panel above the top screen and records its window.
fn push_panel(rc: &Rc<RefCell<Core>>, c: &mut Core, ctx: &mut Ctx, window: Window) -> Outcome {
    show_panel(rc, c, ctx, window, false)
}

/// Shows a panel above the top screen or in place of the open panel.
fn show_panel(
    rc: &Rc<RefCell<Core>>,
    c: &mut Core,
    _ctx: &mut Ctx,
    window: Window,
    replace: bool,
) -> Outcome {
    let was_open = c.panel.is_some();
    let serial = next_serial(c);
    c.panel = Some(Layer {
        serial,
        window: window.clone(),
    });
    let surface = panels::panel_surface(window, serial, rc.clone());
    if replace && was_open {
        Outcome::Navigate(Nav::Replace(surface))
    } else {
        Outcome::Navigate(Nav::Push(surface))
    }
}

fn next_serial(c: &mut Core) -> u64 {
    c.next_serial += 1;
    c.next_serial
}

fn goto_percent(c: &mut Core, env: &mut SurfaceEnv<'_>, percent: f64, hint: PaintHint) {
    let moved = c.session.as_mut().map(|s| s.go_to_percent(percent));
    match moved {
        Some(Ok(())) => c.repaint_page(env.ctx, hint),
        Some(Err(e)) => env.notice(format!("cannot move: {e}")),
        None => {}
    }
}

fn finish_highlight(
    c: &mut Core,
    env: &mut SurfaceEnv<'_>,
    result: Option<kaia_book::Result<kaia_book::AnnotationId>>,
) {
    match result {
        Some(Ok(_)) => {
            c.clear_selection(env.ctx);
            env.toast("Highlighted", 2.0);
        }
        Some(Err(e)) => {
            env.toast(format!("Cannot highlight. {e}"), 3.0);
        }
        None => {}
    }
}

fn highlight_color(c: &Core) -> &'static str {
    if c.color_panel {
        "yellow"
    } else {
        "gray"
    }
}

fn search_status(c: &Core) -> String {
    match c.session.as_ref() {
        Some(s) if !s.search_results().is_empty() => format!("{} matches.", s.search_results().len()),
        _ => "Type a word and press Search.".to_string(),
    }
}

/// The Contents panel for `tab`, built from the session.
fn contents_window(c: &mut Core, env: &SurfaceEnv<'_>, tab: usize) -> Window {
    let (items, empty) = match c.session.as_mut() {
        None => (Vec::new(), "No book is open."),
        Some(session) => match tab {
            1 => {
                let marks = session.bookmarks().unwrap_or_default();
                (
                    panels::annotation_items(&marks, true),
                    "No bookmarks yet. Use the bookmark button in the top bar.",
                )
            }
            2 => {
                let all = session.annotations().unwrap_or_default();
                let highlights: Vec<_> = all.into_iter().filter(|a| !a.is_bookmark()).collect();
                (
                    panels::annotation_items(&highlights, false),
                    "No highlights yet. Hold a word to select it.",
                )
            }
            _ => {
                let current = session.current().chapter;
                let items = session
                    .toc()
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| ContentItem {
                        label: entry.title.clone(),
                        detail: entry.page.map(|p| format!("Page {}", p + 1)).unwrap_or_default(),
                        current: current.as_deref() == Some(entry.title.as_str()),
                        action: format!("reader:contents:goto:{index}"),
                        level: entry.level.saturating_sub(1),
                    })
                    .collect();
                (items, "This book has no table of contents.")
            }
        },
    };
    panels::contents(env.services, env.screen, tab.min(2), items, empty)
}

/// The Text and layout sheet, or the zoom sheet of a fixed-layout book.
fn layout_window(c: &Core, env: &SurfaceEnv<'_>) -> Window {
    match c.session.as_ref() {
        Some(session) => {
            let reflow = session.is_reflowable().then(|| session.layout().clone());
            panels::layout(env.services, env.screen, reflow.as_ref(), session.zoom())
        }
        None => panels::layout(env.services, env.screen, None, ZoomMode::FitPage),
    }
}

fn location_value(value: Value) -> Option<kaia_book::Location> {
    match value {
        Value::Number(page) if page.is_finite() && page >= 0.0 => {
            Some(kaia_book::Location::Page(page as u32))
        }
        Value::Str(pointer) => Some(kaia_book::Location::Engine(pointer)),
        _ => None,
    }
}
