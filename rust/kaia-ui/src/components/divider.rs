//! [`Divider`]: a horizontal rule across the full width.

use kaia_gfx::Size;

use crate::geometry::Constraints;
use crate::theme::Tone;
use crate::widget::{LayoutCx, PaintCx, Widget};

pub struct Divider {
    tone: Tone,
}

impl Divider {
    pub fn new(tone: Tone) -> Self {
        Self { tone }
    }
}

impl Widget for Divider {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(c.max.w, cx.theme.line.divider))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let color = cx.color(self.tone);
        let b = cx.bounds();
        cx.fill_rect(b, color);
    }
}
