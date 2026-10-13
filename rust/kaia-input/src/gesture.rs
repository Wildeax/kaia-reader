//! Gesture values produced by the recognizer.
//!
//! Names and field meanings follow KOReader's `frontend/device/gesturedetector.lua`.
//! `pos` is the field KOReader sets as `pos`, and it means what KOReader means by it
//! for each gesture. Midpoints round down. Distances and spans are in pixels.

use kaia_gfx::Point;

/// A compass direction. [`Direction::name`] returns KOReader's string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    North,
    NorthEast,
    East,
    SouthEast,
    South,
    SouthWest,
    West,
    NorthWest,
}

impl Direction {
    pub fn name(self) -> &'static str {
        match self {
            Direction::North => "north",
            Direction::NorthEast => "northeast",
            Direction::East => "east",
            Direction::SouthEast => "southeast",
            Direction::South => "south",
            Direction::SouthWest => "southwest",
            Direction::West => "west",
            Direction::NorthWest => "northwest",
        }
    }

    /// Combines the vertical part (north or south) with the horizontal part (east or west).
    pub(crate) fn from_parts(north: bool, east: bool) -> Self {
        match (north, east) {
            (true, true) => Direction::NorthEast,
            (true, false) => Direction::NorthWest,
            (false, true) => Direction::SouthEast,
            (false, false) => Direction::SouthWest,
        }
    }
}

/// The axis of a pinch or spread, from KOReader's `DIRECTION_TABLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    Horizontal,
    Vertical,
    Diagonal,
}

impl Axis {
    pub fn name(self) -> &'static str {
        match self {
            Axis::Horizontal => "horizontal",
            Axis::Vertical => "vertical",
            Axis::Diagonal => "diagonal",
        }
    }

    /// The axis of a pan direction, or None when there is no direction.
    pub(crate) fn of(direction: Option<Direction>) -> Option<Self> {
        match direction? {
            Direction::East | Direction::West => Some(Axis::Horizontal),
            Direction::North | Direction::South => Some(Axis::Vertical),
            _ => Some(Axis::Diagonal),
        }
    }
}

/// The turn of a rotate gesture. KOReader names are "cw" and "ccw".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Turn {
    Clockwise,
    CounterClockwise,
}

impl Turn {
    pub fn name(self) -> &'static str {
        match self {
            Turn::Clockwise => "cw",
            Turn::CounterClockwise => "ccw",
        }
    }
}

/// A recognized gesture. Each variant lists the fields KOReader sets for it.
#[derive(Debug, Clone, PartialEq)]
pub enum Gesture {
    /// A finger touched the screen (KOReader `touch`, emitted once per contact).
    Touch {
        pos: Point,
        time_us: u64,
    },
    Tap {
        pos: Point,
        time_us: u64,
    },
    DoubleTap {
        pos: Point,
        time_us: u64,
    },
    /// The contact stayed still for the hold interval.
    Hold {
        pos: Point,
        time_us: u64,
    },
    /// A held contact lifted.
    HoldRelease {
        pos: Point,
        time_us: u64,
    },
    /// A held contact moved. `start_pos` is None only for a rare outward pan path.
    HoldPan {
        pos: Point,
        start_pos: Option<Point>,
        direction: Option<Direction>,
        distance: f64,
        time_us: u64,
    },
    Pan {
        pos: Point,
        start_pos: Point,
        direction: Option<Direction>,
        distance: f64,
        time_us: u64,
    },
    PanRelease {
        pos: Point,
        time_us: u64,
    },
    /// `pos` is the start point. `end_pos` is the lift point.
    Swipe {
        pos: Point,
        end_pos: Point,
        direction: Option<Direction>,
        distance: f64,
        time_us: u64,
    },
    /// A swipe with two or more direction changes. `directions` lists them in order.
    Multiswipe {
        pos: Point,
        end_pos: Point,
        direction: Option<Direction>,
        distance: f64,
        directions: Vec<Direction>,
        time_us: u64,
    },
    /// `pos` is the midpoint between the two fingers. `span` is their distance.
    TwoFingerTap {
        pos: Point,
        span: f64,
        time_us: u64,
    },
    TwoFingerHold {
        pos: Point,
        span: f64,
        time_us: u64,
    },
    TwoFingerHoldRelease {
        pos: Point,
        span: f64,
        time_us: u64,
    },
    TwoFingerHoldPan {
        pos: Point,
        start_pos: Point,
        direction: Option<Direction>,
        distance: f64,
        time_us: u64,
    },
    TwoFingerHoldPanRelease {
        pos: Point,
        span: f64,
        time_us: u64,
    },
    /// Both fingers moved in the same direction.
    TwoFingerPan {
        pos: Point,
        start_pos: Point,
        direction: Option<Direction>,
        distance: f64,
        time_us: u64,
    },
    TwoFingerPanRelease {
        pos: Point,
        time_us: u64,
    },
    TwoFingerSwipe {
        pos: Point,
        end_pos: Point,
        direction: Option<Direction>,
        distance: f64,
        time_us: u64,
    },
    /// The fingers moved toward each other, in different directions.
    InwardPan {
        pos: Point,
        start_pos: Point,
        axis: Option<Axis>,
        distance: f64,
        span: f64,
        start_span: f64,
        time_us: u64,
    },
    /// The fingers moved apart. `pos` is the start midpoint and `end_pos` the end midpoint.
    OutwardPan {
        pos: Point,
        end_pos: Point,
        axis: Option<Axis>,
        distance: f64,
        span: f64,
        start_span: f64,
        time_us: u64,
    },
    /// An inward pan that completed as a swipe on lift.
    Pinch {
        pos: Point,
        start_pos: Point,
        axis: Option<Axis>,
        distance: f64,
        span: f64,
        start_span: f64,
        time_us: u64,
    },
    /// An outward pan that completed as a swipe on lift.
    Spread {
        pos: Point,
        end_pos: Point,
        axis: Option<Axis>,
        distance: f64,
        span: f64,
        start_span: f64,
        time_us: u64,
    },
    /// `pos` is the position of the still finger. `angle` is in degrees.
    Rotate {
        pos: Point,
        angle: f64,
        turn: Turn,
        time_us: u64,
    },
}

impl Gesture {
    /// KOReader's name for the gesture, the `ges` field.
    pub fn name(&self) -> &'static str {
        match self {
            Gesture::Touch { .. } => "touch",
            Gesture::Tap { .. } => "tap",
            Gesture::DoubleTap { .. } => "double_tap",
            Gesture::Hold { .. } => "hold",
            Gesture::HoldRelease { .. } => "hold_release",
            Gesture::HoldPan { .. } => "hold_pan",
            Gesture::Pan { .. } => "pan",
            Gesture::PanRelease { .. } => "pan_release",
            Gesture::Swipe { .. } => "swipe",
            Gesture::Multiswipe { .. } => "multiswipe",
            Gesture::TwoFingerTap { .. } => "two_finger_tap",
            Gesture::TwoFingerHold { .. } => "two_finger_hold",
            Gesture::TwoFingerHoldRelease { .. } => "two_finger_hold_release",
            Gesture::TwoFingerHoldPan { .. } => "two_finger_hold_pan",
            Gesture::TwoFingerHoldPanRelease { .. } => "two_finger_hold_pan_release",
            Gesture::TwoFingerPan { .. } => "two_finger_pan",
            Gesture::TwoFingerPanRelease { .. } => "two_finger_pan_release",
            Gesture::TwoFingerSwipe { .. } => "two_finger_swipe",
            Gesture::InwardPan { .. } => "inward_pan",
            Gesture::OutwardPan { .. } => "outward_pan",
            Gesture::Pinch { .. } => "pinch",
            Gesture::Spread { .. } => "spread",
            Gesture::Rotate { .. } => "rotate",
        }
    }

    /// The KOReader `pos` field.
    pub fn pos(&self) -> Point {
        match *self {
            Gesture::Touch { pos, .. }
            | Gesture::Tap { pos, .. }
            | Gesture::DoubleTap { pos, .. }
            | Gesture::Hold { pos, .. }
            | Gesture::HoldRelease { pos, .. }
            | Gesture::HoldPan { pos, .. }
            | Gesture::Pan { pos, .. }
            | Gesture::PanRelease { pos, .. }
            | Gesture::Swipe { pos, .. }
            | Gesture::Multiswipe { pos, .. }
            | Gesture::TwoFingerTap { pos, .. }
            | Gesture::TwoFingerHold { pos, .. }
            | Gesture::TwoFingerHoldRelease { pos, .. }
            | Gesture::TwoFingerHoldPan { pos, .. }
            | Gesture::TwoFingerHoldPanRelease { pos, .. }
            | Gesture::TwoFingerPan { pos, .. }
            | Gesture::TwoFingerPanRelease { pos, .. }
            | Gesture::TwoFingerSwipe { pos, .. }
            | Gesture::InwardPan { pos, .. }
            | Gesture::OutwardPan { pos, .. }
            | Gesture::Pinch { pos, .. }
            | Gesture::Spread { pos, .. }
            | Gesture::Rotate { pos, .. } => pos,
        }
    }

    /// The time of the event that produced the gesture, in microseconds.
    pub fn time_us(&self) -> u64 {
        match *self {
            Gesture::Touch { time_us, .. }
            | Gesture::Tap { time_us, .. }
            | Gesture::DoubleTap { time_us, .. }
            | Gesture::Hold { time_us, .. }
            | Gesture::HoldRelease { time_us, .. }
            | Gesture::HoldPan { time_us, .. }
            | Gesture::Pan { time_us, .. }
            | Gesture::PanRelease { time_us, .. }
            | Gesture::Swipe { time_us, .. }
            | Gesture::Multiswipe { time_us, .. }
            | Gesture::TwoFingerTap { time_us, .. }
            | Gesture::TwoFingerHold { time_us, .. }
            | Gesture::TwoFingerHoldRelease { time_us, .. }
            | Gesture::TwoFingerHoldPan { time_us, .. }
            | Gesture::TwoFingerHoldPanRelease { time_us, .. }
            | Gesture::TwoFingerPan { time_us, .. }
            | Gesture::TwoFingerPanRelease { time_us, .. }
            | Gesture::TwoFingerSwipe { time_us, .. }
            | Gesture::InwardPan { time_us, .. }
            | Gesture::OutwardPan { time_us, .. }
            | Gesture::Pinch { time_us, .. }
            | Gesture::Spread { time_us, .. }
            | Gesture::Rotate { time_us, .. } => time_us,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_and_axis_names_match_koreader() {
        assert_eq!(Direction::NorthWest.name(), "northwest");
        assert_eq!(Direction::from_parts(false, true), Direction::SouthEast);
        assert_eq!(Axis::of(Some(Direction::North)), Some(Axis::Vertical));
        assert_eq!(Axis::of(Some(Direction::SouthEast)), Some(Axis::Diagonal));
        assert_eq!(Axis::of(None), None);
        assert_eq!(Turn::CounterClockwise.name(), "ccw");
    }

    #[test]
    fn gesture_names_match_koreader_ges_strings() {
        let p = Point::new(0, 0);
        let names = [
            Gesture::Touch { pos: p, time_us: 0 }.name(),
            Gesture::Tap { pos: p, time_us: 0 }.name(),
            Gesture::DoubleTap { pos: p, time_us: 0 }.name(),
            Gesture::HoldRelease { pos: p, time_us: 0 }.name(),
            Gesture::PanRelease { pos: p, time_us: 0 }.name(),
            Gesture::TwoFingerTap {
                pos: p,
                span: 0.0,
                time_us: 0,
            }
            .name(),
            Gesture::TwoFingerPanRelease { pos: p, time_us: 0 }.name(),
            Gesture::InwardPan {
                pos: p,
                start_pos: p,
                axis: None,
                distance: 0.0,
                span: 0.0,
                start_span: 0.0,
                time_us: 0,
            }
            .name(),
            Gesture::Rotate {
                pos: p,
                angle: 0.0,
                turn: Turn::Clockwise,
                time_us: 0,
            }
            .name(),
        ];
        assert_eq!(
            names,
            [
                "touch",
                "tap",
                "double_tap",
                "hold_release",
                "pan_release",
                "two_finger_tap",
                "two_finger_pan_release",
                "inward_pan",
                "rotate",
            ]
        );
    }
}
