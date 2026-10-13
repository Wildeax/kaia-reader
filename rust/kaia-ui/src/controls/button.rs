//! [`Button`] and [`IconButton`]: taps that emit an action or an event.

use kaia_gfx::text::FontSystem;
use kaia_gfx::{Point, Size};
use kaia_input::{Gesture, Key};

use super::{measure_text, Reaction, UNBOUNDED};
use crate::components::draw_icon_at;
use crate::geometry::Constraints;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, PaintHint, UiEvent, Widget};

/// How a button looks at rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Filled with ink, text in white.
    Primary,
    /// Outlined in ink, text in ink.
    Secondary,
    /// Text only, no outline.
    Text,
}

/// A labeled button with an optional icon. The content is text, an icon, or
/// an icon with text. The button is at least `touch_target` high and wide.
pub struct Button {
    style: ButtonStyle,
    text: Option<String>,
    icon: Option<String>,
    enabled: bool,
    pressed: bool,
    reaction: Reaction,
}

impl Button {
    pub fn new(style: ButtonStyle) -> Self {
        Self {
            style,
            text: None,
            icon: None,
            enabled: true,
            pressed: false,
            reaction: Reaction::None,
        }
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// An icon from `resources/icons/mdlight`, by name without the extension.
    pub fn icon(mut self, name: impl Into<String>) -> Self {
        self.icon = Some(name.into());
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

    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = Some(text.into());
    }

    /// A disabled button shows its content in `ink_muted` and ignores taps.
    pub fn set_enabled(&mut self, yes: bool) {
        self.enabled = yes;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The size of the content (icon, text, or both), before padding.
    fn content_size(&self, fonts: &mut FontSystem, t: &Theme) -> Size {
        let icon = self.icon.as_ref().map(|_| t.icon_size);
        let text = self
            .text
            .as_deref()
            .map(|s| measure_text(fonts, t, s, &t.font.body_bold, Tone::Ink));
        match (icon, text) {
            (Some(i), Some((tw, th))) => Size::new(i + t.space.sm + tw, i.max(th)),
            (Some(i), None) => Size::new(i, i),
            (None, Some((tw, th))) => Size::new(tw, th),
            (None, None) => Size::new(0, 0),
        }
    }

    /// Fill, text and outline tones for the current state.
    fn tones(&self) -> (Option<Tone>, Tone, Option<Tone>) {
        if !self.enabled {
            let outline = match self.style {
                ButtonStyle::Text => None,
                _ => Some(Tone::InkMuted),
            };
            return (None, Tone::InkMuted, outline);
        }
        // A press inverts the button: filled becomes outlined and outlined
        // or plain becomes filled.
        if self.pressed {
            return match self.style {
                ButtonStyle::Primary => (None, Tone::Ink, Some(Tone::Ink)),
                _ => (Some(Tone::Ink), Tone::Surface, None),
            };
        }
        match self.style {
            ButtonStyle::Primary => (Some(Tone::Ink), Tone::Surface, None),
            ButtonStyle::Secondary => (None, Tone::Ink, Some(Tone::Ink)),
            ButtonStyle::Text => (None, Tone::Ink, None),
        }
    }

    /// Presses the button: the press shows as an inverted frame until the
    /// app paints it, then the button returns to normal.
    fn press(&mut self, cx: &mut EventCx) {
        cx.request_focus();
        if !self.enabled {
            return;
        }
        self.pressed = true;
        cx.request_paint_with(PaintHint::Fast);
        cx.schedule_release::<Button>(|b, u| {
            b.pressed = false;
            u.request_paint();
        });
        self.reaction.fire(cx, Vec::new());
    }
}

impl Widget for Button {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let content = self.content_size(cx.fonts, t);
        let pad_h = t.space.md + t.line.focus;
        let pad_v = t.space.sm + t.line.focus;
        let w = (content.w + 2 * pad_h).max(t.touch_target);
        let h = (content.h + 2 * pad_v).max(t.touch_target);
        c.constrain(Size::new(w, h))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        super::clear_bounds(cx);
        let t = cx.theme;
        let ring = t.line.focus;
        let radius = t.radius.button;
        let inner = cx.bounds().inset(ring as i32);
        let (fill, fg, outline) = self.tones();
        if let Some(tone) = fill {
            let color = cx.color(tone);
            cx.fill_rrect(inner, radius, color);
        }
        if let Some(tone) = outline {
            let color = cx.color(tone);
            cx.border(inner, radius, ring, color);
        }

        let content = self.content_size(cx.fonts, t);
        let mut x = inner.x + (inner.w as i32 - content.w as i32) / 2;
        let fg_color = cx.color(fg);
        if let Some(name) = &self.icon {
            let px = t.icon_size;
            let y = inner.y + (inner.h as i32 - px as i32) / 2;
            draw_icon_at(cx, name, px, fg_color, Point::new(x, y));
            x += (px + t.space.sm) as i32;
        }
        if let Some(text) = &self.text {
            let style = t.style(&t.font.body_bold, fg);
            let tl = cx.fonts.layout(text, &style, UNBOUNDED, Some(1), true);
            let y = inner.y + (inner.h as i32 - tl.height as i32) / 2;
            cx.text(&tl, Point::new(x, y));
        }
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

/// A square button with an icon only. The icon is `icon_size` and the
/// button is `touch_target` square.
pub struct IconButton {
    icon: String,
    enabled: bool,
    pressed: bool,
    reaction: Reaction,
}

impl IconButton {
    pub fn new(icon: impl Into<String>) -> Self {
        Self {
            icon: icon.into(),
            enabled: true,
            pressed: false,
            reaction: Reaction::None,
        }
    }

    pub fn enabled(mut self, yes: bool) -> Self {
        self.enabled = yes;
        self
    }

    pub fn on_action(mut self, id: impl Into<String>) -> Self {
        self.reaction = Reaction::Action(id.into());
        self
    }

    pub fn on_event(mut self, name: impl Into<String>) -> Self {
        self.reaction = Reaction::Event(name.into());
        self
    }

    pub fn set_icon(&mut self, icon: impl Into<String>) {
        self.icon = icon.into();
    }

    pub fn set_enabled(&mut self, yes: bool) {
        self.enabled = yes;
    }
}

impl Widget for IconButton {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let side = cx.theme.touch_target;
        c.constrain(Size::new(side, side))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        super::clear_bounds(cx);
        let t = cx.theme;
        let ring = t.line.focus;
        let inner = cx.bounds().inset(ring as i32);
        let (fill, fg) = match (self.enabled, self.pressed) {
            (false, _) => (None, Tone::InkMuted),
            (true, true) => (Some(Tone::Ink), Tone::Surface),
            (true, false) => (None, Tone::Ink),
        };
        if let Some(tone) = fill {
            let color = cx.color(tone);
            cx.fill_rrect(inner, t.radius.button, color);
        }
        let px = t.icon_size;
        let color = cx.color(fg);
        let at = Point::new(
            inner.x + (inner.w as i32 - px as i32) / 2,
            inner.y + (inner.h as i32 - px as i32) / 2,
        );
        draw_icon_at(cx, &self.icon, px, color, at);
        cx.focus_ring(t.radius.button + ring);
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
        cx.request_focus();
        if self.enabled {
            self.pressed = true;
            cx.request_paint_with(PaintHint::Fast);
            cx.schedule_release::<IconButton>(|b, u| {
                b.pressed = false;
                u.request_paint();
            });
            self.reaction.fire(cx, Vec::new());
        }
        Handled::Yes
    }
}
