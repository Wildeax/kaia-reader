//! The reader's shared state: the book session, the windows the reader updates,
//! the selection and the refresh counter.
//!
//! The surface, the overlay and the panels share a [`Core`] through an `Rc`. The
//! app runs on one thread, and no borrow of the core lasts past a call. Scheduled
//! pump ticks hold the same `Rc`. A tick runs only while the engine has work (a
//! layout change or a search), so an idle reader schedules nothing.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use crate::slider::Slider;
use kaia_book::{BookEvent, BookSession, OpenOptions, Point, Selection, Size, TextLayout, ZoomMode};
use kaia_events::Ctx;
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_ui::components::Label;
use kaia_ui::controls::{TextField, ToolButton};
use kaia_ui::widget::PaintHint;
use kaia_ui::Window;

use super::footer::{Footer, Status};
use super::overlay::{bookmark_label, OverlayModel};
use super::page::{PageView, PAGE_KEY};
use super::panels::{search_rows, SearchRow, FONT_CHOICES};
use super::work::WorkError;
use kaia_ui::composite::PagedList;

/// Seconds between pump ticks while the engine has work.
const PUMP_SECONDS: f64 = 0.02;

/// The image share from which a page is an image page. The reader takes a full
/// refresh on a turn to such a page and on the turn after it, so the ghosting from
/// the picture clears. Half the page area is "mostly an image" (B-7). KOReader's
/// clearing check uses 0.075 (`frontend/apps/reader/modules/readerview.lua`), and
/// the Nickel parity notes give no number (`docs/kaia/nickel/PARITY.md` T3-6).
pub const IMAGE_PAGE_SHARE: f32 = 0.5;

/// A window the reader updates, with the serial its surface uses to clear it.
#[derive(Clone)]
pub struct Layer {
    pub serial: u64,
    pub window: Window,
}

/// What a page turn did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    Moved,
    /// Forward at the last page. The reader offers the end of book sheet.
    AtEnd,
    /// No page change: the first page, a layout change in progress, or no book.
    Blocked,
}

/// The word the dictionary sheet shows, kept for the sheet's actions: the word list,
/// the book's dictionary order and the lookup again after a change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LookupState {
    pub word: String,
    /// The sentence around the word, when the reader had the page text.
    pub context: Option<String>,
    /// The dictionary of the first definition, if the lookup found one.
    pub dictionary: Option<String>,
    /// The title of the book the word was looked up in.
    pub book_title: String,
}

pub struct Core {
    pub(super) gestures: super::navigation::Gestures,
    pub book: i64,
    pub last_page_turn: i64,
    pub periodic_sync: kaia_sync::PeriodicPush,
    pub title: String,
    pub session: Option<BookSession>,
    pub(super) job: Option<super::work::Reply>,
    pub(super) queued: std::collections::VecDeque<(String, Vec<kaia_events::Value>)>,
    pub(super) opened: bool,
    pub(super) events: std::collections::VecDeque<kaia_events::Event>,
    pending_hint: PaintHint,
    /// The book area, excluding the header and footer strips.
    pub viewport: Size,
    pub footer_height: u32,
    pub header_height: u32,
    pub header: super::header::Header,
    /// Why the book cannot be read, shown on the page.
    pub error: Option<String>,
    pub page_window: Option<Window>,
    pub overlay: Option<Layer>,
    pub panel: Option<Layer>,
    pub next_serial: u64,
    pub(super) panel_name: Option<super::commands::PanelName>,
    pub(super) advanced: Option<super::typography::Advanced>,
    pub(super) ink_on: bool,
    pub(super) overview: Option<super::zoom::Overview>,
    pub pages_since_full: u32,
    /// Page turns between full refreshes, from `reading.full_refresh_pages`. Zero
    /// means only chapter starts and picture pages take a full refresh.
    pub refresh_every: u32,
    pub selecting: bool,
    pub selection: Option<Selection>,
    pub sticker_anchor: Option<kaia_storage::Anchor>,
    pub moving_sticker: Option<String>,
    pub editing_sticker: Option<String>,
    pub stickers: Vec<super::stickers::Drawing>,
    pub links: Vec<(kaia_gfx::Rect, kaia_book::Link)>,
    pub search_text: String,
    pub(super) search_scope: super::search::Scope,
    search_painted: Option<(u64, usize, bool)>,
    search_paint_at: std::time::Duration,
    search_dirty: bool,
    pub contents_tab: usize,
    pub(super) annotation_filter: usize,
    pub(super) annotation_reverse: bool,
    /// The word of the open dictionary sheet. None when no sheet has looked up a word.
    pub lookup: Option<LookupState>,
    pub color_panel: bool,
    pub ink_color_content: bool,
    /// Seconds of reading per page, for the time-left line. Set when the book opens.
    pub seconds_per_page: f64,
    /// The items of the reader footer that are on.
    pub footer: Footer,
    /// The clock and battery for the overlay, read when the bars open.
    pub status: Status,
    pub pump_due: bool,
    pub layout_pending: bool,
    pub closed: bool,
    /// True while reader plugins wait for their document thread. The pump keeps
    /// ticking then, so the reader sees the answer without input.
    pub(super) plugin_waiting: bool,
    /// The page of the last frame, and whether that page is an image page. The
    /// image rule compares the next frame with it.
    shown: Option<(u32, bool)>,
    /// Set by the open job: true when the book had no saved or imported state.
    fresh: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Core {
    pub fn new(book: i64) -> Self {
        Self {
            gestures: super::navigation::Gestures::default(),
            book,
            last_page_turn: 0,
            periodic_sync: kaia_sync::PeriodicPush::default(),
            title: String::new(),
            session: None,
            job: None,
            queued: std::collections::VecDeque::new(),
            opened: false,
            events: std::collections::VecDeque::new(),
            pending_hint: PaintHint::Full,
            viewport: Size { w: 1, h: 1 },
            footer_height: 0,
            header_height: 0,
            header: super::header::Header::default(),
            error: None,
            page_window: None,
            overlay: None,
            panel: None,
            next_serial: 0,
            panel_name: None,
            advanced: None,
            ink_on: false,
            overview: None,
            pages_since_full: 0,
            refresh_every: 0,
            shown: None,
            selecting: false,
            selection: None,
            sticker_anchor: None,
            moving_sticker: None,
            editing_sticker: None,
            stickers: Vec::new(),
            links: Vec::new(),
            search_text: String::new(),
            search_scope: super::search::Scope::Book,
            search_painted: None,
            search_paint_at: std::time::Duration::ZERO,
            search_dirty: false,
            contents_tab: 0,
            annotation_filter: 0,
            annotation_reverse: false,
            lookup: None,
            color_panel: false,
            ink_color_content: false,
            pump_due: false,
            layout_pending: false,
            seconds_per_page: 60.0,
            footer: Footer::default(),
            status: Status::default(),
            closed: false,
            plugin_waiting: false,
            fresh: Default::default(),
        }
    }

    /// Opens the book at `path` for `viewport`. A failure becomes the page message.
    pub fn open(&mut self, path: &Path, opts: OpenOptions) {
        if self.job.is_some() {
            return;
        }
        let path = path.to_owned();
        let fresh = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.fresh = fresh.clone();
        self.job = Some(super::work::spawn(self.viewport, move || {
            let session = BookSession::open(path, opts).map_err(|e| WorkError::Open(e.to_string()))?;
            // Opening writes a state file only for an import or a migration, and
            // the frame saves after this task. So the file is missing here exactly
            // when the book had no state of its own.
            let new = session.import_report().is_none()
                && session.migration_report().is_none()
                && session.state_issue().is_none()
                && !session.state_path().exists();
            fresh.store(new, std::sync::atomic::Ordering::Relaxed);
            Ok((session, None, None))
        }));
    }

    /// True when the open book had no saved or imported reading state before
    /// this open. KOReader calls such a book new.
    pub fn book_is_new(&self) -> bool {
        self.fresh.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The reading progress to store in the library, or None without a book.
    pub fn percent(&self) -> Option<f64> {
        self.session.as_ref().map(|s| s.current().percent)
    }

    /// True while the engine has work: a layout change or a search.
    pub fn work_pending(&self) -> bool {
        self.job.is_some()
            || !self.events.is_empty()
            || !self.queued.is_empty()
            || self.search_dirty
            || self.layout_pending
            || self.plugin_waiting
            || self.session.as_ref().is_some_and(|s| s.search_running())
    }

    /// Clears the selection and repaints the page.
    pub fn clear_selection(&mut self, ctx: &mut Ctx) {
        self.selection = None;
        self.sticker_anchor = None;
        self.selecting = false;
        self.repaint_page(ctx, PaintHint::Partial);
    }

    /// Renders the current page into the page widget. A layout change in progress
    /// keeps the old frame: the widget is not touched until the layout applies.
    pub fn repaint_page(&mut self, _ctx: &mut Ctx, hint: PaintHint) {
        if hint == PaintHint::Full {
            self.pages_since_full = 0;
        }
        self.pending_hint = hint;
        if self.job.is_some() {
            return;
        }
        let Some(session) = self.session.take() else {
            return;
        };
        let selection = self.selection.clone();
        self.job = Some(super::work::spawn(self.viewport, move || {
            Ok((session, selection, None))
        }));
    }

    pub(super) fn set_pending_hint(&mut self, hint: PaintHint) {
        self.pending_hint = hint;
    }

    fn receive_frame(&mut self, ctx: &mut Ctx) {
        let Some(reply) = self.job.as_ref() else {
            return;
        };
        let result = match reply.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => Err(WorkError::Stopped),
        };
        self.job = None;
        let frame = match result {
            Ok(frame) => frame,
            Err(error) => {
                let error = error.message();
                self.error = Some(error.clone());
                self.queued.clear();
                if let Some(window) = &self.page_window {
                    window.update::<PageView, _>(ctx, PAGE_KEY, move |view, cx| {
                        view.message = Some(error);
                        cx.request_paint();
                    });
                }
                return;
            }
        };
        self.ink_color_content = frame
            .ink
            .as_ref()
            .is_some_and(|ink| ink.strokes.iter().any(|d| d.stroke.color != "black"));
        let info = frame.session.current();
        let image_page = frame
            .session
            .page_image_share()
            .is_some_and(|share| share >= IMAGE_PAGE_SHARE);
        // The share of the new page is known only now, so the image rule runs
        // here. A turn to an image page, or away from one, becomes a full refresh.
        // A quick turn keeps its fast frames, and a repaint of the same page keeps its hint.
        if let Some((shown, was_image)) = self.shown {
            if shown != info.page && self.pending_hint == PaintHint::Partial && (image_page || was_image) {
                self.pending_hint = PaintHint::Full;
                self.pages_since_full = 0;
            }
        }
        self.shown = Some((info.page, image_page));
        self.overview = frame.overview;
        self.session = Some(frame.session);
        self.selection = frame.selection;
        self.sticker_anchor = frame.sticker_anchor;
        self.stickers = frame.stickers.clone();
        self.links = frame.links;
        self.error = frame.error.map(|error| error.message());
        let Some(window) = self.page_window.clone() else {
            return;
        };
        let selection = self
            .selection
            .as_ref()
            .map(|s| s.screen_rects.clone())
            .unwrap_or_default();
        let hint = self.pending_hint;
        let error = self.error.clone();
        let chapter_progress = self
            .session
            .as_ref()
            .and_then(|session| super::caption::ChapterProgress::from_toc(&info, session.toc()));
        let logical = self.session.as_ref().and_then(|s| s.logical_progress());
        let mut caption_info = info.clone();
        let chapter_progress = if let Some(logical) = logical {
            caption_info.page = logical.page - 1;
            caption_info.page_count = logical.total as usize;
            Some(super::caption::ChapterProgress {
                page: logical.chapter_page,
                total: logical.chapter_total,
            })
        } else {
            chapter_progress
        };
        let header = self.header.clone();
        let title = self.title.clone();
        let header_height = self.header_height;
        let footer = self.footer.clone();
        let pace = self.seconds_per_page;
        let footer_height = self.footer_height;
        let fixed_layout = self.session.as_ref().is_some_and(|s| !s.is_reflowable());
        let zoomed = fixed_layout
            && self
                .session
                .as_mut()
                .and_then(|s| s.page_transform(self.viewport).ok())
                .is_some_and(|t| t.full_image.w > self.viewport.w || t.full_image.h > self.viewport.h);
        let ink_color_content = &mut self.ink_color_content;
        window.update::<PageView, _>(ctx, PAGE_KEY, move |view, cx| {
            view.fixed_layout = fixed_layout;
            view.zoomed = zoomed;
            if frame.page.is_some() {
                view.page = frame.page;
            }
            view.message = error;
            view.set_header(header, title, caption_info.clone(), chapter_progress);
            view.header_height = header_height;
            view.logical_progress = logical;
            view.set_footer(footer, info.clone(), pace);
            view.footer_height = footer_height;
            *ink_color_content |= view.ink_color != "black";
            view.ink = frame.ink;
            if let Some(ink) = &mut view.ink {
                ink.shift_down(header_height as i32);
            }
            view.marks = frame.marks;
            view.bookmarked = frame.bookmarked;
            view.stickers = frame.stickers;
            view.hits = frame.hits;
            view.selection = selection;
            for mark in &mut view.marks {
                mark.rect.y += header_height as i32;
            }
            for rect in view.hits.iter_mut().chain(view.selection.iter_mut()) {
                rect.y += header_height as i32;
            }
            for sticker in &mut view.stickers {
                sticker.rect.y += header_height as i32;
                sticker.anchor_line.y += header_height as i32;
            }
            view.position = Some((info.page, info.page_count));
            cx.request_paint_with(hint);
        });
        self.refresh_overlay(ctx);
    }

    /// The figures the overlay shows, or None without a book.
    pub fn overlay_model(&mut self) -> Option<OverlayModel> {
        let session = self.session.as_mut()?;
        let info = session.current();
        let bookmarked = session.is_bookmarked().unwrap_or(false);
        Some(OverlayModel {
            ink: self.ink_on,
            overview: self.overview.clone(),
            title: self.title.clone(),
            fixed_layout: !session.is_reflowable(),
            chapter: info.chapter.clone().unwrap_or_else(|| self.title.clone()),
            clock: self.status.clock.clone(),
            battery: self.status.battery.map(|b| format!("{b}%")).unwrap_or_default(),
            page: info.page,
            page_count: info.page_count,
            footer: {
                let logical = session.logical_progress();
                kaia_i18n::format(
                    &kaia_i18n::tr("Page %1 of %2"),
                    &[
                        &logical.map_or(info.page + 1, |p| p.page).to_string(),
                        &logical.map_or(info.page_count as u32, |p| p.total).to_string(),
                    ],
                )
            },
            bookmarked,
            can_go_back: session.can_go_back(),
        })
    }

    /// Updates the slider, the chapter line and the bookmark button of the overlay.
    pub fn refresh_overlay(&mut self, ctx: &mut Ctx) {
        let hint = if self.pending_hint == PaintHint::Fast {
            PaintHint::Fast
        } else {
            PaintHint::Ui
        };

        for layer in [&self.overlay, &self.panel].into_iter().flatten() {
            layer
                .window
                .update::<super::zoom::Map, _>(ctx, "zoom-map", |map, cx| {
                    map.model = self.overview.clone();
                    cx.request_paint_with(hint);
                });
            let value = self.overview.as_ref().map_or(100.0, |o| o.percent as f64);
            layer
                .window
                .update::<Slider, _>(ctx, "zoom-slider", |slider, cx| {
                    slider.set_value(value);
                    cx.request_paint_with(hint);
                });
        }
        let Some(layer) = self.overlay.clone() else {
            return;
        };
        let Some(model) = self.overlay_model() else {
            return;
        };
        let value = f64::from(model.page + 1);
        layer
            .window
            .update::<Slider, _>(ctx, "overlay-slider", move |slider, cx| {
                if slider.value() != value {
                    slider.set_value(value);
                    cx.request_paint_with(hint);
                }
            });
        // Text layout waits for release so a drag keeps its fast refresh.
        if hint == PaintHint::Fast {
            return;
        }
        let chapter = model.chapter.clone();
        layer
            .window
            .update::<Label, _>(ctx, "overlay-chapter", move |label, cx| {
                label.set_text(chapter);
                cx.request_layout();
                cx.request_paint_with(hint);
            });
        let footer = model.footer.clone();
        layer
            .window
            .update::<Label, _>(ctx, "overlay-footer", move |label, cx| {
                label.set_text(footer);
                cx.request_layout();
                cx.request_paint_with(hint);
            });
        let label = bookmark_label(model.bookmarked);
        layer
            .window
            .update::<ToolButton, _>(ctx, "overlay-bookmark", move |button, cx| {
                button.set_label(label);
                cx.request_layout();
                cx.request_paint_with(hint);
            });
    }

    /// Turns one page. The refresh is full on a chapter start, on every
    /// `refresh_every`th turn when that is set, and on every page of a fixed-layout
    /// book (PDF, comic). A turn to an image page, or away from one, is also full,
    /// but that rule runs in `receive_frame`, once the new page is measured.
    /// Other turns are partial.
    pub fn turn(&mut self, ctx: &mut Ctx, forward: bool) -> Turn {
        let Some(mut session) = self.session.take() else {
            return Turn::Blocked;
        };
        let current = session.current();
        if forward && !session.can_turn_view(true, self.viewport).unwrap_or(false) {
            self.session = Some(session);
            return Turn::AtEnd;
        }
        if !forward && !session.can_turn_view(false, self.viewport).unwrap_or(false) {
            self.session = Some(session);
            return Turn::Blocked;
        }
        let page = if forward {
            current.page + 1
        } else {
            current.page.saturating_sub(1)
        };
        let chapter_start = session.toc().iter().any(|t| t.page == Some(page));
        // A fixed-layout page is a picture, and stock clears the screen for it.
        let picture = !session.is_reflowable();
        self.pending_hint = if chapter_start
            || picture
            || self.refresh_every > 0 && self.pages_since_full + 1 >= self.refresh_every
        {
            self.pages_since_full = 0;
            PaintHint::Full
        } else {
            self.pages_since_full += 1;
            PaintHint::Partial
        };
        self.last_page_turn = crate::sync::now_unix();
        self.selection = None;
        self.sticker_anchor = None;
        self.selecting = false;
        let viewport = self.viewport;
        self.job = Some(super::work::spawn(viewport, move || {
            let error = session
                .turn_view(forward, viewport)
                .err()
                .map(|e| WorkError::Turn(e.to_string()));
            Ok((session, None, error))
        }));
        let _ = ctx;
        Turn::Moved
    }

    /// Jumps to `location` (a search hit, a bookmark or a contents entry) and repaints fully.
    pub fn go_to(&mut self, ctx: &mut Ctx, location: kaia_book::Location) -> bool {
        let Some(mut session) = self.session.take() else {
            self.queued.push_back((
                "reader:location".into(),
                vec![match location {
                    kaia_book::Location::Page(p) => kaia_events::Value::Number(f64::from(p)),
                    kaia_book::Location::Engine(p) => kaia_events::Value::Str(p),
                }],
            ));
            return false;
        };
        self.pending_hint = PaintHint::Full;
        self.pages_since_full = 0;
        self.selection = None;
        self.sticker_anchor = None;
        self.selecting = false;
        self.job = Some(super::work::spawn(self.viewport, move || {
            let error = session
                .jump_to(location)
                .err()
                .map(|e| WorkError::GoTo(e.to_string()));
            Ok((session, None, error))
        }));
        let _ = ctx;
        true
    }

    /// Restores history on the worker without recording another jump.
    pub fn back(&mut self) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        self.selection = None;
        self.selecting = false;
        self.sticker_anchor = None;
        self.pending_hint = PaintHint::Full;
        self.pages_since_full = 0;
        self.job = Some(super::work::spawn(self.viewport, move || {
            let error = session.back().err().map(|e| WorkError::GoTo(e.to_string()));
            Ok((session, None, error))
        }));
    }

    /// Selects the word under a point. The point is in screen pixels.
    pub fn hold(&mut self, ctx: &mut Ctx, point: Point) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        self.selecting = true;
        self.pending_hint = PaintHint::Partial;
        let point = Point::new(point.x, point.y - self.header_height as i32);
        let viewport = self.viewport;
        self.job = Some(super::work::spawn(viewport, move || {
            let point = page_point(&mut session, point, viewport);
            let mut selection = session.select_word_at(point, viewport).ok().flatten();
            if selection.is_none() && !session.is_reflowable() {
                let t = session
                    .page_transform(viewport)
                    .map_err(|e| WorkError::Engine(e.to_string()))?;
                let p = t.to_page(point);
                let pos = kaia_book::Pos::Point {
                    page: session.current().page,
                    x: p.x,
                    y: p.y,
                };
                selection = Some(Selection {
                    page: session.current().page,
                    text: String::new(),
                    pos0: pos.clone(),
                    pos1: pos,
                    page_rects: vec![kaia_book::RectF::new(p.x, p.y, p.x + 1.0, p.y + 1.0)],
                    screen_rects: Vec::new(),
                });
            }
            Ok((session, selection, None))
        }));
        let _ = ctx;
    }

    pub fn drag(&mut self, ctx: &mut Ctx, point: Point) {
        if !self.selecting || self.selection.as_ref().is_some_and(|s| s.text.is_empty()) {
            return;
        }
        let Some(mut session) = self.session.take() else {
            return;
        };
        let point = Point::new(point.x, point.y - self.header_height as i32);
        let viewport = self.viewport;
        self.pending_hint = PaintHint::Partial;
        self.job = Some(super::work::spawn(viewport, move || {
            let point = page_point(&mut session, point, viewport);
            let selection = session.extend_selection(point, viewport).ok().flatten();
            Ok((session, selection, None))
        }));
        let _ = ctx;
    }

    /// Ends the hold. True when a selection exists and the popover should open.
    pub fn release(&mut self) -> bool {
        self.selecting = false;
        self.selection.is_some()
    }

    /// Changes the reflowable layout. The change runs on the engine thread. The
    /// page updates when it applies (see [`Core::pump`]).
    pub fn change_layout(&mut self, change: impl FnOnce(&mut TextLayout)) -> Result<(), String> {
        let session = self.session.as_mut().ok_or_else(|| tr("No book is open."))?;
        if !session.is_reflowable() {
            return Err(tr("This book has no text settings."));
        }
        let current = session.layout().clone();
        let mut next = current.clone();
        change(&mut next);
        if next == current {
            return Ok(());
        }
        session.set_layout(next).map_err(|e| e.to_string())?;
        self.layout_pending = true;
        Ok(())
    }

    /// Sets the fixed-layout zoom. The page is repainted at once.
    pub fn set_zoom(&mut self, ctx: &mut Ctx, zoom: ZoomMode) -> Result<(), String> {
        let session = self.session.as_mut().ok_or_else(|| tr("No book is open."))?;
        session.set_viewport(self.viewport).map_err(|e| e.to_string())?;
        session.set_zoom(zoom).map_err(|e| e.to_string())?;
        self.repaint_page(ctx, PaintHint::Full);
        Ok(())
    }

    /// Handles the engine's events. Called from the pump tick and after a command.
    pub fn pump(&mut self, ctx: &mut Ctx) {
        if self.closed {
            return;
        }
        self.receive_frame(ctx);
        let events = match self.session.as_mut() {
            Some(session) => session.poll(),
            None => Vec::new(),
        };
        for event in events {
            match event {
                BookEvent::LayoutApplied { .. } => {
                    self.layout_pending = false;
                    self.error = None;
                    self.repaint_page(ctx, PaintHint::Full);
                }
                BookEvent::LayoutFailed { message } => {
                    self.layout_pending = false;
                    self.error = Some(tr_format(
                        &tr("Cannot lay out this book. %1"),
                        &[message.as_str()],
                    ));
                    self.repaint_page(ctx, PaintHint::Full);
                }
                BookEvent::SearchProgress { .. } | BookEvent::SearchFinished { .. } => {
                    self.search_dirty = true;
                }
                BookEvent::PageRendered { .. } => {}
            }
        }
        if self.search_dirty {
            self.update_search(ctx);
        }
    }

    pub(super) fn reset_search_paint(&mut self) {
        self.search_painted = None;
        self.search_paint_at = std::time::Duration::ZERO;
    }

    /// Shows the search results so far in the search panel, if it is open.
    pub fn update_search(&mut self, ctx: &mut Ctx) {
        if self.search_scope != super::search::Scope::Book {
            return;
        }
        let Some(layer) = self.panel.clone() else {
            self.search_dirty = false;
            return;
        };
        // An empty field lists the recent searches. A search that is still running
        // must not paint its hits over that list.
        if self.search_text.trim().is_empty() {
            self.search_dirty = false;
            return;
        }
        let snapshot = self
            .session
            .as_ref()
            .map(|s| (layer.serial, s.search_results().len(), s.search_running()));
        if snapshot == self.search_painted {
            self.search_dirty = false;
            return;
        }
        let now = ctx.scheduler.now();
        if !search_paint_due(self.search_painted, snapshot, now, self.search_paint_at) {
            self.search_dirty = true;
            return;
        }
        self.search_dirty = false;
        self.search_painted = snapshot;
        self.search_paint_at = now + std::time::Duration::from_millis(500);
        let (rows, status) = match self.session.as_ref() {
            Some(session) => {
                let results = session.search_results();
                let status = if session.search_running() {
                    tr_format(&tr("Searching. %1 found so far."), &[&results.len().to_string()])
                } else if results.is_empty() {
                    tr("No matches.")
                } else {
                    tr_format(
                        &tr_n("%1 match.", "%1 matches.", results.len() as i64),
                        &[&results.len().to_string()],
                    )
                };
                (search_rows(results), status)
            }
            None => (Vec::new(), String::new()),
        };
        layer
            .window
            .update::<PagedList<SearchRow>, _>(ctx, "search-results", move |list, cx| {
                list.set_items(rows, cx);
            });
        layer
            .window
            .update::<Label, _>(ctx, "search-status", move |label, cx| {
                label.set_text(status);
                cx.request_layout();
                cx.request_paint();
            });
    }
}

/// The face for a font index from the Text and layout sheet.
pub fn font_face(index: usize) -> Option<String> {
    FONT_CHOICES
        .get(index)
        .and_then(|(_, name)| name.map(str::to_string))
}

/// Reads the text of a field in a panel window. None when the window has no such field.
pub fn read_field(window: &Window, ctx: &mut Ctx, key: &str) -> Option<String> {
    let mut text = None;
    window.update::<TextField, _>(ctx, key, |field, _| text = Some(field.text().to_string()));
    text
}

/// Schedules a pump tick when the engine has work and no tick is due.
pub fn schedule_if_pending(core: &Rc<RefCell<Core>>, ctx: &mut Ctx) {
    {
        let mut c = core.borrow_mut();
        if c.pump_due || c.closed || !c.work_pending() {
            return;
        }
        c.pump_due = true;
    }
    let rc = core.clone();
    ctx.scheduler.schedule_in(
        PUMP_SECONDS,
        Box::new(move |ctx: &mut Ctx| {
            {
                let mut c = rc.borrow_mut();
                c.pump_due = false;
                c.pump(ctx);
            }
            schedule_if_pending(&rc, ctx);
        }),
    );
}

fn search_paint_due(
    previous: Option<(u64, usize, bool)>,
    next: Option<(u64, usize, bool)>,
    now: std::time::Duration,
    deadline: std::time::Duration,
) -> bool {
    next != previous && (previous.map(|p| p.0) != next.map(|p| p.0) || now >= deadline)
}

fn page_point(session: &mut BookSession, point: Point, viewport: Size) -> Point {
    if !session.is_reflowable() {
        if let Ok(t) = session.page_transform(viewport) {
            return Point::new(
                point.x - (viewport.w as i32 - t.image.w as i32) / 2,
                point.y - (viewport.h as i32 - t.image.h as i32) / 2,
            );
        }
    }
    point
}

#[cfg(test)]
mod refresh_tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn search_paints_only_changed_results_at_two_hz() {
        let searching = Some((1, 0, true));
        let changed = Some((1, 1, true));
        let deadline = Duration::from_millis(500);
        for ms in (0..500).step_by(20) {
            assert!(!search_paint_due(
                searching,
                searching,
                Duration::from_millis(ms),
                deadline
            ));
            assert!(!search_paint_due(
                searching,
                changed,
                Duration::from_millis(ms),
                deadline
            ));
        }
        assert!(search_paint_due(searching, changed, deadline, deadline));
        assert!(search_paint_due(
            searching,
            Some((1, 0, false)),
            deadline,
            deadline
        ));
        assert!(!search_paint_due(
            searching,
            searching,
            Duration::from_secs(5),
            deadline
        ));
    }
}
