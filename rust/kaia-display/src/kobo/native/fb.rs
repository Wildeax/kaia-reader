//! The framebuffer geometry that the native backend maps, checked the way KOReader
//! checks it in `base/ffi/framebuffer_linux.lua` (`reinit`). The checks take the two
//! kernel structs as values, so they run on the host.

use kaia_gfx::Size;

use super::abi::{FbFixScreeninfo, FbVarScreeninfo, FB_TYPE_PACKED_PIXELS};
use super::pixels::FbPixels;

/// The page size that `framebuffer_linux.lua` aligns the mapping to (`PAGE_ALIGN`).
pub const PAGE_SIZE: usize = 4096;

/// What the backend needs to know about the mapped framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FbGeometry {
    /// Visible size in native portrait, from `xres` and `yres`.
    pub size: Size,
    /// Bytes per row (`finfo.line_length`).
    pub line_length: usize,
    /// `line_length * yres`, the bytes the screen really uses (`fb_size`).
    pub fb_size: usize,
    /// `fb_size` rounded up to the page size. This is the length that is mapped.
    pub map_len: usize,
    /// How one pixel is stored.
    pub pixels: FbPixels,
}

/// Checks the framebuffer against the panel size and derives the geometry.
///
/// `expected` is the native size the caller asks for. KOReader does not check it,
/// but the native backend needs the panel size to map Kaia regions, so a mismatch is
/// an error here.
///
/// The checks, each from `framebuffer_linux.lua`:
///
/// - `finfo.type` is packed pixels, and `vinfo.xres` and `yres` are positive.
/// - `line_length * 8` is a multiple of `bits_per_pixel` (`stride_pixels`).
/// - `fb_size <= smem_len`. KOReader's assert also tests `finfo.smem`, which the
///   kernel struct does not have, so only this half is ported.
///
/// The backend also needs a row of pixels to fit in `line_length`, and the 32 bpp
/// channels to sit at byte offsets 0, 8 or 16. Those are checks added here. KOReader
/// does not make them, and it uses the channel offsets only to detect BGR order.
pub fn validate(fix: &FbFixScreeninfo, var: &FbVarScreeninfo, expected: Size) -> Result<FbGeometry, String> {
    if fix.kind != FB_TYPE_PACKED_PIXELS {
        return Err(format!("framebuffer type {} is not packed pixels", fix.kind));
    }
    if var.xres == 0 || var.yres == 0 {
        return Err("framebuffer reports no resolution".into());
    }
    let size = Size::new(var.xres, var.yres);
    if size != expected {
        return Err(format!(
            "framebuffer is {}x{}, expected {}x{} at native rotation",
            size.w, size.h, expected.w, expected.h
        ));
    }

    let pixels = match var.bits_per_pixel {
        8 => FbPixels::Gray8,
        32 => {
            let channels = [var.red, var.green, var.blue];
            for c in channels {
                if c.length != 8 || c.offset > 16 || c.offset % 8 != 0 {
                    return Err(format!(
                        "unsupported 32 bpp channel: offset {}, length {}",
                        c.offset, c.length
                    ));
                }
            }
            let (r, g, b) = (var.red.offset, var.green.offset, var.blue.offset);
            if r == g || g == b || r == b {
                return Err("32 bpp channels share a byte offset".into());
            }
            FbPixels::Rgb32 {
                red: r,
                green: g,
                blue: b,
            }
        }
        other => return Err(format!("{other} bits per pixel is not supported (use 8 or 32)")),
    };

    let line_length = fix.line_length as usize;
    if !(fix.line_length as u64 * 8).is_multiple_of(var.bits_per_pixel as u64) {
        return Err("line_length does not end on a pixel boundary".into());
    }
    let row_bytes = var.xres as usize * pixels.bytes_per_pixel();
    if line_length < row_bytes {
        return Err(format!(
            "line_length {line_length} is shorter than a row of {row_bytes} bytes"
        ));
    }

    let fb_size = line_length * var.yres as usize;
    if fb_size > fix.smem_len as usize {
        return Err(format!(
            "framebuffer needs {fb_size} bytes but reports {} bytes of memory",
            fix.smem_len
        ));
    }

    Ok(FbGeometry {
        size,
        line_length,
        fb_size,
        map_len: page_align(fb_size),
        pixels,
    })
}

/// Rounds `size` up to a whole number of pages (`PAGE_ALIGN` in `framebuffer_linux.lua`).
pub const fn page_align(size: usize) -> usize {
    (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1)
}

#[cfg(test)]
mod tests {
    use super::super::abi::FbBitfield;
    use super::*;

    const PANEL: Size = Size::new(1264, 1680);

    /// Test values for a 32 bpp BGRX framebuffer at the panel size. The offsets are
    /// assumed, not read from a device. See the report.
    fn bgrx() -> (FbFixScreeninfo, FbVarScreeninfo) {
        let fix = FbFixScreeninfo {
            kind: FB_TYPE_PACKED_PIXELS,
            line_length: 1264 * 4,
            smem_len: 16 * 1024 * 1024,
            ..Default::default()
        };
        let var = FbVarScreeninfo {
            xres: 1264,
            yres: 1680,
            bits_per_pixel: 32,
            red: FbBitfield {
                offset: 16,
                length: 8,
                msb_right: 0,
            },
            green: FbBitfield {
                offset: 8,
                length: 8,
                msb_right: 0,
            },
            blue: FbBitfield {
                offset: 0,
                length: 8,
                msb_right: 0,
            },
            ..Default::default()
        };
        (fix, var)
    }

    #[test]
    fn accepts_a_bgrx_framebuffer_at_the_panel_size() {
        let (fix, var) = bgrx();
        let g = validate(&fix, &var, PANEL).unwrap();
        assert_eq!(g.size, PANEL);
        assert_eq!(g.line_length, 5056);
        assert_eq!(g.fb_size, 5056 * 1680);
        assert_eq!(g.map_len, 8_495_104);
        assert_eq!(
            g.pixels,
            FbPixels::Rgb32 {
                red: 16,
                green: 8,
                blue: 0
            }
        );
    }

    #[test]
    fn accepts_an_8_bpp_framebuffer() {
        let (mut fix, mut var) = bgrx();
        fix.line_length = 1264;
        var.bits_per_pixel = 8;
        let g = validate(&fix, &var, PANEL).unwrap();
        assert_eq!(g.pixels, FbPixels::Gray8);
    }

    #[test]
    fn rejects_a_landscape_framebuffer() {
        let (fix, mut var) = bgrx();
        var.xres = 1680;
        var.yres = 1264;
        assert!(validate(&fix, &var, PANEL).unwrap_err().contains("expected"));
    }

    #[test]
    fn rejects_other_depths() {
        let (fix, mut var) = bgrx();
        var.bits_per_pixel = 16;
        assert!(validate(&fix, &var, PANEL).unwrap_err().contains("16 bits"));
    }

    #[test]
    fn rejects_a_framebuffer_that_is_not_packed_pixels() {
        let (mut fix, var) = bgrx();
        fix.kind = 1;
        assert!(validate(&fix, &var, PANEL).is_err());
    }

    #[test]
    fn rejects_a_line_that_cannot_hold_a_row() {
        let (mut fix, var) = bgrx();
        fix.line_length = 1000;
        assert!(validate(&fix, &var, PANEL).unwrap_err().contains("shorter"));
    }

    #[test]
    fn rejects_a_mapping_larger_than_the_reported_memory() {
        let (mut fix, var) = bgrx();
        fix.smem_len = 1024 * 1024;
        assert!(validate(&fix, &var, PANEL)
            .unwrap_err()
            .contains("bytes of memory"));
    }

    #[test]
    fn rejects_channels_that_do_not_fit_a_byte_lane() {
        let (fix, mut var) = bgrx();
        var.red.offset = 24;
        assert!(validate(&fix, &var, PANEL).is_err());
        let (fix, mut var) = bgrx();
        var.green.offset = 16;
        assert!(validate(&fix, &var, PANEL).unwrap_err().contains("share"));
    }

    #[test]
    fn page_align_rounds_up_to_4096() {
        assert_eq!(page_align(0), 0);
        assert_eq!(page_align(1), 4096);
        assert_eq!(page_align(4096), 4096);
        assert_eq!(page_align(4097), 8192);
    }
}
