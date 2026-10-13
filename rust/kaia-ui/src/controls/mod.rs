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
//! Each control clears its bounds to its background tone before it draws, so a
//! changed state never leaves the old drawing behind. The background is the
//! control's own [`background`](Button::background) setting when it has one.
//! Otherwise it is the tone the parent sets: a [`Frame`](crate::components::Frame)
//! with a fill passes that fill to its children, and the default is `Surface`.
//! Keycaps in the on-screen keyboard keep their own fills.

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

use std::rc::Rc;

use kaia_events::{Event, Value};
use kaia_gfx::text::FontSystem;

use crate::theme::{FontSpec, Theme, Tone};
use crate::widget::{AppAction, EventCx, PaintCx};

/// Text wider than any screen. Controls measure with it, so text never wraps
/// or truncates while they size themselves.
pub(crate) const UNBOUNDED: u32 = 1 << 20;

/// A closure that runs inside the event handler of the control that fires it.
/// It can change the control's parent through the context, for example by
/// asking the window to lay out again.
pub type Hook = Rc<dyn Fn(&mut EventCx<'_>)>;

/// What a control does when the user changes it.
#[derive(Clone, Default)]
pub enum Reaction {
    #[default]
    None,
    /// Queues an [`AppAction`] with this id. Value controls put the new value in its args.
    Action(String),
    /// Queues a named event. Value controls put the new value in its args.
    Event(String),
    /// Runs a closure in the control's handler. The app never sees it.
    Hook(Hook),
}

impl Reaction {
    /// A reaction that runs `f`. Use it when the widget that changes must
    /// stay inside the widget tree, such as a menu that opens a sub-page.
    pub fn hook(f: impl Fn(&mut EventCx<'_>) + 'static) -> Self {
        Reaction::Hook(Rc::new(f))
    }

    pub(crate) fn fire(&self, cx: &mut EventCx, args: Vec<Value>) {
        match self {
            Reaction::None => {}
            Reaction::Action(id) => cx.action(AppAction { id: id.clone(), args }),
            Reaction::Event(name) => cx.emit(Event::with_args(name.clone(), args)),
            Reaction::Hook(f) => f(cx),
        }
    }
}

impl std::fmt::Debug for Reaction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Reaction::None => write!(f, "None"),
            Reaction::Action(id) => write!(f, "Action({id:?})"),
            Reaction::Event(name) => write!(f, "Event({name:?})"),
            Reaction::Hook(_) => write!(f, "Hook"),
        }
    }
}

/// Hooks compare by identity: two reactions are equal when they share the closure.
impl PartialEq for Reaction {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Reaction::None, Reaction::None) => true,
            (Reaction::Action(a), Reaction::Action(b)) | (Reaction::Event(a), Reaction::Event(b)) => a == b,
            (Reaction::Hook(a), Reaction::Hook(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for Reaction {}

/// The tone behind a control: its own background when set, otherwise the
/// one inherited through the paint context.
pub(crate) fn backdrop(cx: &PaintCx, own: Option<Tone>) -> Tone {
    own.unwrap_or_else(|| cx.background())
}

/// Clears the control's bounds to its backdrop. Called first in every paint.
pub(crate) fn clear_bounds(cx: &mut PaintCx, own: Option<Tone>) {
    let color = cx.color(backdrop(cx, own));
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
