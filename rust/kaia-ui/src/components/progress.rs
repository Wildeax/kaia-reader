//! [`ProgressBar`]: a bar with the percent beside it, so the value never
//! depends on color alone.

use kaia_gfx::text::TextLayout;
use kaia_gfx::{Point, Rect, Size};

use crate::geometry::Constraints;
use crate::theme::Tone;
use crate::widget::{LayoutCx, PaintCx, Widget};

/// A bar that fills `value` (0.0 to 1.0) of its track, with the percent
/// written at the right edge. The bar is `line.progress` tall, and the
/// filled part uses the accent tone.
pub struct ProgressBar {
    value: f32,
    label: Option<TextLayout>,
}

impl ProgressBar {
    pub fn new(value: f32) -> Self {
        Self {
            value: value.clamp(0.0, 1.0),
            label: None,
        }
    }

    fn percent_text(&self) -> String {
        format!("{}%", (self.value * 100.0).round() as u32)
    }
}

impl Widget for ProgressBar {
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let style = cx.theme.style(&cx.theme.font.caption, Tone::Ink);
        let text = self.percent_text();
        let tl = cx.fonts.layout(&text, &style, c.max.w, Some(1), false);
        let bar = cx.theme.line.progress;
        let size = c.constrain(Size::new(c.max.w, bar.max(tl.height)));
        self.label = Some(tl);
        size
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let bar_h = cx.theme.line.progress;
        let gap = cx.theme.space.sm;
        let label_w = self.label.as_ref().map_or(0, |tl| tl.width);
        let label_h = self.label.as_ref().map_or(0, |tl| tl.height);
        let track_w = b.w.saturating_sub(label_w + gap);
        let track = Rect::new(b.x, b.y + (b.h as i32 - bar_h as i32) / 2, track_w, bar_h);
        let track_color = cx.color(Tone::Divider);
        cx.fill_rect(track, track_color);

        let fill_w = (track_w as f32 * self.value).round() as u32;
        let fill = Rect::new(track.x, track.y, fill_w, bar_h);
        let fill_color = cx.color(Tone::Accent);
        cx.fill_rect(fill, fill_color);

        if let Some(tl) = &self.label {
            let x = b.x + b.w.saturating_sub(label_w) as i32;
            let y = b.y + (b.h as i32 - label_h as i32) / 2;
            cx.text(tl, Point::new(x, y));
        }
    }
}
