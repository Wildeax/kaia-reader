//! Display and input for Kaia.
//!
//! [`Display`] shows frames on a screen and [`InputSource`] yields touch, key and
//! window events. Two backends ship here: [`HeadlessDisplay`] and [`ScriptedInput`]
//! for tests, and [`DesktopDisplay`], a window through minifb, behind the `desktop`
//! feature. The Kobo backend (FBInk and evdev) will implement the same traits.
//!
//! Frames are always drawn in logical orientation. [`Refresh`] names the region to
//! update and the [`Waveform`] to use. The waveform table on [`Waveform`] maps
//! KOReader's refresh modes to these waveforms.

pub mod display;
pub mod headless;
pub mod input;
pub mod rotation;
pub mod waveform;

#[cfg(feature = "desktop")]
pub mod desktop;

pub use display::{check_present, Display, DisplayError, PanelSpec};
pub use headless::{HeadlessDisplay, ScriptedInput};
pub use input::{InputEvent, InputSource};
pub use rotation::{logical_rect_to_panel, rotate_to_panel, Rotation};
pub use waveform::{Refresh, Waveform};

#[cfg(feature = "desktop")]
pub use desktop::{DesktopDisplay, DesktopOptions};
