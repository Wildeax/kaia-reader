//! [`Toggle`]: a two-part switch with text on both halves. The state shows
//! by fill and by label, so it never depends on color alone.

use kaia_events::Value;
use kaia_gfx::{Color, Point, Rect, Size};
use kaia_input::{Gesture, Key};

use super::{measure_text, Reaction, UNBOUNDED};
use crate::geometry::Constraints;
use crate::theme::Tone;
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};

/// A switch with an "off" label on the left half and an "on" label on the right.
/// The active half is filled with ink. Taps and Press toggle it.
pub struct Toggle {
    background: Option<Tone>,
    on: bool,
    off_label: String,
    on_label: String,
    enabled: bool,
    reaction: Reaction,
    /// Draw the pill of a list row instead of the two labelled halves.
    pill: bool,
}

/// Draws the pill of a switch in `track`: a track with a knob, filled with `color`
/// when on, and the knob on the right. Off, the knob sits on the left in `color`.
/// The same drawing serves list rows and pill toggles.
pub(crate) fn draw_pill(cx: &mut PaintCx, track: Rect, on: bool, color: Color, paper: Color) {
    let ring = cx.theme.line.focus;
    let h = track.h;
    let mid_y = track.y + (h / 2) as i32;
    let knob = (h as i32 / 2) - ring as i32;
    if on {
        cx.fill_rrect(track, h / 2, color);
        cx.fill_circle(
            Point::new(track.right() - h as i32 / 2, mid_y),
            knob as f32,
            paper,
        );
    } else {
        cx.border(track, h / 2, ring, color);
        cx.fill_circle(Point::new(track.x + h as i32 / 2, mid_y), knob as f32, color);
    }
}

impl Toggle {
    pub fn new(on: bool) -> Self {
        Self {
            background: None,
            on,
            off_label: "Off".to_string(),
            on_label: "On".to_string(),
            enabled: true,
            reaction: Reaction::None,
            pill: false,
        }
    }

    /// A switch drawn as the pill of a list row, so every on or off setting
    /// looks the same, with or without a row around it.
    pub fn pill(on: bool) -> Self {
        Self {
            pill: true,
            ..Self::new(on)
        }
    }

    /// The tone behind this control, which it clears its bounds to. Without
    /// this, the control uses the tone from its parent [`Frame`](crate::components::Frame),
    /// or `Surface`.
    pub fn background(mut self, tone: Tone) -> Self {
        self.background = Some(tone);
        self
    }

    /// The text on the off and on halves.
    pub fn labels(mut self, off: impl Into<String>, on: impl Into<String>) -> Self {
        self.off_label = off.into();
        self.on_label = on.into();
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

    pub fn set_on(&mut self, on: bool) {
        self.on = on;
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    pub fn set_enabled(&mut self, yes: bool) {
        self.enabled = yes;
    }

    fn toggle(&mut self, cx: &mut EventCx) {
        cx.request_focus_if_focusable();
        if !self.enabled {
            return;
        }
        self.on = !self.on;
        cx.request_paint();
        self.reaction.fire(cx, vec![Value::Bool(self.on)]);
    }
}

impl Widget for Toggle {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        if self.pill {
            // The pill is one and a half touch targets wide, inside one touch target high.
            let w = t.touch_target * 3 / 2;
            return c.constrain(Size::new(w, t.touch_target));
        }
        let off = measure_text(cx.fonts, t, &self.off_label, &t.font.body_bold, Tone::Ink).0;
        let on = measure_text(cx.fonts, t, &self.on_label, &t.font.body_bold, Tone::Ink).0;
        let half = (off.max(on) + 2 * t.space.md).max(t.touch_target);
        let ring = t.line.focus;
        c.constrain(Size::new(2 * half + 2 * ring, t.touch_target + 2 * ring))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        super::clear_bounds(cx, self.background);
        let t = cx.theme;
        let ring = t.line.focus;
        let radius = t.radius.button;
        if self.pill {
            let b = cx.bounds();
            let (w, h) = (t.touch_target * 3 / 2, t.touch_target / 2);
            let track = Rect::new(
                b.x + (b.w as i32 - w as i32) / 2,
                b.y + (b.h as i32 - h as i32) / 2,
                w,
                h,
            );
            let tone = if self.enabled { Tone::Ink } else { Tone::InkMuted };
            let color = cx.color(tone);
            let paper = cx.color(super::backdrop(cx, self.background));
            draw_pill(cx, track, self.on, color, paper);
            cx.focus_ring(radius + ring);
            return;
        }
        let inner = cx.bounds().inset(ring as i32);
        let ink = if self.enabled { Tone::Ink } else { Tone::InkMuted };
        let mid = inner.x + (inner.w / 2) as i32;
        let left = Rect::new(inner.x, inner.y, (mid - inner.x) as u32, inner.h);
        let right = Rect::new(mid, inner.y, (inner.right() - mid) as u32, inner.h);
        // The "on" half is inked. The other half is paper.
        let inactive = if self.on { left } else { right };

        let ink_color = cx.color(ink);
        let surface = cx.color(super::backdrop(cx, self.background));
        // Ink under the whole track, then paper over the inactive half. The
        // track keeps its rounded outer corners on both sides.
        cx.fill_rrect(inner, radius, ink_color);
        cx.fill_rect(inactive, surface);
        cx.border(inner, radius, t.line.divider, ink_color);

        let (left_tone, right_tone) = if self.on {
            (ink, Tone::Surface)
        } else {
            (Tone::Surface, ink)
        };
        for (rect, text, tone) in [
            (left, self.off_label.as_str(), left_tone),
            (right, self.on_label.as_str(), right_tone),
        ] {
            let style = t.style(&t.font.body_bold, tone);
            let tl = cx.fonts.layout(text, &style, UNBOUNDED, Some(1), true);
            let at = Point::new(
                rect.x + (rect.w as i32 - tl.width as i32) / 2,
                rect.y + (rect.h as i32 - tl.height as i32) / 2,
            );
            cx.text(&tl, at);
        }
        cx.focus_ring(radius + ring);
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
