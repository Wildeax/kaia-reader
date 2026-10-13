//! [`OnScreenKeyboard`]: a touch keyboard for the focused [`TextField`](super::TextField).

use kaia_gfx::{Point, Rect, Size};
use kaia_input::{Gesture, Key};

use super::keyboard_layout::{KeyAction, KeyDef, KeyboardLayout, Layer};
use super::{Reaction, UNBOUNDED};
use crate::geometry::Constraints;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, PaintHint, UiEvent, Widget};

/// Units per row in every layout.
const ROW_UNITS: f32 = 10.0;
/// Rows per layer in every layout.
const ROWS: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Case {
    Lower,
    /// The next letter is upper case, then the state returns to lower case.
    Shift,
    /// Upper case until the caps key is tapped again.
    Caps,
}

/// One key on screen. `rect` is relative to the keyboard.
#[derive(Clone, Debug, PartialEq)]
pub struct KeyBox {
    pub rect: Rect,
    pub action: KeyAction,
    pub label: String,
}

/// A keyboard with letters, numbers and symbols. Taps send keys to the focused
/// widget with [`EventCx::key_to_focus`]: characters as `Key::Char`, delete
/// as `Key::Backspace`, Enter as `Key::Press`. The hide key emits the
/// `on_hide` event. The keyboard is never focused itself.
pub struct OnScreenKeyboard {
    background: Option<Tone>,
    layout: &'static KeyboardLayout,
    layer: Layer,
    case: Case,
    /// The key that is showing its press feedback.
    pressed: Option<Rect>,
    on_hide: Reaction,
}

impl OnScreenKeyboard {
    pub fn new(layout: &'static KeyboardLayout) -> Self {
        Self {
            background: None,
            layout,
            layer: Layer::Letters,
            case: Case::Lower,
            pressed: None,
            on_hide: Reaction::None,
        }
    }

    /// The tone behind this control, which it clears its bounds to. Without
    /// this, the control uses the tone from its parent [`Frame`](crate::components::Frame),
    /// or `Surface`.
    pub fn background(mut self, tone: Tone) -> Self {
        self.background = Some(tone);
        self
    }

    /// Queues this named event when the hide key is tapped.
    pub fn on_hide(mut self, name: impl Into<String>) -> Self {
        self.on_hide = Reaction::Event(name.into());
        self
    }

    pub fn layer(&self) -> Layer {
        self.layer
    }

    /// True while the next letter, or every letter, is upper case.
    pub fn is_upper(&self) -> bool {
        self.case != Case::Lower
    }

    /// True when caps lock is on.
    pub fn is_caps(&self) -> bool {
        self.case == Case::Caps
    }

    /// The height of the keyboard for `theme`.
    pub fn height(theme: &Theme) -> u32 {
        2 * theme.space.sm + ROWS * theme.touch_target + (ROWS - 1) * theme.space.xs
    }

    /// Every key on the current layer, laid out for a keyboard `width` pixels wide.
    pub fn key_boxes(&self, t: &Theme, width: u32) -> Vec<KeyBox> {
        let margin = t.space.sm as f32;
        let gap = t.space.xs as f32;
        let cell = (width as f32 - 2.0 * margin) / ROW_UNITS;
        let row_h = t.touch_target as f32;
        let mut out = Vec::new();
        for (ri, row) in self.rows().iter().enumerate() {
            let units: f32 = row.iter().map(|k| k.width).sum();
            let mut x = margin + (ROW_UNITS - units) / 2.0 * cell;
            let y = margin + ri as f32 * (row_h + gap);
            for key in row.iter() {
                let w = key.width * cell;
                let rect = Rect::new(
                    (x + gap / 2.0).round() as i32,
                    y.round() as i32,
                    (w - gap).round().max(0.0) as u32,
                    row_h.round() as u32,
                );
                out.push(KeyBox {
                    rect,
                    action: key.action,
                    label: self.label_for(key.action),
                });
                x += w;
            }
        }
        out
    }

    fn rows(&self) -> &'static [&'static [KeyDef]] {
        match self.layer {
            Layer::Letters => self.layout.letters,
            Layer::Numbers => self.layout.numbers,
            Layer::Symbols => self.layout.symbols,
        }
    }

    fn label_for(&self, action: KeyAction) -> String {
        let labels = &self.layout.labels;
        match action {
            KeyAction::Char(c) => {
                if self.case == Case::Lower {
                    c.to_string()
                } else {
                    upper(c).to_string()
                }
            }
            KeyAction::Shift => match self.case {
                Case::Caps => labels.caps,
                _ => labels.shift,
            }
            .to_string(),
            KeyAction::Backspace => labels.delete.to_string(),
            KeyAction::Enter => labels.enter.to_string(),
            KeyAction::Space => labels.space.to_string(),
            KeyAction::Hide => labels.hide.to_string(),
            KeyAction::Layer(Layer::Letters) => "ABC".to_string(),
            KeyAction::Layer(Layer::Numbers) => "123".to_string(),
            KeyAction::Layer(Layer::Symbols) => "#+=".to_string(),
        }
    }

    fn activate(&mut self, cx: &mut EventCx, action: KeyAction) {
        match action {
            KeyAction::Char(c) => {
                let ch = if self.case == Case::Lower { c } else { upper(c) };
                cx.key_to_focus(Key::Char(ch));
                if self.case == Case::Shift {
                    self.case = Case::Lower;
                    cx.request_paint();
                }
            }
            KeyAction::Shift => {
                self.case = match self.case {
                    Case::Lower => Case::Shift,
                    Case::Shift => Case::Caps,
                    Case::Caps => Case::Lower,
                };
                cx.request_paint();
            }
            KeyAction::Backspace => cx.key_to_focus(Key::Backspace),
            KeyAction::Enter => cx.key_to_focus(Key::Press),
            KeyAction::Space => cx.key_to_focus(Key::Char(' ')),
            KeyAction::Layer(layer) => {
                self.layer = layer;
                cx.request_paint();
            }
            KeyAction::Hide => self.on_hide.fire(cx, Vec::new()),
        }
    }
}

/// The upper-case form of `c`. Characters with several capitals use the first.
fn upper(c: char) -> char {
    c.to_uppercase().next().unwrap_or(c)
}

impl Widget for OnScreenKeyboard {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(c.max.w, Self::height(cx.theme)))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        super::clear_bounds(cx, self.background);
        let t = cx.theme;
        let b = cx.bounds();
        for k in self.key_boxes(t, b.w) {
            let rect = k.rect.translate(b.x, b.y);
            let pressed = self.pressed == Some(k.rect);
            let shift_on = matches!(k.action, KeyAction::Shift) && self.case != Case::Lower;
            let engaged = pressed || shift_on;
            let function = !matches!(k.action, KeyAction::Char(_));
            let (fill, fg) = if engaged {
                (Tone::Ink, Tone::Surface)
            } else if function {
                (Tone::SurfaceAlt, Tone::Ink)
            } else {
                (Tone::Surface, Tone::Ink)
            };
            let fill_color = cx.color(fill);
            cx.fill_rrect(rect, t.radius.button, fill_color);
            if !engaged {
                let edge = cx.color(Tone::Divider);
                cx.border(rect, t.radius.button, t.line.hairline, edge);
            }
            let style = t.style(&t.font.body, fg);
            let tl = cx.fonts.layout(&k.label, &style, UNBOUNDED, Some(1), true);
            let at = Point::new(
                rect.x + (rect.w as i32 - tl.width as i32) / 2,
                rect.y + (rect.h as i32 - tl.height as i32) / 2,
            );
            cx.text(&tl, at);
        }
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let UiEvent::Gesture(Gesture::Tap { pos, .. }) = ev else {
            return Handled::No;
        };
        let t = cx.theme();
        let b = cx.bounds();
        let rel = Point::new(pos.x - b.x, pos.y - b.y);
        let hit = self.key_boxes(t, b.w).into_iter().find(|k| k.rect.contains(rel));
        if let Some(k) = hit {
            // Press feedback: the key inverts with a fast refresh, and the
            // release runs after the app has painted that frame.
            self.pressed = Some(k.rect);
            cx.request_paint_rect(k.rect, PaintHint::Fast);
            cx.schedule_release::<OnScreenKeyboard>(|kb, u| {
                if let Some(r) = kb.pressed.take() {
                    u.request_paint_rect(r, PaintHint::Ui);
                }
            });
            self.activate(cx, k.action);
        }
        Handled::Yes
    }
}
