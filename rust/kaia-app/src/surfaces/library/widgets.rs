//! Local library layouts: anchored panels, fitted covers and scrollable rows.

use kaia_gfx::{Pixmap, Point, Rect, Size};
use kaia_input::{Gesture, Key};
use kaia_ui::components::Image;
use kaia_ui::composite::CoverImage;
use kaia_ui::geometry::Constraints;
use kaia_ui::{AppAction, EventCx, Handled, LayoutCx, PaintCx, Theme, Tone, UiEvent, Widget, WidgetPod};

pub struct Overlay {
    base: Option<WidgetPod>,
    panel: WidgetPod,
    rect: Rect,
    bounds: Rect,
    blocker: WidgetPod,
    close: String,
}

impl Overlay {
    /// Coordinates are logical pixels. The panel stays within the window.
    pub fn new(base: Option<WidgetPod>, panel: WidgetPod, rect: Rect, close: &str) -> Self {
        Self {
            close: close.into(),
            base,
            panel: WidgetPod::new(Shield(panel)),
            rect,
            bounds: Rect::default(),
            blocker: WidgetPod::new(Dismiss {
                action: close.into(),
                backdrop: false,
            }),
        }
    }
    /// Covers the caller with the same gray surround as the inset dialogs.
    pub fn dialog(mut self) -> Self {
        self.blocker = WidgetPod::new(Dismiss {
            action: self.close.clone(),
            backdrop: true,
        });
        self
    }
}

impl Widget for Overlay {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        if let Some(base) = &mut self.base {
            base.layout(cx, Constraints::tight(c.max));
        }
        self.blocker.layout(cx, Constraints::tight(c.max));
        let dp = |v| cx.theme.scale.dp(v);
        let w = dp(self.rect.w).min(c.max.w.saturating_sub(dp(12)));
        let h = dp(self.rect.h).min(c.max.h.saturating_sub(dp(12)));
        let x = dp(self.rect.x.max(0) as u32).min(c.max.w.saturating_sub(w)) as i32;
        let x = if cx.theme.rtl {
            c.max.w as i32 - x - w as i32
        } else {
            x
        };
        let y = dp(self.rect.y.max(0) as u32).min(c.max.h.saturating_sub(h)) as i32;
        self.bounds = Rect::new(x, y, w, h);
        self.panel.layout(cx, Constraints::tight(Size::new(w, h)));
        self.panel.set_origin(Point::new(x, y));
        c.max
    }
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        // The base paints normally, but the panel's full-window shield owns input.
        let mut result = Vec::new();
        if let Some(base) = &mut self.base {
            result.push(base);
        }
        result.push(&mut self.blocker);
        result.push(&mut self.panel);
        result
    }
}

struct Dismiss {
    action: String,
    backdrop: bool,
}
impl Widget for Dismiss {
    kaia_ui::impl_as_any!();
    fn paint(&mut self, cx: &mut PaintCx) {
        if self.backdrop {
            cx.fill_rect(cx.bounds(), kaia_gfx::Color::gray(77));
        }
    }
    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.max
    }
    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        if matches!(ev, UiEvent::Gesture(Gesture::Tap { .. })) {
            cx.action(AppAction::new(&self.action));
        }
        if matches!(ev, UiEvent::Gesture(_)) {
            Handled::Yes
        } else {
            Handled::No
        }
    }
}

/// Fills the overlay bounds for input while leaving the underlying page visible.
pub struct Shield(pub WidgetPod);
impl Widget for Shield {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        self.0.layout(cx, c);
        c.max
    }
    fn event(&mut self, _cx: &mut EventCx, ev: &UiEvent) -> Handled {
        if matches!(ev, UiEvent::Gesture(_)) {
            Handled::Yes
        } else {
            Handled::No
        }
    }
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.0]
    }
}

pub struct FittedCover {
    child: WidgetPod,
}
impl FittedCover {
    pub fn new(title: String, cover: Option<Pixmap>) -> Self {
        Self {
            child: match cover {
                Some(image) => WidgetPod::new(Image::new(image, kaia_gfx::image::FitMode::Contain)),
                None => WidgetPod::new(CoverImage::new(title, None)),
            },
        }
    }
}
impl Widget for FittedCover {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let size = self.child.layout(cx, Constraints::loose(c.max));
        self.child
            .set_origin(Point::new(0, c.max.h.saturating_sub(size.h) as i32));
        c.max
    }
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.child]
    }
}

type Builder<T> = Box<dyn Fn(&T, &Theme) -> WidgetPod>;
/// Builds only visible rows. Vertical gestures and page keys move the viewport.
pub struct Scroll<T> {
    items: Vec<T>,
    build: Builder<T>,
    columns: u32,
    pitch: u32,
    measure_single_row: bool,
    cells: Vec<WidgetPod>,
    first: usize,
    offset: i32,
    position: Option<std::rc::Rc<std::cell::Cell<i32>>>,
    max: i32,
    drag: Option<i32>,
    size: Size,
}
impl<T> Scroll<T> {
    pub fn new(
        items: Vec<T>,
        columns: u32,
        pitch: u32,
        build: impl Fn(&T, &Theme) -> WidgetPod + 'static,
    ) -> Self {
        Self {
            items,
            build: Box::new(build),
            columns,
            pitch,
            measure_single_row: false,
            cells: Vec::new(),
            first: usize::MAX,
            offset: 0,
            position: None,
            max: 0,
            drag: None,
            size: Size::new(0, 0),
        }
    }
    pub fn position(mut self, position: std::rc::Rc<std::cell::Cell<i32>>) -> Self {
        self.offset = position.get();
        self.position = Some(position);
        self
    }
    /// Measures one text row at the viewport width before calculating its scroll range.
    pub fn measure_single_row(mut self) -> Self {
        self.measure_single_row = true;
        self
    }
    pub fn offset(&self) -> i32 {
        self.offset
    }
}
impl<T: 'static> Widget for Scroll<T> {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let gap = cx.theme.scale.dp(8);
        let width = c.max.w.saturating_sub(cx.theme.scale.dp(25)) / self.columns;
        let pitch = if self.measure_single_row && self.items.len() == 1 {
            (self.build)(&self.items[0], cx.theme)
                .layout(
                    cx,
                    Constraints::loose(Size::new(width.saturating_sub(gap), u32::MAX)),
                )
                .h
                .saturating_add(gap)
                .max(1)
        } else {
            cx.theme.scale.dp(self.pitch).max(1)
        };
        let rows = self.items.len().div_ceil(self.columns as usize) as u32;
        self.max = (rows * pitch).saturating_sub(c.max.h) as i32;
        self.offset = self.offset.clamp(0, self.max);
        if let Some(position) = &self.position {
            position.set(self.offset);
        }
        let first = self.offset as usize / pitch as usize * self.columns as usize;
        let count = (c.max.h.div_ceil(pitch) + 1) as usize * self.columns as usize;
        if first != self.first || self.size != c.max {
            self.cells = self
                .items
                .iter()
                .skip(first)
                .take(count)
                .map(|item| (self.build)(item, cx.theme))
                .collect();
            self.first = first;
        }
        self.size = c.max;
        for (i, cell) in self.cells.iter_mut().enumerate() {
            cell.layout(
                cx,
                Constraints::tight(Size::new(width.saturating_sub(gap), pitch.saturating_sub(gap))),
            );
            let col = i as u32 % self.columns;
            let x = if cx.theme.rtl {
                c.max.w.saturating_sub((col + 1) * width)
            } else {
                col * width
            };
            cell.set_origin(Point::new(
                x as i32,
                (i as u32 / self.columns * pitch) as i32 - self.offset % pitch as i32,
            ));
        }
        c.max
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        cx.clear_background();
        if self.max == 0 {
            return;
        }
        let b = cx.bounds();
        let ink = cx.color(Tone::Ink);
        let x = if cx.theme.rtl { b.x + 3 } else { b.right() - 6 };
        cx.fill_rect(Rect::new(x, b.y, 1, b.h), ink);
        let total = b.h + self.max as u32;
        let h = (b.h as u64 * b.h as u64 / total as u64) as u32;
        let y = self.offset as u64 * (b.h - h) as u64 / self.max as u64;
        cx.fill_rect(Rect::new(x - 2, b.y + y as i32, 5, h.max(12)), ink);
    }
    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let next = match ev {
            UiEvent::Gesture(Gesture::Pan { pos, start_pos, .. }) => {
                let start = *self.drag.get_or_insert(self.offset);
                start + start_pos.y - pos.y
            }
            UiEvent::Gesture(Gesture::PanRelease { .. }) => {
                self.drag = None;
                return Handled::Yes;
            }
            UiEvent::Gesture(Gesture::Swipe { pos, end_pos, .. }) => {
                self.drag = None;
                self.offset + pos.y - end_pos.y
            }
            UiEvent::Key(k) if k.pressed && matches!(k.key, Key::LPgFwd | Key::RPgFwd | Key::Down) => {
                self.offset + self.size.h as i32
            }
            UiEvent::Key(k) if k.pressed && matches!(k.key, Key::LPgBack | Key::RPgBack | Key::Up) => {
                self.offset - self.size.h as i32
            }
            _ => return Handled::No,
        };
        self.offset = next.clamp(0, self.max);
        if let Some(position) = &self.position {
            position.set(self.offset);
        }
        cx.request_layout();
        cx.request_paint();
        Handled::Yes
    }
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.cells.iter_mut().collect()
    }
}
