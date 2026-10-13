//! Maps one desktop mouse onto touch slots, so gestures can be tested on the emulator.
//!
//! A plain press drives slot 0. A right button press, or a press with Ctrl held, drives
//! a second finger on slot 1. The second finger mirrors the pointer through the anchor
//! point (usually the screen centre), so a drag pinches or spreads around the anchor.

use kaia_gfx::Point;

use crate::event::{Phase, TouchEvent};

/// The mouse button that started a press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Press {
    two_fingers: bool,
}

/// State of one mouse pointer mapped onto touch slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopPointer {
    anchor: Point,
    press: Option<Press>,
}

impl DesktopPointer {
    pub fn new(anchor: Point) -> Self {
        Self { anchor, press: None }
    }

    pub fn is_pressed(&self) -> bool {
        self.press.is_some()
    }

    /// Starts a touch. Returns no events when a press is already active.
    pub fn press(&mut self, pos: Point, time_us: u64, button: MouseButton, ctrl: bool) -> Vec<TouchEvent> {
        if self.press.is_some() {
            return Vec::new();
        }
        let two_fingers = button == MouseButton::Right || ctrl;
        self.press = Some(Press { two_fingers });
        let mut out = vec![sample(0, Phase::Down, pos, time_us)];
        if two_fingers {
            out.push(sample(1, Phase::Down, self.mirror(pos), time_us));
        }
        out
    }

    /// Moves the active touches. Returns no events when nothing is pressed.
    pub fn move_to(&mut self, pos: Point, time_us: u64) -> Vec<TouchEvent> {
        let Some(press) = self.press else {
            return Vec::new();
        };
        let mut out = vec![sample(0, Phase::Move, pos, time_us)];
        if press.two_fingers {
            out.push(sample(1, Phase::Move, self.mirror(pos), time_us));
        }
        out
    }

    /// Lifts the active touches. Slot 0 lifts first, then slot 1, as the two-finger
    /// tap logic expects.
    pub fn release(&mut self, pos: Point, time_us: u64) -> Vec<TouchEvent> {
        let Some(press) = self.press.take() else {
            return Vec::new();
        };
        let mut out = vec![sample(0, Phase::Up, pos, time_us)];
        if press.two_fingers {
            out.push(sample(1, Phase::Up, self.mirror(pos), time_us));
        }
        out
    }

    fn mirror(&self, pos: Point) -> Point {
        Point::new(2 * self.anchor.x - pos.x, 2 * self.anchor.y - pos.y)
    }
}

fn sample(slot: u8, phase: Phase, pos: Point, time_us: u64) -> TouchEvent {
    TouchEvent {
        slot,
        phase,
        pos,
        time_us,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn left_press_uses_one_slot() {
        let mut p = DesktopPointer::new(Point::new(500, 800));
        let down = p.press(Point::new(450, 800), 0, MouseButton::Left, false);
        assert_eq!(down.len(), 1);
        assert_eq!(down[0].slot, 0);
        let up = p.release(Point::new(450, 800), 10);
        assert_eq!(up.len(), 1);
        assert!(!p.is_pressed());
    }

    #[test]
    fn right_button_or_ctrl_adds_a_mirrored_second_finger() {
        let mut p = DesktopPointer::new(Point::new(500, 800));
        let down = p.press(Point::new(450, 800), 0, MouseButton::Right, false);
        assert_eq!(down.len(), 2);
        assert_eq!(down[1].slot, 1);
        assert_eq!(down[1].pos, Point::new(550, 800));

        p.release(Point::new(450, 800), 5);
        let down = p.press(Point::new(450, 800), 0, MouseButton::Left, true);
        assert_eq!(down.len(), 2, "ctrl makes a two-finger press");
    }

    #[test]
    fn moves_and_release_order() {
        let mut p = DesktopPointer::new(Point::new(500, 800));
        p.press(Point::new(450, 800), 0, MouseButton::Right, false);
        let moved = p.move_to(Point::new(400, 800), 10);
        assert_eq!(moved[0].pos, Point::new(400, 800));
        assert_eq!(moved[1].pos, Point::new(600, 800));
        let up = p.release(Point::new(400, 800), 20);
        assert_eq!(up.iter().map(|e| e.slot).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(up[0].phase, Phase::Up);
    }

    #[test]
    fn events_without_a_press_are_ignored() {
        let mut p = DesktopPointer::new(Point::new(0, 0));
        assert!(p.move_to(Point::new(1, 1), 0).is_empty());
        assert!(p.release(Point::new(1, 1), 0).is_empty());
    }
}
