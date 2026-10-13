//! Touch, key and pen input, and the gesture recognizer.
//!
//! The recognizer is a port of KOReader's `frontend/device/gesturedetector.lua`.
//! Gesture names, fields and thresholds follow KOReader, so gesture bindings can
//! use the same strings. The recognizer reads no clock. Time comes from the events
//! and from the `now_us` argument of [`GestureDetector::poll`].

pub mod desktop;
pub mod detector;
pub mod event;
pub mod gesture;
pub mod script;

pub use desktop::{DesktopPointer, MouseButton};
pub use detector::{dp_to_px, GestureConfig, GestureDetector};
pub use event::{Key, KeyEvent, PenEvent, Phase, TouchEvent};
pub use gesture::{Axis, Direction, Gesture, HoldPanDirection, Turn};
pub use script::{parse_script, replay, replay_with, ScriptError, Step};
