//! [`ToolButton`]: an icon above a one-line label, drawn the way the nav bar
//! draws its items. It is at least `touch_target` high and wide, and it fills
//! its cell when the parent gives it more room.

use kaia_gfx::{Point, Size};
use kaia_input::{Gesture, Key};

use super::{clear_bounds, measure_text, Reaction};
use crate::components::draw_icon_at;
use crate::geometry::Constraints;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, PaintHint, UiEvent, Widget};

/// An icon with a label under it. A tap queues its reaction, and a press
/// inverts the button until the app has painted the pressed frame.
pub struct ToolButton {
    background: Option<Tone>,
    icon: String,
    label: String,
    enabled: bool,
    pressed: bool,
    reaction: Reaction,
}

impl ToolButton {
    /// A button with `icon` from `resources/icons/mdlight`, by name without the
    /// extension, and the label `label` under it.
    pub fn new(icon: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            background: None,
            icon: icon.into(),
            label: label.into(),
            enabled: true,
            pressed: false,
            reaction: Reaction::None,
        }
    }

    /// The tone behind this control, which it clears its bounds to. Without
    /// this, the control uses the tone from its parent frame, or `Surface`.
    pub fn background(mut self, tone: Tone) -> Self {
        self.background = Some(tone);
        self
    }

    pub fn enabled(mut self, yes: bool) -> Self {
        self.enabled = yes;
        self
    }

    /// Queues an [`AppAction`](crate::AppAction) with this id on tap.
    pub fn on_action(mut self, id: impl Into<String>) -> Self {
        self.reaction = Reaction::Action(id.into());
        self
    }

    /// Queues a named event on tap.
    pub fn on_event(mut self, name: impl Into<String>) -> Self {
        self.reaction = Reaction::Event(name.into());
        self
    }

    /// Runs `f` in the handler on tap. See [`Reaction::hook`].
    pub fn on_hook(mut self, f: impl Fn(&mut EventCx<'_>) + 'static) -> Self {
        self.reaction = Reaction::hook(f);
        self
    }

    /// Changes the label, for example when a toggle changes its wording.
    /// Call `request_layout` in the update when the width may change.
    pub fn set_label(&mut self, label: impl Into<String>) {
        self.label = label.into();
    }

    pub fn set_icon(&mut self, icon: impl Into<String>) {
        self.icon = icon.into();
    }

    pub fn set_enabled(&mut self, yes: bool) {
        self.enabled = yes;
    }

    /// Whether the control is filled, and the tone of its icon and label.
    /// A press inverts the control: a filled control becomes plain, and a
    /// plain one becomes filled.
    fn look(&self) -> (bool, Tone) {
        match (self.enabled, self.pressed) {
            (false, _) => (false, Tone::InkMuted),
            (true, true) => (true, Tone::Surface),
            (true, false) => (false, Tone::Ink),
        }
    }

    fn press(&mut self, cx: &mut EventCx) {
        cx.request_focus_if_focusable();
        if !self.enabled {
            return;
        }
        self.pressed = true;
        cx.request_paint_with(PaintHint::Fast);
        cx.schedule_release::<ToolButton>(|b, u| {
            b.pressed = false;
            u.request_paint();
        });
        self.reaction.fire(cx, Vec::new());
    }
}

impl Widget for ToolButton {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t: &Theme = cx.theme;
        let (label_w, label_h) = measure_text(cx.fonts, t, &self.label, &t.font.label, Tone::Ink);
        let w = (label_w + 2 * t.space.sm).max(t.touch_target);
        let h = (t.icon_size + t.space.xs + label_h + 2 * t.space.xs).max(t.touch_target);
        c.constrain(Size::new(w, h))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        clear_bounds(cx, self.background);
        let t = cx.theme;
        let ring = t.line.focus;
        let radius = t.radius.button;
        let inner = cx.bounds().inset(ring as i32);
        let (filled, fg) = self.look();
        if filled {
            let fill = cx.color(Tone::Ink);
            cx.fill_rrect(inner, radius, fill);
        }

        let icon = t.icon_size as i32;
        let gap = t.space.xs as i32;
        let style = t.style(&t.font.label, fg);
        let label = cx
            .fonts
            .layout(&self.label, &style, inner.w.max(1), Some(1), true);
        let stack = icon + gap + label.height as i32;
        let top = inner.y + (inner.h as i32 - stack) / 2;
        let fg_color = cx.color(fg);
        draw_icon_at(
            cx,
            &self.icon,
            t.icon_size,
            fg_color,
            Point::new(inner.x + (inner.w as i32 - icon) / 2, top),
        );
        let at = Point::new(
            inner.x + (inner.w as i32 - label.width as i32) / 2,
            top + icon + gap,
        );
        cx.text(&label, at);
        cx.focus_ring(radius + ring);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let activate = match ev {
            UiEvent::Gesture(Gesture::Tap { .. }) => true,
            UiEvent::Key(k) => k.pressed && k.key == Key::Press && cx.is_focused(),
            _ => false,
        };
        if !activate {
            return Handled::No;
        }
        self.press(cx);
        Handled::Yes
    }
}
