//! Screen rotation and the mapping between logical and panel coordinates.
//!
//! The panel has a fixed size, portrait on the Kobo. Frames are drawn in logical
//! orientation. A [`Rotation`] names the logical screen by how far it is turned
//! clockwise from the panel. The mapping follows KOReader's rotation modes, so the same
//! number means the same orientation on both sides:
//!
//! - [`Rotation::Deg90`] is KOReader mode 1: "landscape with bottom on the right side of
//!   screen" (`base/ffi/framebuffer.lua`, line 51 and `DEVICE_ROTATED_CLOCKWISE`, line
//!   79). The logical top-left pixel is at the panel's bottom-left corner.
//! - [`Rotation::Deg270`] is KOReader mode 3, the opposite landscape. The logical top-left
//!   pixel is at the panel's top-right corner.
//!
//! The formulas are the inverses of KOReader's blit buffer mapping
//! (`BB_mt.__index:getPhysicalCoordinates`, `base/ffi/blitbuffer.lua`, line 769). KOReader
//! sets that buffer rotation to `-90 * mode` (`fb:setRotationMode`,
//! `base/ffi/framebuffer.lua`, line 457).
//!
//! A backend calls [`rotate_to_panel`] before it sends a frame to the hardware. Touch
//! positions use [`Rotation::panel_to_logical`] on the way back.

use kaia_gfx::{Pixmap, Point, Rect, Size};

/// How far the logical screen is turned clockwise from the panel, in KOReader's
/// rotation modes (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Rotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl Rotation {
    pub const fn degrees(self) -> u32 {
        match self {
            Rotation::Deg0 => 0,
            Rotation::Deg90 => 90,
            Rotation::Deg180 => 180,
            Rotation::Deg270 => 270,
        }
    }

    /// True when the logical width and height are the panel's height and width.
    pub const fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Deg90 | Rotation::Deg270)
    }

    /// The logical screen size for a panel of `panel` size.
    pub const fn logical_size(self, panel: Size) -> Size {
        if self.swaps_axes() {
            Size::new(panel.h, panel.w)
        } else {
            panel
        }
    }

    /// The panel pixel that shows a logical pixel. `panel` is the panel size.
    ///
    /// Deg90 is `(x, y) -> (y, H - 1 - x)` and Deg270 is `(x, y) -> (W - 1 - y, x)`, the
    /// KOReader buffer rotations 3 and 1 (`getPhysicalCoordinates`, line 769 onward).
    pub const fn logical_to_panel(self, p: Point, panel: Size) -> Point {
        let (w, h) = (panel.w as i32, panel.h as i32);
        match self {
            Rotation::Deg0 => p,
            Rotation::Deg90 => Point::new(p.y, h - 1 - p.x),
            Rotation::Deg180 => Point::new(w - 1 - p.x, h - 1 - p.y),
            Rotation::Deg270 => Point::new(w - 1 - p.y, p.x),
        }
    }

    /// The logical pixel that a panel pixel shows. Inverse of [`Rotation::logical_to_panel`].
    pub const fn panel_to_logical(self, p: Point, panel: Size) -> Point {
        let (w, h) = (panel.w as i32, panel.h as i32);
        match self {
            Rotation::Deg0 => p,
            Rotation::Deg90 => Point::new(h - 1 - p.y, p.x),
            Rotation::Deg180 => Point::new(w - 1 - p.x, h - 1 - p.y),
            Rotation::Deg270 => Point::new(p.y, w - 1 - p.x),
        }
    }
}

/// The panel-space rectangle that covers a logical rectangle. An empty input gives an
/// empty output.
pub fn logical_rect_to_panel(rect: Rect, rotation: Rotation, panel: Size) -> Rect {
    if rect.is_empty() {
        return Rect::new(0, 0, 0, 0);
    }
    let a = rotation.logical_to_panel(Point::new(rect.x, rect.y), panel);
    let b = rotation.logical_to_panel(Point::new(rect.right() - 1, rect.bottom() - 1), panel);
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    Rect::new(x0, y0, (x1 - x0 + 1) as u32, (y1 - y0 + 1) as u32)
}

/// Turns a logical frame into panel order. The result has the panel size and the same
/// pixel format as `frame`.
pub fn rotate_to_panel(frame: &Pixmap, rotation: Rotation) -> Pixmap {
    if rotation == Rotation::Deg0 {
        return frame.clone();
    }
    let logical = frame.size();
    let panel = if rotation.swaps_axes() {
        Size::new(logical.h, logical.w)
    } else {
        logical
    };
    let bpp = frame.format().bytes_per_pixel();
    let src_stride = frame.stride();
    let mut out = Pixmap::new(panel.w, panel.h, frame.format());
    let dst_stride = out.stride();
    let src = frame.data();
    let dst = out.data_mut();
    for py in 0..panel.h as i32 {
        for px in 0..panel.w as i32 {
            let s = rotation.panel_to_logical(Point::new(px, py), panel);
            let si = s.y as usize * src_stride + s.x as usize * bpp;
            let di = py as usize * dst_stride + px as usize * bpp;
            dst[di..di + bpp].copy_from_slice(&src[si..si + bpp]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_gfx::PixelFormat;

    const ALL: [Rotation; 4] = [
        Rotation::Deg0,
        Rotation::Deg90,
        Rotation::Deg180,
        Rotation::Deg270,
    ];

    #[test]
    fn logical_size_swaps_for_quarter_turns() {
        let panel = Size::new(2, 3);
        assert_eq!(Rotation::Deg0.logical_size(panel), Size::new(2, 3));
        assert_eq!(Rotation::Deg90.logical_size(panel), Size::new(3, 2));
        assert_eq!(Rotation::Deg180.logical_size(panel), Size::new(2, 3));
        assert_eq!(Rotation::Deg270.logical_size(panel), Size::new(3, 2));
    }

    #[test]
    fn mappings_round_trip_for_every_pixel() {
        let panel = Size::new(4, 3);
        for rot in ALL {
            let logical = rot.logical_size(panel);
            for y in 0..logical.h as i32 {
                for x in 0..logical.w as i32 {
                    let p = Point::new(x, y);
                    let back = rot.panel_to_logical(rot.logical_to_panel(p, panel), panel);
                    assert_eq!(back, p, "{rot:?}");
                }
            }
        }
    }

    #[test]
    fn deg90_puts_the_logical_top_left_at_the_panel_bottom_left() {
        // framebuffer.lua line 51: mode 1 is "landscape with bottom on the right side of
        // screen". The logical top-left pixel is then at the bottom left of the panel.
        // Panel 2 wide and 3 tall.
        let panel = Size::new(2, 3);
        assert_eq!(
            Rotation::Deg90.logical_to_panel(Point::new(0, 0), panel),
            Point::new(0, 2)
        );
    }

    #[test]
    fn deg90_puts_the_logical_bottom_edge_on_the_panel_right_edge() {
        // The bottom edge of a Deg90 screen is the row y = logical height - 1 = panel
        // width - 1, which must land on the panel's right column. Panel 2 wide, 3 tall.
        let panel = Size::new(2, 3);
        for x in 0..3 {
            let p = Rotation::Deg90.logical_to_panel(Point::new(x, 1), panel);
            assert_eq!(p.x, 1, "logical x {x}");
        }
    }

    #[test]
    fn deg270_puts_the_logical_top_left_at_the_panel_top_right() {
        // Mode 3, the opposite landscape: the bottom edge is on the left.
        let panel = Size::new(2, 3);
        assert_eq!(
            Rotation::Deg270.logical_to_panel(Point::new(0, 0), panel),
            Point::new(1, 0)
        );
    }

    #[test]
    fn rotate_to_panel_matches_koreader_mode_1() {
        // Logical frame at Deg90 is 3 wide and 2 tall:
        //   0  1  2
        //  10 11 12
        // Each panel pixel (px, py) shows logical (h - 1 - py, px), with the panel 2 wide
        // and 3 tall. The panel rows, top to bottom, are:
        //   2 12
        //   1 11
        //   0 10
        let logical = Pixmap::from_data(3, 2, PixelFormat::Gray8, vec![0, 1, 2, 10, 11, 12]).unwrap();
        let panel = rotate_to_panel(&logical, Rotation::Deg90);
        let expected = Pixmap::from_data(2, 3, PixelFormat::Gray8, vec![2, 12, 1, 11, 0, 10]).unwrap();
        assert_eq!(panel, expected);
    }

    #[test]
    fn rotate_keeps_pixel_format() {
        let rgb = Pixmap::new(3, 2, PixelFormat::Rgb24);
        let out = rotate_to_panel(&rgb, Rotation::Deg270);
        assert_eq!(out.format(), PixelFormat::Rgb24);
        assert_eq!(out.size(), Size::new(2, 3));
    }

    #[test]
    fn rect_mapping_covers_the_same_pixels() {
        let panel = Size::new(4, 3);
        let rect = Rect::new(1, 0, 2, 1);
        for rot in ALL {
            let mapped = logical_rect_to_panel(rect, rot, panel);
            assert_eq!(mapped.w * mapped.h, 2, "{rot:?}");
            for y in rect.y..rect.bottom() {
                for x in rect.x..rect.right() {
                    let q = rot.logical_to_panel(Point::new(x, y), panel);
                    assert!(mapped.contains(q), "{rot:?} {q:?} not in {mapped:?}");
                }
            }
        }
    }
}
