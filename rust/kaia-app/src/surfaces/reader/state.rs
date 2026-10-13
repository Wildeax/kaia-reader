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

use kaia_book::{
    BookError, BookEvent, BookSession, Drawer, OpenOptions, PageInfo, Point, Selection, Size, TextLayout,
    ZoomMode,
};
use kaia_events::Ctx;
use kaia_ui::components::Label;
use kaia_ui::controls::{Button, Slider, TextField};
use kaia_ui::widget::PaintHint;
use kaia_ui::Window;

use super::overlay::{bookmark_label, OverlayModel};
use super::page::{Mark, MarkStyle, PageView, PAGE_KEY};
use super::panels::{search_rows, SearchRow, FONT_CHOICES};
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
    pub search_text: String,
    pub contents_tab: usize,
    pub color_panel: bool,
    pub pump_due: bool,
    pub layout_pending: bool,
    pub closed: bool,
}

impl Core {
    pub fn new(book: i64) -> Self {
        Self {
            book,
            last_page_turn: 0,
            periodic_sync: kaia_sync::PeriodicPush::default(),
            title: String::new(),
            session: None,
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
            search_text: String::new(),
            contents_tab: 0,
            color_panel: false,
            pump_due: false,
            layout_pending: false,
            closed: false,
        }
    }

    /// Opens the book at `path` for `viewport`. A failure becomes the page message.
    pub fn open(&mut self, path: &Path, opts: OpenOptions) {
        match BookSession::open(path, opts) {
            Ok(session) => self.session = Some(session),
            Err(e) => self.error = Some(format!("Cannot open this book. {e}")),
        }
    }

    /// The reading progress to store in the library, or None without a book.
    pub fn percent(&self) -> Option<f64> {
        self.session.as_ref().map(|s| s.current().percent)
    }

    /// True while the engine has work: a layout change or a search.
    pub fn work_pending(&self) -> bool {
        self.layout_pending || self.session.as_ref().is_some_and(|s| s.search_running())
    }

    /// Clears the selection and repaints the page.
    pub fn clear_selection(&mut self, ctx: &mut Ctx) {
        self.selection = None;
        self.selecting = false;
        self.repaint_page(ctx, PaintHint::Partial);
    }

    /// Renders the current page into the page widget. A layout change in progress
    /// keeps the old frame: the widget is not touched until the layout applies.
    pub fn repaint_page(&mut self, ctx: &mut Ctx, hint: PaintHint) {
        if hint == PaintHint::Full {
            self.pages_since_full = 0;
        }
        let Some(window) = self.page_window.clone() else {
            return;
        };
        let viewport = self.viewport;
        let mut page = None;
        let mut marks = Vec::new();
        let mut hits = Vec::new();
        let mut position = None;
        if let Some(session) = self.session.as_mut() {
            match session.render_current(viewport) {
                Ok(pixmap) => page = Some(pixmap),
                Err(BookError::LayoutPending) => return,
                Err(e) => self.error = Some(format!("Cannot draw this page. {e}")),
            }
            let info = session.current();
            position = Some((info.page, info.page_count));
            if let Ok(list) = session.annotations_on_page(viewport) {
                for item in list {
                    if item.annotation.is_bookmark() {
                        continue;
                    }
                    let style = match item.annotation.drawer.as_deref().and_then(Drawer::parse) {
                        Some(Drawer::Underscore) => MarkStyle::Underline,
                        Some(Drawer::Strikeout) => MarkStyle::Strike,
                        _ => MarkStyle::Box,
                    };
                    marks.extend(item.rects.iter().map(|rect| Mark { rect: *rect, style }));
                }
            }
            if let Ok(transform) = session.page_transform(viewport) {
                for result in session.search_results() {
                    if result.page == info.page {
                        hits.extend(result.rects.iter().map(|r| transform.to_screen(*r)));
                    }
                }
            }
        }
        let selection = self
            .selection
            .as_ref()
            .map(|s| s.screen_rects.clone())
            .unwrap_or_default();
        let message = if page.is_none() { self.error.clone() } else { None };
        window.update::<PageView, _>(ctx, PAGE_KEY, move |view, cx| {
            if page.is_some() {
                view.page = page;
            }
            view.message = message;
            view.marks = marks;
            view.hits = hits;
            view.selection = selection;
            view.position = position;
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
            chapter: chapter_line(&info),
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
        let value = f64::from(model.page + 1);
        layer
            .window
            .update::<Slider, _>(ctx, "overlay-slider", move |slider, cx| {
                slider.set_value(value);
                cx.request_paint();
            });
        let chapter = model.chapter.clone();
        layer
            .window
            .update::<Label, _>(ctx, "overlay-chapter", move |label, cx| {
                label.set_text(chapter);
                cx.request_layout();
                cx.request_paint();
            });
        let label = bookmark_label(model.bookmarked);
        layer
            .window
            .update::<Button, _>(ctx, "overlay-bookmark", move |button, cx| {
                button.set_text(label);
                cx.request_paint();
            });
    }

    /// Turns one page. The refresh is partial, except on a chapter start and on
    /// every [`REFRESH_EVERY`]th turn, which are full.
    pub fn turn(&mut self, ctx: &mut Ctx, forward: bool) -> Turn {
        let moved = match self.session.as_mut() {
            Some(session) => {
                let result = if forward {
                    session.next_page()
                } else {
                    session.prev_page()
                };
                match result {
                    Ok(true) => {
                        let page = session.current().page;
                        let chapter_start = session.toc().iter().any(|t| t.page == Some(page));
                        Ok((true, chapter_start))
                    }
                    Ok(false) => Ok((false, false)),
                    Err(e) => Err(e),
                }
            }
            None => return Turn::Blocked,
        };
        match moved {
            Ok((true, chapter_start)) => {
                self.last_page_turn = crate::sync::now_unix();
                let hint = if chapter_start
                    || self.refresh_every > 0 && self.pages_since_full + 1 >= self.refresh_every
                {
                    self.pages_since_full = 0;
                    PaintHint::Full
                } else {
                    self.pages_since_full += 1;
                    PaintHint::Partial
                };
                self.selection = None;
                self.selecting = false;
                self.repaint_page(ctx, hint);
                Turn::Moved
            }
            Ok((false, _)) => {
                if forward {
                    Turn::AtEnd
                } else {
                    Turn::Blocked
                }
            }
            Err(BookError::LayoutPending) => Turn::Blocked,
            Err(e) => {
                self.error = Some(format!("Cannot turn the page. {e}"));
                Turn::Blocked
            }
        }
    }

    /// Jumps to `location` (a search hit, a bookmark or a contents entry) and repaints fully.
    pub fn go_to(&mut self, ctx: &mut Ctx, location: kaia_book::Location) -> bool {
        let result = match self.session.as_mut() {
            Some(session) => session.go_to(location),
            None => return false,
        };
        match result {
            Ok(()) => {
                self.selection = None;
                self.selecting = false;
                self.repaint_page(ctx, PaintHint::Full);
                true
            }
            Err(e) => {
                self.error = Some(format!("Cannot go there. {e}"));
                false
            }
        }
    }

    /// Selects the word under a point. The point is in screen pixels.
    pub fn hold(&mut self, ctx: &mut Ctx, point: Point) {
        let viewport = self.viewport;
        let Some(session) = self.session.as_mut() else {
            return;
        };
        self.selecting = true;
        match session.select_word_at(point, viewport) {
            Ok(selection) => self.selection = selection,
            Err(BookError::LayoutPending) => {}
            Err(_) => self.selection = None,
        }
        self.repaint_page(ctx, PaintHint::Partial);
    }

    /// Extends the selection to a point while the hold lasts.
    pub fn drag(&mut self, ctx: &mut Ctx, point: Point) {
        if !self.selecting {
            return;
        }
        let viewport = self.viewport;
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if let Ok(Some(selection)) = session.extend_selection(point, viewport) {
            self.selection = Some(selection);
        }
        self.repaint_page(ctx, PaintHint::Partial);
    }

    /// Ends the hold. True when a selection exists and the popover should open.
    pub fn release(&mut self) -> bool {
        self.selecting = false;
        self.selection.is_some()
    }

    /// Changes the reflowable layout. The change runs on the engine thread. The
    /// page updates when it applies (see [`Core::pump`]).
    pub fn change_layout(&mut self, change: impl FnOnce(&mut TextLayout)) -> Result<(), String> {
        let session = self.session.as_mut().ok_or("No book is open.")?;
        if !session.is_reflowable() {
            return Err("This book has no text settings.".to_string());
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
        let session = self.session.as_mut().ok_or("No book is open.")?;
        session.set_zoom(zoom).map_err(|e| e.to_string())?;
        self.repaint_page(ctx, PaintHint::Full);
        Ok(())
    }

    /// Handles the engine's events. Called from the pump tick and after a command.
    pub fn pump(&mut self, ctx: &mut Ctx) {
        if self.closed {
            return;
        }
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
                    self.error = Some(format!("Cannot lay out this book. {message}"));
                    self.repaint_page(ctx, PaintHint::Full);
                }
                BookEvent::SearchProgress { .. } | BookEvent::SearchFinished { .. } => {
                    self.update_search(ctx);
                }
                BookEvent::PageRendered { .. } => {}
            }
        }
    }

    /// Shows the search results so far in the search panel, if it is open.
    pub fn update_search(&mut self, ctx: &mut Ctx) {
        let Some(layer) = self.panel.clone() else {
            return;
        };
        let (rows, status) = match self.session.as_ref() {
            Some(session) => {
                let results = session.search_results();
                let status = if session.search_running() {
                    format!("Searching. {} found so far.", results.len())
                } else if results.is_empty() {
                    "No matches.".to_string()
                } else {
                    format!("{} matches.", results.len())
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

/// The chapter line of the overlay: the chapter and the pages left in it.
pub fn chapter_line(info: &PageInfo) -> String {
    match (&info.chapter, info.pages_left_in_chapter) {
        (Some(chapter), Some(1)) => format!("{chapter}, 1 page left in chapter"),
        (Some(chapter), Some(n)) => format!("{chapter}, {n} pages left in chapter"),
        (Some(chapter), None) => chapter.clone(),
        _ => String::new(),
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
