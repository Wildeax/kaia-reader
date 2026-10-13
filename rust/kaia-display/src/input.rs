//! The input trait and the events it yields.

use std::time::Duration;

use kaia_gfx::Size;
use kaia_input::{KeyEvent, TouchEvent};

/// One thing the user did, or one change the system made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    Touch(TouchEvent),
    Key(KeyEvent),
    /// The window was closed or the app should exit.
    Quit,
    /// The logical screen size changed, for example after a rotation.
    Resize(Size),
}

/// A source of input events: the desktop window, a scripted replay, and later evdev
/// on the Kobo.
pub trait InputSource {
    /// Returns the events that arrived since the last call.
    ///
    /// With `timeout` set, returns after that long at the latest, possibly with no
    /// events. With `timeout` None, waits until at least one event arrives.
    fn poll(&mut self, timeout: Option<Duration>) -> Vec<InputEvent>;
}
