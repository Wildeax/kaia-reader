//! Device services for Kaia: battery, frontlight and warmth, suspend and
//! standby, Wi-Fi, USB storage handover, device identity and paths.
//!
//! Each Kobo behaviour is a port of a KOReader file. The doc comment next
//! to it gives the file and line. Nothing here has run on a Libra Colour
//! yet, so every Kobo path and step is unverified on hardware. The
//! individual doc comments say where Kaia differs from KOReader.
//!
//! Layout:
//! - [`sysfs`]: device file access and command execution, injectable through
//!   [`sysfs::Root`], [`sysfs::SysfsWriter`] and [`sysfs::Runner`].
//! - [`plan`]: step lists for sequences (suspend, standby, Wi-Fi, USB handover).
//! - [`battery`], [`frontlight`], [`network`], [`power`], [`usb`], [`events`]:
//!   the device services.
//! - [`device`], [`paths`]: identity and locations.
//! - [`platform`]: the bundle and [`platform::Platform::detect`].
//!
//! The crate has no `unsafe` code. Ioctl-based KOReader paths are replaced
//! by sysfs writes, and each replacement is noted where it happens.

pub mod battery;
pub mod device;
pub mod error;
pub mod events;
pub mod frontlight;
pub mod network;
pub mod paths;
pub mod plan;
pub mod platform;
pub mod power;
pub mod sysfs;
pub mod usb;
pub mod wpa;

pub use battery::{Battery, KoboBattery, NoBattery};
pub use device::{DeviceInfo, Model};
pub use error::PlatformError;
pub use frontlight::{Frontlight, FrontlightConfig, SysfsFrontlight};
pub use network::{DesktopNetwork, KoboNetwork, Network};
pub use paths::Paths;
pub use plan::{Action, OnError, Outcome, Plan, Step};
pub use platform::{Env, Kind, Platform};
pub use power::{PowerContext, PowerManager, ResumeContext};
pub use sysfs::{Root, Runner, SysfsWriter, SystemIo};
pub use wpa::ScanResult;
