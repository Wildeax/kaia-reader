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
//! start is a full refresh. So is a page of a fixed-layout book. With a page count
//! in `reading.full_refresh_pages`, every Nth turn is one too.

mod commands;
pub(crate) mod dict;
mod footer;
mod navigation;
mod overlay;
pub mod page;
pub mod panels;
mod persistence;
#[cfg(feature = "lua")]
mod plugin_doc;
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
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_ui::components::Stack;
use kaia_ui::controls::Button;
use kaia_ui::widget::PaintHint;
use kaia_ui::{WidgetPod, Window};

use commands::{Command, LayoutChange, PanelName};
use overlay::Overlay;
use page::{PageView, PAGE_KEY};
use panels::ContentItem;
use state::{font_face, read_field, schedule_if_pending, Core, Layer, LookupState, Turn};

use crate::services::Services;
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
    let font_family = settings
        .read("reading.font_family")
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    // The weight is saved per family, so the default face brings its own weight.
    let font_weight = crate::surfaces::settings::keys::font_weight(settings, font_family.as_deref()) as f32;
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
        font_family,
        font_weight,
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
    /// The host that plugins use, or will use once its document is ready.
    #[cfg(feature = "lua")]
    plugin_host: Option<Rc<RefCell<plugins::Host>>>,
    /// Plugins run for this book: they are on and this is the top reader.
    #[cfg(feature = "lua")]
    plugin_wanted: bool,
    /// The plugin document could not open. Plugins stay off for this reader.
    #[cfg(feature = "lua")]
    plugin_failed: bool,
    /// The layout changed and DocumentRerendered waits for the plugin document.
    #[cfg(feature = "lua")]
    plugin_rerender: bool,
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
            #[cfg(feature = "lua")]
            plugin_host: None,
            #[cfg(feature = "lua")]
            plugin_wanted: false,
            #[cfg(feature = "lua")]
            plugin_failed: false,
            #[cfg(feature = "lua")]
            plugin_rerender: false,
            persistence: persistence::Persistence::default(),
            remote: None,
            core: Rc::new(RefCell::new(Core::new(book))),
        }
    }

    /// True while plugins for this book wait for their first attach. The app
    /// then holds plugin callbacks and navigation, as it does while a book opens.
    #[cfg(feature = "lua")]
    fn plugin_attaching(&self) -> bool {
        self.plugin_wanted && !self.plugin_attached && !self.plugin_failed
    }

    /// Connects reader plugins and sends them the reader's events.
    ///
    /// Nothing here opens or lays out a document on the app thread. The plugin
    /// host's document opens on its own thread. The reader attaches plugins when
    /// that document is laid out, so `DocSettingsLoad` and `ReaderReady` see a
    /// ready book. After a layout change, `DocumentRerendered` waits the same way.
    /// Ready means laid out for the native layout, with the current page's text
    /// cached. Page updates go out at once, and the host asks for the new page's
    /// text.
    #[cfg(feature = "lua")]
    fn plugin_tick(&mut self, env: &mut SurfaceEnv<'_>) {
        self.plugin_wanted = env.services.settings.is_true(crate::plugins::ENABLED)
            && env.services.plugins.active_book == Some(self.book);
        if !self.plugin_wanted {
            self.core.borrow_mut().plugin_waiting = false;
            return;
        }
        {
            let core = self.core.borrow();
            if core.session.is_none() || core.layout_pending || core.job.is_some() {
                return;
            }
        }
        if !self.plugin_attached || env.services.plugins.reader_book != Some(self.book) {
            if self.plugin_failed {
                return;
            }
            if self.plugin_attached {
                // Another reader took the plugins. Start again with a new host.
                self.plugin_attached = false;
                self.plugin_host = None;
            }
            let host = match &self.plugin_host {
                Some(host) => host.clone(),
                None => match plugins::Host::new(self.core.clone(), &env.services.paths.data_dir) {
                    Ok(host) => {
                        let host = Rc::new(RefCell::new(host));
                        self.plugin_host = Some(host.clone());
                        host
                    }
                    Err(e) => {
                        self.plugin_failed = true;
                        env.notice(tr_format(
                            &tr("Cannot connect reader plugins: %1"),
                            &[&e.to_string()],
                        ));
                        return;
                    }
                },
            };
            host.borrow().tick();
            let failure = host.borrow().failure();
            if let Some(e) = failure {
                self.plugin_failed = true;
                self.plugin_host = None;
                self.core.borrow_mut().plugin_waiting = false;
                env.notice(tr_format(&tr("Cannot connect reader plugins: %1"), &[&e]));
                return;
            }
            if !host.borrow().ready() {
                self.core.borrow_mut().plugin_waiting = true;
                schedule_if_pending(&self.core, env.ctx);
                return;
            }
            env.services
                .plugins
                .requests
                .push(crate::plugins::Request::Reader(host));
            self.plugin_attached = true;
            env.services.plugins.reader_book = Some(self.book);
        }
        let Some(host) = self.plugin_host.clone() else {
            return;
        };
        host.borrow().tick();
        let page = self.core.borrow().session.as_ref().map(|s| s.current().page + 1);
        if page != self.plugin_page {
            self.plugin_page = page;
            env.services
                .plugins
                .event("PageUpdate", vec![Value::from(page.unwrap_or(1) as i64)]);
        }
        let layout = self.core.borrow().session.as_ref().map(|s| s.layout().clone());
        if self.plugin_layout.is_some() && self.plugin_layout != layout {
            self.plugin_rerender = true;
        }
        self.plugin_layout = layout;
        let laying_out = host.borrow().laying_out();
        let ready = host.borrow().ready();
        if self.plugin_rerender && ready {
            self.plugin_rerender = false;
            if let Some(e) = host.borrow().layout_error() {
                eprintln!("kaia: plugin document layout: {e}");
            }
            env.services.plugins.event("DocumentRerendered", vec![]);
        }
        self.core.borrow_mut().plugin_waiting = !ready;
        schedule_if_pending(&self.core, env.ctx);
        // Annotation order needs the engine lock, which a plugin layout holds.
        // Read the counts after that layout, so the app thread does not wait.
        if !laying_out {
            let mut core = self.core.borrow_mut();
            let session = core.session.as_mut().unwrap();
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
        }
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
                    tr("Use synced position?"),
                    tr_format(
                        &tr("Another device is at %1 percent."),
                        &[&format!("{:.0}", remote.percentage * 100.0)],
                    ),
                    vec![
                        kaia_ui::composite::DialogButton::new(tr("Keep this position"), "dialog:dismiss"),
                        kaia_ui::composite::DialogButton::new(tr("Go there"), "reader:sync:accept"),
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
                    tr("This book's saved state needs a newer Kaia. Changes cannot be saved.")
                }
                kaia_book::StateIssue::Corrupt => {
                    tr("The saved state was damaged. Kaia will save edits in a recovered file.")
                }
                kaia_book::StateIssue::Locked => {
                    tr("Another process has this book open. Changes cannot be saved.")
                }
            };
            env.toast(message, 6.0);
        }
        if let Some(error) = session.save_error() {
            env.toast(tr_format(&tr("Cannot save this book: %1"), &[error]), 6.0);
        }
        if session.state_is_stale() || session.migration_report().is_some() {
            let count = session
                .migration_report()
                .map_or(0, |r| r.annotations_need_reanchoring);
            env.dialog(kaia_ui::composite::Dialog::new(
                env.theme(),
                tr("Book file changed"),
                tr_format(
                    &tr_n(
                        "Saved progress was kept. Check the reading position and %1 annotation.",
                        "Saved progress was kept. Check the reading position and %1 annotations.",
                        count as i64,
                    ),
                    &[&count.to_string()],
                ),
                vec![
                    kaia_ui::composite::DialogButton::new(tr("Review later"), "dialog:dismiss"),
                    kaia_ui::composite::DialogButton::new(tr("Reviewed"), "reader:acknowledge-change"),
                ],
            ));
        }
    }
}

impl Surface for ReaderSurface {
    fn busy(&self) -> bool {
        #[cfg(feature = "lua")]
        if self.plugin_attaching() {
            return true;
        }
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
                env.notice(tr_format(&tr("no book %1"), &[&self.book.to_string()]));
                return None;
            }
        };
        let screen = env.screen;
        let mut opts = OpenOptions::new(env.services.paths.data_dir.clone());
        let footer = footer::Footer::from_settings(&env.services.settings);
        opts.viewport = footer.viewport(screen, env.theme());
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
            c.footer_height = footer.height(env.theme());
            c.viewport = opts.viewport;
            c.footer = footer;
            c.color_panel = color_panel;
            c.refresh_every = crate::surfaces::settings::keys::full_refresh_pages(&env.services.settings);
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
                env.notice(tr_format(&tr("cannot record the open: %1"), &[&e.to_string()]));
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
        navigation::interrupt(&self.core, &ev.name, env.ctx);
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
        if ev.name == "ScreenResized" {
            // A rotation. The session is on the worker while a job runs, so the
            // event waits for it, as the sync events above do.
            if self.core.borrow().job.is_some() {
                self.core.borrow_mut().events.push_back(ev.clone());
                return Outcome::Handled;
            }
            let dimension = |i: usize| ev.args.get(i).and_then(Value::as_i64).unwrap_or(0).max(0) as u32;
            let size = kaia_gfx::Size::new(dimension(0), dimension(1));
            let mut c = self.core.borrow_mut();
            let size = c.footer.viewport(size, env.theme());
            c.footer_height = c.footer.height(env.theme());
            c.viewport = size;
            let result = c
                .session
                .as_mut()
                .map(|s| s.set_viewport(size).map(|()| s.is_reflowable()));
            match result {
                // A reflowable book lays out again for the new size. A fixed-layout
                // book fits its pages to the new size on the next paint.
                Some(Ok(reflowable)) => c.layout_pending |= reflowable,
                Some(Err(e)) => env.notice(tr_format(&tr("cannot resize the page: %1"), &[&e.to_string()])),
                None => {}
            }
            c.repaint_page(env.ctx, PaintHint::Full);
            drop(c);
            schedule_if_pending(&self.core, env.ctx);
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
        if let Some(outcome) = navigation::command(&self.core, cmd, args, env) {
            return outcome;
        }
        if cmd.starts_with("reader:") && self.core.borrow().job.is_some() {
            self.core
                .borrow_mut()
                .queued
                .push_back((cmd.to_string(), args.to_vec()));
            return Outcome::Handled;
        }
        if let Some(outcome) = sheet_command(&self.core, cmd, env) {
            schedule_if_pending(&self.core, env.ctx);
            return outcome;
        }
        if cmd == "reader:dictionary-word" {
            if let Some(word) = args.first().and_then(Value::as_str) {
                let mut c = self.core.borrow_mut();
                let preferred = preferred_dictionaries(&c);
                return open_dictionary(&self.core, &mut c, env, word, None, preferred);
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
                    env.toast(
                        tr_format(&tr("Cannot save confirmation: %1"), &[&e.to_string()]),
                        5.0,
                    );
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
                        .ok_or(tr("No book is open."))
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
                                env.toast(tr("Long-press the new place, then choose Sticker."), 6.0);
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
                            env.toast(tr("This sticker belongs to another book."), 4.0);
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
            env.toast(tr("The page is still loading. Try again when it finishes."), 3.0);
            return false;
        }
        self.persistence.save(&mut self.core.borrow_mut(), env)
    }

    fn on_hide(&mut self, env: &mut SurfaceEnv<'_>) {
        navigation::interrupt(&self.core, "Close", env.ctx);
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
                if let Some(link) = c
                    .links
                    .iter()
                    .find(|(rect, _)| rect.contains(Point::new(x as i32, y as i32)))
                    .map(|(_, link)| link.clone())
                {
                    if let Some(text) = link.note {
                        let window = panels::link_text(env.services, env.screen, &tr("Footnote"), &text);
                        return push_panel(rc, &mut c, env.ctx, window);
                    }
                    match link.target {
                        kaia_book::LinkTarget::Page(page) => {
                            c.go_to(env.ctx, link.location.unwrap_or(kaia_book::Location::Page(page)));
                            return Outcome::Handled;
                        }
                        kaia_book::LinkTarget::Uri(uri) => {
                            let text = tr_format(&tr("Invalid or external link:\n%1"), &[&uri]);
                            let window = panels::link_text(env.services, env.screen, &tr("Link"), &text);
                            return push_panel(rc, &mut c, env.ctx, window);
                        }
                    }
                }
                if let Some(annotation) = highlight_at(&mut c, x, y) {
                    let window = panels::highlight_sheet(env.services, env.screen, &annotation);
                    return push_panel(rc, &mut c, env.ctx, window);
                }
                let w = f64::from(env.screen.w);
                let h = f64::from(env.screen.h);
                // The top right corner is the dog-ear. It bookmarks the page, as KOReader's does.
                let corner = f64::from(env.theme().touch_target);
                if x >= w - corner && y < corner {
                    return bookmark(&mut c, env);
                }
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
            Command::Back => {
                c.back();
                if c.overlay.is_some() {
                    Outcome::Navigate(Nav::Pop)
                } else {
                    Outcome::Handled
                }
            }
            Command::GotoOpen => {
                let window = panels::go_to(env.services, env.screen);
                show_panel(rc, &mut c, env.ctx, window, true)
            }
            Command::GotoPage | Command::GotoPercent => {
                let Some(layer) = c.panel.as_ref() else {
                    return Outcome::Handled;
                };
                let text = read_field(&layer.window, env.ctx, "goto-field").unwrap_or_default();
                let count = c.session.as_ref().map_or(0, |s| s.page_count());
                let percent = command == Command::GotoPercent || text.trim().ends_with('%');
                let Some(page) = navigation::entered_page(&text, percent, count) else {
                    env.toast(tr("Invalid page number or percentage"), 3.0);
                    return Outcome::Handled;
                };
                c.go_to(env.ctx, kaia_book::Location::Page(page));
                Outcome::Navigate(Nav::Pop)
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
                    let (drawer, color) = highlight_style(env.services, c.color_panel);
                    let edge = panels::edge_for(env.screen, c.selection.as_ref());
                    let window = panels::selection_popover(
                        env.services,
                        env.screen,
                        &word,
                        c.moving_sticker.is_some(),
                        (drawer.as_str(), &color),
                        edge,
                    );
                    return push_panel(rc, &mut c, env.ctx, window);
                }
                Outcome::Handled
            }
            Command::Overlay => open_overlay(rc, &mut c, env),
            Command::Bookmark => bookmark(&mut c, env),
            Command::Footer(item, on) => {
                let was_visible = c.footer.is_visible();
                c.footer.set(item, on);
                if c.footer.is_visible() != was_visible {
                    let viewport = c.footer.viewport(env.screen, env.theme());
                    if let Some(session) = c.session.as_mut() {
                        match session.set_viewport(viewport) {
                            Ok(()) => c.layout_pending |= session.is_reflowable(),
                            Err(e) => {
                                c.footer.set(item, !on);
                                env.notice(tr_format(&tr("Cannot resize the page. %1"), &[&e.to_string()]));
                                return Outcome::Handled;
                            }
                        }
                    }
                    c.viewport = viewport;
                    c.footer_height = c.footer.height(env.theme());
                }
                env.services.settings.save(item.key(), Value::Bool(on));
                save_settings(env);
                c.refresh_overlay(env.ctx);
                c.repaint_page(env.ctx, PaintHint::Full);
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
                    env.toast(tr("Type a word to search."), 2.0);
                    return Outcome::Handled;
                }
                let started = c.session.as_mut().map(|s| s.start_search(&text));
                match started {
                    Some(Ok(())) => c.update_search(env.ctx),
                    Some(Err(e)) => env.notice(tr_format(&tr("cannot search: %1"), &[&e.to_string()])),
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
                // The Footer sheet replaces the open panel, More, as the word list does.
                // Other panels wait until the open one closes.
                let replace = name == PanelName::Footer;
                if c.panel.is_some() && !replace {
                    return Outcome::Handled;
                }
                let window = match name {
                    PanelName::Contents => {
                        let tab = c.contents_tab;
                        contents_window(&mut c, env, tab)
                    }
                    PanelName::Layout => layout_window(&c, env),
                    PanelName::More => panels::more(env.services, env.screen),
                    PanelName::Footer => panels::footer_sheet(env.services, env.screen, &c.footer),
                };
                show_panel(rc, &mut c, env.ctx, window, replace)
            }
            Command::PanelClose => Outcome::Navigate(Nav::Pop),
            Command::FontList => {
                // The list is pushed, not shown in place of the sheet. The sheet stays
                // the open panel, so picking a face can update its Font button.
                if c.panel.is_none() {
                    return Outcome::Handled;
                }
                // Fonts copied into the user folder since the last scan show up in the list.
                crate::surfaces::settings::commands::rescan_user_fonts();
                let face = c.session.as_ref().and_then(|s| s.layout().font_family.clone());
                let window = panels::font_list(env.services, env.screen, panels::font_index(face.as_deref()));
                let serial = next_serial(&mut c);
                Outcome::Navigate(Nav::Push(panels::panel_surface(window, serial, rc.clone())))
            }
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
                    env.toast(tr("This entry has no page."), 2.0);
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
                let font = matches!(change, LayoutChange::Font(_));
                let result = match change {
                    LayoutChange::Font(index) => {
                        // The built-in faces come from `font_face`, the user faces from the list.
                        let face = font_face(index).or_else(|| panels::user_face_at(index));
                        // The new face brings the weight saved for it, regular when it has none.
                        let weight = crate::surfaces::settings::keys::font_weight(
                            &env.services.settings,
                            face.as_deref(),
                        );
                        let result = c.change_layout(|l| {
                            l.font_family = face;
                            l.font_weight = weight as f32;
                        });
                        // The font list closes. The sheet below shows the new face on its button.
                        if result.is_ok() {
                            if let Some(layer) = c.panel.clone() {
                                let name = panels::font_choice_label(index);
                                layer
                                    .window
                                    .update::<Button, _>(env.ctx, "layout-font", |button, cx| {
                                        button.set_text(name);
                                        cx.request_layout();
                                    });
                            }
                        }
                        result
                    }
                    LayoutChange::Size(size) => c.change_layout(|l| l.font_size = size as f32),
                    LayoutChange::Weight(index) => {
                        let value = panels::font_weight_at(index);
                        let family = c.session.as_ref().and_then(|s| s.layout().font_family.clone());
                        let result = c.change_layout(|l| l.font_weight = value);
                        if result.is_ok() {
                            // The weight is kept for this face, so the next book in it starts there.
                            let key = crate::surfaces::settings::keys::font_weight_key(family.as_deref());
                            env.services.settings.save(key, Value::from(f64::from(value)));
                            save_settings(env);
                        }
                        result
                    }
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
                match result {
                    Err(message) => {
                        env.toast(message, 3.0);
                        Outcome::Handled
                    }
                    Ok(()) if font => Outcome::Navigate(Nav::Pop),
                    Ok(()) => Outcome::Handled,
                }
            }
            Command::Pinch {
                larger,
                distance,
                axis,
            } => {
                // Only a reflowable book has a text size. On a fixed-layout book the
                // gesture does nothing, as the Text and layout sheet does not open for it.
                let Some(current) = c
                    .session
                    .as_ref()
                    .filter(|s| s.is_reflowable())
                    .map(|s| s.layout().font_size)
                else {
                    return Outcome::Handled;
                };
                // KOReader's limits are 12 to 40 units. A pinch never grows the text and a
                // spread never shrinks it, so a size above the limit stays where it is.
                let units = commands::whole_units(env.screen, current);
                let step = commands::pinch_step(distance, axis, env.screen);
                let target = if larger {
                    (units + step).min(40.0).max(units)
                } else {
                    (units - step).max(12.0).min(units)
                };
                let px = koreader_px(env.screen, target);
                match c.change_layout(|l| l.font_size = px) {
                    Ok(()) => env.toast(
                        tr_format(&tr("Font size set to %1 px."), &[&(px as i64).to_string()]),
                        2.0,
                    ),
                    Err(message) => env.toast(message, 3.0),
                };
                Outcome::Handled
            }
            Command::SelHighlight => highlight_selection(&mut c, env, None),
            Command::SelColor(name) => {
                if kaia_book::highlight_rgb(&name).is_none() {
                    return Outcome::Handled;
                }
                env.services
                    .settings
                    .save(HIGHLIGHT_COLOR_KEY, Value::from(name.as_str()));
                save_settings(env);
                highlight_selection(&mut c, env, Some(&name))
            }
            Command::SelMore => {
                if c.selection.is_none() {
                    return Outcome::Handled;
                }
                let (drawer, _) = highlight_style(env.services, c.color_panel);
                let edge = panels::edge_for(env.screen, c.selection.as_ref());
                let window = panels::selection_more(env.services, env.screen, edge, drawer.as_str());
                show_panel(rc, &mut c, env.ctx, window, true)
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
                    env.toast(tr("Copied"), 2.0);
                    c.clear_selection(env.ctx);
                }
                Outcome::Navigate(Nav::Pop)
            }
            Command::SelDict => {
                let Some(selection) = c.selection.clone() else {
                    return Outcome::Handled;
                };
                let word = kaia_dict::clean_selection(&selection.text);
                // Read the session before the selection clears. Clearing repaints the
                // page, and the repaint takes the session until its frame comes back.
                let context = c
                    .session
                    .as_mut()
                    .and_then(|s| s.selection_sentence(&selection).ok().flatten());
                let preferred = preferred_dictionaries(&c);
                c.clear_selection(env.ctx);
                open_dictionary(rc, &mut c, env, &word, context, preferred)
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
                    env.notice(tr_format(&tr("cannot search: %1"), &[&e.to_string()]));
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
                    let (drawer, color) = highlight_style(env.services, c.color_panel);
                    let result = c
                        .session
                        .as_mut()
                        .map(|s| s.add_highlight(&selection, drawer, &color, note.as_deref()));
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
                        env.toast(
                            tr_format(&tr("Cannot mark the book as finished: %1"), &[&e.to_string()]),
                            5.0,
                        );
                    }
                    Some(Ok(())) => {
                        if let Err(e) = crate::book_index::mark_finished(env.services, c.book) {
                            env.notice(e);
                            return Outcome::Handled;
                        }
                        env.toast(tr("Marked as finished"), 2.0);
                    }
                    None => return Outcome::Handled,
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
    // The clock and battery are read when the bars open. They do not redraw on their own.
    c.status = footer::Status::now(env.services.platform.battery.capacity());
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

/// Adds the bookmark on the current page, or removes it. The overlay shows the new
/// state at once, and the page repaints so its dog-ear follows.
fn bookmark(c: &mut Core, env: &mut SurfaceEnv<'_>) -> Outcome {
    let result = c.session.as_mut().map(|s| s.toggle_bookmark());
    match result {
        Some(Ok(added)) => {
            let message = if added {
                tr("Bookmark added")
            } else {
                tr("Bookmark removed")
            };
            env.toast(message, 2.0);
            c.refresh_overlay(env.ctx);
            c.repaint_page(env.ctx, PaintHint::Partial);
        }
        Some(Err(e)) => env.notice(tr_format(
            &tr("cannot change the bookmark: %1"),
            &[&e.to_string()],
        )),
        None => {}
    }
    Outcome::Handled
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

/// Highlights the selection with the saved drawer, in `color` or else in the saved
/// color. The popover closes either way.
fn highlight_selection(c: &mut Core, env: &mut SurfaceEnv<'_>, color: Option<&str>) -> Outcome {
    let Some(selection) = c.selection.clone() else {
        return Outcome::Handled;
    };
    let (drawer, saved) = highlight_style(env.services, c.color_panel);
    let color = color.unwrap_or(&saved);
    let result = c
        .session
        .as_mut()
        .map(|s| s.add_highlight(&selection, drawer, color, None));
    finish_highlight(c, env, result);
    Outcome::Navigate(Nav::Pop)
}

fn finish_highlight(
    c: &mut Core,
    env: &mut SurfaceEnv<'_>,
    result: Option<kaia_book::Result<kaia_book::AnnotationId>>,
) {
    match result {
        Some(Ok(_)) => {
            c.clear_selection(env.ctx);
            env.toast(tr("Highlighted"), 2.0);
        }
        Some(Err(e)) => {
            env.toast(tr_format(&tr("Cannot highlight. %1"), &[&e.to_string()]), 3.0);
        }
        None => {}
    }
}

/// Settings keys of the last highlight choice. A new highlight takes them.
const HIGHLIGHT_COLOR_KEY: &str = "reading.highlight_color";
const HIGHLIGHT_DRAWER_KEY: &str = "reading.highlight_drawer";

/// The drawer and the color for a new highlight: the last choice in the settings, or
/// KOReader's defaults. KOReader's default color is yellow on a color panel and gray
/// on a grayscale one.
fn highlight_style(services: &Services, color_panel: bool) -> (Drawer, String) {
    let drawer = services
        .settings
        .read(HIGHLIGHT_DRAWER_KEY)
        .as_str()
        .and_then(Drawer::parse)
        .unwrap_or(Drawer::Lighten);
    let fallback = if color_panel { "yellow" } else { "gray" };
    let color = services
        .settings
        .read(HIGHLIGHT_COLOR_KEY)
        .as_str()
        .filter(|name| kaia_book::highlight_rgb(name).is_some())
        .unwrap_or(fallback)
        .to_string();
    (drawer, color)
}

/// The highlight under a tap, if there is one. Bookmarks have no area, so they never match.
fn highlight_at(c: &mut Core, x: f64, y: f64) -> Option<kaia_book::Annotation> {
    let point = Point::new(x as i32, y as i32);
    let viewport = c.viewport;
    let session = c.session.as_mut()?;
    // Listing the highlights costs engine calls, so a book with none skips it.
    if !session.has_highlights() {
        return None;
    }
    let items = session.annotations_on_page(viewport).ok()?;
    items
        .into_iter()
        .find(|item| !item.annotation.is_bookmark() && item.rects.iter().any(|r| r.contains(point)))
        .map(|item| item.annotation)
}

fn save_settings(env: &mut SurfaceEnv<'_>) {
    if let Err(e) = env.services.settings.flush(&env.services.settings_path()) {
        env.notice(tr_format(&tr("Cannot save settings: %1"), &[&e.to_string()]));
    }
}

/// The commands of the highlight sheets, the dictionary sheet's actions, the
/// Dictionary settings screen and the word list. None for any other command.
fn sheet_command(rc: &Rc<RefCell<Core>>, cmd: &str, env: &mut SurfaceEnv<'_>) -> Option<Outcome> {
    let parts: Vec<&str> = cmd.strip_prefix("reader:")?.split(':').collect();
    Some(match parts.as_slice() {
        ["highlight", "color", name] => default_style(rc, env, Some(*name), None),
        ["highlight", "drawer", name] => default_style(rc, env, None, Some(*name)),
        ["annot", id, "color", name] => annotation_style(rc, env, id, None, Some(*name)),
        ["annot", id, "drawer", name] => annotation_style(rc, env, id, Some(*name), None),
        ["dict", "first"] => prefer_dictionary(rc, env),
        ["dicts", "open"] => open_dictionary_settings(rc, env),
        ["dicts", action @ ("toggle" | "up" | "down"), index] => move_dictionary(rc, env, action, index),
        ["words", "open"] => open_words(rc, env),
        ["words", "add"] => add_word(rc, env),
        ["words", "delete", id] => delete_word(rc, env, id),
        _ => return None,
    })
}

/// Saves a color or a drawer as the default for new highlights, then shows the
/// selection popover again with the choice marked.
fn default_style(
    rc: &Rc<RefCell<Core>>,
    env: &mut SurfaceEnv<'_>,
    color: Option<&str>,
    drawer: Option<&str>,
) -> Outcome {
    if let Some(name) = color {
        if kaia_book::highlight_rgb(name).is_none() {
            return Outcome::Handled;
        }
        env.services.settings.save(HIGHLIGHT_COLOR_KEY, Value::from(name));
    }
    if let Some(drawer) = drawer.and_then(Drawer::parse) {
        env.services
            .settings
            .save(HIGHLIGHT_DRAWER_KEY, Value::from(drawer.as_str()));
    }
    save_settings(env);
    let mut c = rc.borrow_mut();
    let Some(word) = c.selection.as_ref().map(|s| s.text.clone()) else {
        return Outcome::Handled;
    };
    let (drawer, color) = highlight_style(env.services, c.color_panel);
    let edge = panels::edge_for(env.screen, c.selection.as_ref());
    let window = panels::selection_popover(
        env.services,
        env.screen,
        &word,
        c.moving_sticker.is_some(),
        (drawer.as_str(), &color),
        edge,
    );
    show_panel(rc, &mut c, env.ctx, window, true)
}

/// Changes the color or the drawer of a highlight, then shows its sheet again. The
/// page is drawn again, so the marks follow the new choice.
fn annotation_style(
    rc: &Rc<RefCell<Core>>,
    env: &mut SurfaceEnv<'_>,
    id: &str,
    drawer: Option<&str>,
    color: Option<&str>,
) -> Outcome {
    let Ok(id) = id.parse::<u64>() else {
        return Outcome::Handled;
    };
    let drawer = match drawer {
        Some(name) => match Drawer::parse(name) {
            Some(parsed) => Some(parsed),
            None => return Outcome::Handled,
        },
        None => None,
    };
    if color.is_some_and(|name| kaia_book::highlight_rgb(name).is_none()) {
        return Outcome::Handled;
    }
    let mut c = rc.borrow_mut();
    let Some(session) = c.session.as_mut() else {
        return Outcome::Handled;
    };
    let Some(current) = session.annotation(id) else {
        return Outcome::Handled;
    };
    let drawer = drawer
        .or_else(|| current.drawer.as_deref().and_then(Drawer::parse))
        .unwrap_or(Drawer::Lighten);
    let color = color
        .map(str::to_string)
        .or(current.color.clone())
        .unwrap_or_else(|| "yellow".to_string());
    if let Err(e) = session.set_highlight_style(id, drawer, &color) {
        env.toast(
            tr_format(&tr("Cannot change this highlight: %1"), &[&e.to_string()]),
            3.0,
        );
        return Outcome::Handled;
    }
    let Some(updated) = session.annotation(id) else {
        return Outcome::Handled;
    };
    c.repaint_page(env.ctx, PaintHint::Ui);
    let window = panels::highlight_sheet(env.services, env.screen, &updated);
    show_panel(rc, &mut c, env.ctx, window, true)
}

/// The dictionaries this book tries first. Empty without a book.
fn preferred_dictionaries(c: &Core) -> Vec<String> {
    c.session
        .as_ref()
        .map(|s| s.preferred_dictionaries().to_vec())
        .unwrap_or_default()
}

/// Looks `word` up and shows the dictionary sheet. `context` is the sentence around
/// the word, when the reader has it, and `preferred` is the book's dictionaries that
/// go first. The word stays with the sheet, so its actions can use it.
fn open_dictionary(
    rc: &Rc<RefCell<Core>>,
    c: &mut Core,
    env: &mut SurfaceEnv<'_>,
    word: &str,
    context: Option<String>,
    preferred: Vec<String>,
) -> Outcome {
    let answer = dict::lookup(word, env.services, &preferred);
    #[cfg(feature = "lua")]
    env.services
        .plugins
        .requests
        .push(crate::plugins::Request::Dictionary(
            word.into(),
            answer.text.clone(),
        ));
    let preferred_now = answer
        .first
        .as_ref()
        .is_some_and(|first| preferred.contains(first));
    let window = panels::dictionary_sheet(
        env.services,
        env.screen,
        word,
        &answer.text,
        Some(panels::DictActions {
            first: answer.first.as_deref(),
            preferred: preferred_now,
        }),
    );
    c.lookup = Some(LookupState {
        word: word.to_string(),
        context,
        dictionary: answer.first,
        book_title: c.title.clone(),
    });
    show_panel(rc, c, env.ctx, window, true)
}

/// Makes the lookup's dictionary the first one for this book, or stops doing so. The
/// word is looked up again, so the sheet shows the new order.
fn prefer_dictionary(rc: &Rc<RefCell<Core>>, env: &mut SurfaceEnv<'_>) -> Outcome {
    let mut c = rc.borrow_mut();
    let Some(lookup) = c.lookup.clone() else {
        return Outcome::Handled;
    };
    let Some(name) = lookup.dictionary.clone() else {
        return Outcome::Handled;
    };
    let Some(session) = c.session.as_mut() else {
        return Outcome::Handled;
    };
    let mut names = session.preferred_dictionaries().to_vec();
    let added = !names.contains(&name);
    if added {
        names.insert(0, name.clone());
    } else {
        names.retain(|n| n != &name);
    }
    if let Err(e) = session.set_preferred_dictionaries(names) {
        env.toast(
            tr_format(&tr("Cannot save this book: %1"), &[&e.to_string()]),
            5.0,
        );
        return Outcome::Handled;
    }
    let preferred = session.preferred_dictionaries().to_vec();
    let answer = dict::lookup(&lookup.word, env.services, &preferred);
    let preferred_now = answer
        .first
        .as_ref()
        .is_some_and(|first| preferred.contains(first));
    let window = panels::dictionary_sheet(
        env.services,
        env.screen,
        &lookup.word,
        &answer.text,
        Some(panels::DictActions {
            first: answer.first.as_deref(),
            preferred: preferred_now,
        }),
    );
    let message = if added {
        tr_format(
            &tr("%1 is now the preferred dictionary for this document."),
            &[&name],
        )
    } else {
        tr_format(
            &tr("%1 is no longer a preferred dictionary for this document."),
            &[&name],
        )
    };
    env.toast(message, 2.0);
    c.lookup = Some(LookupState {
        word: lookup.word,
        context: lookup.context,
        dictionary: answer.first,
        book_title: lookup.book_title,
    });
    show_panel(rc, &mut c, env.ctx, window, true)
}

/// Saves the word of the dictionary sheet to the word list, with its book and sentence.
fn add_word(rc: &Rc<RefCell<Core>>, env: &mut SurfaceEnv<'_>) -> Outcome {
    let Some(lookup) = rc.borrow().lookup.clone() else {
        return Outcome::Handled;
    };
    if lookup.word.chars().count() > kaia_storage::WORD_MAX_CHARS {
        env.toast(tr("Select a single word to add it to the word list."), 3.0);
        return Outcome::Handled;
    }
    let now = crate::sync::now_unix();
    let saved = dict::word_store(env.services).and_then(|mut store| {
        store.add_word(
            &lookup.word,
            Some(&lookup.book_title),
            lookup.context.as_deref(),
            now,
        )
    });
    match saved {
        Ok(_) => {
            env.toast(tr("Added to word list"), 2.0);
        }
        Err(e) => env.notice(tr_format(&tr("Cannot save the word list: %1"), &[&e.to_string()])),
    }
    Outcome::Handled
}

/// The book and the sentence of a saved word, as one line of the word list.
fn detail_text(book: Option<&str>, context: Option<&str>) -> String {
    let context = context.map(|c| panels::shorten(c, 160));
    match (book, context) {
        (Some(book), Some(context)) => tr_format(&tr("%1: %2"), &[book, context.as_str()]),
        (Some(book), None) => book.to_string(),
        (None, Some(context)) => context,
        (None, None) => String::new(),
    }
}

/// The saved words, newest first, as rows of the word list.
fn word_rows(services: &Services) -> Result<Vec<panels::WordRow>, String> {
    let store = dict::word_store(services).map_err(|e| e.to_string())?;
    let words = store.list_words().map_err(|e| e.to_string())?;
    Ok(words
        .into_iter()
        .map(|w| panels::WordRow {
            id: w.id,
            detail: detail_text(w.book_title.as_deref(), w.context.as_deref()),
            date: kaia_book::local_date(w.created_at),
            word: w.word,
        })
        .collect())
}

/// Shows the word list in place of the panel that opened it.
fn open_words(rc: &Rc<RefCell<Core>>, env: &mut SurfaceEnv<'_>) -> Outcome {
    match word_rows(env.services) {
        Ok(rows) => {
            let window = panels::words(env.services, env.screen, rows);
            let mut c = rc.borrow_mut();
            show_panel(rc, &mut c, env.ctx, window, true)
        }
        Err(e) => {
            env.notice(tr_format(&tr("Cannot open the word list: %1"), &[&e]));
            Outcome::Handled
        }
    }
}

/// Removes a saved word, then shows the word list again.
fn delete_word(rc: &Rc<RefCell<Core>>, env: &mut SurfaceEnv<'_>, id: &str) -> Outcome {
    let Ok(id) = id.parse::<i64>() else {
        return Outcome::Handled;
    };
    let removed = dict::word_store(env.services).and_then(|mut store| store.delete_word(id));
    if let Err(e) = removed {
        env.notice(tr_format(&tr("Cannot save the word list: %1"), &[&e.to_string()]));
        return Outcome::Handled;
    }
    env.toast(tr("Removed from word list"), 2.0);
    open_words(rc, env)
}

/// The rows of the Dictionary settings screen, in lookup order. Call `dict::load` first.
fn dict_rows(services: &Services) -> Vec<panels::DictRow> {
    let off = dict::disabled(services);
    dict::ordered(services)
        .into_iter()
        .enumerate()
        .map(|(index, name)| panels::DictRow {
            index,
            enabled: !off.contains(&name),
            name,
        })
        .collect()
}

/// Shows the Dictionary settings screen in place of the dictionary sheet.
fn open_dictionary_settings(rc: &Rc<RefCell<Core>>, env: &mut SurfaceEnv<'_>) -> Outcome {
    dict::load(env.services);
    let rows = dict_rows(env.services);
    let hint = dict::install_hint(&env.services.paths.data_dir);
    let window = panels::dictionary_settings(env.services, env.screen, rows, &hint);
    let mut c = rc.borrow_mut();
    show_panel(rc, &mut c, env.ctx, window, true)
}

/// Turns a dictionary on or off, or moves it up or down the lookup order, then shows
/// the screen again. `index` is the row's place in the lookup order.
fn move_dictionary(rc: &Rc<RefCell<Core>>, env: &mut SurfaceEnv<'_>, action: &str, index: &str) -> Outcome {
    let change = match action {
        "toggle" => dict::Change::Toggle,
        "up" => dict::Change::Up,
        "down" => dict::Change::Down,
        _ => return Outcome::Handled,
    };
    let Ok(index) = index.parse::<usize>() else {
        return Outcome::Handled;
    };
    if !dict::apply(env.services, index, change) {
        return Outcome::Handled;
    }
    save_settings(env);
    open_dictionary_settings(rc, env)
}

fn search_status(c: &Core) -> String {
    match c.session.as_ref() {
        Some(s) if !s.search_results().is_empty() => {
            let count = s.search_results().len();
            tr_format(
                &tr_n("%1 match.", "%1 matches.", count as i64),
                &[&count.to_string()],
            )
        }
        _ => tr("Type a word and press Search."),
    }
}

/// The Contents panel for `tab`, built from the session.
fn contents_window(c: &mut Core, env: &SurfaceEnv<'_>, tab: usize) -> Window {
    let (items, empty) = match c.session.as_mut() {
        None => (Vec::new(), tr("No book is open.")),
        Some(session) => match tab {
            1 => {
                let marks = session.bookmarks().unwrap_or_default();
                (
                    panels::annotation_items(&marks, true),
                    tr("No bookmarks yet. Use the bookmark button in the top bar."),
                )
            }
            2 => {
                let all = session.annotations().unwrap_or_default();
                let highlights: Vec<_> = all.into_iter().filter(|a| !a.is_bookmark()).collect();
                (
                    panels::annotation_items(&highlights, false),
                    tr("No highlights yet. Hold a word to select it."),
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
                        detail: entry
                            .page
                            .map(|p| tr_format(&tr("Page %1"), &[&(p + 1).to_string()]))
                            .unwrap_or_default(),
                        current: current.as_deref() == Some(entry.title.as_str()),
                        action: format!("reader:contents:goto:{index}"),
                        level: entry.level.saturating_sub(1),
                    })
                    .collect();
                (items, tr("This book has no table of contents."))
            }
        },
    };
    panels::contents(env.services, env.screen, tab.min(2), items, &empty)
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
