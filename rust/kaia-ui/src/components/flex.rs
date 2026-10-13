//! [`Column`] and [`Row`]: children along one axis, with gaps, flex children
//! and alignment. [`Spacer`] and [`Expanded`] take the space left over.

use kaia_gfx::{Point, Size};

use crate::geometry::{Alignment, Constraints};
use crate::pod::WidgetPod;
use crate::widget::{LayoutCx, Widget};

/// Where children go along the main axis when space is left over.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MainAlign {
    #[default]
    Start,
    Center,
    End,
    /// Spreads the leftover space between children.
    SpaceBetween,
}

/// Where children go on the cross axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CrossAlign {
    #[default]
    Start,
    Center,
    End,
    /// Every child takes the full cross size.
    Stretch,
}

/// Whether the container fills its main axis or shrinks to its children.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MainSize {
    /// Fill the maximum main size (the default, as in Flutter).
    #[default]
    Max,
    /// Shrink to the children and the gaps.
    Min,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Horizontal,
    Vertical,
}

struct Flex {
    axis: Axis,
    children: Vec<WidgetPod>,
    gap: u32,
    main: MainAlign,
    cross: CrossAlign,
    size: MainSize,
    hide_overflow: bool,
}

impl Flex {
    fn new(axis: Axis, children: Vec<WidgetPod>) -> Self {
        Self {
            axis,
            children,
            gap: 0,
            main: MainAlign::Start,
            cross: CrossAlign::Start,
            size: MainSize::Max,
            hide_overflow: false,
        }
    }

    fn vertical(&self) -> bool {
        self.axis == Axis::Vertical
    }

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let vertical = self.vertical();
        let (max_main, max_cross, min_main, min_cross) = if vertical {
            (c.max.h, c.max.w, c.min.h, c.min.w)
        } else {
            (c.max.w, c.max.h, c.min.w, c.min.h)
        };
        let n = self.children.len();
        let cross_align = self.cross;
        let gap = self.gap;
        let mut sizes = vec![Size::new(0, 0); n];

        // With `hide_overflow`, a child whose natural size does not fit in what is
        // left is hidden, and so is every child after it. Each child is measured
        // visible first, because a hidden pod lays out to zero. Flex children stay shown.
        if self.hide_overflow {
            let mut used: u64 = 0;
            let mut count: u64 = 0;
            let mut cut = false;
            for child in self.children.iter_mut() {
                child.set_visible(true);
                if child.flex() > 0 {
                    continue;
                }
                let s = child.layout(
                    cx,
                    child_constraints(vertical, cross_align, 0, max_main, max_cross),
                );
                let step = main_of(vertical, s) as u64 + if count > 0 { gap as u64 } else { 0 };
                let fits = !cut && used + step <= max_main as u64;
                cut |= !fits;
                child.set_visible(fits);
                if fits {
                    used += step;
                    count += 1;
                }
            }
        }

        // Gaps between the children that are laid out. A hidden child in hide
        // mode takes no gap.
        let laid_out = if self.hide_overflow {
            self.children.iter().filter(|c| c.is_visible()).count()
        } else {
            n
        };
        let gaps = self.gap as u64 * laid_out.saturating_sub(1) as u64;

        // Pass 1: children without flex take their natural size.
        let mut used: u64 = 0;
        for (i, child) in self.children.iter_mut().enumerate() {
            if !child.is_visible() || child.flex() > 0 {
                continue;
            }
            let room = (max_main as u64).saturating_sub(used + gaps) as u32;
            let s = child.layout(cx, child_constraints(vertical, cross_align, 0, room, max_cross));
            sizes[i] = s;
            used += main_of(vertical, s) as u64;
        }

        // Pass 2: flex children share what is left, in proportion to their weights.
        let total_flex: u64 = self
            .children
            .iter()
            .filter(|c| c.is_visible())
            .map(|c| c.flex() as u64)
            .sum();
        let free = (max_main as u64).saturating_sub(used + gaps);
        let mut flexed: u64 = 0;
        for (i, child) in self.children.iter_mut().enumerate() {
            let weight = child.flex() as u64;
            if !child.is_visible() || weight == 0 {
                continue;
            }
            let share = (free * weight).checked_div(total_flex).unwrap_or(0) as u32;
            let s = child.layout(
                cx,
                child_constraints(vertical, cross_align, share, share, max_cross),
            );
            sizes[i] = s;
            flexed += main_of(vertical, s) as u64;
        }

        // Main size and offset of the first child.
        let content = used + flexed + gaps;
        let main_size = match self.size {
            MainSize::Max => max_main as u64,
            MainSize::Min => content,
        };
        let main_size = main_size.clamp(min_main as u64, (max_main as u64).max(min_main as u64)) as u32;
        let leftover = (main_size as u64).saturating_sub(content) as u32;
        let visible_count = self.children.iter().filter(|c| c.is_visible()).count() as u32;
        let (mut pos, extra) = match self.main {
            MainAlign::Start => (0u32, 0u32),
            MainAlign::Center => (leftover / 2, 0),
            MainAlign::End => (leftover, 0),
            MainAlign::SpaceBetween if visible_count > 1 => (0, leftover / (visible_count - 1)),
            MainAlign::SpaceBetween => (0, 0),
        };

        // Cross size: the largest child, or the full cross extent when stretching.
        let natural_cross = self
            .children
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_visible())
            .map(|(i, _)| cross_of(vertical, sizes[i]))
            .max()
            .unwrap_or(0);
        let cross_size = if self.cross == CrossAlign::Stretch {
            max_cross
        } else {
            natural_cross
        };
        let cross_size = cross_size.clamp(min_cross, max_cross.max(min_cross));

        for (i, child) in self.children.iter_mut().enumerate() {
            if !child.is_visible() {
                continue;
            }
            let s = sizes[i];
            let cross_extent = cross_of(vertical, s);
            let cross_off = match cross_align {
                CrossAlign::Start | CrossAlign::Stretch => 0,
                CrossAlign::Center => Alignment::Center.place(cross_size, cross_extent),
                CrossAlign::End => Alignment::End.place(cross_size, cross_extent),
            };
            let origin = if vertical {
                Point::new(cross_off, pos as i32)
            } else {
                Point::new(pos as i32, cross_off)
            };
            child.set_origin(origin);
            pos += main_of(vertical, s) + gap + extra;
        }

        if vertical {
            Size::new(cross_size, main_size)
        } else {
            Size::new(main_size, cross_size)
        }
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.children.iter_mut().collect()
    }
}

fn main_of(vertical: bool, s: Size) -> u32 {
    if vertical {
        s.h
    } else {
        s.w
    }
}

fn cross_of(vertical: bool, s: Size) -> u32 {
    if vertical {
        s.w
    } else {
        s.h
    }
}

/// Constraints for one child: the main axis is `main_min..=main_max`, the
/// cross axis is `..=max_cross`, or exactly `max_cross` when stretching.
fn child_constraints(
    vertical: bool,
    cross: CrossAlign,
    main_min: u32,
    main_max: u32,
    max_cross: u32,
) -> Constraints {
    let cross_min = if cross == CrossAlign::Stretch {
        max_cross
    } else {
        0
    };
    let (min, max) = if vertical {
        (Size::new(cross_min, main_min), Size::new(max_cross, main_max))
    } else {
        (Size::new(main_min, cross_min), Size::new(main_max, max_cross))
    };
    Constraints { min, max }
}

macro_rules! linear {
    ($(#[$doc:meta])* $name:ident, $axis:expr) => {
        $(#[$doc])*
        pub struct $name(Flex);

        impl $name {
            /// Hides the children that do not fit whole, instead of letting them
            /// run past the end and get cut in half. The container owns the
            /// visibility of its children while this is on.
            pub fn hide_overflow(mut self) -> Self {
                self.0.hide_overflow = true;
                self
            }

            pub fn new(children: Vec<WidgetPod>) -> Self {
                Self(Flex::new($axis, children))
            }

            /// Space between neighbouring children, in pixels.
            pub fn gap(mut self, px: u32) -> Self {
                self.0.gap = px;
                self
            }

            pub fn main_align(mut self, align: MainAlign) -> Self {
                self.0.main = align;
                self
            }

            pub fn cross_align(mut self, align: CrossAlign) -> Self {
                self.0.cross = align;
                self
            }

            /// Shrinks to the children instead of filling the main axis.
            pub fn shrink(mut self) -> Self {
                self.0.size = MainSize::Min;
                self
            }
        }

        impl Widget for $name {
            crate::impl_as_any!();

            fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
                self.0.layout(cx, c)
            }

            fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
                self.0.children_mut()
            }
        }
    };
}

linear!(
    /// Children stacked top to bottom.
    Column,
    Axis::Vertical
);

linear!(
    /// Children placed left to right.
    Row,
    Axis::Horizontal
);

/// Empty space that takes a share of the main axis. A `Spacer` is a flex
/// child with weight `flex`.
pub struct Spacer {
    flex: u32,
}

impl Spacer {
    pub fn new(flex: u32) -> Self {
        Self { flex: flex.max(1) }
    }
}

impl Widget for Spacer {
    crate::impl_as_any!();

    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.min
    }

    fn flex(&self) -> u32 {
        self.flex
    }
}

/// A child that takes a share of the main axis. The child gets the full share
/// as its size constraint.
pub struct Expanded {
    child: WidgetPod,
    flex: u32,
}

impl Expanded {
    pub fn new(child: WidgetPod, flex: u32) -> Self {
        Self {
            child,
            flex: flex.max(1),
        }
    }
}

impl Widget for Expanded {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let s = self.child.layout(cx, c);
        self.child.set_origin(Point::new(0, 0));
        s
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.child]
    }

    fn flex(&self) -> u32 {
        self.flex
    }
}
