//! Kernel and driver ABI for the native Kobo backend: the framebuffer structs, the
//! HWTCON update structs and the ioctl request numbers.
//!
//! Each item names the KOReader cdecl it copies. `base/ffi/linux_fb_h.lua` holds the
//! framebuffer structs and the `FBIO*` numbers. `base/ffi/mxcfb_kobo_h.lua` holds the
//! HWTCON structs and the `HWTCON_*` numbers.
//!
//! Layouts follow the kernel headers on armv7 (32-bit `unsigned long`) and aarch64
//! (64-bit). The structs with a pointer-sized field carry size checks for both widths.
//! This module has no unsafe code and compiles on every host.

use std::mem::size_of;
use std::os::raw::{c_char, c_ulong};

// --- Framebuffer (linux_fb_h.lua) ---

/// `FBIOGET_VSCREENINFO` (`linux_fb_h.lua`, 17920).
pub const FBIOGET_VSCREENINFO: u32 = 0x4600;
/// `FBIOGET_FSCREENINFO` (`linux_fb_h.lua`, 17922).
pub const FBIOGET_FSCREENINFO: u32 = 0x4602;
/// `FB_TYPE_PACKED_PIXELS` (`linux_fb_h.lua`). Only packed pixels are supported.
pub const FB_TYPE_PACKED_PIXELS: u32 = 0;

/// `struct fb_bitfield`. Position and width of one color channel.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FbBitfield {
    pub offset: u32,
    pub length: u32,
    pub msb_right: u32,
}

/// `struct fb_fix_screeninfo` (`linux_fb_h.lua`). The C field `type` is `kind` here.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FbFixScreeninfo {
    pub id: [c_char; 16],
    pub smem_start: c_ulong,
    pub smem_len: u32,
    pub kind: u32,
    pub type_aux: u32,
    pub visual: u32,
    pub xpanstep: u16,
    pub ypanstep: u16,
    pub ywrapstep: u16,
    pub line_length: u32,
    pub mmio_start: c_ulong,
    pub mmio_len: u32,
    pub accel: u32,
    pub capabilities: u16,
    pub reserved: [u16; 2],
}

/// `struct fb_var_screeninfo` (`linux_fb_h.lua`). Every field is 32 bits.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct FbVarScreeninfo {
    pub xres: u32,
    pub yres: u32,
    pub xres_virtual: u32,
    pub yres_virtual: u32,
    pub xoffset: u32,
    pub yoffset: u32,
    pub bits_per_pixel: u32,
    pub grayscale: u32,
    pub red: FbBitfield,
    pub green: FbBitfield,
    pub blue: FbBitfield,
    pub transp: FbBitfield,
    pub nonstd: u32,
    pub activate: u32,
    pub height: u32,
    pub width: u32,
    pub accel_flags: u32,
    pub pixclock: u32,
    pub left_margin: u32,
    pub right_margin: u32,
    pub upper_margin: u32,
    pub lower_margin: u32,
    pub hsync_len: u32,
    pub vsync_len: u32,
    pub sync: u32,
    pub vmode: u32,
    pub rotate: u32,
    pub colorspace: u32,
    pub reserved: [u32; 4],
}

const _: () = assert!(size_of::<FbBitfield>() == 12);
const _: () = assert!(size_of::<FbVarScreeninfo>() == 160);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<FbFixScreeninfo>() == 68);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<FbFixScreeninfo>() == 80);

// --- HWTCON update (mxcfb_kobo_h.lua) ---

/// `HWTCON_SEND_UPDATE` (`mxcfb_kobo_h.lua`, 1076119086). Encodes `_IOW('F', 0x2e, 36)`.
pub const HWTCON_SEND_UPDATE: u32 = 1076119086;
/// `HWTCON_WAIT_FOR_UPDATE_SUBMISSION` (`mxcfb_kobo_h.lua`, 1074021943). `_IOW('F', 0x37, 4)`.
pub const HWTCON_WAIT_FOR_UPDATE_SUBMISSION: u32 = 1074021943;
/// `HWTCON_WAIT_FOR_UPDATE_COMPLETE` (`mxcfb_kobo_h.lua`, 3221767727). `_IOWR('F', 0x2f, 8)`.
/// The size field says 8 bytes, so the buffer passed here is `HwtconUpdateMarkerData`.
pub const HWTCON_WAIT_FOR_UPDATE_COMPLETE: u32 = 3221767727;

/// `UPDATE_MODE_PARTIAL` (`mxcfb_kobo_h.lua`).
pub const UPDATE_MODE_PARTIAL: u32 = 0;
/// `UPDATE_MODE_FULL` (`mxcfb_kobo_h.lua`).
pub const UPDATE_MODE_FULL: u32 = 1;

/// `enum HWTCON_WAVEFORM_MODE_ENUM` (`mxcfb_kobo_h.lua`). `REAGL` is an alias of `GLR16`.
pub const HWTCON_WAVEFORM_MODE_INIT: u32 = 0;
pub const HWTCON_WAVEFORM_MODE_DU: u32 = 1;
pub const HWTCON_WAVEFORM_MODE_GC16: u32 = 2;
pub const HWTCON_WAVEFORM_MODE_GL16: u32 = 3;
pub const HWTCON_WAVEFORM_MODE_GLR16: u32 = 4;
pub const HWTCON_WAVEFORM_MODE_REAGL: u32 = 4;
pub const HWTCON_WAVEFORM_MODE_A2: u32 = 6;
pub const HWTCON_WAVEFORM_MODE_GCK16: u32 = 8;
pub const HWTCON_WAVEFORM_MODE_GLKW16: u32 = 9;
pub const HWTCON_WAVEFORM_MODE_GCC16: u32 = 10;
pub const HWTCON_WAVEFORM_MODE_GLRC16: u32 = 11;
pub const HWTCON_WAVEFORM_MODE_AUTO: u32 = 257;

/// `EPDC_FLAG_ENABLE_INVERSION` (`mxcfb_kobo_h.lua`). Same bit as `HWTCON_FLAG_USE_DITHERING`.
pub const EPDC_FLAG_ENABLE_INVERSION: u32 = 1;
/// `HWTCON_FLAG_USE_DITHERING` (`mxcfb_kobo_h.lua`).
pub const HWTCON_FLAG_USE_DITHERING: u32 = 1;
/// `HWTCON_FLAG_CFA_EINK_G2` (`mxcfb_kobo_h.lua`). The CFA post-process flag for color
/// screens, used when `CFA_PROCESSING_FLAG` is not stomped by the user setting.
pub const HWTCON_FLAG_CFA_EINK_G2: u32 = 1536;
/// `HWTCON_FLAG_USE_DITHERING_Y8_Y4_S` (`mxcfb_kobo_h.lua`), the dither mode for
/// non-DU waveforms.
pub const HWTCON_FLAG_USE_DITHERING_Y8_Y4_S: i32 = 0x102;
/// `HWTCON_FLAG_USE_DITHERING_Y8_Y1_S` (`mxcfb_kobo_h.lua`), the dither mode for DU.
pub const HWTCON_FLAG_USE_DITHERING_Y8_Y1_S: i32 = 0x302;

/// `struct hwtcon_rect` (`mxcfb_kobo_h.lua`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HwtconRect {
    pub top: u32,
    pub left: u32,
    pub width: u32,
    pub height: u32,
}

/// `struct hwtcon_update_data` (`mxcfb_kobo_h.lua`). Sent with `HWTCON_SEND_UPDATE`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HwtconUpdateData {
    pub update_region: HwtconRect,
    pub waveform_mode: u32,
    pub update_mode: u32,
    pub update_marker: u32,
    pub flags: u32,
    pub dither_mode: i32,
}

/// `struct hwtcon_update_marker_data` (`mxcfb_kobo_h.lua`). KOReader passes a 4-byte
/// `uint32_t[1]` to the complete wait. This struct is 8 bytes, which is what the
/// request number encodes, so the kernel can write back the second word without
/// overrunning the buffer.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HwtconUpdateMarkerData {
    pub update_marker: u32,
    pub collision_test: u32,
}

const _: () = assert!(size_of::<HwtconRect>() == 16);
const _: () = assert!(size_of::<HwtconUpdateData>() == 36);
const _: () = assert!(size_of::<HwtconUpdateMarkerData>() == 8);

/// Linux `_IOC` encoding: direction, size, type and number packed into 32 bits.
pub const fn ioc(dir: u32, ty: u8, nr: u8, size: u32) -> u32 {
    (dir << 30) | (size << 16) | ((ty as u32) << 8) | (nr as u32)
}

/// `_IOC_WRITE`.
pub const IOC_WRITE: u32 = 1;
/// `_IOC_READ`.
pub const IOC_READ: u32 = 2;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hwtcon_numbers_match_the_ioctl_encoding() {
        // The request numbers in mxcfb_kobo_h.lua are the `_IOC` encodings of the
        // command letter 'F' with the sizes of the structs they take.
        assert_eq!(
            HWTCON_SEND_UPDATE,
            ioc(IOC_WRITE, b'F', 0x2e, size_of::<HwtconUpdateData>() as u32)
        );
        assert_eq!(HWTCON_WAIT_FOR_UPDATE_SUBMISSION, ioc(IOC_WRITE, b'F', 0x37, 4));
        assert_eq!(
            HWTCON_WAIT_FOR_UPDATE_COMPLETE,
            ioc(
                IOC_READ | IOC_WRITE,
                b'F',
                0x2f,
                size_of::<HwtconUpdateMarkerData>() as u32
            )
        );
    }

    #[test]
    fn framebuffer_numbers_match_linux_fb_h() {
        // linux_fb_h.lua: FBIOGET_VSCREENINFO = 17920, FBIOGET_FSCREENINFO = 17922.
        assert_eq!(FBIOGET_VSCREENINFO, 17920);
        assert_eq!(FBIOGET_FSCREENINFO, 17922);
    }

    #[test]
    fn hwtcon_dither_modes_match_the_enum() {
        // mxcfb_kobo_h.lua: HWTCON_FLAG_USE_DITHERING_Y8_Y4_S = 0x102,
        // HWTCON_FLAG_USE_DITHERING_Y8_Y1_S = 0x302.
        assert_eq!(HWTCON_FLAG_USE_DITHERING_Y8_Y4_S, 0x102);
        assert_eq!(HWTCON_FLAG_USE_DITHERING_Y8_Y1_S, 0x302);
    }

    #[test]
    fn waveform_numbers_match_the_enum() {
        // mxcfb_kobo_h.lua, HWTCON_WAVEFORM_MODE_ENUM.
        assert_eq!(HWTCON_WAVEFORM_MODE_DU, 1);
        assert_eq!(HWTCON_WAVEFORM_MODE_GC16, 2);
        assert_eq!(HWTCON_WAVEFORM_MODE_GLR16, 4);
        assert_eq!(HWTCON_WAVEFORM_MODE_REAGL, HWTCON_WAVEFORM_MODE_GLR16);
        assert_eq!(HWTCON_WAVEFORM_MODE_A2, 6);
        assert_eq!(HWTCON_WAVEFORM_MODE_GCK16, 8);
        assert_eq!(HWTCON_WAVEFORM_MODE_GLKW16, 9);
        assert_eq!(HWTCON_WAVEFORM_MODE_GCC16, 10);
        assert_eq!(HWTCON_WAVEFORM_MODE_GLRC16, 11);
        assert_eq!(HWTCON_WAVEFORM_MODE_AUTO, 257);
    }
}
