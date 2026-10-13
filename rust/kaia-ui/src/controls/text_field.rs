//! [`TextField`]: one line of text with a caret, horizontal scrolling and a
//! password mode.

use std::cmp::Ordering;

use kaia_events::Value;
use kaia_gfx::text::FontSystem;
use kaia_gfx::{Point, Rect, Size};
use kaia_input::{Gesture, Key};

use super::{Reaction, UNBOUNDED};
use crate::geometry::Constraints;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};

/// A single-line field. A tap places the caret at the nearest character. Key
/// events (`Char`, `Backspace`, `Del`, `Left`, `Right`) edit the text, and
/// `Press` submits it. The text scrolls so the caret stays visible.
///
/// Password mode draws each character as a dot. The text itself is unchanged.
pub struct TextField {
    background: Option<Tone>,
    text: String,
    placeholder: String,
    /// Caret position in characters, from 0 to the character count.
    caret: usize,
    /// Pixels the text is scrolled to the left.
    scroll: f32,
    max_len: Option<usize>,
    password: bool,
    on_change: Reaction,
    on_submit: Reaction,
}

impl Default for TextField {
    fn default() -> Self {
        Self::new()
    }
}

impl TextField {
    pub fn new() -> Self {
        Self {
            background: None,
            text: String::new(),
            placeholder: String::new(),
            caret: 0,
            scroll: 0.0,
            max_len: None,
            password: false,
            on_change: Reaction::None,
            on_submit: Reaction::None,
        }
    }

    /// The tone behind this control, which it clears its bounds to. Without
    /// this, the control uses the tone from its parent [`Frame`](crate::components::Frame),
    /// or `Surface`.
    pub fn background(mut self, tone: Tone) -> Self {
        self.background = Some(tone);
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self.caret = self.len();
        self
    }

    /// Text shown while the field is empty.
    pub fn placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// The field takes no more characters than `n`.
    pub fn max_length(mut self, n: usize) -> Self {
        self.max_len = Some(n);
        self
    }

    pub fn password(mut self, yes: bool) -> Self {
        self.password = yes;
        self
    }

    /// Queues a named event after each change. Its argument is the new text.
    pub fn on_change(mut self, name: impl Into<String>) -> Self {
        self.on_change = Reaction::Event(name.into());
        self
    }

    /// Queues a named event when `Press` is pressed. Its argument is the text.
    pub fn on_submit(mut self, name: impl Into<String>) -> Self {
        self.on_submit = Reaction::Event(name.into());
        self
    }

    /// The text as typed, even in password mode.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The caret in characters.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Pixels the text is scrolled left. Zero while the text fits.
    pub fn scroll(&self) -> f32 {
        self.scroll
    }

    /// Replaces the text and moves the caret to the end. Does not fire `on_change`.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.caret = self.len();
    }

    pub fn set_password(&mut self, yes: bool) {
        self.password = yes;
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn at_limit(&self) -> bool {
        self.max_len.is_some_and(|m| self.len() >= m)
    }

    /// The byte index of character `i`, or the end of the text.
    fn byte_index(&self, i: usize) -> usize {
        self.text
            .char_indices()
            .nth(i)
            .map_or(self.text.len(), |(b, _)| b)
    }

    /// What is drawn: the text, or one dot per character in password mode.
    fn shown(&self) -> String {
        if self.password {
            "\u{2022}".repeat(self.len())
        } else {
            self.text.clone()
        }
    }

    /// The text area inside the field: inside the focus ring and the padding.
    fn area(&self, t: &Theme, bounds: Rect) -> Rect {
        let inner = bounds.inset(t.line.focus as i32);
        let pad = t.space.sm;
        Rect::new(
            inner.x + pad as i32,
            inner.y,
            inner.w.saturating_sub(2 * pad),
            inner.h,
        )
    }

    /// Pixel x of every caret position: entry `i` is the x after `i` characters.
    fn boundaries(&self, fonts: &mut FontSystem, t: &Theme) -> Vec<f32> {
        let style = t.style(&t.font.body, Tone::Ink);
        let mut xs = vec![0.0_f32];
        let mut prefix = String::new();
        for ch in self.shown().chars() {
            prefix.push(ch);
            let w = fonts.layout(&prefix, &style, UNBOUNDED, None, false).width;
            xs.push(w as f32);
        }
        xs
    }

    /// Scrolls so the caret is inside the visible width.
    fn follow_caret(&mut self, xs: &[f32], visible: u32) {
        let vis = visible as f32;
        let caret_x = xs[self.caret.min(xs.len() - 1)];
        if caret_x - self.scroll < 0.0 {
            self.scroll = caret_x;
        }
        if caret_x - self.scroll > vis {
            self.scroll = caret_x - vis;
        }
        let total = xs.last().copied().unwrap_or(0.0);
        self.scroll = self.scroll.clamp(0.0, (total - vis).max(0.0));
    }

    fn remove_at(&mut self, i: usize) {
        let at = self.byte_index(i);
        self.text.remove(at);
    }

    fn edit(&mut self, cx: &mut EventCx, key: Key) -> Handled {
        let changed = match key {
            Key::Char(c) => {
                if c.is_control() {
                    return Handled::No;
                }
                if self.at_limit() {
                    return Handled::Yes;
                }
                let at = self.byte_index(self.caret);
                self.text.insert(at, c);
                self.caret += 1;
                true
            }
            Key::Backspace => {
                if self.caret == 0 {
                    return Handled::Yes;
                }
                self.caret -= 1;
                self.remove_at(self.caret);
                true
            }
            Key::Del => {
                if self.caret >= self.len() {
                    return Handled::Yes;
                }
                self.remove_at(self.caret);
                true
            }
            Key::Left => {
                self.caret = self.caret.saturating_sub(1);
                false
            }
            Key::Right => {
                if self.caret < self.len() {
                    self.caret += 1;
                }
                false
            }
            Key::Press => {
                self.on_submit.fire(cx, vec![Value::Str(self.text.clone())]);
                return Handled::Yes;
            }
            _ => return Handled::No,
        };
        let t = cx.theme();
        let area = self.area(t, cx.bounds());
        let xs = self.boundaries(cx.fonts(), t);
        self.follow_caret(&xs, area.w);
        if changed {
            self.on_change.fire(cx, vec![Value::Str(self.text.clone())]);
        }
        cx.request_paint();
        Handled::Yes
    }
}

/// The caret position nearest to `x` (pixels from the start of the text).
fn caret_for_x(xs: &[f32], x: f32) -> usize {
    xs.iter()
        .enumerate()
        .min_by(|a, b| {
            (a.1 - x)
                .abs()
                .partial_cmp(&(b.1 - x).abs())
                .unwrap_or(Ordering::Equal)
        })
        .map_or(0, |(i, _)| i)
}

impl Widget for TextField {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(c.max.w, cx.theme.touch_target))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        super::clear_bounds(cx, self.background);
        let t = cx.theme;
        let ring = t.line.focus;
        let radius = t.radius.button;
        let b = cx.bounds();
        let edge = cx.color(Tone::Divider);
        cx.border(b.inset(ring as i32), radius, t.line.divider, edge);

        let area = self.area(t, b);
        let xs = self.boundaries(cx.fonts, t);
        self.follow_caret(&xs, area.w);
        let style = t.style(&t.font.body, Tone::Ink);
        let line_h = style.line_height.ceil() as u32;
        let text_y = area.y + (area.h as i32 - line_h as i32) / 2;

        if self.text.is_empty() {
            if !self.placeholder.is_empty() {
                let muted = t.style(&t.font.body, Tone::InkMuted);
                let tl = cx
                    .fonts
                    .layout(&self.placeholder, &muted, UNBOUNDED, Some(1), true);
                cx.text(&tl, Point::new(area.x, text_y));
            }
        } else {
            let shown = self.shown();
            let tl = cx.fonts.layout(&shown, &style, UNBOUNDED, None, false);
            let x0 = area.x - self.scroll.round() as i32;
            cx.clipped(area, |cx| cx.text(&tl, Point::new(x0, text_y)));
        }

        if cx.is_focused() {
            let caret_x = area.x + (xs[self.caret.min(xs.len() - 1)] - self.scroll).round() as i32;
            if caret_x >= area.x && caret_x <= area.right() {
                let caret = cx.color(Tone::Ink);
                cx.fill_rect(Rect::new(caret_x, text_y, ring, line_h), caret);
            }
        }
        cx.focus_ring(radius + ring);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        match ev {
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                cx.request_focus();
                let t = cx.theme();
                let area = self.area(t, cx.bounds());
                let xs = self.boundaries(cx.fonts(), t);
                let x = (pos.x - area.x) as f32 + self.scroll;
                self.caret = caret_for_x(&xs, x);
                self.follow_caret(&xs, area.w);
                cx.request_paint();
                Handled::Yes
            }
            UiEvent::Key(k) if k.pressed && cx.is_focused() => self.edit(cx, k.key),
            _ => Handled::No,
        }
    }
}
