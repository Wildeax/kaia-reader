//! [`Label`] (one line, ellipsis) and [`Paragraph`] (wrapped, with a line limit).

use kaia_gfx::text::TextLayout;
use kaia_gfx::{Point, Size};

use crate::geometry::Constraints;
use crate::theme::{FontSpec, Tone};
use crate::widget::{LayoutCx, PaintCx, Widget};

/// One line of text. Text wider than the space ends with an ellipsis.
pub struct Label {
    text: String,
    font: FontSpec,
    tone: Tone,
    layout: Option<TextLayout>,
}

impl Label {
    pub fn new(text: impl Into<String>, font: FontSpec, tone: Tone) -> Self {
        Self {
            text: text.into(),
            font,
            tone,
            layout: None,
        }
    }
}

impl Widget for Label {
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let style = cx.theme.style(&self.font, self.tone);
        let tl = cx.fonts.layout(&self.text, &style, c.max.w, Some(1), true);
        let size = c.constrain(Size::new(tl.width, tl.height));
        self.layout = Some(tl);
        size
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        if let Some(tl) = &self.layout {
            let b = cx.bounds();
            cx.text(tl, Point::new(b.x, b.y));
        }
    }
}

/// Wrapped text. With `max_lines`, the last line ends with an ellipsis when
/// the text is cut.
pub struct Paragraph {
    text: String,
    font: FontSpec,
    tone: Tone,
    max_lines: Option<usize>,
    layout: Option<TextLayout>,
}

impl Paragraph {
    pub fn new(text: impl Into<String>, font: FontSpec, tone: Tone) -> Self {
        Self {
            text: text.into(),
            font,
            tone,
            max_lines: None,
            layout: None,
        }
    }

    pub fn max_lines(mut self, n: usize) -> Self {
        self.max_lines = Some(n);
        self
    }
}

impl Widget for Paragraph {
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let style = cx.theme.style(&self.font, self.tone);
        let tl = cx.fonts.layout(&self.text, &style, c.max.w, self.max_lines, true);
        let size = c.constrain(Size::new(tl.width, tl.height));
        self.layout = Some(tl);
        size
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        if let Some(tl) = &self.layout {
            let b = cx.bounds();
            cx.text(tl, Point::new(b.x, b.y));
        }
    }
}
