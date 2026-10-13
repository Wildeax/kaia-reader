//! Backends for the app loop. [`Backend`] is a display that also yields input.
//! The desktop window ([`kaia_display::DesktopDisplay`]) is one. [`HeadlessBackend`]
//! is the other: a recording display with scripted input, for tests and for
//! screenshots without a window.
//!
//! ## Script format
//!
//! One command per line. Blank lines and `#` comments are ignored.
//!
//! ```text
//! down 0 500 800 0     touch down: slot x y time_ms (kaia_input format)
//! move 0 540 800 100   touch move
//! up   0 540 800 120   touch up
//! tap 632 840          down and up at x y, 100 ms after the previous event
//! key RPgFwd           key press and release (names as kaia_input::Key)
//! wait 300             pause for 300 ms of real time
//! ```
//!
//! `poll` lines from the kaia_input format are accepted and ignored. The app
//! reads the clock for gesture timers, so they are not needed.

use std::collections::VecDeque;
use std::fmt;
use std::thread;
use std::time::Duration;

use kaia_display::{Display, DisplayError, HeadlessDisplay, InputEvent, InputSource, PanelSpec};
use kaia_gfx::{PixelFormat, Pixmap, Point, Size};
use kaia_input::{parse_script, Key, KeyEvent, Phase, Step, TouchEvent};

/// A display that also yields input. The app loop needs both.
pub trait Backend: Display + InputSource {}

impl<T: Display + InputSource> Backend for T {}

/// How long an idle headless poll sleeps when nothing is scheduled.
const IDLE: Duration = Duration::from_millis(10);

/// One item of a scripted session.
#[derive(Debug, Clone, PartialEq)]
enum Item {
    Input(InputEvent),
    Wait(Duration),
}

/// A queue of scripted input. [`ScriptInput::poll`] returns the queued events in
/// order, sleeping through `wait` items.
#[derive(Debug, Default)]
pub struct ScriptInput {
    items: VecDeque<Item>,
    quit_at_end: bool,
    quit_sent: bool,
    /// Time of the last scripted touch, in milliseconds, for `tap`.
    cursor_ms: u64,
}

/// A script line that could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "script line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ScriptError {}

impl ScriptInput {
    /// No scripted input. The backend idles until the app stops.
    pub fn idle() -> Self {
        Self::default()
    }

    /// Input from a script (see the module docs). When `quit_at_end` is set, the
    /// backend asks the app to quit after the last item.
    pub fn from_script(text: &str, quit_at_end: bool) -> Result<Self, ScriptError> {
        let mut input = ScriptInput {
            quit_at_end,
            ..ScriptInput::default()
        };
        for (index, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let line_no = index + 1;
            let fail = |message: String| ScriptError {
                line: line_no,
                message,
            };
            let mut words = line.split_whitespace();
            match words.next() {
                Some("key") => {
                    let name = words.next().ok_or_else(|| fail("key needs a name".into()))?;
                    let key = parse_key(name).ok_or_else(|| fail(format!("unknown key {name}")))?;
                    let time_us = input.cursor_ms * 1000;
                    input.push(Item::Input(InputEvent::Key(KeyEvent {
                        key,
                        pressed: true,
                        time_us,
                    })));
                    input.push(Item::Input(InputEvent::Key(KeyEvent {
                        key,
                        pressed: false,
                        time_us,
                    })));
                }
                Some("wait") => {
                    let ms: u64 = words
                        .next()
                        .and_then(|w| w.parse().ok())
                        .ok_or_else(|| fail("wait needs milliseconds".into()))?;
                    input.push(Item::Wait(Duration::from_millis(ms)));
                }
                Some("tap") => {
                    let coords: Vec<i32> = words.filter_map(|w| w.parse().ok()).collect();
                    let [x, y] = coords[..] else {
                        return Err(fail("tap needs x and y".into()));
                    };
                    let t0 = input
                        .cursor_ms
                        .checked_add(100)
                        .ok_or_else(|| fail("timestamp is too large".into()))?;
                    input.cursor_ms = t0
                        .checked_add(50)
                        .filter(|ms| ms.checked_mul(1000).is_some())
                        .ok_or_else(|| fail("timestamp is too large".into()))?;
                    let pos = Point::new(x, y);
                    input.push(Item::Input(InputEvent::Touch(TouchEvent {
                        slot: 0,
                        phase: Phase::Down,
                        pos,
                        time_us: t0 * 1000,
                    })));
                    input.push(Item::Input(InputEvent::Touch(TouchEvent {
                        slot: 0,
                        phase: Phase::Up,
                        pos,
                        time_us: input.cursor_ms * 1000,
                    })));
                }
                _ => {
                    // Touch lines use the kaia_input format. Poll lines are ignored.
                    let steps = parse_script(line).map_err(|e| fail(e.to_string()))?;
                    for step in steps {
                        if let Step::Touch(touch) = step {
                            input.cursor_ms = touch.time_us / 1000;
                            input.push(Item::Input(InputEvent::Touch(touch)));
                        }
                    }
                }
            }
        }
        Ok(input)
    }

    fn push(&mut self, item: Item) {
        self.items.push_back(item);
    }

    /// True when nothing is queued.
    pub fn is_done(&self) -> bool {
        self.items.is_empty()
    }
}

impl InputSource for ScriptInput {
    fn poll(&mut self, timeout: Option<Duration>) -> Vec<InputEvent> {
        let mut out = Vec::new();
        loop {
            match self.items.front_mut() {
                Some(Item::Input(event)) => {
                    out.push(event.clone());
                    self.items.pop_front();
                }
                Some(Item::Wait(left)) => {
                    if !out.is_empty() {
                        return out;
                    }
                    let slice = timeout.map_or(*left, |t| t.min(*left));
                    thread::sleep(slice);
                    *left -= slice;
                    if left.is_zero() {
                        self.items.pop_front();
                    }
                    return out;
                }
                None => {
                    // The batch goes out first. The quit comes on the next poll, after
                    // the app has handled these events.
                    if !out.is_empty() {
                        return out;
                    }
                    if self.quit_at_end && !self.quit_sent {
                        self.quit_sent = true;
                        out.push(InputEvent::Quit);
                    } else {
                        thread::sleep(timeout.map_or(IDLE, |t| t.min(IDLE)));
                    }
                    return out;
                }
            }
        }
    }
}

/// The key names a script accepts.
fn parse_key(name: &str) -> Option<Key> {
    Some(match name {
        "LPgBack" => Key::LPgBack,
        "LPgFwd" => Key::LPgFwd,
        "RPgBack" => Key::RPgBack,
        "RPgFwd" => Key::RPgFwd,
        "Back" => Key::Back,
        "Press" => Key::Press,
        "Up" => Key::Up,
        "Down" => Key::Down,
        "Left" => Key::Left,
        "Right" => Key::Right,
        single if single.chars().count() == 1 => Key::Char(single.chars().next()?),
        _ => return None,
    })
}

/// A headless display with scripted input. It records every refresh, so tests can
/// check what was sent to the panel.
pub struct HeadlessBackend {
    display: HeadlessDisplay,
    input: ScriptInput,
}

impl HeadlessBackend {
    pub fn new(panel: PanelSpec, input: ScriptInput) -> Self {
        Self {
            display: HeadlessDisplay::new(panel),
            input,
        }
    }

    pub fn display(&self) -> &HeadlessDisplay {
        &self.display
    }

    pub fn input_mut(&mut self) -> &mut ScriptInput {
        &mut self.input
    }
}

impl Display for HeadlessBackend {
    fn size(&self) -> Size {
        self.display.size()
    }

    fn format(&self) -> PixelFormat {
        self.display.format()
    }

    fn dpi(&self) -> u32 {
        self.display.dpi()
    }

    fn present(&mut self, frame: &Pixmap, refresh: &kaia_display::Refresh) -> Result<(), DisplayError> {
        self.display.present(frame, refresh)
    }

    fn set_rotation(&mut self, rotation: kaia_display::Rotation) -> Result<(), DisplayError> {
        self.display.set_rotation(rotation)
    }
}

impl InputSource for HeadlessBackend {
    fn poll(&mut self, timeout: Option<Duration>) -> Vec<InputEvent> {
        self.input.poll(timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_touch_key_and_tap_lines() {
        let mut input =
            ScriptInput::from_script("down 0 10 20 0\nup 0 10 20 50\nkey RPgFwd\ntap 5 6\n", false).unwrap();
        let events = input.poll(None);
        assert_eq!(events.len(), 2 + 2 + 2);
        assert!(matches!(events[0], InputEvent::Touch(t) if t.phase == Phase::Down));
        assert!(matches!(events[2], InputEvent::Key(k) if k.key == Key::RPgFwd && k.pressed));
        assert!(input.is_done());
    }

    #[test]
    fn quits_once_at_the_end_when_asked() {
        let mut input = ScriptInput::from_script("key Back\n", true).unwrap();
        assert_eq!(input.poll(None).len(), 2);
        assert_eq!(input.poll(None), vec![InputEvent::Quit]);
        assert!(input.poll(None).is_empty());
    }

    #[test]
    fn reports_the_line_of_a_bad_command() {
        let err = ScriptInput::from_script("key Back\nkey Nope\n", false).unwrap_err();
        assert_eq!(err.line, 2);
    }
}
