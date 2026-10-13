//! [`Icon`]: a monochrome SVG from `resources/icons/mdlight`.

use kaia_gfx::{Color, Point, Rect, Size};

use crate::geometry::Constraints;
use crate::theme::Tone;
use crate::widget::{LayoutCx, PaintCx, Widget};

/// A square icon of `px` pixels. The file is `<name>.svg` in the icon
/// directory. A missing icon draws an outline of its box, so the gap shows
/// in a golden image.
pub struct Icon {
    name: String,
    px: u32,
    tone: Tone,
}

impl Icon {
    pub fn new(name: impl Into<String>, px: u32, tone: Tone) -> Self {
        Self {
            name: name.into(),
            px,
            tone,
        }
    }

    /// Switches to another icon of the same size.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Draws the icon `name` at `px` square with its top-left corner at `at`.
/// A missing icon draws an outline of the box in `color`.
pub(crate) fn draw_icon_at(cx: &mut PaintCx, name: &str, px: u32, color: Color, at: Point) {
    match cx.icons.get(name, Size::new(px, px), color) {
        Ok(mask) => cx.blit_alpha(&mask, at),
        Err(_) => {
            let hairline = cx.theme.line.hairline;
            cx.border(Rect::new(at.x, at.y, px, px), 0, hairline, color);
        }
    }
}

impl Widget for Icon {
    crate::impl_as_any!();

    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(self.px, self.px))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let color = cx.color(self.tone);
        let x = b.x + (b.w as i32 - self.px as i32) / 2;
        let y = b.y + (b.h as i32 - self.px as i32) / 2;
        draw_icon_at(cx, &self.name, self.px, color, Point::new(x, y));
    }
}
