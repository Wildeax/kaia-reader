//! Raw FFI for FBInk, the E Ink library that KOReader builds for Kobo.
//!
//! The declarations follow FBInk's public header `fbink.h` at the commit that
//! KOReader pins (`base/thirdparty/fbink/CMakeLists.txt`, commit
//! `92e127008145b2a22fba7c59815d810d716310dd`). Only the functions and structs that
//! Kaia calls are declared. The structs are `#[repr(C)]` and their layouts are
//! checked against the header (see the tests at the bottom of this file).
//!
//! FBInk's enums are `__attribute__((packed))` in C, so they are plain `u8` (or
//! `i8`) values here, with the numbers from the header as constants. A Rust enum
//! would be undefined behavior on a value FBInk does not list.
//!
//! # Safety
//!
//! Every function here takes raw pointers or file descriptors that the caller must
//! keep valid for the duration of the call. The `kaia-display` Kobo backend is the
//! only user, and it documents each call.
//!
//! # Build
//!
//! This crate links nothing by itself. `build.rs` adds the link flags when the
//! `KAIA_FBINK_LIB` variable names a directory with `libfbink`. See `build.rs`.
//!
//! FBInk is GPL-3.0-or-later. Kaia is AGPL-3.0-or-later. Linking FBInk into a Kaia
//! binary makes that binary a combined work under the GPL terms.

#![allow(non_camel_case_types, non_upper_case_globals)]

use core::ffi::{c_char, c_int, c_long, c_short};

/// Opens the framebuffer when passed to a function that takes an fd. Not used by
/// Kaia, which always passes the fd from [`fbink_open`].
pub const FBFD_AUTO: c_int = -1;

/// Passed as a marker, meaning "the last update this FBInk session sent".
pub const LAST_MARKER: u32 = 0;

/// Bit in [`fbink_features`] for image support. `fbink_print_raw_data` returns
/// `-ENOSYS` when the build has no image support.
pub const FBINK_FEATURE_IMAGE: u32 = 1 << 5;

// Waveform modes (`WFM_MODE_INDEX_E`). Values come from `fbink.h`, checked by
// compiling the header on the host.
pub const WFM_AUTO: u8 = 0;
pub const WFM_DU: u8 = 1;
pub const WFM_GC16: u8 = 2;
pub const WFM_GC4: u8 = 3;
pub const WFM_A2: u8 = 4;
pub const WFM_GL16: u8 = 5;
pub const WFM_REAGL: u8 = 6;
pub const WFM_REAGLD: u8 = 7;
pub const WFM_GC16_FAST: u8 = 8;
pub const WFM_GL16_FAST: u8 = 9;
pub const WFM_DU4: u8 = 10;
pub const WFM_GL4: u8 = 11;
pub const WFM_GL16_INV: u8 = 12;
pub const WFM_GCK16: u8 = 13;
pub const WFM_GLKW16: u8 = 14;
pub const WFM_GC16HQ: u8 = 20;
pub const WFM_GCC16: u8 = 26;
pub const WFM_GLRC16: u8 = 27;
pub const WFM_GCCK16: u8 = 32;
pub const WFM_GLRCK16: u8 = 33;

// Dithering modes (`HW_DITHER_INDEX_E`).
pub const HWD_PASSTHROUGH: u8 = 0;
pub const HWD_FLOYD_STEINBERG: u8 = 1;
pub const HWD_ATKINSON: u8 = 2;
pub const HWD_ORDERED: u8 = 3;
pub const HWD_QUANT_ONLY: u8 = 4;

// CFA post-process modes (`CFA_MODE_INDEX_E`).
pub const CFA_DEFAULT: u8 = 0;
pub const CFA_AIE_S4: u8 = 1;
pub const CFA_AIE_S7: u8 = 2;
pub const CFA_AIE_S9: u8 = 3;
pub const CFA_G0: u8 = 4;
pub const CFA_G1: u8 = 5;
pub const CFA_G2: u8 = 6;
pub const CFA_NTX: u8 = 7;
pub const CFA_NTX_SF: u8 = 8;
pub const CFA_SKIP: u8 = 9;

/// `FBInkConfig` from `fbink.h`. Zeroed is a sane default, as in the header.
///
/// Fields that FBInk reads only at `fbink_init` time are: `is_centered`, `fontmult`,
/// `fontname`, `fg_color`, `bg_color`, `no_viewport`, `is_verbose`, `is_quiet` and
/// `to_syslog`. Init again after changing them.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FBInkConfig {
    pub row: c_short,
    pub col: c_short,
    pub fontmult: u8,
    pub fontname: u8,
    pub is_inverted: bool,
    /// Request a black flash on refresh (`UPDATE_MODE_FULL` instead of `PARTIAL`).
    pub is_flashing: bool,
    pub is_cleared: bool,
    pub is_centered: bool,
    pub hoffset: c_short,
    pub voffset: c_short,
    pub is_halfway: bool,
    pub is_padded: bool,
    pub is_rpadded: bool,
    pub fg_color: u8,
    pub bg_color: u8,
    pub is_overlay: bool,
    pub is_bgless: bool,
    pub is_fgless: bool,
    pub no_viewport: bool,
    pub is_verbose: bool,
    /// Hide the hardware setup messages that `fbink_init` prints.
    pub is_quiet: bool,
    /// Ignore the alpha channel of the input image. Needed for RGB input (24 bpp).
    pub ignore_alpha: bool,
    pub halign: u8,
    pub valign: u8,
    pub scaled_width: c_short,
    pub scaled_height: c_short,
    /// Waveform mode, one of the `WFM_*` constants. Zero is `WFM_AUTO`.
    pub wfm_mode: u8,
    /// Dithering mode, one of the `HWD_*` constants.
    pub dithering_mode: u8,
    pub sw_dithering: bool,
    /// CFA post-process mode, one of the `CFA_*` constants.
    pub cfa_mode: u8,
    pub is_nightmode: bool,
    /// Skip the eInk refresh. Drawing still happens. Use this to draw a region and
    /// refresh it in a separate call.
    pub no_refresh: bool,
    pub no_merge: bool,
    pub is_animated: bool,
    pub saturation_boost: u8,
    pub to_syslog: bool,
}

/// `FBInkState` from `fbink.h`: what FBInk found out about the device and the
/// framebuffer. [`fbink_get_state`] fills it in. Build one with
/// [`FBInkState::zeroed`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FBInkState {
    pub user_hz: c_long,
    pub font_name: *const c_char,
    pub view_width: u32,
    pub view_height: u32,
    /// Effective screen width at the current rotation.
    pub screen_width: u32,
    /// Effective screen height at the current rotation.
    pub screen_height: u32,
    pub scanline_stride: u32,
    /// Bits per pixel of the framebuffer.
    pub bpp: u32,
    pub inverted_grayscale: bool,
    pub device_name: [c_char; 32],
    pub device_codename: [c_char; 32],
    pub device_platform: [c_char; 32],
    pub device_id: u16,
    pub pen_fg_color: u8,
    pub pen_bg_color: u8,
    pub screen_dpi: u16,
    pub font_w: u16,
    pub font_h: u16,
    pub max_cols: u16,
    pub max_rows: u16,
    pub view_hori_origin: u8,
    pub view_vert_origin: u8,
    pub view_vert_offset: u8,
    pub fontsize_mult: u8,
    pub glyph_width: u8,
    pub glyph_height: u8,
    pub is_perfect_fit: bool,
    pub is_mtk: bool,
    pub is_sunxi: bool,
    pub sunxi_has_fbdamage: bool,
    pub sunxi_force_rota: i8,
    pub is_kindle_legacy: bool,
    pub is_kobo_non_mt: bool,
    /// The wait-for-complete ioctl may time out on this device.
    pub unreliable_wait_for: bool,
    pub can_wake_epdc: bool,
    pub ntx_boot_rota: u8,
    pub ntx_rota_quirk: u8,
    pub rotation_map: [u8; 4],
    pub touch_swap_axes: bool,
    pub touch_mirror_x: bool,
    pub touch_mirror_y: bool,
    pub is_ntx_quirky_landscape: bool,
    /// Native rotation from the framebuffer's `vinfo.rotate`.
    pub current_rota: u8,
    pub can_rotate: bool,
    pub can_hw_invert: bool,
    pub has_eclipse_wfm: bool,
    pub has_color_panel: bool,
    pub pixel_format: u8,
    pub can_wait_for_submission: bool,
}

impl FBInkState {
    /// An all-zero state, to pass to [`fbink_get_state`] as the output buffer.
    pub fn zeroed() -> Self {
        // SAFETY: every field is an integer, a bool, an array of those, or a raw
        // pointer. The all-zero bit pattern is a valid value for each of them: 0 for
        // integers, false for bools, and the null pointer for `font_name`.
        unsafe { core::mem::zeroed() }
    }
}

// Size checks that run at compile time on every target, so `cargo check` for armv7
// catches a layout change without running the tests.
const _: () = assert!(core::mem::size_of::<FBInkConfig>() == 42);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(core::mem::size_of::<FBInkState>() == 184);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(core::mem::size_of::<FBInkState>() == 176);

extern "C" {
    /// Opens the framebuffer device. Returns the fd, or a negative value on failure.
    pub fn fbink_open() -> c_int;

    /// Closes an fd from [`fbink_open`]. Returns 0 on success.
    pub fn fbink_close(fbfd: c_int) -> c_int;

    /// Sets up FBInk's view of the framebuffer and the hardware. Call it once after
    /// `fbink_open`, and again when the framebuffer changes. Returns a negative value
    /// on failure.
    pub fn fbink_init(fbfd: c_int, cfg: *const FBInkConfig) -> c_int;

    /// Fills `state` with what FBInk knows about the device and the framebuffer.
    pub fn fbink_get_state(cfg: *const FBInkConfig, state: *mut FBInkState);

    /// Returns the feature bits this FBInk build was compiled with (`FBINK_FEATURE_*`).
    pub fn fbink_features() -> u32;

    /// Draws raw scanlines at (`x_off`, `y_off`). `data` holds `w * h` pixels of one
    /// format. The format comes from `len / (w * h)`: 1 is Y8 (gray), 3 is RGB, 4 is
    /// RGBA. `len` must be exact. Needs image support (`FBINK_FEATURE_IMAGE`).
    /// Returns the number of lines printed, or a negative value on failure.
    /// With `cfg.no_refresh` set, it draws without refreshing the panel.
    pub fn fbink_print_raw_data(
        fbfd: c_int,
        data: *const u8,
        w: c_int,
        h: c_int,
        len: usize,
        x_off: c_short,
        y_off: c_short,
        cfg: *const FBInkConfig,
    ) -> c_int;

    /// Refreshes a rectangle of the panel with the waveform and flash in `cfg`.
    /// The coordinates are top, left, width and height in panel pixels, at the
    /// framebuffer's native rotation. Returns a negative value on failure.
    pub fn fbink_refresh(
        fbfd: c_int,
        region_top: u32,
        region_left: u32,
        region_width: u32,
        region_height: u32,
        cfg: *const FBInkConfig,
    ) -> c_int;

    /// Waits until the EPDC has taken the update with `marker`. Returns a negative
    /// value (`-EINVAL` for a marker it does not know, `-ENOSYS` on unsupported
    /// devices).
    pub fn fbink_wait_for_submission(fbfd: c_int, marker: u32) -> c_int;

    /// Waits until the update with `marker` has finished on the panel. Pass
    /// [`LAST_MARKER`] to wait for the most recent update.
    pub fn fbink_wait_for_complete(fbfd: c_int, marker: u32) -> c_int;

    /// The marker of the last refresh this FBInk session sent, or [`LAST_MARKER`].
    pub fn fbink_get_last_marker() -> u32;

    /// Converts a native framebuffer rotation to the canonical one (Kobo only).
    pub fn fbink_rota_native_to_canonical(rotate: u32) -> u8;

    /// Converts a canonical rotation to the native framebuffer rotation (Kobo only).
    pub fn fbink_rota_canonical_to_native(rotate: u8) -> u32;
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    /// Compares each field offset with the value the header gives.
    macro_rules! offsets {
        ($ty:ident { $($field:ident = $off:expr),+ $(,)? }) => {
            $(
                assert_eq!(
                    offset_of!($ty, $field),
                    $off,
                    concat!(stringify!($ty), ".", stringify!($field))
                );
            )+
        };
    }

    // Sizes and offsets come from clang's record layout for the header at the
    // pinned commit, built for x86_64-unknown-linux-gnu and armv7-unknown-linux-gnueabihf
    // with `-fdump-record-layouts`. FBInkConfig has no pointer or long fields,
    // so its layout is the same on both targets.

    #[test]
    fn config_layout_matches_header() {
        assert_eq!(size_of::<FBInkConfig>(), 42);
        offsets!(FBInkConfig {
            row = 0,
            col = 2,
            fontmult = 4,
            fontname = 5,
            is_inverted = 6,
            is_flashing = 7,
            is_cleared = 8,
            is_centered = 9,
            hoffset = 10,
            voffset = 12,
            is_halfway = 14,
            is_padded = 15,
            is_rpadded = 16,
            fg_color = 17,
            bg_color = 18,
            is_overlay = 19,
            is_bgless = 20,
            is_fgless = 21,
            no_viewport = 22,
            is_verbose = 23,
            is_quiet = 24,
            ignore_alpha = 25,
            halign = 26,
            valign = 27,
            scaled_width = 28,
            scaled_height = 30,
            wfm_mode = 32,
            dithering_mode = 33,
            sw_dithering = 34,
            cfa_mode = 35,
            is_nightmode = 36,
            no_refresh = 37,
            no_merge = 38,
            is_animated = 39,
            saturation_boost = 40,
            to_syslog = 41,
        });
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn state_layout_matches_header_on_64_bit() {
        assert_eq!(size_of::<FBInkState>(), 184);
        offsets!(FBInkState {
            user_hz = 0,
            font_name = 8,
            view_width = 16,
            view_height = 20,
            screen_width = 24,
            screen_height = 28,
            scanline_stride = 32,
            bpp = 36,
            inverted_grayscale = 40,
            device_name = 41,
            device_codename = 73,
            device_platform = 105,
            device_id = 138,
            pen_fg_color = 140,
            pen_bg_color = 141,
            screen_dpi = 142,
            font_w = 144,
            font_h = 146,
            max_cols = 148,
            max_rows = 150,
            view_hori_origin = 152,
            view_vert_origin = 153,
            view_vert_offset = 154,
            fontsize_mult = 155,
            glyph_width = 156,
            glyph_height = 157,
            is_perfect_fit = 158,
            is_mtk = 159,
            is_sunxi = 160,
            sunxi_has_fbdamage = 161,
            sunxi_force_rota = 162,
            is_kindle_legacy = 163,
            is_kobo_non_mt = 164,
            unreliable_wait_for = 165,
            can_wake_epdc = 166,
            ntx_boot_rota = 167,
            ntx_rota_quirk = 168,
            rotation_map = 169,
            touch_swap_axes = 173,
            touch_mirror_x = 174,
            touch_mirror_y = 175,
            is_ntx_quirky_landscape = 176,
            current_rota = 177,
            can_rotate = 178,
            can_hw_invert = 179,
            has_eclipse_wfm = 180,
            has_color_panel = 181,
            pixel_format = 182,
            can_wait_for_submission = 183,
        });
    }

    #[cfg(target_pointer_width = "32")]
    #[test]
    fn state_layout_matches_header_on_32_bit() {
        assert_eq!(size_of::<FBInkState>(), 176);
        offsets!(FBInkState {
            user_hz = 0,
            font_name = 4,
            view_width = 8,
            view_height = 12,
            screen_width = 16,
            screen_height = 20,
            scanline_stride = 24,
            bpp = 28,
            inverted_grayscale = 32,
            device_name = 33,
            device_codename = 65,
            device_platform = 97,
            device_id = 130,
            pen_fg_color = 132,
            pen_bg_color = 133,
            screen_dpi = 134,
            font_w = 136,
            font_h = 138,
            max_cols = 140,
            max_rows = 142,
            view_hori_origin = 144,
            view_vert_origin = 145,
            view_vert_offset = 146,
            fontsize_mult = 147,
            glyph_width = 148,
            glyph_height = 149,
            is_perfect_fit = 150,
            is_mtk = 151,
            is_sunxi = 152,
            sunxi_has_fbdamage = 153,
            sunxi_force_rota = 154,
            is_kindle_legacy = 155,
            is_kobo_non_mt = 156,
            unreliable_wait_for = 157,
            can_wake_epdc = 158,
            ntx_boot_rota = 159,
            ntx_rota_quirk = 160,
            rotation_map = 161,
            touch_swap_axes = 165,
            touch_mirror_x = 166,
            touch_mirror_y = 167,
            is_ntx_quirky_landscape = 168,
            current_rota = 169,
            can_rotate = 170,
            can_hw_invert = 171,
            has_eclipse_wfm = 172,
            has_color_panel = 173,
            pixel_format = 174,
            can_wait_for_submission = 175,
        });
    }

    #[test]
    fn feature_and_marker_constants_match_header() {
        assert_eq!(FBINK_FEATURE_IMAGE, 32);
        assert_eq!(LAST_MARKER, 0);
        assert_eq!(FBFD_AUTO, -1);
    }
}
