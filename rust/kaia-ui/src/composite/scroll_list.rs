//! A vertical list that scrolls by whole rows when a swipe ends.

use kaia_gfx::{Point, Rect, Size};
use kaia_input::Gesture;

use crate::geometry::Constraints;
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};
use crate::{Theme, Tone, WidgetPod};

/// A list without pager buttons. Only complete rows are exposed to painting,
/// focus and hit testing. Swipes settle on row boundaries without animation.
pub struct ScrollList<T> {
    items: Vec<T>,
    build: Box<RowBuilder<T>>,
    rows: Vec<WidgetPod>,
    first: usize,
    visible: usize,
    last_start: usize,
    heights: Vec<u32>,
    gutter: u32,
}

type RowBuilder<T> = dyn Fn(&T, &Theme) -> WidgetPod;

impl<T: 'static> ScrollList<T> {
    pub fn new(items: Vec<T>, build: impl Fn(&T, &Theme) -> WidgetPod + 'static) -> Self {
        Self {
            items,
            build: Box::new(build),
            rows: Vec::new(),
            first: 0,
            visible: 0,
            last_start: 0,
            heights: Vec::new(),
            gutter: 0,
        }
    }

    pub fn first_visible(&self) -> usize {
        self.first
    }

    fn scroll(&mut self, distance: i32, cx: &mut EventCx<'_>) {
        let mut pixels = distance.unsigned_abs();
        let old = self.first;
        while pixels > 0 {
            let index = if distance < 0 {
                if self.first >= self.last_start {
                    break;
                }
                let index = self.first;
                self.first += 1;
                index
            } else {
                if self.first == 0 {
                    break;
                }
                self.first -= 1;
                self.first
            };
            pixels = pixels.saturating_sub(self.heights[index].max(1));
        }
        if self.first != old {
            cx.request_layout();
            cx.request_paint();
        }
    }
}

impl<T: 'static> Widget for ScrollList<T> {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        self.gutter = super::settings_dialog::px(cx.theme, 48);
        let width = c.max.w.saturating_sub(self.gutter);
        let row_constraints = Constraints {
            min: Size::new(width, 0),
            max: Size::new(width, c.max.h),
        };
        let mut rows: Vec<_> = self
            .items
            .iter()
            .map(|item| (self.build)(item, cx.theme))
            .collect();
        self.heights = rows
            .iter_mut()
            .map(|row| row.layout(cx, row_constraints).h)
            .collect();
        // Fill the final viewport from the bottom, so it has no avoidable gap.
        self.last_start = self.items.len();
        let mut height = 0u32;
        while self.last_start > 0 {
            let next = self.heights[self.last_start - 1];
            if height.saturating_add(next) > c.max.h {
                break;
            }
            height += next;
            self.last_start -= 1;
        }
        self.first = self.first.min(self.last_start);
        self.rows.clear();
        let mut y = 0u32;
        for mut row in rows.into_iter().skip(self.first) {
            let height = row.size().h;
            if y.saturating_add(height) > c.max.h {
                break;
            }
            row.set_origin(Point::new(0, y as i32));
            y += height;
            self.rows.push(row);
        }
        self.visible = self.rows.len();
        c.max
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let bounds = cx.bounds();
        cx.fill_rect(bounds, cx.color(Tone::Surface));
        if self.last_start == 0 || self.items.is_empty() {
            return;
        }
        let ink = cx.color(Tone::Ink);
        let line = super::settings_dialog::px(cx.theme, 1).max(1);
        let width = super::settings_dialog::px(cx.theme, 12);
        let x = bounds.right() - (self.gutter / 3) as i32;
        cx.fill_rect(Rect::new(x, bounds.y, line, bounds.h), ink);
        let height = (bounds.h as u64 * self.visible as u64 / self.items.len() as u64) as u32;
        let top = ((bounds.h - height) as u64 * self.first as u64 / self.last_start as u64) as i32;
        let thumb = Rect::new(x - width as i32 / 2, bounds.y + top, width, height);
        cx.fill_rect(thumb, cx.color(Tone::Surface));
        cx.border(thumb, width / 2, line, ink);
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> Handled {
        if let UiEvent::Gesture(Gesture::Swipe { pos, end_pos, .. }) = event {
            let dy = end_pos.y - pos.y;
            if dy.abs() > (end_pos.x - pos.x).abs() {
                self.scroll(dy, cx);
                return Handled::Yes;
            }
        }
        Handled::No
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.rows.iter_mut().collect()
    }
}
