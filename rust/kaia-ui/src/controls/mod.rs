//! Interactive controls. A control reports what the user did as an
//! [`AppAction`](crate::AppAction) or a named event. The app sets the state
//! back with [`Window::update`](crate::Window::update).
//!
//! Press feedback follows KOReader's button. A tap inverts the control with a
//! `Fast` refresh, and the control returns to normal after the app has painted
//! that frame. See [`EventCx::schedule_release`](crate::EventCx::schedule_release).
//! Focused controls draw the `line.focus` outline inside their bounds, so every
//! focusable control reserves `line.focus` on each side.
//!
//! Each control clears its bounds to `surface` before it draws, so a changed
//! state never leaves the old drawing behind. Controls therefore expect a
//! surface background. Put them on a tinted card only with that in mind.

mod button;
mod checkbox;
mod keyboard;
mod keyboard_layout;
mod radio;
mod slider;
mod stepper;
mod text_field;
mod toggle;

pub use button::{Button, ButtonStyle, IconButton};
pub use checkbox::Checkbox;
pub use keyboard::OnScreenKeyboard;
pub use keyboard_layout::{KeyAction, KeyDef, KeyLabels, KeyboardLayout, Layer, ENGLISH};
pub use radio::RadioGroup;
pub use slider::Slider;
pub use stepper::Stepper;
pub use text_field::TextField;
pub use toggle::Toggle;

use kaia_events::{Event, Value};
use kaia_gfx::text::FontSystem;

use crate::theme::{FontSpec, Theme, Tone};
use crate::widget::{AppAction, EventCx, PaintCx};

/// Text wider than any screen. Controls measure with it, so text never wraps
/// or truncates while they size themselves.
pub(crate) const UNBOUNDED: u32 = 1 << 20;

/// What a control does when the user changes it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Reaction {
    #[default]
    None,
    /// Queues an [`AppAction`] with this id. Value controls put the new value in its args.
    Action(String),
    /// Queues a named event. Value controls put the new value in its args.
    Event(String),
}

impl Reaction {
    pub(crate) fn fire(&self, cx: &mut EventCx, args: Vec<Value>) {
        match self {
            Reaction::None => {}
            Reaction::Action(id) => cx.action(AppAction { id: id.clone(), args }),
            Reaction::Event(name) => cx.emit(Event::with_args(name.clone(), args)),
        }
    }
}

/// Paints the control's bounds in the surface tone. Called first in every paint.
pub(crate) fn clear_bounds(cx: &mut PaintCx) {
    let color = cx.color(Tone::Surface);
    let b = cx.bounds();
    cx.fill_rect(b, color);
}

/// The size of one line of `text` in `font`.
pub(crate) fn measure_text(
    fonts: &mut FontSystem,
    theme: &Theme,
    text: &str,
    font: &FontSpec,
    tone: Tone,
) -> (u32, u32) {
    let style = theme.style(font, tone);
    let tl = fonts.layout(text, &style, UNBOUNDED, Some(1), false);
    (tl.width, tl.height)
}

/// Turns a numeric value into the text a control shows.
pub(crate) type Formatter = Box<dyn Fn(f64) -> String>;

/// Whole numbers without a decimal point, other numbers as Rust prints them.
pub(crate) fn default_format(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}
