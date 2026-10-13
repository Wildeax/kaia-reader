//! Kobo backends for [`Display`](crate::Display) and [`InputSource`](crate::InputSource).
//!
//! - [`plan`] and [`decode`] are pure and compile on every host. They hold the rules
//!   for waveforms and waits, and for touch and key decoding. Their tests run on the
//!   host.
//! - [`native`] drives the panel the way KOReader does: it maps `/dev/fb0` and sends
//!   the HWTCON ioctls. Its pure parts compile on every host. Its device part is Linux
//!   only. See [`NativeKoboDisplay`]. This is the default for [`KoboDisplay::open_auto`].
//! - `display` (Linux only) draws on the E Ink panel through FBInk. See
//!   [`KoboDisplay`]. It is used only when `KAIA_DISPLAY=fbink` is set.
//! - `evdev` (Linux only) reads the touch and key event nodes. See [`EvdevInput`].
//!
//! The rules come from KOReader's Kobo code, named in each module. KOReader's Lua
//! framebuffer does not call FBInk. It sends the MXCFB and HWTCON ioctls itself
//! (`base/ffi/framebuffer_mxcfb.lua`). The native path does the same. The FBInk path
//! stands in for those ioctls, and it is kept for comparison.
//!
//! # Not verified on a device
//!
//! Nothing in this module has run on a Kobo Libra Colour. These points need a device
//! test before the backend is trusted:
//!
//! - FBInk build: KOReader's pinned FBInk is built with `MINIMAL=1` and no image
//!   support. `fbink_print_raw_data` needs image support, so [`KoboDisplay::open`]
//!   refuses a build without it. Kaia needs an FBInk build with `IMAGE=1`.
//! - Panel orientation: the framebuffer must start in native portrait at
//!   1264 by 1680. [`KoboDisplay::open`] refuses any other size. Rotation is done in
//!   software, so the framebuffer is never rotated.
//! - FULL update mode: KOReader forces FULL mode for REAGL and Kaleido updates
//!   (`mxc_update`). The plan does the same. That may flash on every partial refresh.
//! - Color path: `GCC16` and `GLRC16` with the CFA mode `CFA_G2`, for refreshes with
//!   [`Refresh::dither`](crate::Refresh::dither) set. KOReader sets
//!   `HWTCON_FLAG_CFA_EINK_G2` for those. The mapping from that flag to FBInk's `CFA_G2`
//!   is an assumption.
//! - Dithering: [`Refresh::dither`](crate::Refresh::dither) requests hardware dithering.
//!   The FBInk path sends `HWD_ORDERED` for it, and `HWD_PASSTHROUGH` otherwise. FBInk's
//!   mapping of those modes to KOReader's HWTCON `Y8` dither is not verified. The native
//!   path sends KOReader's flags and dither modes.
//! - Waits: errors from the wait calls are ignored, as KOReader ignores them. A wait
//!   that times out costs latency and draws nothing wrong.
//! - Region alignment: partial regions are widened to a 16 pixel grid (KOReader
//!   `alignment_constraint`). Whether KOReader rounds the same way is not checked.
//! - Touch ranges: the raw ranges come from `EVIOCGABS` at runtime. The test values
//!   are assumptions. The touch node is `/dev/input/event1` and the keys are on
//!   `/dev/input/event0`, as KOReader's Kobo defaults say. KOReader finds the nodes
//!   with FBInk's input scan, which this backend does not do.
//! - Sleep cover: KOReader maps key codes 35 and 59 to the sleep cover. It does not
//!   read `SW_LID`, so this backend does not either.
//! - Struct layout: `input_event` is 16 bytes with a 32-bit `time_t` (armv7) and 24
//!   bytes on 64-bit. Both are checked at compile time. The armv7 kernel and glibc
//!   combination on the Kobo is assumed, not tested.

pub mod decode;
pub mod native;
pub mod plan;

#[cfg(target_os = "linux")]
mod display;
#[cfg(target_os = "linux")]
mod evdev;

#[cfg(target_os = "linux")]
pub use display::KoboDisplay;
#[cfg(target_os = "linux")]
pub use evdev::EvdevInput;
#[cfg(target_os = "linux")]
pub use native::NativeKoboDisplay;

#[cfg(target_os = "linux")]
impl KoboDisplay {
    /// Opens the Libra Colour panel with the backend that the environment picks.
    ///
    /// The native backend is the default. `KAIA_DISPLAY=fbink` (any case) selects the
    /// FBInk backend, which needs an FBInk build with image support. The result is
    /// boxed as a [`Display`](crate::Display), so the methods that only one backend
    /// has are not reachable through it.
    pub fn open_auto() -> Result<Box<dyn crate::display::Display>, crate::display::DisplayError> {
        let fbink =
            std::env::var("KAIA_DISPLAY").is_ok_and(|value| value.trim().eq_ignore_ascii_case("fbink"));
        if fbink {
            Ok(Box::new(Self::libra_colour()?))
        } else {
            Ok(Box::new(NativeKoboDisplay::libra_colour()?))
        }
    }
}
