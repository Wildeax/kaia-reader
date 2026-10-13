//! Single-child and overlay containers: [`Stack`], [`Padding`], [`Align`],
//! [`SizedBox`] and [`Frame`].

use kaia_gfx::{Point, Size};

use crate::geometry::{Alignment, Constraints, Insets};
use crate::pod::WidgetPod;
use crate::theme::Tone;
use crate::widget::{LayoutCx, PaintCx, Widget};

/// Children drawn on top of each other, all at the top left corner. The
/// stack is as large as its largest child.
pub struct Stack {
    children: Vec<WidgetPod>,
}

impl Stack {
    pub fn new(children: Vec<WidgetPod>) -> Self {
        Self { children }
    }
}

impl Widget for Stack {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let inner = c.loosen();
        let mut size = Size::new(0, 0);
        for child in &mut self.children {
            let s = child.layout(cx, inner);
            child.set_origin(Point::new(0, 0));
            size = Size::new(size.w.max(s.w), size.h.max(s.h));
        }
        c.constrain(size)
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.children.iter_mut().collect()
    }
}

/// Space around a child.
pub struct Padding {
    child: WidgetPod,
    insets: Insets,
}

impl Padding {
    pub fn new(child: WidgetPod, insets: Insets) -> Self {
        Self { child, insets }
    }
}

/// Lays out `child` inside `insets` and returns the padded size. Shared by
/// [`Padding`] and [`Frame`].
fn layout_inset(child: &mut WidgetPod, cx: &mut LayoutCx, c: Constraints, insets: Insets) -> Size {
    let s = child.layout(cx, c.deflate(insets));
    child.set_origin(Point::new(insets.left as i32, insets.top as i32));
    c.constrain(Size::new(s.w + insets.horizontal(), s.h + insets.vertical()))
}

impl Widget for Padding {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        layout_inset(&mut self.child, cx, c, self.insets)
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.child]
    }
}

/// Places a child at an alignment inside the space it is given. The widget
/// fills its maximum constraint, as Flutter's `Align` does.
pub struct Align {
    child: WidgetPod,
    horizontal: Alignment,
    vertical: Alignment,
}

impl Align {
    pub fn new(child: WidgetPod, horizontal: Alignment, vertical: Alignment) -> Self {
        Self {
            child,
            horizontal,
            vertical,
        }
    }
}

impl Widget for Align {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let size = c.constrain(c.max);
        let s = self.child.layout(cx, c.loosen());
        let x = self.horizontal.place(size.w, s.w);
        let y = self.vertical.place(size.h, s.h);
        self.child.set_origin(Point::new(x, y));
        size
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.child]
    }
}

/// A fixed width, height or both. Without a child, it is an empty box of that size.
pub struct SizedBox {
    child: Option<WidgetPod>,
    width: Option<u32>,
    height: Option<u32>,
}

impl SizedBox {
    pub fn new(child: Option<WidgetPod>) -> Self {
        Self {
            child,
            width: None,
            height: None,
        }
    }

    pub fn width(mut self, px: u32) -> Self {
        self.width = Some(px);
        self
    }

    pub fn height(mut self, px: u32) -> Self {
        self.height = Some(px);
        self
    }
}

impl Widget for SizedBox {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let mut inner = c;
        if let Some(w) = self.width {
            let w = w.min(c.max.w);
            inner.min.w = w;
            inner.max.w = w;
        }
        if let Some(h) = self.height {
            let h = h.min(c.max.h);
            inner.min.h = h;
            inner.max.h = h;
        }
        let s = match &mut self.child {
            Some(child) => {
                let s = child.layout(cx, inner);
                child.set_origin(Point::new(0, 0));
                s
            }
            None => Size::new(0, 0),
        };
        let size = Size::new(self.width.unwrap_or(s.w), self.height.unwrap_or(s.h));
        c.constrain(size)
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.child.iter_mut().collect()
    }
}

/// A box with an optional fill, a border, rounded corners and padding. The
/// fill and border are drawn before the child.
pub struct Frame {
    child: WidgetPod,
    fill: Option<Tone>,
    border: Option<(Tone, u32)>,
    radius: u32,
    padding: Insets,
}

impl Frame {
    pub fn new(child: WidgetPod) -> Self {
        Self {
            child,
            fill: None,
            border: None,
            radius: 0,
            padding: Insets::default(),
        }
    }

    pub fn fill(mut self, tone: Tone) -> Self {
        self.fill = Some(tone);
        self
    }

    /// A border `width` pixels wide, inside the frame's edge.
    pub fn border(mut self, tone: Tone, width: u32) -> Self {
        self.border = Some((tone, width));
        self
    }

    /// Corner radius in pixels.
    pub fn radius(mut self, px: u32) -> Self {
        self.radius = px;
        self
    }

    pub fn padding(mut self, insets: Insets) -> Self {
        self.padding = insets;
        self
    }
}

impl Widget for Frame {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        layout_inset(&mut self.child, cx, c, self.padding)
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let rect = cx.bounds();
        if let Some(tone) = self.fill {
            let color = cx.color(tone);
            cx.fill_rrect(rect, self.radius, color);
        }
        if let Some((tone, width)) = self.border {
            let color = cx.color(tone);
            cx.border(rect, self.radius, width, color);
        }
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.child]
    }

    /// The fill, when the frame has one. Children sit on it.
    fn child_background(&self) -> Option<Tone> {
        self.fill
    }
}
