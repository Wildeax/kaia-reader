//! [`Button`] and [`IconButton`]: taps that emit an action or an event.

use kaia_gfx::text::FontSystem;
use kaia_gfx::{Point, Size};
use kaia_input::{Gesture, Key};

use super::{measure_text, Reaction};
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
    accented: bool,
    background: Option<Tone>,
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
            accented: false,
            background: None,
            style,
            text: None,
            icon: None,
            enabled: true,
            pressed: false,
            reaction: Reaction::None,
        }
    }

    /// Uses the theme accent for this button's ink and filled state.
    pub fn accent(mut self, selected: bool) -> Self {
        self.accented = selected;
        self
    }

    /// The tone behind this control, which it clears its bounds to. Without
    /// this, the control uses the tone from its parent [`Frame`](crate::components::Frame),
    /// or `Surface`.
    pub fn background(mut self, tone: Tone) -> Self {
        self.background = Some(tone);
        self
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

    /// Runs `f` in the handler on tap. See [`Reaction::hook`].
    pub fn on_hook(mut self, f: impl Fn(&mut EventCx<'_>) + 'static) -> Self {
        self.reaction = Reaction::hook(f);
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
        let ink = if self.accented { Tone::Accent } else { Tone::Ink };
        // A press inverts the button: filled becomes outlined and outlined
        // or plain becomes filled.
        if self.pressed {
            return match self.style {
                ButtonStyle::Primary => (None, ink, Some(ink)),
                _ => (Some(ink), Tone::Surface, None),
            };
        }
        match self.style {
            ButtonStyle::Primary => (Some(ink), Tone::Surface, None),
            ButtonStyle::Secondary => (None, ink, Some(ink)),
            ButtonStyle::Text => (None, ink, None),
        }
    }

    /// Presses the button: the press shows as an inverted frame until the
    /// app paints it, then the button returns to normal.
    fn press(&mut self, cx: &mut EventCx) {
        cx.request_focus_if_focusable();
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
        super::clear_bounds(cx, self.background);
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

        // The text gets the width inside the outline after the icon, with one
        // outline width of margin on each side, and ends with an ellipsis when it
        // is longer. A button at its natural width has more room than the text needs.
        let icon_px = self.icon.as_ref().map(|_| t.icon_size);
        let icon_block = icon_px.map_or(0, |px| px + t.space.sm);
        let room = (inner.w as i32 - 2 * ring as i32 - icon_block as i32).max(1) as u32;
        let fg_color = cx.color(fg);
        let text_layout = self.text.as_ref().map(|text| {
            let style = t.style(&t.font.body_bold, fg);
            cx.fonts.layout(text, &style, room, Some(1), true)
        });
        let content_w = icon_px.map_or(0, |px| px + if text_layout.is_some() { t.space.sm } else { 0 })
            + text_layout.as_ref().map_or(0, |tl| tl.width);
        let mut x = inner.x + (inner.w as i32 - content_w as i32).max(0) / 2;
        if let Some(px) = icon_px {
            let name = self.icon.as_deref().unwrap_or_default();
            let y = inner.y + (inner.h as i32 - px as i32) / 2;
            draw_icon_at(cx, name, px, fg_color, Point::new(x, y));
            x += (px + t.space.sm) as i32;
        }
        if let Some(tl) = &text_layout {
            let y = inner.y + (inner.h as i32 - tl.height as i32) / 2;
            cx.text(tl, Point::new(x, y));
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
    background: Option<Tone>,
    icon: String,
    enabled: bool,
    pressed: bool,
    reaction: Reaction,
}

impl IconButton {
    /// Sets the reaction without a builder method. Composite widgets use it
    /// for reactions they build at run time.
    pub(crate) fn reaction(mut self, reaction: Reaction) -> Self {
        self.reaction = reaction;
        self
    }

    pub fn new(icon: impl Into<String>) -> Self {
        Self {
            background: None,
            icon: icon.into(),
            enabled: true,
            pressed: false,
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

    pub fn on_action(mut self, id: impl Into<String>) -> Self {
        self.reaction = Reaction::Action(id.into());
        self
    }

    pub fn on_event(mut self, name: impl Into<String>) -> Self {
        self.reaction = Reaction::Event(name.into());
        self
    }

    /// Runs `f` in the handler on tap. See [`Reaction::hook`].
    pub fn on_hook(mut self, f: impl Fn(&mut EventCx<'_>) + 'static) -> Self {
        self.reaction = Reaction::hook(f);
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
        super::clear_bounds(cx, self.background);
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
        cx.request_focus_if_focusable();
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
