//! [`PagedList`] and [`Grid`]: items split into pages instead of scrolled.
//! Both run on [`Pager`]. E Ink must not scroll, so a page shows as many rows
//! as fit. The page turns with the Previous and Next buttons or a horizontal
//! swipe.
//!
//! The first item sets the row height. If a page still does not fit, the
//! pager shows one row fewer. The pager builds cells for the visible page only
//! and keeps them across layouts, so a layout request does not rebuild them,
//! rescale their covers or lose a keyed update. Setting the items, turning the
//! page, a new size or a change in rows per page builds the cells again.

use kaia_gfx::{Point, Rect, Size};
use kaia_input::Gesture;

use crate::geometry::Constraints;
use crate::pod::WidgetPod;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, UpdateCx, Widget};

/// Button and page text for a pager. [`PageLabels::default`] is English.
pub struct PageLabels {
    pub previous: String,
    pub next: String,
    /// The indicator text for a 1-based page number and the page count.
    pub page_of: Box<dyn Fn(usize, usize) -> String>,
}

impl PageLabels {
    pub fn new(
        previous: impl Into<String>,
        next: impl Into<String>,
        page_of: impl Fn(usize, usize) -> String + 'static,
    ) -> Self {
        Self {
            previous: previous.into(),
            next: next.into(),
            page_of: Box::new(page_of),
        }
    }
}

impl Default for PageLabels {
    fn default() -> Self {
        Self::new("Previous", "Next", |page, pages| {
            format!("Page {page} of {pages}")
        })
    }
}

type Build<T> = Box<dyn Fn(&T, &Theme) -> WidgetPod>;

/// The page logic shared by [`PagedList`] and [`Grid`].
pub(crate) struct Pager<T> {
    items: Vec<T>,
    build: Build<T>,
    columns: u32,
    gap: u32,
    pub(crate) labels: PageLabels,
    page: usize,
    pages: usize,
    /// Rows per page. `None` until the first item is measured for the current size.
    rows: Option<u32>,
    measured_for: Option<Size>,
    cells: Vec<WidgetPod>,
    /// Counts `set_items` calls, so cached cells never outlive their items.
    generation: u64,
    /// What the cached cells were built for.
    cells_for: Option<CellsKey>,
}

/// The inputs that decide which cells are on the page and how they lay out.
#[derive(Clone, Copy, PartialEq, Eq)]
struct CellsKey {
    generation: u64,
    page: usize,
    rows: u32,
    cell: Constraints,
}

/// Where the navigation strip draws its parts, for a pager at `b`.
struct NavRects {
    prev: Rect,
    next: Rect,
    label: Rect,
}

fn nav_rects(b: Rect, t: &Theme) -> NavRects {
    let touch = t.touch_target;
    let side = t.space.sm as i32;
    // The strip sits one gap above the bottom edge, so it never touches a nav bar below.
    let strip_y = b.bottom() - touch as i32 - side;
    // Three touch targets wide: enough for "Previous" in the body bold face.
    // The buttons sit on the edges of the pager, so they line up with the page margin.
    let btn_w = (3 * touch).min(b.w / 3);
    let prev = Rect::new(b.x, strip_y, btn_w, touch);
    let next = Rect::new(b.right() - btn_w as i32, strip_y, btn_w, touch);
    let label_x = prev.right() + side;
    let label_w = (next.x - side - label_x).max(0) as u32;
    NavRects {
        prev,
        next,
        label: Rect::new(label_x, strip_y, label_w, touch),
    }
}

/// Height left for the rows: the strip and a gap of `space.sm` take the rest.
fn content_height(t: &Theme, h: u32) -> u32 {
    h.saturating_sub(t.touch_target + t.space.sm)
}

fn nav_button(cx: &mut PaintCx, rect: Rect, text: &str, enabled: bool) {
    let t = cx.theme;
    let tone = if enabled { Tone::Ink } else { Tone::InkMuted };
    let color = cx.color(tone);
    cx.border(rect, t.radius.button, t.line.focus, color);
    let style = t.style(&t.font.body_bold, tone);
    let max_w = rect.w.saturating_sub(2 * t.space.sm).max(1);
    let tl = cx.fonts.layout(text, &style, max_w, Some(1), true);
    let at = Point::new(
        rect.x + (rect.w as i32 - tl.width as i32) / 2,
        rect.y + (rect.h as i32 - tl.height as i32) / 2,
    );
    cx.text(&tl, at);
}

impl<T> Pager<T> {
    pub(crate) fn new(items: Vec<T>, build: Build<T>, columns: u32, gap: u32) -> Self {
        Self {
            items,
            build,
            columns: columns.max(1),
            gap,
            labels: PageLabels::default(),
            page: 0,
            pages: 0,
            rows: None,
            measured_for: None,
            cells: Vec::new(),
            generation: 0,
            cells_for: None,
        }
    }

    /// Replaces the items and returns to the first page. The next layout
    /// measures the rows again.
    pub(crate) fn set_items(&mut self, items: Vec<T>) {
        self.items = items;
        self.page = 0;
        self.rows = None;
        self.generation += 1;
        self.cells_for = None;
    }

    /// Builds the cells again on the next layout, keeping the items and the page.
    pub(crate) fn rebuild_cells(&mut self) {
        self.cells_for = None;
    }

    pub(crate) fn items(&self) -> &[T] {
        &self.items
    }

    pub(crate) fn page(&self) -> usize {
        self.page
    }

    /// The page count from the last layout. Zero before the first layout or with no items.
    pub(crate) fn page_count(&self) -> usize {
        self.pages
    }

    pub(crate) fn set_page(&mut self, page: usize) {
        self.page = page.min(self.pages.saturating_sub(1));
    }

    pub(crate) fn children(&mut self) -> Vec<&mut WidgetPod> {
        self.cells.iter_mut().collect()
    }

    fn per_page(&self, rows: u32) -> usize {
        (rows * self.columns) as usize
    }

    /// Rows that fit when the first item has the height `probe` gives it.
    fn fit_rows(&self, cx: &mut LayoutCx, cell_c: Constraints, avail: u32) -> u32 {
        let max_rows = (self.items.len() as u32).div_ceil(self.columns).max(1);
        let mut probe = (self.build)(&self.items[0], cx.theme);
        let h = probe.layout(cx, cell_c).h;
        if h == 0 {
            return max_rows;
        }
        let rows = (avail + self.gap) / (h + self.gap);
        rows.clamp(1, max_rows)
    }

    pub(crate) fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let size = c.constrain(c.max);
        let cols = self.columns;
        let cell_w = size.w.saturating_sub(self.gap * (cols - 1)) / cols;
        let avail = content_height(t, size.h);
        let cell_c = Constraints {
            min: Size::new(cell_w, 0),
            max: Size::new(cell_w, avail),
        };
        if self.measured_for != Some(size) {
            self.rows = None;
            self.measured_for = Some(size);
        }
        let n = self.items.len();
        if n == 0 {
            self.cells.clear();
            self.cells_for = None;
            self.pages = 0;
            self.page = 0;
            return size;
        }
        loop {
            let rows = match self.rows {
                Some(r) => r,
                None => {
                    let r = self.fit_rows(cx, cell_c, avail);
                    self.rows = Some(r);
                    r
                }
            };
            let per = self.per_page(rows);
            self.pages = n.div_ceil(per);
            self.page = self.page.min(self.pages - 1);
            let start = self.page * per;
            let end = (start + per).min(n);

            let key = CellsKey {
                generation: self.generation,
                page: self.page,
                rows,
                cell: cell_c,
            };
            let mut cells: Vec<WidgetPod> = if self.cells_for == Some(key) {
                std::mem::take(&mut self.cells)
            } else {
                (start..end).map(|i| (self.build)(&self.items[i], t)).collect()
            };
            let mut row_heights: Vec<u32> = Vec::new();
            for row in cells.chunks_mut(cols as usize) {
                let mut row_h = 0;
                for cell in row {
                    row_h = row_h.max(cell.layout(cx, cell_c).h);
                }
                row_heights.push(row_h);
            }
            let gaps = self.gap * (row_heights.len() as u32).saturating_sub(1);
            let total: u32 = row_heights.iter().sum::<u32>() + gaps;
            if total > avail && rows > 1 {
                // The page does not fit. Show one row fewer and keep the first item in view.
                self.rows = Some(rows - 1);
                self.page = start / self.per_page(rows - 1);
                self.cells_for = None;
                continue;
            }

            let mut y = 0i32;
            for (r, row) in cells.chunks_mut(cols as usize).enumerate() {
                for (k, cell) in row.iter_mut().enumerate() {
                    let x = (k as u32 * (cell_w + self.gap)) as i32;
                    cell.set_origin(Point::new(x, y));
                }
                y += (row_heights[r] + self.gap) as i32;
            }
            self.cells = cells;
            self.cells_for = Some(key);
            break;
        }
        size
    }

    pub(crate) fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let background = cx.color(cx.background());
        cx.fill_rect(b, background);
        let t = cx.theme;
        let r = nav_rects(b, t);
        let at_start = self.page == 0;
        let at_end = self.page + 1 >= self.pages;
        nav_button(cx, r.prev, &self.labels.previous, !at_start);
        nav_button(cx, r.next, &self.labels.next, !at_end);
        let text = (self.labels.page_of)(self.page + 1, self.pages.max(1));
        let style = t.style(&t.font.body, Tone::Ink);
        let tl = cx.fonts.layout(&text, &style, r.label.w.max(1), Some(1), true);
        let at = Point::new(
            r.label.x + (r.label.w as i32 - tl.width as i32) / 2,
            r.label.y + (r.label.h as i32 - tl.height as i32) / 2,
        );
        cx.text(&tl, at);
    }

    pub(crate) fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let r = nav_rects(cx.bounds(), cx.theme());
        let target = match ev {
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                if r.prev.contains(*pos) {
                    Some((self.page.checked_sub(1), true))
                } else if r.next.contains(*pos) {
                    Some((Some(self.page + 1), true))
                } else {
                    None
                }
            }
            UiEvent::Gesture(Gesture::Swipe { pos, end_pos, .. }) => {
                let dx = end_pos.x - pos.x;
                let dy = end_pos.y - pos.y;
                if dx.abs() <= dy.abs() {
                    None
                } else if dx < 0 {
                    Some((Some(self.page + 1), false))
                } else {
                    Some((self.page.checked_sub(1), false))
                }
            }
            _ => None,
        };
        let Some((next, in_strip)) = target else {
            return Handled::No;
        };
        match next {
            Some(page) if page < self.pages => {
                self.page = page;
                cx.request_layout();
                cx.request_paint();
                Handled::Yes
            }
            // A tap on a strip button at an end is still the strip's tap.
            _ if in_strip => Handled::Yes,
            _ => Handled::No,
        }
    }
}

/// Items in one column, one row per item, split into pages.
///
/// `build` makes the widget for an item. It gets the theme, so rows can read
/// tokens. Rows have no gap between them: a [`ListRow`](super::ListRow) draws
/// its own divider.
pub struct PagedList<T> {
    pager: Pager<T>,
}

impl<T: 'static> PagedList<T> {
    pub fn new(items: Vec<T>, build: impl Fn(&T, &Theme) -> WidgetPod + 'static) -> Self {
        Self {
            pager: Pager::new(items, Box::new(build), 1, 0),
        }
    }

    pub fn labels(mut self, labels: PageLabels) -> Self {
        self.pager.labels = labels;
        self
    }

    /// Replaces the items and goes back to page 1. Use it in a keyed update:
    /// `window.update::<PagedList<Book>, _>(ctx, "list", |list, cx| list.set_items(books, cx))`.
    pub fn set_items(&mut self, items: Vec<T>, cx: &mut UpdateCx<'_>) {
        self.pager.set_items(items);
        cx.request_layout();
        cx.request_paint();
    }

    /// Shows page `page` (0-based), clamped to the last page.
    pub fn set_page(&mut self, page: usize, cx: &mut UpdateCx<'_>) {
        self.pager.set_page(page);
        cx.request_layout();
        cx.request_paint();
    }

    /// Builds the cells of the page again, keeping the items and the page. Use
    /// it when the `build` closure reads state that is not in the items.
    pub fn rebuild_cells(&mut self, cx: &mut UpdateCx<'_>) {
        self.pager.rebuild_cells();
        cx.request_layout();
        cx.request_paint();
    }

    pub fn items(&self) -> &[T] {
        self.pager.items()
    }

    /// The 0-based page on screen.
    pub fn page(&self) -> usize {
        self.pager.page()
    }

    /// The page count. Zero before the first layout or with no items.
    pub fn page_count(&self) -> usize {
        self.pager.page_count()
    }

    /// Builds the cells again without an update context. Composite widgets use it
    /// when they also request the layout themselves.
    pub(crate) fn pager_rebuild(&mut self) {
        self.pager.rebuild_cells();
    }

    /// Replaces the items without an update context. Composite widgets use it
    /// during their own layout.
    pub(crate) fn reset_items(&mut self, items: Vec<T>) {
        self.pager.set_items(items);
    }
}

impl<T: 'static> Widget for PagedList<T> {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        self.pager.layout(cx, c)
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        self.pager.paint(cx);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        self.pager.event(cx, ev)
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.pager.children()
    }
}

/// Items in a grid of fixed columns, split into pages like [`PagedList`].
/// Each row is as tall as its tallest cell.
pub struct Grid<T> {
    pager: Pager<T>,
}

impl<T: 'static> Grid<T> {
    /// A grid with `columns` columns and no gap between cells. Set the gap with [`Grid::gap`].
    pub fn new(items: Vec<T>, columns: u32, build: impl Fn(&T, &Theme) -> WidgetPod + 'static) -> Self {
        Self {
            pager: Pager::new(items, Box::new(build), columns, 0),
        }
    }

    /// The space between cells, in pixels, both across and down.
    pub fn gap(mut self, px: u32) -> Self {
        self.pager.gap = px;
        self
    }

    pub fn labels(mut self, labels: PageLabels) -> Self {
        self.pager.labels = labels;
        self
    }

    /// Replaces the items and goes back to page 1. See [`PagedList::set_items`].
    pub fn set_items(&mut self, items: Vec<T>, cx: &mut UpdateCx<'_>) {
        self.pager.set_items(items);
        cx.request_layout();
        cx.request_paint();
    }

    pub fn set_page(&mut self, page: usize, cx: &mut UpdateCx<'_>) {
        self.pager.set_page(page);
        cx.request_layout();
        cx.request_paint();
    }

    /// Builds the cells of the page again. See [`PagedList::rebuild_cells`].
    pub fn rebuild_cells(&mut self, cx: &mut UpdateCx<'_>) {
        self.pager.rebuild_cells();
        cx.request_layout();
        cx.request_paint();
    }

    pub fn items(&self) -> &[T] {
        self.pager.items()
    }

    pub fn page(&self) -> usize {
        self.pager.page()
    }

    pub fn page_count(&self) -> usize {
        self.pager.page_count()
    }
}

impl<T: 'static> Widget for Grid<T> {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        self.pager.layout(cx, c)
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        self.pager.paint(cx);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        self.pager.event(cx, ev)
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.pager.children()
    }
}
