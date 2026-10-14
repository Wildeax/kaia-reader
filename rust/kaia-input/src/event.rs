//! Raw input events.
//!
//! Touch samples follow KOReader's `frontend/device/input.lua`: each slot carries a
//! position and a timestamp, and a lift is a sample whose tracking id is -1 there
//! (here, [`Phase::Up`]).

use kaia_gfx::Point;

/// What a touch sample reports for its slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The contact touched the screen.
    Down,
    /// The contact moved and stayed down.
    Move,
    /// The contact left the screen.
    Up,
}

/// One sample for one touch slot. Times are microseconds on the event clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchEvent {
    pub slot: u8,
    pub phase: Phase,
    pub pos: Point,
    pub time_us: u64,
}

/// A key the device or a desktop keyboard can send.
///
/// The named variants use KOReader's key names (see `Key::name`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Page-turn buttons on the left side of the device.
    LPgBack,
    LPgFwd,
    /// Page-turn buttons on the right side of the device.
    RPgBack,
    RPgFwd,
    Power,
    /// The magnetic cover sensor reports the cover closing and opening.
    SleepCoverClosed,
    SleepCoverOpened,
    Home,
    Back,
    Menu,
    Up,
    Down,
    Left,
    Right,
    /// The OK or Enter key.
    Press,
    Backspace,
    Del,
    End,
    /// A text key from a desktop keyboard.
    Char(char),
}

impl Key {
    /// KOReader's name for the key. Text keys use the upper-case character.
    pub fn name(self) -> String {
        let fixed = match self {
            Key::LPgBack => "LPgBack",
            Key::LPgFwd => "LPgFwd",
            Key::RPgBack => "RPgBack",
            Key::RPgFwd => "RPgFwd",
            Key::Power => "Power",
            Key::SleepCoverClosed => "SleepCoverClosed",
            Key::SleepCoverOpened => "SleepCoverOpened",
            Key::Home => "Home",
            Key::Back => "Back",
            Key::Menu => "Menu",
            Key::Up => "Up",
            Key::Down => "Down",
            Key::Left => "Left",
            Key::Right => "Right",
            Key::Press => "Press",
            Key::Backspace => "Backspace",
            Key::Del => "Del",
            Key::End => "End",
            Key::Char(c) => return c.to_ascii_uppercase().to_string(),
        };
        fixed.to_string()
    }
}

/// A key press or release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    pub pressed: bool,
    pub time_us: u64,
}

/// A stylus sample. Routed separately from finger gestures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenEvent {
    pub phase: Phase,
    pub pos: Point,
    pub pressure: u32,
    /// True when the eraser end of the stylus is in use.
    pub eraser: bool,
    /// True when the stylus barrel button is held.
    pub button: bool,
    pub time_us: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_names_follow_koreader() {
        assert_eq!(Key::RPgFwd.name(), "RPgFwd");
        assert_eq!(Key::SleepCoverOpened.name(), "SleepCoverOpened");
        assert_eq!(Key::Press.name(), "Press");
        assert_eq!(Key::Char('a').name(), "A");
        assert_eq!(Key::Char('7').name(), "7");
    }
}
