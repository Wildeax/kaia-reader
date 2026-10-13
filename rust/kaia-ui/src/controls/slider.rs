//! [`Slider`]: a track with discrete steps. A tap or a drag sets the value.

use kaia_events::Value;
use kaia_gfx::{Point, Rect, Size};
use kaia_input::{Gesture, Key};

use super::{default_format, measure_text, Formatter, Reaction, UNBOUNDED};
use crate::geometry::Constraints;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};
use kaia_gfx::text::FontSystem;

/// A horizontal track from `min` to `max` in steps of `step`. The knob sits
/// on the step. A tap on the track, or a pan, moves it to the nearest step.
/// The value is printed to the right of the track. The event argument is the new value.
pub struct Slider {
    value: f64,
    min: f64,
    max: f64,
    step: f64,
    format: Formatter,
    enabled: bool,
    reaction: Reaction,
}

impl Slider {
    /// The value is snapped to the nearest step inside `min..=max`.
    pub fn new(value: f64, min: f64, max: f64, step: f64) -> Self {
        let mut s = Self {
            value: min,
            min,
            max,
            step: step.max(f64::MIN_POSITIVE),
            format: Box::new(default_format),
            enabled: true,
            reaction: Reaction::None,
        };
        s.value = s.snap(value);
        s
    }

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
        self.value = self.snap(v);
    }

    pub fn set_enabled(&mut self, yes: bool) {
        self.enabled = yes;
    }

    /// The nearest step inside the range.
    fn snap(&self, v: f64) -> f64 {
        let k = ((v - self.min) / self.step).round().max(0.0);
        (self.min + k * self.step).min(self.max)
    }

    /// Number of steps between `min` and `max`.
    fn steps(&self) -> u32 {
        ((self.max - self.min) / self.step).round().max(1.0) as u32
    }

    fn label_width(&self, fonts: &mut FontSystem, t: &Theme) -> u32 {
        let lo = measure_text(fonts, t, &(self.format)(self.min), &t.font.body, Tone::Ink).0;
        let hi = measure_text(fonts, t, &(self.format)(self.max), &t.font.body, Tone::Ink).0;
        lo.max(hi)
    }

    /// The track inside `bounds`: left of the value label, inset by the focus ring.
    fn track(&self, fonts: &mut FontSystem, t: &Theme, bounds: Rect) -> Rect {
        let ring = t.line.focus as i32;
        let gap = t.space.md as i32;
        let label = self.label_width(fonts, t) as i32;
        let x0 = bounds.x + ring + gap;
        let x1 = bounds.right() - ring - gap - label - gap;
        let h = t.line.progress;
        let y = bounds.y + (bounds.h as i32 - h as i32) / 2;
        Rect::new(x0, y, (x1 - x0).max(0) as u32, h)
    }

    /// The value for the absolute x coordinate on `track`.
    fn value_at(&self, track: Rect, x: i32) -> f64 {
        if track.w == 0 {
            return self.value;
        }
        let frac = ((x - track.x) as f64 / track.w as f64).clamp(0.0, 1.0);
        let k = (frac * self.steps() as f64).round();
        (self.min + k * self.step).min(self.max)
    }

    fn commit(&mut self, cx: &mut EventCx, v: f64) {
        let v = self.snap(v);
        if v == self.value {
            return;
        }
        self.value = v;
        cx.request_paint();
        self.reaction.fire(cx, vec![Value::Number(v)]);
    }

    fn move_by(&mut self, cx: &mut EventCx, steps: f64) {
        if self.enabled {
            let next = self.value + steps * self.step;
            self.commit(cx, next);
        }
    }

    fn drag_to(&mut self, cx: &mut EventCx, x: i32) {
        if !self.enabled {
            return;
        }
        let t = cx.theme();
        let b = cx.bounds();
        let track = self.track(cx.fonts(), t, b);
        let v = self.value_at(track, x);
        self.commit(cx, v);
    }
}

impl Widget for Slider {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(c.max.w, cx.theme.touch_target))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        super::clear_bounds(cx);
        let t = cx.theme;
        let b = cx.bounds();
        let track = self.track(cx.fonts, t, b);
        let tone = if self.enabled { Tone::Ink } else { Tone::InkMuted };
        let track_color = cx.color(Tone::Divider);
        cx.fill_rect(track, track_color);

        let span = self.max - self.min;
        let frac = if span > 0.0 {
            ((self.value - self.min) / span).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let fill_w = (track.w as f64 * frac).round() as u32;
        let accent = cx.color(Tone::Accent);
        cx.fill_rect(Rect::new(track.x, track.y, fill_w, track.h), accent);

        let knob_r = (t.touch_target / 3) as f32;
        let center = Point::new(track.x + fill_w as i32, track.y + (track.h / 2) as i32);
        let knob = cx.color(tone);
        cx.fill_circle(center, knob_r, knob);

        let text = (self.format)(self.value);
        let style = t.style(&t.font.body, tone);
        let tl = cx.fonts.layout(&text, &style, UNBOUNDED, Some(1), true);
        let ring = t.line.focus as i32;
        let at = Point::new(
            b.right() - ring - tl.width as i32,
            b.y + (b.h as i32 - tl.height as i32) / 2,
        );
        cx.text(&tl, at);
        cx.focus_ring(t.radius.button + t.line.focus);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        match ev {
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                cx.request_focus();
                self.drag_to(cx, pos.x);
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Pan { pos, .. }) => {
                self.drag_to(cx, pos.x);
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Swipe { end_pos, .. }) => {
                self.drag_to(cx, end_pos.x);
                Handled::Yes
            }
            UiEvent::Key(k) if k.pressed && cx.is_focused() => match k.key {
                Key::Left | Key::Down => {
                    self.move_by(cx, -1.0);
                    Handled::Yes
                }
                Key::Right | Key::Up => {
                    self.move_by(cx, 1.0);
                    Handled::Yes
                }
                _ => Handled::No,
            },
            _ => Handled::No,
        }
    }
}
