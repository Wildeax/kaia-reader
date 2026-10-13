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
mod persistence;
#[cfg(feature = "lua")]
mod plugins;
mod state;
mod sticker_actions;
pub mod stickers;
mod work;

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
fn koreader_default_layout(screen: kaia_gfx::Size, settings: &kaia_events::Settings) -> TextLayout {
    let base = TextLayout::default();
    TextLayout {
        font_size: koreader_px(
            screen,
            settings
                .read("reading.font_size")
                .as_f64()
                .filter(|n| n.is_finite())
                .unwrap_or(22.0)
                .clamp(12.0, 40.0),
        ),
        font_family: settings
            .read("reading.font_family")
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned),
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
    #[cfg(feature = "lua")]
    plugin_page: Option<u32>,
    #[cfg(feature = "lua")]
    plugin_attached: bool,
    #[cfg(feature = "lua")]
    plugin_layout: Option<TextLayout>,
    #[cfg(feature = "lua")]
    plugin_annotations: Option<(i64, i64)>,
    persistence: persistence::Persistence,
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
            #[cfg(feature = "lua")]
            plugin_page: None,
            #[cfg(feature = "lua")]
            plugin_attached: false,
            #[cfg(feature = "lua")]
            plugin_layout: None,
            #[cfg(feature = "lua")]
            plugin_annotations: None,
            persistence: persistence::Persistence::default(),
            remote: None,
            core: Rc::new(RefCell::new(Core::new(book))),
        }
    }
    #[cfg(feature = "lua")]
    fn plugin_tick(&mut self, env: &mut SurfaceEnv<'_>) {
        if !env.services.settings.is_true(crate::plugins::ENABLED)
            || env.services.plugins.active_book != Some(self.book)
        {
            return;
        }
        {
            let core = self.core.borrow();
            if core.session.is_none() || core.layout_pending || core.job.is_some() {
                return;
            }
        }
        if !self.plugin_attached || env.services.plugins.reader_book != Some(self.book) {
            match plugins::Host::new(self.core.clone(), &env.services.paths.data_dir) {
                Ok(host) => {
                    env.services
                        .plugins
                        .requests
                        .push(crate::plugins::Request::Reader(Rc::new(RefCell::new(host))));
                    self.plugin_attached = true;
                    env.services.plugins.reader_book = Some(self.book);
                }
                Err(e) => {
                    env.notice(format!("Cannot connect reader plugins: {e}"));
                    return;
                }
            }
        }
        let page = self.core.borrow().session.as_ref().map(|s| s.current().page + 1);
        if page != self.plugin_page {
            self.plugin_page = page;
            env.services
                .plugins
                .event("PageUpdate", vec![Value::from(page.unwrap_or(1) as i64)]);
        }
        let mut core = self.core.borrow_mut();
        let session = core.session.as_mut().unwrap();
        let layout = session.layout().clone();
        if self.plugin_layout.as_ref().is_some_and(|old| old != &layout) {
            env.services.plugins.event("DocumentRerendered", vec![]);
        }
        self.plugin_layout = Some(layout);
        if let Ok(items) = session.annotations() {
            let counts = (
                items.iter().filter(|a| a.drawer.is_some()).count() as i64,
                items.iter().filter(|a| a.note.is_some()).count() as i64,
            );
            if let Some(old) = self.plugin_annotations.filter(|old| *old != counts) {
                env.services.plugins.event(
                    "AnnotationsModified",
                    vec![Value::Table(
                        [
                            (
                                kaia_events::Key::from("nb_highlights_added"),
                                Value::from(counts.0 - old.0),
                            ),
                            (
                                kaia_events::Key::from("nb_notes_added"),
                                Value::from(counts.1 - old.1),
                            ),
                        ]
                        .into(),
                    )],
                );
            }
            self.plugin_annotations = Some(counts);
        }
        drop(core);
        if let (Some(ui), Some(window)) = (&env.services.plugins.ui, &self.core.borrow().page_window) {
            let ui = Rc::downgrade(ui);
            window.update::<PageView, _>(env.ctx, PAGE_KEY, move |view, cx| {
                if view.plugins.is_none() {
                    cx.request_paint();
                }
                view.plugins = Some(ui);
            });
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
    fn busy(&self) -> bool {
        let c = self.core.borrow();
        c.job.is_some() || c.layout_pending || !c.queued.is_empty() || !c.events.is_empty()
    }

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
        opts.default_layout = koreader_default_layout(screen, &env.services.settings);
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
            if let Some(value) = self.location.take() {
                c.queued.push_back(("reader:location".into(), vec![value]));
            }
            if c.job.is_none() {
                c.repaint_page(env.ctx, PaintHint::Full);
            }
            c.percent()
        };
        // Opening a book is a reading event, so the library records it now.
        if let Some(percent) = progress {
            if let Err(e) = env.services.library.record_open(row.id, percent) {
                env.notice(format!("cannot record the open: {e}"));
            }
        }
        schedule_if_pending(&self.core, env.ctx);
        Some(window)
    }

    fn on_show(&mut self, env: &mut SurfaceEnv<'_>) {
        if self.core.borrow().session.is_none() {
            return;
        }
        self.core.borrow_mut().opened = true;
        if let Some(percent) = self.core.borrow().percent() {
            let _ = env.services.library.record_open(self.book, percent);
        }
        self.report_state(env);
        #[cfg(feature = "lua")]
        self.plugin_tick(env);
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
        if ev.name == "ReaderTick" {
            self.core.borrow_mut().pump(env.ctx);
            if self.core.borrow().session.is_some() && !self.core.borrow().opened {
                self.on_show(env);
            }
            #[cfg(feature = "lua")]
            self.plugin_tick(env);
            self.persistence.tick(&mut self.core.borrow_mut(), env);
            if self.core.borrow().job.is_none() {
                let event = self.core.borrow_mut().events.pop_front();
                if let Some(event) = event {
                    return self.on_event(&event, env);
                }
                let next = self.core.borrow_mut().queued.pop_front();
                if let Some((cmd, args)) = next {
                    let result = self.on_command(&cmd, &args, env);
                    schedule_if_pending(&self.core, env.ctx);
                    self.persistence.tick(&mut self.core.borrow_mut(), env);
                    return result;
                }
            }
            schedule_if_pending(&self.core, env.ctx);
            return Outcome::Handled;
        }
        if self.core.borrow().job.is_some()
            && matches!(
                ev.name.as_str(),
                "SyncProgress" | "SyncPushed" | "Suspend" | "SaveState" | "Resume"
            )
        {
            self.core.borrow_mut().events.push_back(ev.clone());
            return Outcome::Handled;
        }
        if matches!(ev.name.as_str(), "Suspend" | "SaveState" | "Resume") {
            self.persistence.tick(&mut self.core.borrow_mut(), env);
            self.persistence.event(&ev.name, env);
        }
        if ev.name == "SyncPushed" {
            let mut core = self.core.borrow_mut();
            if core.session.as_ref().is_some_and(|s| {
                ev.args.first().and_then(Value::as_str) == Some(s.identity().partial_md5.as_str())
                    && env
                        .services
                        .sync
                        .pending_progress(&s.identity().partial_md5)
                        .is_none()
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
            self.persistence.save(&mut self.core.borrow_mut(), env);
            if let Some(session) = self.core.borrow().session.as_ref() {
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
        if cmd.starts_with("reader:") && self.core.borrow().job.is_some() {
            self.core
                .borrow_mut()
                .queued
                .push_back((cmd.to_string(), args.to_vec()));
            return Outcome::Handled;
        }
        if cmd == "reader:dictionary-word" {
            if let Some(word) = args.first().and_then(Value::as_str) {
                let text = dict::lookup(word, env.services);
                #[cfg(feature = "lua")]
                env.services
                    .plugins
                    .requests
                    .push(crate::plugins::Request::Dictionary(word.into(), text.clone()));
                let window = panels::dictionary(env.services, env.screen, word, &text);
                return show_panel(&self.core, &mut self.core.borrow_mut(), env.ctx, window, true);
            }
            return Outcome::Handled;
        }
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
            if let Some(value) = args.first() {
                if let Some(id) = value
                    .get("repair_sticker")
                    .as_str()
                    .or_else(|| value.get("open_sticker").as_str())
                {
                    let mut c = self.core.borrow_mut();
                    let result = c
                        .session
                        .as_ref()
                        .ok_or("No book is open.".to_string())
                        .and_then(stickers::store)
                        .and_then(|s| s.get_sticker(id).map_err(|e| e.to_string()));
                    match result {
                        Ok(sticker)
                            if c.session
                                .as_ref()
                                .is_some_and(|s| s.identity().partial_md5 == sticker.doc.partial_md5) =>
                        {
                            if value.get("repair_sticker").as_str().is_some() {
                                c.moving_sticker = Some(id.into());
                                env.toast("Long-press the new place, then choose Sticker.", 6.0);
                            } else {
                                let location = match sticker.anchor {
                                    kaia_storage::Anchor::Reflowable { locator, .. } => {
                                        kaia_book::Location::Engine(locator)
                                    }
                                    kaia_storage::Anchor::Fixed { page, .. } => {
                                        kaia_book::Location::Page(page.saturating_sub(1))
                                    }
                                    kaia_storage::Anchor::Book {} => kaia_book::Location::Page(0),
                                };
                                c.go_to(env.ctx, location);
                                c.queued
                                    .push_back((format!("reader:sticker:open:{id}"), Vec::new()));
                            }
                        }
                        Ok(_) => {
                            env.toast("This sticker belongs to another book.", 4.0);
                        }
                        Err(e) => {
                            env.toast(e, 4.0);
                        }
                    }
                    drop(c);
                    schedule_if_pending(&self.core, env.ctx);
                    return Outcome::Handled;
                }
            }
            if let Some(location) = args.first().cloned().and_then(location_value) {
                self.core.borrow_mut().go_to(env.ctx, location);
            }
            return Outcome::Handled;
        }
        command(&self.core, cmd, args, env)
    }

    fn can_hide(&mut self, env: &mut SurfaceEnv<'_>) -> bool {
        if self.core.borrow().job.is_some() {
            env.toast("The page is still loading. Try again when it finishes.", 3.0);
            return false;
        }
        self.persistence.save(&mut self.core.borrow_mut(), env)
    }

    fn on_hide(&mut self, env: &mut SurfaceEnv<'_>) {
        #[cfg(feature = "lua")]
        if self.plugin_attached {
            env.services.plugins.event("CloseDocument", vec![]);
            env.services.plugins.event("FlushSettings", vec![]);
            env.services
                .plugins
                .requests
                .push(crate::plugins::Request::Library({
                    let core = self.core.clone();
                    Box::new(move || {
                        core.borrow_mut().session = None;
                    })
                }));
        }
        self.persistence.event("Close", env);
        let mut c = self.core.borrow_mut();
        if let Some(session) = c.session.as_ref() {
            env.services.sync.push(
                &env.services.settings,
                session,
                env.services.paths.data_dir.join("sync-queue.json"),
                env.ctx.scheduler.now(),
            );
        }
        c.closed = true;
        c.overlay = None;
        c.panel = None;
        #[cfg(feature = "lua")]
        if !self.plugin_attached {
            c.session = None;
        }
        // The plugin host keeps the session until CloseDocument has returned.
        #[cfg(not(feature = "lua"))]
        {
            c.session = None;
        }
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
    if cmd == "reader:slider:release" || (cfg!(feature = "lua") && cmd == "reader:plugin-refresh") {
        rc.borrow_mut().repaint_page(env.ctx, PaintHint::Ui);
        schedule_if_pending(rc, env.ctx);
        return Outcome::Handled;
    }
    if let Some(outcome) = sticker_actions::command(rc, cmd, env) {
        schedule_if_pending(rc, env.ctx);
        return outcome;
    }
    let Some(command) = commands::parse(cmd, args) else {
        return Outcome::NotMine;
    };
    let outcome = handle(rc, command, env);
    if cmd == "reader:slider" && args.get(1).and_then(Value::as_bool) != Some(true) {
        rc.borrow_mut().set_pending_hint(PaintHint::Ui);
    }
    schedule_if_pending(rc, env.ctx);
    outcome
}

fn handle(rc: &Rc<RefCell<Core>>, command: Command, env: &mut SurfaceEnv<'_>) -> Outcome {
    {
        let mut c = rc.borrow_mut();
        match command {
            Command::Tap(x, y) => {
                if let Some(id) = c
                    .stickers
                    .iter()
                    .rev()
                    .find(|s| {
                        s.hit_rect(env.theme().touch_target)
                            .contains(Point::new(x as i32, y as i32))
                    })
                    .map(|s| s.id.clone())
                {
                    return sticker_actions::open(rc, &mut c, &id, env);
                }
                if c.selection.is_some() {
                    c.clear_selection(env.ctx);
                    return Outcome::Handled;
                }
                let w = f64::from(env.screen.w);
                let h = f64::from(env.screen.h);
                let centre = x >= w / 3.0 && x < 2.0 * w / 3.0 && y >= h / 3.0 && y < 2.0 * h / 3.0;
                let preset = env
                    .services
                    .settings
                    .read("reading.tap_zones")
                    .as_str()
                    .unwrap_or("default");
                if (centre || y < h / 10.0) && preset != "page_only" {
                    return open_overlay(rc, &mut c, env);
                }
                step(
                    rc,
                    &mut c,
                    env,
                    if preset == "left_handed" {
                        x < 2.0 * w / 3.0
                    } else {
                        x >= w / 3.0
                    },
                )
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
                    let window = panels::selection_popover(
                        env.services,
                        env.screen,
                        &word,
                        c.moving_sticker.is_some(),
                    );
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
                c.reset_search_paint();
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
                let location = c
                    .session
                    .as_ref()
                    .and_then(|s| s.toc().get(index))
                    .and_then(|entry| {
                        entry
                            .xpointer
                            .clone()
                            .map(kaia_book::Location::Engine)
                            .or_else(|| entry.page.map(kaia_book::Location::Page))
                    });
                if let Some(location) = location {
                    c.go_to(env.ctx, location);
                    Outcome::Navigate(Nav::Pop)
                } else {
                    env.toast("This entry has no page.", 2.0);
                    Outcome::Handled
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
                #[cfg(feature = "lua")]
                env.services
                    .plugins
                    .requests
                    .push(crate::plugins::Request::Dictionary(word.clone(), text.clone()));
                c.clear_selection(env.ctx);
                let window = panels::dictionary(env.services, env.screen, &word, &text);
                show_panel(rc, &mut c, env.ctx, window, true)
            }
            Command::SelSearch => {
                let Some(selection) = c.selection.clone() else {
                    return Outcome::Handled;
                };
                let text = selection.text.trim().to_string();
                c.reset_search_paint();
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
                        if let Some(session) = c.session.as_ref() {
                            if let Err(e) = persistence::update_index(session, c.book, env) {
                                env.notice(e);
                            }
                        }
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
        Turn::Moved => Outcome::Handled,
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
    let count = c.session.as_ref().map_or(0, |s| s.page_count());
    if count > 0 {
        let page = ((percent * count as f64).round() as i64 - 1).clamp(0, count as i64 - 1) as u32;
        c.go_to(env.ctx, kaia_book::Location::Page(page));
        c.set_pending_hint(hint);
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
