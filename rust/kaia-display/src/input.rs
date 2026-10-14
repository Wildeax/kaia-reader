//! The input trait and the events it yields.

use std::time::Duration;

use kaia_gfx::Size;
use kaia_input::{KeyEvent, TouchEvent};

/// One thing the user did, or one change the system made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputEvent {
    Touch(TouchEvent),
    Pen(kaia_input::PenEvent),
    /// Touch samples that arrived in one input frame (one evdev SYN_REPORT).
    /// The gesture detector applies all of them before it runs, like KOReader.
    TouchFrame(Vec<TouchEvent>),
    Key(KeyEvent),
    /// The hardware light button was released.
    Light,
    /// An orientation sensor reported a new orientation.
    Orientation(crate::Rotation),
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

/// Alt-drag simulates the pen. The right button uses the eraser.
#[derive(Default)]
pub struct DesktopPen {
    previous: Option<kaia_gfx::Point>,
    eraser: bool,
}

impl DesktopPen {
    pub fn active(&self) -> bool {
        self.previous.is_some()
    }

    pub fn sample(
        &mut self,
        pos: kaia_gfx::Point,
        down: bool,
        eraser: bool,
        time_us: u64,
    ) -> Option<kaia_input::PenEvent> {
        use kaia_input::Phase;
        let phase = match (self.previous, down) {
            (None, true) => {
                self.eraser = eraser;
                Phase::Down
            }
            (Some(_), false) => Phase::Up,
            (Some(last), true) if last != pos => Phase::Move,
            _ => return None,
        };
        self.previous = down.then_some(pos);
        Some(kaia_input::PenEvent {
            phase,
            pos,
            pressure: if down { 1024 } else { 0 },
            eraser: self.eraser,
            button: false,
            time_us,
        })
    }
}

#[cfg(test)]
mod pen_tests {
    use super::*;
    use kaia_gfx::Point;
    use kaia_input::Phase;
    #[test]
    fn desktop_pen_latches_eraser_until_lift() {
        let mut pen = DesktopPen::default();
        let down = pen.sample(Point::new(2, 3), true, true, 1).unwrap();
        assert_eq!(down.phase, Phase::Down);
        assert!(down.eraser);
        assert!(pen.sample(Point::new(2, 3), true, false, 2).is_none());
        assert_eq!(
            pen.sample(Point::new(4, 5), true, false, 3).unwrap().phase,
            Phase::Move
        );
        let up = pen.sample(Point::new(4, 5), false, false, 4).unwrap();
        assert_eq!(up.phase, Phase::Up);
        assert!(up.eraser);
        assert!(!pen.active());
    }
}
