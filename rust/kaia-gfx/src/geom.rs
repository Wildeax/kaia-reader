//! Integer screen geometry. Coordinates are pixels, origin top left.

/// A point in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// A size in pixels. Never negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Size {
    pub w: u32,
    pub h: u32,
}

/// A rectangle in pixels: top-left corner plus size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

impl Size {
    pub const fn new(w: u32, h: u32) -> Self {
        Self { w, h }
    }
}

/// `v` as an `i32`, stopping at the largest value.
fn clamp_i32(v: i64) -> i32 {
    v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

/// `v` as a `u32`, stopping at zero and at the largest value.
fn clamp_u32(v: i64) -> u32 {
    v.clamp(0, u32::MAX as i64) as u32
}

/// Rectangle arithmetic stops at the limits of `i32` and `u32` instead of
/// overflowing, so a huge rectangle from a bad layout cannot panic a debug build.
impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }

    pub fn from_size(size: Size) -> Self {
        Self::new(0, 0, size.w, size.h)
    }

    pub fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }

    pub fn right(&self) -> i32 {
        clamp_i32(self.x as i64 + self.w as i64)
    }

    pub fn bottom(&self) -> i32 {
        clamp_i32(self.y as i64 + self.h as i64)
    }

    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// The overlap of two rectangles, or an empty rectangle at self's origin.
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x1 <= x0 || y1 <= y0 {
            return Rect::new(self.x, self.y, 0, 0);
        }
        Rect::new(
            x0,
            y0,
            clamp_u32(x1 as i64 - x0 as i64),
            clamp_u32(y1 as i64 - y0 as i64),
        )
    }

    /// The smallest rectangle containing both. An empty input is ignored.
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = self.right().max(other.right());
        let y1 = self.bottom().max(other.bottom());
        Rect::new(
            x0,
            y0,
            clamp_u32(x1 as i64 - x0 as i64),
            clamp_u32(y1 as i64 - y0 as i64),
        )
    }

    /// Shrinks every side by `d` pixels (or grows when negative).
    pub fn inset(&self, d: i32) -> Rect {
        let w = clamp_u32(self.w as i64 - 2 * d as i64);
        let h = clamp_u32(self.h as i64 - 2 * d as i64);
        Rect::new(self.x.saturating_add(d), self.y.saturating_add(d), w, h)
    }

    pub fn translate(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(
            self.x.saturating_add(dx),
            self.y.saturating_add(dy),
            self.w,
            self.h,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_and_union() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Rect::new(5, 5, 5, 5));
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert!(a.intersect(&Rect::new(20, 20, 1, 1)).is_empty());
    }

    #[test]
    fn methods_saturate_instead_of_overflowing() {
        let far = Rect::new(i32::MAX - 5, i32::MAX - 5, 100, 100);
        assert_eq!(far.right(), i32::MAX);
        assert_eq!(far.bottom(), i32::MAX);
        let low = Rect::new(i32::MIN, i32::MIN, u32::MAX, u32::MAX);
        assert_eq!(low.right(), i32::MAX);
        assert_eq!(low.bottom(), i32::MAX);
        let u = low.union(&far);
        assert_eq!((u.x, u.y), (i32::MIN, i32::MIN));
        assert_eq!((u.w, u.h), (u32::MAX, u32::MAX));
        assert_eq!(far.translate(100, 100).x, i32::MAX);
        assert_eq!(Rect::new(i32::MIN + 1, 0, 10, 10).translate(-100, 0).x, i32::MIN);
        assert_eq!(far.inset(-10).x, i32::MAX - 15);
        assert_eq!(Rect::new(i32::MIN + 1, 0, 10, 10).inset(-10).x, i32::MIN);
        assert!(low.intersect(&far).w > 0);
        assert!(low.contains(Point::new(0, 0)));
    }

    #[test]
    fn contains_is_half_open() {
        let r = Rect::new(0, 0, 10, 10);
        assert!(r.contains(Point::new(9, 9)));
        assert!(!r.contains(Point::new(10, 0)));
    }
}
