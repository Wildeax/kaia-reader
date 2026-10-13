//! Integer rectangles for refresh regions.
//!
//! The overlap rules follow KOReader's `ui/geometry`: rectangles that only
//! share an edge count as overlapping, so their refreshes merge.

/// A rectangle with its top-left corner at `(x, y)` and size `w` by `h`.
/// Sizes are signed so that the zero-area checks of KOReader's `Geom`
/// translate directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    /// `Geom:area()` is `w * h`. A rectangle with zero area overlaps nothing.
    pub fn area(&self) -> i64 {
        i64::from(self.w) * i64::from(self.h)
    }

    /// `Geom:openIntersectWith(rect_b)`, mirrored from `notOpenIntersectWith`.
    /// Returns false when `other` has zero area. Shared edges count as overlap.
    pub fn open_intersects(&self, other: &Rect) -> bool {
        if other.area() == 0 {
            return false;
        }
        !(self.x > other.x + other.w
            || self.y > other.y + other.h
            || other.x > self.x + self.w
            || other.y > self.y + self.h)
    }

    /// `Geom:combine(rect_b)`: the bounding box of both rectangles.
    pub fn union(&self, other: &Rect) -> Rect {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.w).max(other.x + other.w);
        let y1 = (self.y + self.h).max(other.y + other.h);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_edge_counts_as_overlap() {
        let a = Rect::new(0, 0, 10, 10);
        let touching = Rect::new(10, 0, 10, 10);
        let apart = Rect::new(11, 0, 10, 10);
        assert!(a.open_intersects(&touching));
        assert!(!a.open_intersects(&apart));
    }

    #[test]
    fn zero_area_never_overlaps() {
        let a = Rect::new(0, 0, 10, 10);
        assert!(!a.open_intersects(&Rect::new(5, 5, 0, 3)));
        assert!(!a.open_intersects(&Rect::new(5, 5, 3, 0)));
    }

    #[test]
    fn union_is_bounding_box() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(20, 5, 5, 5);
        assert_eq!(a.union(&b), Rect::new(0, 0, 25, 10));
    }
}
