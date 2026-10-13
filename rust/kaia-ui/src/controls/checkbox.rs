//! [`Checkbox`]: a square box with a check mark and an optional label.

use kaia_events::Value;
use kaia_gfx::{Point, Rect, Size};
use kaia_input::{Gesture, Key};

use super::{measure_text, Reaction, UNBOUNDED};
use crate::components::draw_icon_at;
use crate::geometry::Constraints;
use crate::theme::Tone;
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};

/// A box that shows a check mark when checked. The whole row toggles on tap.
pub struct Checkbox {
    background: Option<Tone>,
    checked: bool,
    label: Option<String>,
    enabled: bool,
    reaction: Reaction,
}

impl Checkbox {
    pub fn new(checked: bool) -> Self {
        Self {
            background: None,
            checked,
            label: None,
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

    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.label = Some(text.into());
        self
    }

    pub fn enabled(mut self, yes: bool) -> Self {
        self.enabled = yes;
        self
    }

    /// Queues a named event on change. Its argument is the new state as a bool.
    pub fn on_event(mut self, name: impl Into<String>) -> Self {
        self.reaction = Reaction::Event(name.into());
        self
    }

    /// Queues an action on change. Its argument is the new state as a bool.
    pub fn on_action(mut self, id: impl Into<String>) -> Self {
        self.reaction = Reaction::Action(id.into());
        self
    }

    pub fn set_checked(&mut self, yes: bool) {
        self.checked = yes;
    }

    pub fn is_checked(&self) -> bool {
        self.checked
    }

    pub fn set_enabled(&mut self, yes: bool) {
        self.enabled = yes;
    }

    /// The side of the box: half the touch target.
    fn box_px(t: &crate::theme::Theme) -> u32 {
        t.touch_target / 2
    }

    fn toggle(&mut self, cx: &mut EventCx) {
        cx.request_focus_if_focusable();
        if !self.enabled {
            return;
        }
        self.checked = !self.checked;
        cx.request_paint();
        self.reaction.fire(cx, vec![Value::Bool(self.checked)]);
    }
}

impl Widget for Checkbox {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let ring = t.line.focus;
        let mut w = Self::box_px(t) + 2 * ring;
        if let Some(text) = &self.label {
            let (tw, _) = measure_text(cx.fonts, t, text, &t.font.body, Tone::Ink);
            w += t.space.md + tw;
        }
        c.constrain(Size::new(w, t.touch_target))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        super::clear_bounds(cx, self.background);
        let t = cx.theme;
        let ring = t.line.focus;
        let b = cx.bounds();
        let side = Self::box_px(t);
        let tone = if self.enabled { Tone::Ink } else { Tone::InkMuted };
        let color = cx.color(tone);
        let box_rect = Rect::new(
            b.x + ring as i32,
            b.y + (b.h as i32 - side as i32) / 2,
            side,
            side,
        );
        cx.border(box_rect, 0, ring, color);
        if self.checked {
            let px = side - 2 * ring;
            let at = Point::new(box_rect.x + ring as i32, box_rect.y + ring as i32);
            draw_icon_at(cx, "check", px, color, at);
        }
        if let Some(text) = &self.label {
            let style = t.style(&t.font.body, tone);
            let tl = cx.fonts.layout(text, &style, UNBOUNDED, Some(1), true);
            let x = box_rect.right() + t.space.md as i32;
            let y = b.y + (b.h as i32 - tl.height as i32) / 2;
            cx.text(&tl, Point::new(x, y));
        }
        cx.focus_ring(ring);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        match ev {
            UiEvent::Gesture(Gesture::Tap { .. }) => {
                self.toggle(cx);
                Handled::Yes
            }
            UiEvent::Key(k) if k.pressed && k.key == Key::Press && cx.is_focused() => {
                self.toggle(cx);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}
