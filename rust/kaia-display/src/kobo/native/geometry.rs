//! Region math for the native Kobo backend: software rotation and the alignment of
//! refresh regions. The formulas are KOReader's, named in each function.
//!
//! The rotation is [`crate::rotation`]'s, which follows KOReader's modes. Pixels and
//! ioctl regions both go through the same mapping, as they do in KOReader
//! (`getPhysicalCoordinates` and `getPhysicalRect` in `base/ffi/blitbuffer.lua`).

use kaia_gfx::{Point, Rect, Size};

use crate::rotation::{logical_rect_to_panel, Rotation};

/// Refresh regions are widened to this grid, in logical pixels, when the panel is
/// refreshed. `framebuffer_mxcfb.lua`, MTK block: `alignment_constraint = 16` and
/// `dither_alignment_constraint = 16`, because the driver cuts refreshes at exact
/// coordinates one pixel short.
pub const ALIGNMENT_CONSTRAINT: i32 = 16;

/// The screen as the native backend sees it: the native portrait size and the rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Orientation {
    /// The native size, portrait, as the framebuffer reports it.
    pub native: Size,
    /// The rotation of the logical screen. See [`crate::rotation::Rotation`].
    pub rotation: Rotation,
}

impl Orientation {
    pub fn new(native: Size, rotation: Rotation) -> Self {
        Self { native, rotation }
    }

    /// The logical size at this rotation.
    pub fn logical_size(&self) -> Size {
        self.rotation.logical_size(self.native)
    }

    /// The native pixel that shows logical pixel `(x, y)`.
    pub fn physical_point(&self, x: i32, y: i32) -> Point {
        self.rotation.logical_to_panel(Point::new(x, y), self.native)
    }

    /// The native rectangle that covers a logical rectangle. An empty input gives an
    /// empty output.
    pub fn physical_rect(&self, rect: Rect) -> Rect {
        logical_rect_to_panel(rect, self.rotation, self.native)
    }
}

/// KOReader's `getBoundedRect` (`base/ffi/blitbuffer.lua`, line 1150) on a logical
/// screen of `screen`, with an optional alignment constraint.
///
/// The region is clipped to the screen first, then widened outward to the alignment.
/// Each axis moves its origin down to the grid and grows the size by the amount it
/// moved, so the far edge rounds up to the grid. A size that the far edge pushes past
/// the screen is cut back to the screen edge. The result is never negative.
pub fn bounded_rect(region: Rect, screen: Size, alignment: Option<i32>) -> Rect {
    let (max_w, max_h) = (screen.w as i32, screen.h as i32);
    let (mut x, mut y) = (region.x, region.y);
    let (mut w, mut h) = (region.w as i32, region.h as i32);

    // Out of range origins collapse to an empty region at the origin.
    if x >= max_w {
        x = 0;
        w = 0;
    }
    if y >= max_h {
        y = 0;
        h = 0;
    }
    // Negative origins shrink the region from the left and top.
    if x < 0 {
        w += x;
        x = 0;
    }
    if y < 0 {
        h += y;
        y = 0;
    }

    if let Some(a) = alignment {
        let x_orig = x;
        x = align_down(x_orig, a);
        w = align_up(w + (x_orig - x), a);
        let y_orig = y;
        y = align_down(y_orig, a);
        h = align_up(h + (y_orig - y), a);
    }

    if x + w > max_w {
        w = max_w - x;
    }
    if y + h > max_h {
        h = max_h - y;
    }
    Rect::new(x, y, w.max(0) as u32, h.max(0) as u32)
}

/// `ALIGN_DOWN` in `base/ffi/blitbuffer.lua`: `x & ~(a - 1)`. `a` is a power of two.
fn align_down(x: i32, a: i32) -> i32 {
    x & !(a - 1)
}

/// `ALIGN_UP` in `base/ffi/blitbuffer.lua`: `(x + (a - 1)) & ~(a - 1)`.
fn align_up(x: i32, a: i32) -> i32 {
    (x + (a - 1)) & !(a - 1)
}

/// True when `region` is at least as large as `screen` in both directions. KOReader
/// calls this `_isFullScreen` (`framebuffer_mxcfb.lua`), and it runs after the
/// alignment.
pub fn covers(region: Rect, screen: Size) -> bool {
    region.w >= screen.w && region.h >= screen.h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rotation::logical_rect_to_panel;

    const SCREEN: Size = Size::new(1264, 1680);
    const ALIGN: Option<i32> = Some(ALIGNMENT_CONSTRAINT);

    #[test]
    fn bounded_rect_widens_to_the_grid() {
        // x = 5: origin 0, fixup 5, width ALIGN_UP(10 + 5, 16) = 16.
        assert_eq!(
            bounded_rect(Rect::new(5, 5, 10, 10), SCREEN, ALIGN),
            Rect::new(0, 0, 16, 16)
        );
        // x = 1250: origin 1248, fixup 2, width ALIGN_UP(14 + 2, 16) = 16.
        // Far edge 1264 and 1680 are on the grid already.
        assert_eq!(
            bounded_rect(Rect::new(1250, 1670, 14, 10), SCREEN, ALIGN),
            Rect::new(1248, 1664, 16, 16)
        );
        // Far edge 1262 rounds up to 1264.
        assert_eq!(
            bounded_rect(Rect::new(1200, 0, 62, 16), SCREEN, ALIGN),
            Rect::new(1200, 0, 64, 16)
        );
    }

    #[test]
    fn bounded_rect_clips_at_the_far_edge() {
        // x: origin 1232, fixup 8, width ALIGN_UP(100 + 8, 16) = 112 -> 1344, cut to 1264.
        // y: origin 1600, fixup 0, height ALIGN_UP(100, 16) = 112 -> 1712, cut to 1680.
        assert_eq!(
            bounded_rect(Rect::new(1240, 1600, 100, 100), SCREEN, ALIGN),
            Rect::new(1232, 1600, 32, 80)
        );
    }

    #[test]
    fn bounded_rect_handles_negative_origins() {
        // x = -20: width 50 - 20 = 30, origin 0, width ALIGN_UP(30, 16) = 32.
        // y = 0, height ALIGN_UP(10, 16) = 16.
        assert_eq!(
            bounded_rect(Rect::new(-20, 0, 50, 10), SCREEN, ALIGN),
            Rect::new(0, 0, 32, 16)
        );
        // Without alignment the clipped size stays as it is.
        assert_eq!(
            bounded_rect(Rect::new(-5, -5, 10, 10), SCREEN, None),
            Rect::new(0, 0, 5, 5)
        );
    }

    #[test]
    fn bounded_rect_collapses_regions_that_start_off_screen() {
        assert!(bounded_rect(Rect::new(2000, 10, 5, 5), SCREEN, ALIGN).is_empty());
        assert!(bounded_rect(Rect::new(10, 2000, 5, 5), SCREEN, ALIGN).is_empty());
        // A region entirely left of the screen loses all its width.
        assert!(bounded_rect(Rect::new(-30, 0, 10, 10), SCREEN, ALIGN).is_empty());
    }

    #[test]
    fn full_screen_stays_full() {
        let full = Rect::new(0, 0, 1264, 1680);
        assert_eq!(bounded_rect(full, SCREEN, ALIGN), full);
        assert!(covers(full, SCREEN));
    }

    #[test]
    fn a_region_that_aligns_to_the_screen_counts_as_full() {
        // KOReader checks `_isFullScreen` after the alignment: 1260 x 1676 at the
        // origin grows to 1264 x 1680.
        let aligned = bounded_rect(Rect::new(0, 0, 1260, 1676), SCREEN, ALIGN);
        assert_eq!(aligned, Rect::new(0, 0, 1264, 1680));
        assert!(covers(aligned, SCREEN));
        assert!(!covers(Rect::new(0, 0, 1260, 1676), SCREEN));
    }

    #[test]
    fn deg0_keeps_the_panel_orientation() {
        let o = Orientation::new(Size::new(1264, 1680), Rotation::Deg0);
        assert_eq!(o.logical_size(), Size::new(1264, 1680));
        assert_eq!(o.physical_point(7, 9), Point::new(7, 9));
        let r = Rect::new(3, 4, 5, 6);
        assert_eq!(o.physical_rect(r), r);
    }

    #[test]
    fn deg90_puts_the_logical_bottom_on_the_panel_right() {
        // framebuffer.lua line 51: mode 1 is "landscape with bottom on the right side of
        // screen". The logical top-left pixel is at the panel's bottom left, and the
        // logical bottom edge (y = 1263) is on the panel's right edge (x = 1263).
        let o = Orientation::new(Size::new(1264, 1680), Rotation::Deg90);
        assert_eq!(o.logical_size(), Size::new(1680, 1264));
        assert_eq!(o.physical_point(0, 0), Point::new(0, 1679));
        assert_eq!(o.physical_point(0, 1263), Point::new(1263, 1679));
        assert_eq!(o.physical_point(1679, 1263), Point::new(1263, 0));
    }

    #[test]
    fn deg180_and_deg270_follow_koreader_modes() {
        let upside = Orientation::new(Size::new(1264, 1680), Rotation::Deg180);
        assert_eq!(upside.physical_point(0, 0), Point::new(1263, 1679));
        let landscape = Orientation::new(Size::new(1264, 1680), Rotation::Deg270);
        assert_eq!(landscape.logical_size(), Size::new(1680, 1264));
        assert_eq!(landscape.physical_point(0, 0), Point::new(1263, 0));
        assert_eq!(landscape.physical_point(0, 1263), Point::new(0, 0));
    }

    #[test]
    fn physical_rect_agrees_with_koreader_get_physical_rect() {
        // blitbuffer.lua, `getPhysicalRect` (line 781). W and H are the native width and
        // height. Mode 1 is buffer rotation 3, mode 2 is 2 and mode 3 is 1. The rect is
        // x = 10, y = 20, w = 30, h = 40.
        //   rotation 3 (mode 1): (y, H - (x + w), h, w) = (20, 1640, 40, 30)
        //   rotation 2 (mode 2): (W - (x + w), H - (y + h), w, h) = (1224, 1620, 30, 40)
        //   rotation 1 (mode 3): (W - (y + h), x, h, w) = (1204, 10, 40, 30)
        let r = Rect::new(10, 20, 30, 40);
        let native = Size::new(1264, 1680);
        assert_eq!(
            Orientation::new(native, Rotation::Deg0).physical_rect(r),
            Rect::new(10, 20, 30, 40)
        );
        assert_eq!(
            Orientation::new(native, Rotation::Deg90).physical_rect(r),
            Rect::new(20, 1640, 40, 30)
        );
        assert_eq!(
            Orientation::new(native, Rotation::Deg180).physical_rect(r),
            Rect::new(1224, 1620, 30, 40)
        );
        assert_eq!(
            Orientation::new(native, Rotation::Deg270).physical_rect(r),
            Rect::new(1204, 10, 40, 30)
        );
    }

    #[test]
    fn physical_rect_is_the_corner_hull_of_the_physical_points() {
        // For every rotation, a physical rectangle covers exactly the points that the
        // pixel mapping sends into the logical rectangle.
        let native = Size::new(5, 3);
        for rotation in [
            Rotation::Deg0,
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
        ] {
            let o = Orientation::new(native, rotation);
            let logical = o.logical_size();
            for y0 in 0..logical.h as i32 {
                for x0 in 0..logical.w as i32 {
                    for h in 1..=(logical.h as i32 - y0) {
                        for w in 1..=(logical.w as i32 - x0) {
                            let rect = Rect::new(x0, y0, w as u32, h as u32);
                            let got = o.physical_rect(rect);
                            let mut lo = Point::new(i32::MAX, i32::MAX);
                            let mut hi = Point::new(i32::MIN, i32::MIN);
                            for y in y0..y0 + h {
                                for x in x0..x0 + w {
                                    let p = o.physical_point(x, y);
                                    lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
                                    hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
                                }
                            }
                            assert_eq!(got.x, lo.x, "{rotation:?} {rect:?}");
                            assert_eq!(got.y, lo.y, "{rotation:?} {rect:?}");
                            assert_eq!(got.w as i32, hi.x - lo.x + 1, "{rotation:?} {rect:?}");
                            assert_eq!(got.h as i32, hi.y - lo.y + 1, "{rotation:?} {rect:?}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_rotation_maps_the_logical_screen_onto_the_panel_once() {
        let native = Size::new(4, 3);
        for rotation in [
            Rotation::Deg0,
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
        ] {
            let o = Orientation::new(native, rotation);
            let logical = o.logical_size();
            let mut seen = vec![false; (native.w * native.h) as usize];
            for y in 0..logical.h as i32 {
                for x in 0..logical.w as i32 {
                    let p = o.physical_point(x, y);
                    assert!(p.x >= 0 && p.x < native.w as i32, "{rotation:?}");
                    assert!(p.y >= 0 && p.y < native.h as i32, "{rotation:?}");
                    let i = (p.y as u32 * native.w + p.x as u32) as usize;
                    assert!(!seen[i], "{rotation:?} maps two pixels to {p:?}");
                    seen[i] = true;
                }
            }
            assert!(seen.iter().all(|&s| s), "{rotation:?} leaves a pixel unmapped");
        }
    }

    #[test]
    fn native_orientation_is_the_shared_rotation_mapping() {
        // The native backend and the FBInk path both map through crate::rotation. This
        // keeps them agreeing on every pixel and rectangle.
        let native = Size::new(1264, 1680);
        let r = Rect::new(10, 20, 30, 40);
        for rotation in [
            Rotation::Deg0,
            Rotation::Deg90,
            Rotation::Deg180,
            Rotation::Deg270,
        ] {
            let o = Orientation::new(native, rotation);
            assert_eq!(
                o.physical_rect(r),
                logical_rect_to_panel(r, rotation, native),
                "{rotation:?}"
            );
            assert_eq!(
                o.physical_point(7, 9),
                rotation.logical_to_panel(Point::new(7, 9), native),
                "{rotation:?}"
            );
        }
    }
}
