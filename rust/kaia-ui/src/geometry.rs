//! Box constraints, insets and alignment, in pixels.

use kaia_gfx::Size;

/// Box constraints passed from a parent to a child. A child returns a size
/// inside `min` and `max`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Constraints {
    pub min: Size,
    pub max: Size,
}

impl Constraints {
    /// Exactly `size`.
    pub const fn tight(size: Size) -> Self {
        Self { min: size, max: size }
    }

    /// Any size up to `max`.
    pub const fn loose(max: Size) -> Self {
        Self {
            min: Size::new(0, 0),
            max,
        }
    }

    /// Clamps `size` into the range.
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            size.w.clamp(self.min.w, self.max.w.max(self.min.w)),
            size.h.clamp(self.min.h, self.max.h.max(self.min.h)),
        )
    }

    /// The same maximum with no minimum.
    pub fn loosen(&self) -> Self {
        Self::loose(self.max)
    }

    /// The range with `insets` removed from every side. Values stop at zero.
    pub fn deflate(&self, insets: Insets) -> Self {
        let (h, v) = (insets.horizontal(), insets.vertical());
        Self {
            min: Size::new(self.min.w.saturating_sub(h), self.min.h.saturating_sub(v)),
            max: Size::new(self.max.w.saturating_sub(h), self.max.h.saturating_sub(v)),
        }
    }
}

/// Space on each side of a box, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Insets {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl Insets {
    pub const fn all(v: u32) -> Self {
        Self {
            left: v,
            top: v,
            right: v,
            bottom: v,
        }
    }

    /// `h` on the left and right, `v` on the top and bottom.
    pub const fn symmetric(h: u32, v: u32) -> Self {
        Self {
            left: h,
            top: v,
            right: h,
            bottom: v,
        }
    }

    pub const fn horizontal(&self) -> u32 {
        self.left + self.right
    }

    pub const fn vertical(&self) -> u32 {
        self.top + self.bottom
    }
}

/// Placement of a child inside a larger box.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Alignment {
    #[default]
    Start,
    Center,
    End,
}

impl Alignment {
    /// Offset of a child of size `child` in a box of size `total`.
    pub fn place(self, total: u32, child: u32) -> i32 {
        let free = total.saturating_sub(child);
        let offset = match self {
            Alignment::Start => 0,
            Alignment::Center => free / 2,
            Alignment::End => free,
        };
        offset as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constrain_clamps_each_axis() {
        let c = Constraints {
            min: Size::new(10, 20),
            max: Size::new(100, 200),
        };
        assert_eq!(c.constrain(Size::new(5, 500)), Size::new(10, 200));
        assert_eq!(c.constrain(Size::new(50, 50)), Size::new(50, 50));
        assert_eq!(
            Constraints::tight(Size::new(7, 8)).constrain(Size::new(1, 1)),
            Size::new(7, 8)
        );
    }

    #[test]
    fn deflate_removes_insets_and_stops_at_zero() {
        let c = Constraints {
            min: Size::new(4, 4),
            max: Size::new(100, 50),
        };
        let d = c.deflate(Insets::symmetric(10, 5));
        assert_eq!(d.max, Size::new(80, 40));
        assert_eq!(d.min, Size::new(0, 0));
    }

    #[test]
    fn alignment_places_within_the_free_space() {
        assert_eq!(Alignment::Start.place(100, 30), 0);
        assert_eq!(Alignment::Center.place(100, 30), 35);
        assert_eq!(Alignment::End.place(100, 30), 70);
        assert_eq!(
            Alignment::Center.place(10, 30),
            0,
            "a child larger than the box starts at zero"
        );
    }
}
