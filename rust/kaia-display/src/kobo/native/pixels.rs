//! Pixel conversion and rotation for the native Kobo backend. A logical frame region
//! becomes the bytes of the native framebuffer rectangle it covers. Pure code, so it
//! runs on the host.

use kaia_gfx::{PixelFormat, Pixmap, Rect};

use super::geometry::Orientation;

/// How one pixel is stored in the framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FbPixels {
    /// One byte per pixel: the gray level.
    Gray8,
    /// Four bytes per pixel, little endian. Each shift gives the bit position of a
    /// channel in the 32-bit word. Byte 3 holds 0xFF, as KOReader writes the alpha
    /// byte of `ColorRGB32` (`base/ffi/blitbuffer.lua`, `Color8:getColorRGB32`).
    Rgb32 { red: u32, green: u32, blue: u32 },
}

impl FbPixels {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            FbPixels::Gray8 => 1,
            FbPixels::Rgb32 { .. } => 4,
        }
    }
}

/// The gray level of an RGB pixel. The integer weights are those of
/// `ColorRGB24:getColor8` (`base/ffi/blitbuffer.lua`): `(4898 r + 9618 g + 1869 b) >> 14`.
/// A gray pixel (r = g = b) maps to itself.
pub const fn luma(r: u8, g: u8, b: u8) -> u8 {
    ((4898 * r as u32 + 9618 * g as u32 + 1869 * b as u32) >> 14) as u8
}

/// Writes one pixel into `out`, which holds `bytes_per_pixel` bytes.
fn write_pixel(out: &mut [u8], layout: FbPixels, r: u8, g: u8, b: u8) {
    match layout {
        FbPixels::Gray8 => out[0] = luma(r, g, b),
        FbPixels::Rgb32 { red, green, blue } => {
            let word = ((r as u32) << red) | ((g as u32) << green) | ((b as u32) << blue);
            let word = word | (0xFF << 24);
            out[..4].copy_from_slice(&word.to_le_bytes());
        }
    }
}

/// The bytes of a native rectangle, row by row, and where the rectangle is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    /// The native rectangle. Its size gives the shape of `data`.
    pub rect: Rect,
    /// Bytes per pixel of the framebuffer layout.
    pub bytes_per_pixel: usize,
    /// Bytes per row in `data`: `rect.w` times `bytes_per_pixel`.
    pub bytes_per_row: usize,
    /// Pixel bytes, `bytes_per_row * rect.h` long.
    pub data: Vec<u8>,
}

/// Encodes the logical rectangle `logical` of `frame` as a native patch.
///
/// `frame` has the logical screen size. Each pixel of `logical` goes to the native
/// pixel that [`Orientation::physical_point`] gives. The result covers
/// [`Orientation::physical_rect`] of `logical`. Returns `None` for an empty rectangle.
///
/// # Panics
///
/// Panics if `logical` lies outside `frame`. The caller bounds the region first.
pub fn encode_patch(
    frame: &Pixmap,
    logical: Rect,
    orientation: &Orientation,
    layout: FbPixels,
) -> Option<Patch> {
    if logical.is_empty() {
        return None;
    }
    let rect = orientation.physical_rect(logical);
    let bpp = layout.bytes_per_pixel();
    let bytes_per_row = rect.w as usize * bpp;
    let mut data = vec![0u8; bytes_per_row * rect.h as usize];

    let src_bpp = frame.format().bytes_per_pixel();
    let src = frame.data();
    let stride = frame.stride();
    for ly in logical.y..logical.bottom() {
        for lx in logical.x..logical.right() {
            let s = ly as usize * stride + lx as usize * src_bpp;
            let (r, g, b) = match frame.format() {
                PixelFormat::Gray8 => (src[s], src[s], src[s]),
                PixelFormat::Rgb24 => (src[s], src[s + 1], src[s + 2]),
            };
            let p = orientation.physical_point(lx, ly);
            let d = (p.y - rect.y) as usize * bytes_per_row + (p.x - rect.x) as usize * bpp;
            write_pixel(&mut data[d..d + bpp], layout, r, g, b);
        }
    }
    Some(Patch {
        rect,
        bytes_per_pixel: bpp,
        bytes_per_row,
        data,
    })
}

/// Copies a patch into a mapped framebuffer whose rows are `line_length` bytes apart.
///
/// Returns `false` and writes nothing when the patch is empty, starts at a negative
/// coordinate, or would fall outside `fb`.
pub fn store_patch(fb: &mut [u8], line_length: usize, patch: &Patch) -> bool {
    let rect = patch.rect;
    if rect.x < 0 || rect.y < 0 || rect.w == 0 || rect.h == 0 {
        return false;
    }
    let row_len = patch.bytes_per_row;
    let x_off = rect.x as usize * patch.bytes_per_pixel;
    let rows = rect.h as usize;
    if patch.data.len() != row_len * rows || x_off + row_len > line_length {
        return false;
    }
    let last_end = (rect.y as usize + rows - 1) * line_length + x_off + row_len;
    if last_end > fb.len() {
        return false;
    }
    for (i, row) in patch.data.chunks_exact(row_len).enumerate() {
        let start = (rect.y as usize + i) * line_length + x_off;
        fb[start..start + row_len].copy_from_slice(row);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rotation::Rotation;

    #[test]
    fn luma_of_a_gray_pixel_is_the_gray_level() {
        for v in 0..=255u8 {
            assert_eq!(luma(v, v, v), v, "gray {v}");
        }
    }

    #[test]
    fn luma_uses_the_koreader_weights() {
        assert_eq!(luma(0, 0, 0), 0);
        assert_eq!(luma(255, 255, 255), 255);
        // 4898 * 255 >> 14 = 76 (0.299 * 255 = 76.2).
        assert_eq!(luma(255, 0, 0), 76);
        // 9618 * 255 >> 14 = 149.
        assert_eq!(luma(0, 255, 0), 149);
        // 1869 * 255 >> 14 = 29.
        assert_eq!(luma(0, 0, 255), 29);
    }

    #[test]
    fn gray_on_a_bgrx_framebuffer_writes_equal_channels_and_padding() {
        // red at bit 16, green at 8, blue at 0. Bytes in memory: B, G, R, X.
        let layout = FbPixels::Rgb32 {
            red: 16,
            green: 8,
            blue: 0,
        };
        let mut out = [0u8; 4];
        write_pixel(&mut out, layout, 0x40, 0x40, 0x40);
        assert_eq!(out, [0x40, 0x40, 0x40, 0xFF]);
    }

    #[test]
    fn color_on_a_bgrx_framebuffer_puts_blue_first() {
        let layout = FbPixels::Rgb32 {
            red: 16,
            green: 8,
            blue: 0,
        };
        let mut out = [0u8; 4];
        write_pixel(&mut out, layout, 0x11, 0x22, 0x33);
        // Memory order B, G, R, X: 0x33, 0x22, 0x11, 0xFF.
        assert_eq!(out, [0x33, 0x22, 0x11, 0xFF]);
    }

    #[test]
    fn color_on_an_rgbx_framebuffer_puts_red_first() {
        let layout = FbPixels::Rgb32 {
            red: 0,
            green: 8,
            blue: 16,
        };
        let mut out = [0u8; 4];
        write_pixel(&mut out, layout, 0x11, 0x22, 0x33);
        assert_eq!(out, [0x11, 0x22, 0x33, 0xFF]);
    }

    #[test]
    fn gray_frame_to_gray8_keeps_the_levels() {
        let frame = Pixmap::from_data(2, 1, PixelFormat::Gray8, vec![0, 200]).unwrap();
        let o = Orientation::new(kaia_gfx::Size::new(2, 1), Rotation::Deg0);
        let patch = encode_patch(&frame, Rect::new(0, 0, 2, 1), &o, FbPixels::Gray8).unwrap();
        assert_eq!(patch.data, vec![0, 200]);
    }

    #[test]
    fn rotation_quarter_turn_places_pixels_as_koreader_does() {
        // Native 2 wide, 3 tall. Logical 3 wide, 2 tall (Deg90 = KOReader mode 1).
        //   logical  0  1  2        native (x, y)
        //            10 11 12       0 at (0,2), 1 at (0,1), 2 at (0,0)
        //                           10 at (1,2), 11 at (1,1), 12 at (1,0)
        // Native rows: y=0 [2, 12], y=1 [1, 11], y=2 [0, 10].
        let frame = Pixmap::from_data(3, 2, PixelFormat::Gray8, vec![0, 1, 2, 10, 11, 12]).unwrap();
        let o = Orientation::new(kaia_gfx::Size::new(2, 3), Rotation::Deg90);
        let patch = encode_patch(&frame, Rect::new(0, 0, 3, 2), &o, FbPixels::Gray8).unwrap();
        assert_eq!(patch.rect, Rect::new(0, 0, 2, 3));
        assert_eq!(patch.bytes_per_row, 2);
        assert_eq!(patch.data, vec![2, 12, 1, 11, 0, 10]);
    }

    #[test]
    fn a_partial_region_keeps_its_native_placement() {
        // Native 2 wide, 3 tall, Deg90. Logical (2, 0) to (3, 2) is the column of
        // logical pixels 2 and 12. Native: 2 at (0, 0), 12 at (1, 0).
        let frame = Pixmap::from_data(3, 2, PixelFormat::Gray8, vec![0, 1, 2, 10, 11, 12]).unwrap();
        let o = Orientation::new(kaia_gfx::Size::new(2, 3), Rotation::Deg90);
        let patch = encode_patch(&frame, Rect::new(2, 0, 1, 2), &o, FbPixels::Gray8).unwrap();
        assert_eq!(patch.rect, Rect::new(0, 0, 2, 1));
        assert_eq!(patch.data, vec![2, 12]);
    }

    #[test]
    fn color_frame_is_reduced_to_gray_for_an_8_bpp_framebuffer() {
        let frame = Pixmap::from_data(1, 1, PixelFormat::Rgb24, vec![255, 0, 0]).unwrap();
        let o = Orientation::new(kaia_gfx::Size::new(1, 1), Rotation::Deg0);
        let patch = encode_patch(&frame, Rect::new(0, 0, 1, 1), &o, FbPixels::Gray8).unwrap();
        assert_eq!(patch.data, vec![76]);
    }

    #[test]
    fn empty_regions_encode_nothing() {
        let frame = Pixmap::new(4, 4, PixelFormat::Gray8);
        let o = Orientation::new(kaia_gfx::Size::new(4, 4), Rotation::Deg0);
        assert!(encode_patch(&frame, Rect::new(1, 1, 0, 2), &o, FbPixels::Gray8).is_none());
    }

    #[test]
    fn store_patch_writes_rows_at_the_line_length() {
        let patch = Patch {
            rect: Rect::new(1, 1, 2, 2),
            bytes_per_pixel: 1,
            bytes_per_row: 2,
            data: vec![1, 2, 3, 4],
        };
        // Four rows of 4 bytes each. The patch starts at byte 1 of row 1.
        let mut fb = vec![0u8; 16];
        assert!(store_patch(&mut fb, 4, &patch));
        assert_eq!(&fb[..], &[0, 0, 0, 0, 0, 1, 2, 0, 0, 3, 4, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn store_patch_refuses_a_patch_past_the_end() {
        let patch = Patch {
            rect: Rect::new(3, 3, 2, 2),
            bytes_per_pixel: 1,
            bytes_per_row: 2,
            data: vec![1, 2, 3, 4],
        };
        let mut fb = vec![0u8; 16];
        assert!(!store_patch(&mut fb, 4, &patch));
        assert!(fb.iter().all(|&b| b == 0));
    }

    #[test]
    fn rgb32_patch_has_four_bytes_per_pixel() {
        let frame = Pixmap::from_data(1, 1, PixelFormat::Rgb24, vec![0x11, 0x22, 0x33]).unwrap();
        let o = Orientation::new(kaia_gfx::Size::new(1, 1), Rotation::Deg0);
        let layout = FbPixels::Rgb32 {
            red: 16,
            green: 8,
            blue: 0,
        };
        let patch = encode_patch(&frame, Rect::new(0, 0, 1, 1), &o, layout).unwrap();
        assert_eq!(patch.bytes_per_row, 4);
        assert_eq!(patch.data, vec![0x33, 0x22, 0x11, 0xFF]);
    }
}
