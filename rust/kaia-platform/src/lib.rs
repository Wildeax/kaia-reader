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
//! - [`ioctl`]: the RTC and `ntx_io` ioctls, the only `unsafe` code.
//! - [`plan`]: step lists for sequences (suspend, standby, Wi-Fi, USB handover).
//! - [`battery`], [`frontlight`], [`network`], [`power`], [`usb`], [`events`]:
//!   the device services.
//! - [`ntp`]: network time, the SNTP client and the Kobo clock steps.
//! - [`device`], [`paths`]: identity and locations.
//! - [`platform`]: the bundle and [`platform::Platform::detect`].
//!
//! `unsafe` code is confined to [`ioctl`]: the RTC alarm and clock calls and the
//! `ntx_io` brightness call, ported from KOReader's FFI modules. Each has a
//! sysfs fallback where KOReader has one, and the callers use it when the
//! ioctl fails.

#![deny(unsafe_code)]

pub mod battery;
pub mod device;
pub mod error;
pub mod events;
pub mod frontlight;
pub mod ioctl;
pub mod network;
pub mod ntp;
pub mod paths;
pub mod plan;
pub mod platform;
pub mod power;
pub mod reset;
pub mod storage;
pub mod suspend;
pub mod sysfs;
pub mod timezone;
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
