//! [`ListRow`]: one row of a list or a settings menu. The whole row is the tap
//! target. A tap inverts the row for one frame and then restores it, as a
//! button does.

use kaia_events::Value;
use kaia_gfx::text::TextLayout;
use kaia_gfx::{Point, Rect, Size};
use kaia_input::Gesture;

use super::CoverImage;
use crate::components::draw_icon_at;
use crate::controls::Reaction;
use crate::geometry::Constraints;
use crate::pod::WidgetPod;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, PaintHint, UiEvent, UpdateCx, Widget};

/// What sits at the trailing edge of a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Trailing {
    None,
    /// A chevron that points into the next page.
    Chevron,
    /// Value text, such as the current font name.
    Value(String),
    /// A switch. The row flips it on tap and reports the new state as a bool argument.
    Toggle(bool),
    /// A check box. The row flips it on tap and reports the new state as a bool argument.
    Checkbox(bool),
}

/// Primary text (up to two lines), optional secondary text (one line), an
/// optional leading icon or cover, and an optional trailing element.
pub struct ListRow {
    primary: String,
    secondary: Option<String>,
    icon: Option<String>,
    cover: Option<WidgetPod>,
    trailing: Trailing,
    reaction: Reaction,
    enabled: bool,
    pressed: bool,
}

/// Width of the switch: one and a half touch targets.
fn switch_w(t: &Theme) -> u32 {
    t.touch_target * 3 / 2
}

/// Height of the switch and the side of a check box: half a touch target.
fn switch_h(t: &Theme) -> u32 {
    t.touch_target / 2
}

fn thumb_h(t: &Theme) -> u32 {
    t.row_height.saturating_sub(2 * t.space.sm)
}

fn thumb_w(t: &Theme) -> u32 {
    thumb_h(t) * 2 / 3
}

impl ListRow {
    pub fn new(primary: impl Into<String>) -> Self {
        Self {
            primary: primary.into(),
            secondary: None,
            icon: None,
            cover: None,
            trailing: Trailing::None,
            reaction: Reaction::None,
            enabled: true,
            pressed: false,
        }
    }

    /// One line under the primary text.
    pub fn secondary(mut self, text: impl Into<String>) -> Self {
        self.secondary = Some(text.into());
        self
    }

    /// A leading icon from `resources/icons/mdlight`.
    pub fn icon(mut self, name: impl Into<String>) -> Self {
        self.icon = Some(name.into());
        self
    }

    /// A leading cover thumbnail, in place of an icon.
    pub fn cover(mut self, cover: CoverImage) -> Self {
        self.cover = Some(WidgetPod::new(cover));
        self
    }

    pub fn chevron(mut self) -> Self {
        self.trailing = Trailing::Chevron;
        self
    }

    pub fn value(mut self, text: impl Into<String>) -> Self {
        self.trailing = Trailing::Value(text.into());
        self
    }

    pub fn toggle(mut self, on: bool) -> Self {
        self.trailing = Trailing::Toggle(on);
        self
    }

    pub fn checkbox(mut self, checked: bool) -> Self {
        self.trailing = Trailing::Checkbox(checked);
        self
    }

    pub fn enabled(mut self, yes: bool) -> Self {
        self.enabled = yes;
        self
    }

    /// Queues an action on tap. Switches and check boxes add the new state as a bool argument.
    pub fn on_action(mut self, id: impl Into<String>) -> Self {
        self.reaction = Reaction::Action(id.into());
        self
    }

    /// Queues a named event on tap.
    pub fn on_event(mut self, name: impl Into<String>) -> Self {
        self.reaction = Reaction::Event(name.into());
        self
    }

    /// Runs `f` in the row's handler on tap. See [`Reaction::hook`].
    pub fn on_hook(mut self, f: impl Fn(&mut EventCx<'_>) + 'static) -> Self {
        self.reaction = Reaction::hook(f);
        self
    }

    pub fn set_primary(&mut self, text: impl Into<String>) {
        self.primary = text.into();
    }

    pub fn set_value(&mut self, text: impl Into<String>) {
        self.trailing = Trailing::Value(text.into());
    }

    /// Sets a switch or check box state. Other trailing elements are unchanged.
    pub fn set_on(&mut self, on: bool) {
        match &mut self.trailing {
            Trailing::Toggle(v) | Trailing::Checkbox(v) => *v = on,
            _ => {}
        }
    }

    /// The state of a switch or check box. False for other rows.
    pub fn is_on(&self) -> bool {
        matches!(self.trailing, Trailing::Toggle(true) | Trailing::Checkbox(true))
    }

    pub fn trailing(&self) -> &Trailing {
        &self.trailing
    }

    fn leading_w(&self, t: &Theme) -> u32 {
        if self.cover.is_some() {
            thumb_w(t)
        } else if self.icon.is_some() {
            t.icon_size
        } else {
            0
        }
    }

    /// The text layouts for a row `width` pixels wide: primary, then secondary.
    fn text_layouts(
        &self,
        fonts: &mut kaia_gfx::text::FontSystem,
        t: &Theme,
        width: u32,
    ) -> (TextLayout, Option<TextLayout>) {
        let text_w = self.text_width(fonts, t, width);
        let primary = fonts.layout(
            &self.primary,
            &t.style(&t.font.body, self.main_tone()),
            text_w,
            Some(2),
            true,
        );
        let secondary = self.secondary.as_ref().map(|s| {
            fonts.layout(
                s,
                &t.style(&t.font.caption, self.sub_tone()),
                text_w,
                Some(1),
                true,
            )
        });
        (primary, secondary)
    }

    fn text_width(&self, fonts: &mut kaia_gfx::text::FontSystem, t: &Theme, width: u32) -> u32 {
        let lead = self.leading_w(t);
        let lead_block = if lead > 0 { lead + t.space.md } else { 0 };
        let trail = match &self.trailing {
            Trailing::None => 0,
            Trailing::Chevron => t.icon_size,
            Trailing::Value(text) => {
                // The value is drawn at most half the row wide, so it takes no more here.
                let style = t.style(&t.font.body, Tone::InkSecondary);
                fonts
                    .layout(text, &style, (width / 2).max(1), Some(1), true)
                    .width
            }
            Trailing::Toggle(_) => switch_w(t),
            Trailing::Checkbox(_) => switch_h(t),
        };
        let trail_block = if trail > 0 { trail + t.space.md } else { 0 };
        width
            .saturating_sub(t.space.md + lead_block + trail_block + t.space.md)
            .max(1)
    }

    /// Tones for the row's current state. A pressed row is inked, so its text is paper.
    fn main_tone(&self) -> Tone {
        if self.pressed {
            Tone::Surface
        } else if self.enabled {
            Tone::Ink
        } else {
            Tone::InkMuted
        }
    }

    fn sub_tone(&self) -> Tone {
        if self.pressed {
            Tone::Surface
        } else if self.enabled {
            Tone::InkSecondary
        } else {
            Tone::InkMuted
        }
    }
}

impl Widget for ListRow {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let w = c.max.w;
        let (primary, secondary) = self.text_layouts(cx.fonts, t, w);
        let mut block = primary.height;
        if let Some(s) = &secondary {
            block += t.space.xs + s.height;
        }
        let h = t.row_height.max(block + 2 * t.space.sm);
        if let Some(cover) = &mut self.cover {
            let (cw, ch) = (thumb_w(t), thumb_h(t));
            cover.layout(cx, Constraints::tight(Size::new(cw, ch)));
            cover.set_origin(Point::new(t.space.md as i32, ((h - ch) / 2) as i32));
        }
        c.constrain(Size::new(w, h))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let t = cx.theme;
        if self.pressed {
            let ink = cx.color(Tone::Ink);
            cx.fill_rect(b, ink);
        } else {
            let background = cx.color(cx.background());
            cx.fill_rect(b, background);
        }
        let md = t.space.md as i32;
        let lead_w = self.leading_w(t) as i32;
        let text_x = md + if lead_w > 0 { lead_w + md } else { 0 };
        let (primary, secondary) = self.text_layouts(cx.fonts, t, b.w);

        if let Some(name) = &self.icon {
            let color = cx.color(self.main_tone());
            let y = b.y + (b.h as i32 - t.icon_size as i32) / 2;
            draw_icon_at(cx, name, t.icon_size, color, Point::new(b.x + md, y));
        }

        let mut block = primary.height as i32;
        if let Some(s) = &secondary {
            block += t.space.xs as i32 + s.height as i32;
        }
        let top = b.y + (b.h as i32 - block) / 2;
        cx.text(&primary, Point::new(b.x + text_x, top));
        if let Some(s) = &secondary {
            cx.text(
                s,
                Point::new(b.x + text_x, top + primary.height as i32 + t.space.xs as i32),
            );
        }

        self.paint_trailing(cx, b);

        let divider = cx.color(Tone::Divider);
        let rule_w = b.w.saturating_sub(2 * t.space.md);
        cx.fill_rect(
            Rect::new(
                b.x + md,
                b.bottom() - t.line.hairline as i32,
                rule_w,
                t.line.hairline,
            ),
            divider,
        );
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let UiEvent::Gesture(Gesture::Tap { .. }) = ev else {
            return Handled::No;
        };
        if !self.enabled {
            return Handled::Yes;
        }
        match &mut self.trailing {
            Trailing::Toggle(on) | Trailing::Checkbox(on) => *on = !*on,
            _ => {}
        }
        self.pressed = true;
        cx.request_paint_with(PaintHint::Fast);
        cx.schedule_release::<ListRow>(|row, u: &mut UpdateCx<'_>| {
            row.pressed = false;
            u.request_paint();
        });
        let args = match self.trailing {
            Trailing::Toggle(on) | Trailing::Checkbox(on) => vec![Value::Bool(on)],
            _ => Vec::new(),
        };
        self.reaction.fire(cx, args);
        Handled::Yes
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.cover.iter_mut().collect()
    }
}

impl ListRow {
    fn paint_trailing(&self, cx: &mut PaintCx, b: Rect) {
        let t = cx.theme;
        let md = t.space.md as i32;
        let right = b.right() - md;
        let mid_y = b.y + b.h as i32 / 2;
        let fg = if self.pressed {
            Tone::Surface
        } else if self.enabled {
            Tone::Ink
        } else {
            Tone::InkMuted
        };
        let color = cx.color(fg);
        let paper = cx.color(if self.pressed { Tone::Ink } else { Tone::Surface });
        let ring = t.line.focus;
        match &self.trailing {
            Trailing::None => {}
            Trailing::Chevron => {
                let name = if t.rtl { "chevron.left" } else { "chevron.right" };
                let px = t.icon_size;
                draw_icon_at(
                    cx,
                    name,
                    px,
                    color,
                    Point::new(right - px as i32, mid_y - px as i32 / 2),
                );
            }
            Trailing::Value(text) => {
                let style = t.style(
                    &t.font.body,
                    if self.pressed {
                        Tone::Surface
                    } else {
                        Tone::InkSecondary
                    },
                );
                let max_w = (b.w / 2).max(1);
                let tl = cx.fonts.layout(text, &style, max_w, Some(1), true);
                cx.text(
                    &tl,
                    Point::new(right - tl.width as i32, mid_y - tl.height as i32 / 2),
                );
            }
            Trailing::Toggle(on) => {
                let (w, h) = (switch_w(t), switch_h(t));
                let track = Rect::new(right - w as i32, mid_y - h as i32 / 2, w, h);
                let knob = (h as i32 / 2) - ring as i32;
                if *on {
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
            Trailing::Checkbox(checked) => {
                let side = switch_h(t);
                let box_rect = Rect::new(right - side as i32, mid_y - side as i32 / 2, side, side);
                cx.border(box_rect, 0, ring, color);
                if *checked {
                    let px = side.saturating_sub(2 * ring);
                    let at = Point::new(box_rect.x + ring as i32, box_rect.y + ring as i32);
                    draw_icon_at(cx, "check", px, color, at);
                }
            }
        }
    }
}
