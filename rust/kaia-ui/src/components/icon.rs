//! [`Icon`]: a monochrome SVG from `resources/icons/mdlight`.

use kaia_gfx::{Point, Size};

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
}

impl Widget for Icon {
    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(self.px, self.px))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let color = cx.color(self.tone);
        let size = Size::new(self.px, self.px);
        match cx.icons.get(&self.name, size, color) {
            Ok(mask) => {
                let x = b.x + (b.w as i32 - self.px as i32) / 2;
                let y = b.y + (b.h as i32 - self.px as i32) / 2;
                cx.blit_alpha(&mask, Point::new(x, y));
            }
            Err(_) => {
                let hairline = cx.theme.line.hairline;
                cx.border(b, 0, hairline, color);
            }
        }
    }
}
