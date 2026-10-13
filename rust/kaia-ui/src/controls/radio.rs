//! [`RadioGroup`]: a vertical list where one option is selected.

use kaia_events::Value;
use kaia_gfx::{Point, Rect, Size};
use kaia_input::{Gesture, Key};

use super::{Reaction, UNBOUNDED};
use crate::geometry::Constraints;
use crate::theme::Tone;
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};

/// One row per option, `touch_target` high. The selected row has a filled dot
/// and a bold label. A tap selects a row. Up and Down move the selection
/// while the group has focus. The event argument is the 0-based index.
pub struct RadioGroup {
    background: Option<Tone>,
    options: Vec<String>,
    selected: Option<usize>,
    enabled: bool,
    reaction: Reaction,
}

impl RadioGroup {
    pub fn new(options: Vec<String>, selected: Option<usize>) -> Self {
        Self {
            background: None,
            options,
            selected,
            enabled: true,
            reaction: Reaction::None,
        }
    }

    /// The tone behind this control, which it clears its bounds to. Without
    /// this, the control uses the tone from its parent [`Frame`](crate::components::Frame),
    /// or `Surface`.
    pub fn background(mut self, tone: Tone) -> Self {
        self.background = Some(tone);
        self
    }

    pub fn enabled(mut self, yes: bool) -> Self {
        self.enabled = yes;
        self
    }

    pub fn on_event(mut self, name: impl Into<String>) -> Self {
        self.reaction = Reaction::Event(name.into());
        self
    }

    pub fn on_action(mut self, id: impl Into<String>) -> Self {
        self.reaction = Reaction::Action(id.into());
        self
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn set_selected(&mut self, index: Option<usize>) {
        self.selected = index.filter(|&i| i < self.options.len());
    }

    pub fn set_enabled(&mut self, yes: bool) {
        self.enabled = yes;
    }

    fn select(&mut self, cx: &mut EventCx, index: usize) {
        cx.request_focus_if_focusable();
        if !self.enabled || index >= self.options.len() || self.selected == Some(index) {
            return;
        }
        self.selected = Some(index);
        cx.request_paint();
        self.reaction.fire(cx, vec![Value::Number(index as f64)]);
    }
}

impl Widget for RadioGroup {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let rows = self.options.len() as u32;
        c.constrain(Size::new(c.max.w, rows * cx.theme.touch_target))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        super::clear_bounds(cx, self.background);
        let t = cx.theme;
        let b = cx.bounds();
        let row_h = t.touch_target;
        let tone = if self.enabled { Tone::Ink } else { Tone::InkMuted };
        let color = cx.color(tone);
        let divider = cx.color(Tone::Divider);
        let ring = t.line.focus as f32;
        let radius = (row_h / 4) as f32;
        for (i, label) in self.options.iter().enumerate() {
            let row_y = b.y + (i as u32 * row_h) as i32;
            let center = Point::new(
                b.x + t.space.md as i32 + (row_h / 4) as i32,
                row_y + (row_h / 2) as i32,
            );
            cx.stroke_circle(center, radius, ring, color);
            let selected = self.selected == Some(i);
            if selected {
                cx.fill_circle(center, radius * 0.5, color);
            }
            let font = if selected { &t.font.body_bold } else { &t.font.body };
            let style = t.style(font, tone);
            let tl = cx.fonts.layout(label, &style, UNBOUNDED, Some(1), true);
            let x = center.x + radius as i32 + t.space.md as i32;
            let y = row_y + (row_h as i32 - tl.height as i32) / 2;
            cx.text(&tl, Point::new(x, y));
            if i + 1 < self.options.len() {
                let rule = Rect::new(
                    b.x,
                    row_y + row_h as i32 - t.line.divider as i32,
                    b.w,
                    t.line.divider,
                );
                cx.fill_rect(rule, divider);
            }
        }
        cx.focus_ring(t.radius.button);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let row_h = cx.theme().touch_target as i32;
        match ev {
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                let index = ((pos.y - cx.bounds().y) / row_h).max(0) as usize;
                self.select(cx, index);
                Handled::Yes
            }
            UiEvent::Key(k) if k.pressed && cx.is_focused() => {
                let count = self.options.len();
                let current = self.selected.unwrap_or(0);
                let next = match k.key {
                    Key::Up if current > 0 => current - 1,
                    Key::Down if current + 1 < count => current + 1,
                    _ => return Handled::No,
                };
                self.select(cx, next);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}
