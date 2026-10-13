//! The reader widget: one page of a book, rendered by `kaia-doc`.
//!
//! The widget owns the document and the page index. [`super::ReaderSurface`]
//! reads the page through `Window::update` when it saves progress or opens the
//! overlay.

use kaia_doc::{Document, Layout, PageSize};
use kaia_gfx::{PixelFormat, Pixmap, Point, Size};
use kaia_input::{Direction, Gesture};
use kaia_ui::geometry::Constraints;
use kaia_ui::widget::{AppAction, EventCx, Handled, LayoutCx, PaintCx, PaintHint, UiEvent};
use kaia_ui::{Tone, Widget};

/// The key of the reader widget. The reader surface finds it with `Window::update`.
pub const READER_KEY: &str = "reader";

/// Em size, in points, for reflowable books. Matches a comfortable Kobo default.
const EM_PT: f32 = 12.0;

/// Points per inch. Page sizes from `kaia-doc` are in points.
const POINTS_PER_INCH: f32 = 72.0;

/// Edge width, in device-independent units, where a swipe means Back.
const EDGE_DP: u32 = 40;

/// Keeps the page index for a book's progress. The library stores the fraction of
/// the book read, `(page + 1) / count`, so the page comes back from it.
pub fn page_for_progress(percent: f64, count: u32) -> u32 {
    if count == 0 || percent.is_nan() || percent <= 0.0 {
        return 0;
    }
    let page = (percent * f64::from(count)).round() as u32;
    page.saturating_sub(1).min(count - 1)
}

/// The progress the library stores for a page: `(page + 1) / count`.
pub fn progress_for_page(page: u32, count: u32) -> f64 {
    if count == 0 {
        0.0
    } else {
        f64::from(page + 1) / f64::from(count)
    }
}

/// The rendered page and the size of the area it was rendered for.
struct Rendered {
    page: u32,
    bound: Size,
    pixmap: Pixmap,
}

/// The reader widget. It owns the document.
pub struct ReaderPage {
    title: String,
    doc: Option<Box<dyn Document>>,
    error: Option<String>,
    page: u32,
    count: u32,
    dpi: u32,
    rendered: Option<Rendered>,
}

impl ReaderPage {
    /// A reader for `doc`, starting at `page`. A reflowable book is laid out for
    /// the screen at `dpi`, so one page fills the screen.
    pub fn new(
        title: String,
        doc: Result<Box<dyn Document>, String>,
        page: u32,
        screen: Size,
        dpi: u32,
    ) -> Self {
        let (mut doc, mut error) = match doc {
            Ok(doc) => (Some(doc), None),
            Err(e) => (None, Some(format!("Cannot open this book. {e}"))),
        };
        let mut count = 0;
        if let Some(doc) = doc.as_mut() {
            if doc.is_reflowable() {
                let per_inch = POINTS_PER_INCH / dpi.max(1) as f32;
                let layout = Layout::new(screen.w as f32 * per_inch, screen.h as f32 * per_inch, EM_PT);
                if let Err(e) = doc.set_layout(&layout) {
                    error = Some(format!("Cannot lay out this book. {e}"));
                }
            }
            count = doc.page_count() as u32;
            if count == 0 && error.is_none() {
                error = Some("This book has no pages.".to_string());
            }
        }
        let page = page.min(count.saturating_sub(1));
        Self {
            title,
            doc,
            error,
            page,
            count,
            dpi,
            rendered: None,
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// The page shown, 0-based.
    pub fn page(&self) -> u32 {
        self.page
    }

    pub fn page_count(&self) -> u32 {
        self.count
    }

    /// The progress to store in the library for the current page.
    pub fn progress(&self) -> f64 {
        progress_for_page(self.page, self.count)
    }

    pub fn dpi(&self) -> u32 {
        self.dpi
    }

    /// Moves by `delta` pages. Returns false when the page did not change.
    pub fn turn(&mut self, delta: i32) -> bool {
        if self.count == 0 {
            return false;
        }
        let next = (i64::from(self.page) + i64::from(delta)).clamp(0, i64::from(self.count) - 1) as u32;
        if next == self.page {
            return false;
        }
        self.page = next;
        true
    }

    /// Renders the current page to fit `bound`, centred. Sets `error` on failure.
    fn render(&mut self, format: PixelFormat, bound: Size) -> Option<Rendered> {
        let doc = self.doc.as_ref()?;
        let page = self.page;
        let size: PageSize = match doc.page_size(page) {
            Ok(size) => size,
            Err(e) => {
                self.error = Some(format!("Cannot read page {}. {e}", page + 1));
                return None;
            }
        };
        let scale = fit_scale(size, bound);
        match doc.render(page, scale, format) {
            Ok(pixmap) => Some(Rendered { page, bound, pixmap }),
            Err(e) => {
                self.error = Some(format!("Cannot draw page {}. {e}", page + 1));
                None
            }
        }
    }
}

/// The scale at which a page of `size` points fits inside `bound` pixels. Scale 1.0
/// is 72 dpi.
pub fn fit_scale(size: PageSize, bound: Size) -> f32 {
    let w = bound.w as f32 / size.width.max(1.0);
    let h = bound.h as f32 / size.height.max(1.0);
    w.min(h).max(0.01)
}

impl Widget for ReaderPage {
    kaia_ui::impl_as_any!();

    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.max
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let bounds = cx.bounds();
        let background = cx.color(Tone::Surface);
        cx.fill_rect(bounds, background);

        let needs_render = match &self.rendered {
            Some(r) => r.page != self.page || r.bound != bounds.size(),
            None => true,
        };
        if needs_render && self.doc.is_some() {
            let format = cx.theme.pixel_format();
            self.rendered = self.render(format, bounds.size());
        }
        if let Some(r) = &self.rendered {
            let x = bounds.x + (bounds.w as i32 - r.pixmap.width() as i32) / 2;
            let y = bounds.y + (bounds.h as i32 - r.pixmap.height() as i32) / 2;
            cx.blit_pixmap(&r.pixmap, Point::new(x, y));
        } else if let Some(message) = &self.error {
            let style = cx.theme.style(&cx.theme.font.body, Tone::InkSecondary);
            let margin = cx.theme.space.lg as i32;
            let text = cx.fonts.layout(
                message,
                &style,
                bounds.w.saturating_sub(2 * margin as u32),
                None,
                true,
            );
            cx.text(&text, Point::new(bounds.x + margin, bounds.y + margin));
        }
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        match ev {
            UiEvent::Named(e) if e.name == "PageForward" => self.step(cx, 1),
            UiEvent::Named(e) if e.name == "PageBack" => self.step(cx, -1),
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                let b = cx.bounds();
                let (x, y) = (pos.x - b.x, pos.y - b.y);
                let (third_w, third_h) = (b.w as i32 / 3, b.h as i32 / 3);
                let centre = x >= third_w && x < 2 * third_w && y >= third_h && y < 2 * third_h;
                if centre {
                    cx.action(AppAction::new("reader:overlay"));
                    Handled::Yes
                } else if x < third_w {
                    self.step(cx, -1)
                } else {
                    self.step(cx, 1)
                }
            }
            UiEvent::Gesture(Gesture::Swipe { pos, direction, .. }) => {
                let edge = cx.theme().scale.dp(EDGE_DP) as i32;
                let b = cx.bounds();
                if pos.x - b.x < edge {
                    // A swipe from the left edge is Back, handled by the app.
                    return Handled::No;
                }
                match direction {
                    Some(Direction::West) | Some(Direction::NorthWest) | Some(Direction::SouthWest) => {
                        self.step(cx, 1)
                    }
                    Some(Direction::East) | Some(Direction::NorthEast) | Some(Direction::SouthEast) => {
                        self.step(cx, -1)
                    }
                    _ => Handled::No,
                }
            }
            _ => Handled::No,
        }
    }
}

impl ReaderPage {
    /// Turns the page and asks for a partial refresh of the reader, the refresh a
    /// page turn uses. An edge page turns nothing and asks for nothing.
    fn step(&mut self, cx: &mut EventCx, delta: i32) -> Handled {
        if self.turn(delta) {
            cx.request_paint_with(PaintHint::Partial);
        }
        Handled::Yes
    }
}
