//! [`Stepper`]: a minus button, the value, and a plus button.

use kaia_events::Value;
use kaia_gfx::{Rect, Size};
use kaia_input::{Gesture, Key};

use super::{default_format, measure_text, round_to_step, Formatter, Reaction, UNBOUNDED};
use crate::geometry::Constraints;
use crate::theme::Tone;
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};

/// A value between `min` and `max` that changes by `step` per tap. The minus
/// and plus buttons dim at the limits. The event argument is the new value.
pub struct Stepper {
    background: Option<Tone>,
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    format: Formatter,
    enabled: bool,
    reaction: Reaction,
}

impl Stepper {
    /// The value starts clamped to `min..=max`.
    pub fn new(value: f64, min: f64, max: f64, step: f64) -> Self {
        Self {
            background: None,
            value: value.clamp(min, max),
            min,
            max,
            step,
            format: Box::new(default_format),
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

    /// The text for the value, for example `|v| format!("{v} pt")`.
    pub fn format(mut self, f: impl Fn(f64) -> String + 'static) -> Self {
        self.format = Box::new(f);
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

    pub fn value(&self) -> f64 {
        self.value
    }

    pub fn set_value(&mut self, v: f64) {
        self.value = v.clamp(self.min, self.max);
    }

    pub fn set_enabled(&mut self, yes: bool) {
        self.enabled = yes;
    }

    fn can_decrease(&self) -> bool {
        self.enabled && self.value > self.min
    }

    fn can_increase(&self) -> bool {
        self.enabled && self.value < self.max
    }

    fn commit(&mut self, cx: &mut EventCx, v: f64) {
        let v = v.clamp(self.min, self.max);
        if v == self.value {
            return;
        }
        self.value = v;
        cx.request_paint();
        self.reaction.fire(cx, vec![Value::Number(v)]);
    }

    fn step(&mut self, cx: &mut EventCx, dir: f64) {
        if !self.enabled {
            return;
        }
        let next = round_to_step(self.value + dir * self.step, self.step, self.min);
        self.commit(cx, next);
    }
}

impl Widget for Stepper {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let sq = t.touch_target;
        let mut value_w = 0;
        for v in [self.min, self.max, self.value] {
            let text = (self.format)(v);
            value_w = value_w.max(measure_text(cx.fonts, t, &text, &t.font.body_bold, Tone::Ink).0);
        }
        // A value area of at least three touch targets keeps the minus buttons of
        // stacked steppers in one column.
        let value_w = (value_w + 2 * t.space.md).max(3 * sq);
        let ring = t.line.focus;
        c.constrain(Size::new(2 * sq + value_w + 2 * ring, sq + 2 * ring))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        super::clear_bounds(cx, self.background);
        let t = cx.theme;
        let ring = t.line.focus;
        let sq = t.touch_target;
        let radius = t.radius.button;
        let inner = cx.bounds().inset(ring as i32);
        let value_w = inner.w.saturating_sub(2 * sq);
        let minus = Rect::new(inner.x, inner.y, sq, inner.h);
        let value_box = Rect::new(inner.x + sq as i32, inner.y, value_w, inner.h);
        let plus = Rect::new(value_box.right(), inner.y, sq, inner.h);

        let (minus_tone, plus_tone) = (
            if self.can_decrease() {
                Tone::Ink
            } else {
                Tone::InkMuted
            },
            if self.can_increase() {
                Tone::Ink
            } else {
                Tone::InkMuted
            },
        );
        let minus_color = cx.color(minus_tone);
        let plus_color = cx.color(plus_tone);
        // The same ring as a button, so steppers and buttons share one outline weight.
        cx.border(minus, radius, t.line.focus, minus_color);
        cx.border(plus, radius, t.line.focus, plus_color);

        // Minus and plus are drawn as bars, so they need no icon font.
        let bar = sq / 3;
        let thick = ring;
        let (mx, my) = (minus.x + (sq / 2) as i32, minus.y + (minus.h / 2) as i32);
        cx.fill_rect(
            Rect::new(mx - (bar / 2) as i32, my - (thick / 2) as i32, bar, thick),
            minus_color,
        );
        let (px, py) = (plus.x + (sq / 2) as i32, plus.y + (plus.h / 2) as i32);
        cx.fill_rect(
            Rect::new(px - (bar / 2) as i32, py - (thick / 2) as i32, bar, thick),
            plus_color,
        );
        cx.fill_rect(
            Rect::new(px - (thick / 2) as i32, py - (bar / 2) as i32, thick, bar),
            plus_color,
        );

        let text = (self.format)(self.value);
        let tone = if self.enabled { Tone::Ink } else { Tone::InkMuted };
        let style = t.style(&t.font.body_bold, tone);
        let tl = cx.fonts.layout(&text, &style, UNBOUNDED, Some(1), true);
        let at = kaia_gfx::Point::new(
            value_box.x + (value_box.w as i32 - tl.width as i32) / 2,
            value_box.y + (value_box.h as i32 - tl.height as i32) / 2,
        );
        cx.text(&tl, at);
        cx.focus_ring(radius + ring);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        match ev {
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                cx.request_focus_if_focusable();
                let t = cx.theme();
                let b = cx.bounds();
                let edge = (t.line.focus + t.touch_target) as i32;
                let x = pos.x - b.x;
                if x < edge {
                    self.step(cx, -1.0);
                } else if x >= b.w as i32 - edge {
                    self.step(cx, 1.0);
                }
                Handled::Yes
            }
            UiEvent::Key(k) if k.pressed && cx.is_focused() => match k.key {
                Key::Left | Key::Down => {
                    self.step(cx, -1.0);
                    Handled::Yes
                }
                Key::Right | Key::Up => {
                    self.step(cx, 1.0);
                    Handled::Yes
                }
                _ => Handled::No,
            },
            _ => Handled::No,
        }
    }
}
