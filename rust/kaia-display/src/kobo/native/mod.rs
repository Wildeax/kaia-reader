//! The native Kobo backend. It drives the framebuffer and the MediaTek HWTCON interface
//! the way KOReader does on the Libra Colour, without FBInk.
//!
//! The layers, from the bottom:
//!
//! - [`abi`]: the kernel and driver structs and the ioctl numbers, with size checks.
//! - [`fb`]: the framebuffer geometry checks.
//! - [`geometry`]: software rotation and region alignment.
//! - [`pixels`]: conversion from a frame to the framebuffer layout.
//! - [`update`]: the call sequence of KOReader's `mxc_update`, with the state that
//!   KOReader keeps between updates.
//! - `device` (Linux): the mapping, the ioctls and the procfs writes.
//!   [`NativeKoboDisplay`] is there.
//!
//! Every module except `device` is pure and runs on the host. The tests check the
//! struct contents and the call order for a run of refreshes, against the Lua code they
//! copy. The comments name the lines.
//!
//! # Not verified on a device
//!
//! Nothing here has run on a Kobo Libra Colour. These points need a device test:
//!
//! - Framebuffer layout: the native backend takes the byte order from the channel
//!   offsets that the driver reports. KOReader writes red first and uses the BGR flag
//!   only for image decoding. If colors come out swapped, check `red.offset`.
//! - Channel width: 32 bpp channels must be 8 bits wide at offsets 0, 8 and 16.
//! - Dithering: `Refresh::dither` turns on the Kaleido promotion and the hardware
//!   dithering together, as KOReader does for image content. The app must set it on
//!   color content. Gray UI leaves it off. The promotion also needs the color rendering
//!   setting, as KOReader's `isColorEnabled()` does. It starts on, and the app sets it
//!   with `NativeKoboDisplay::set_color_enabled`.
//! - Exit: dropping the display writes `night_mode 0` to `/proc/hwtcon/cmd`, as
//!   `Device:exit` restores the hardware night mode.
//! - Rotation: `Rotation::Deg90` is KOReader's mode 1, as `rotation.rs` documents. Check
//!   on the device that the logical bottom edge is on the panel's right side.
//! - Waits: the complete wait passes an 8-byte struct, where KOReader passes 4 bytes.
//!   The kernel can write back 8 bytes, so the extra word keeps that write in bounds.
//! - Procfs knobs: `fiti_power 1` and `night_mode 4` or `0` are written to
//!   `/proc/hwtcon/cmd`, as KOReader writes them. Write errors are ignored.
//! - Scope: only the MediaTek (HWTCON) family is ported. The i.MX MXCFB paths of
//!   `framebuffer_mxcfb.lua` (NXP Kobos and Mark 7) are not. On those panels the HWTCON
//!   ioctls fail, and each update reports the error. Kaia targets the Libra Colour only.

pub mod abi;
pub mod fb;
pub mod geometry;
pub mod pixels;
pub mod update;

#[cfg(target_os = "linux")]
mod device;

#[cfg(target_os = "linux")]
pub use device::NativeKoboDisplay;
