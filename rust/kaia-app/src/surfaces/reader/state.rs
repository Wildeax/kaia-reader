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
use kaia_book::{
    BookEvent, BookSession, OpenOptions, PageInfo, Point, Selection, Size, TextLayout, ZoomMode,
};
use kaia_events::Ctx;
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_ui::components::Label;
use kaia_ui::controls::{TextField, ToolButton};
use kaia_ui::widget::PaintHint;
use kaia_ui::Window;

use super::overlay::{bookmark_label, OverlayModel};
use super::page::{PageView, PAGE_KEY};
use super::panels::{search_rows, SearchRow, FONT_CHOICES};
use super::work::WorkError;
use kaia_ui::composite::PagedList;

/// Full refresh every this many page turns, so the panel clears its ghosts.
pub const REFRESH_EVERY: u32 = 6;

/// Seconds between pump ticks while the engine has work.
const PUMP_SECONDS: f64 = 0.02;

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

pub struct Core {
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
    pub viewport: Size,
    /// Why the book cannot be read, shown on the page.
    pub error: Option<String>,
    pub page_window: Option<Window>,
    pub overlay: Option<Layer>,
    pub panel: Option<Layer>,
    pub next_serial: u64,
    pub pages_since_full: u32,
    pub refresh_every: u32,
    pub selecting: bool,
    pub selection: Option<Selection>,
    pub sticker_anchor: Option<kaia_storage::Anchor>,
    pub moving_sticker: Option<String>,
    pub editing_sticker: Option<String>,
    pub stickers: Vec<super::stickers::Drawing>,
    pub search_text: String,
    search_painted: Option<(u64, usize, bool)>,
    search_paint_at: std::time::Duration,
    search_dirty: bool,
    pub contents_tab: usize,
    pub color_panel: bool,
    /// Seconds of reading per page, for the time-left line. Set when the book opens.
    pub seconds_per_page: f64,
    pub pump_due: bool,
    pub layout_pending: bool,
    pub closed: bool,
    /// True while reader plugins wait for their document thread. The pump keeps
    /// ticking then, so the reader sees the answer without input.
    pub(super) plugin_waiting: bool,
    /// Set by the open job: true when the book had no saved or imported state.
    fresh: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Core {
    pub fn new(book: i64) -> Self {
        Self {
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
            error: None,
            page_window: None,
            overlay: None,
            panel: None,
            next_serial: 0,
            pages_since_full: 0,
            refresh_every: REFRESH_EVERY,
            selecting: false,
            selection: None,
            sticker_anchor: None,
            moving_sticker: None,
            editing_sticker: None,
            stickers: Vec::new(),
            search_text: String::new(),
            search_painted: None,
            search_paint_at: std::time::Duration::ZERO,
            search_dirty: false,
            contents_tab: 0,
            color_panel: false,
            pump_due: false,
            layout_pending: false,
            seconds_per_page: 60.0,
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
        let info = frame.session.current();
        self.session = Some(frame.session);
        self.selection = frame.selection;
        self.sticker_anchor = frame.sticker_anchor;
        self.stickers = frame.stickers.clone();
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
        window.update::<PageView, _>(ctx, PAGE_KEY, move |view, cx| {
            if frame.page.is_some() {
                view.page = frame.page;
            }
            view.message = error;
            view.marks = frame.marks;
            view.stickers = frame.stickers;
            view.hits = frame.hits;
            view.selection = selection;
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
            title: self.title.clone(),
            page: info.page,
            page_count: info.page_count,
            time_left: time_line(&info, self.seconds_per_page),
            bookmarked,
        })
    }

    /// Updates the slider, the chapter line and the bookmark button of the overlay.
    pub fn refresh_overlay(&mut self, ctx: &mut Ctx) {
        let Some(layer) = self.overlay.clone() else {
            return;
        };
        let Some(model) = self.overlay_model() else {
            return;
        };
        let hint = if self.pending_hint == PaintHint::Fast {
            PaintHint::Fast
        } else {
            PaintHint::Ui
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
        // Text layout waits for release so it cannot promote a Fast preview to Ui.
        if hint == PaintHint::Fast {
            return;
        }
        let time_left = model.time_left.clone();
        layer
            .window
            .update::<Label, _>(ctx, "overlay-chapter", move |label, cx| {
                label.set_text(time_left);
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

    /// Turns one page. The refresh is partial, except on a chapter start and on
    /// every [`REFRESH_EVERY`]th turn, which are full.
    pub fn turn(&mut self, ctx: &mut Ctx, forward: bool) -> Turn {
        let Some(mut session) = self.session.take() else {
            return Turn::Blocked;
        };
        let current = session.current();
        if forward && current.page as usize + 1 >= current.page_count {
            self.session = Some(session);
            return Turn::AtEnd;
        }
        if !forward && current.page == 0 {
            self.session = Some(session);
            return Turn::Blocked;
        }
        let page = if forward {
            current.page + 1
        } else {
            current.page - 1
        };
        let chapter_start = session.toc().iter().any(|t| t.page == Some(page));
        self.pending_hint =
            if chapter_start || self.refresh_every > 0 && self.pages_since_full + 1 >= self.refresh_every {
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
        self.job = Some(super::work::spawn(self.viewport, move || {
            let error = session
                .go_to(kaia_book::Location::Page(page))
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
                .go_to(location)
                .err()
                .map(|e| WorkError::GoTo(e.to_string()));
            Ok((session, None, error))
        }));
        let _ = ctx;
        true
    }

    /// Selects the word under a point. The point is in screen pixels.
    pub fn hold(&mut self, ctx: &mut Ctx, point: Point) {
        let Some(mut session) = self.session.take() else {
            return;
        };
        self.selecting = true;
        self.pending_hint = PaintHint::Partial;
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
        let Some(layer) = self.panel.clone() else {
            self.search_dirty = false;
            return;
        };
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

/// The time-left line of the overlay, such as `12 min left in chapter`. The
/// pages left in the chapter are timed at `seconds_per_page`. A page before the
/// first chapter counts the rest of the book instead. On the last page it says so.
pub fn time_line(info: &PageInfo, seconds_per_page: f64) -> String {
    if info.page_count == 0 {
        return String::new();
    }
    let (pages, in_chapter) = match (&info.chapter, info.pages_left_in_chapter) {
        (Some(_), Some(n)) => (u64::from(n), true),
        _ => (
            info.page_count.saturating_sub(info.page as usize + 1) as u64,
            false,
        ),
    };
    if pages == 0 {
        return if in_chapter {
            tr("Last page of the chapter")
        } else {
            tr("Last page of the book")
        };
    }
    let minutes = (pages as f64 * seconds_per_page / 60.0).round().max(1.0) as u64;
    if in_chapter {
        tr_format(&tr("%1 min left in chapter"), &[&minutes.to_string()])
    } else {
        tr_format(&tr("%1 min left in book"), &[&minutes.to_string()])
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
