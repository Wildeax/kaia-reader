//! [`SheetPanel`]: a panel that rises from the bottom edge with a title, a
//! close button and any content.

use kaia_gfx::{Point, Size};
use kaia_input::Gesture;

use crate::components::{Column, CrossAlign, Expanded, Frame, Label, Padding, Row};
use crate::controls::{IconButton, Reaction};
use crate::geometry::{Constraints, Insets};
use crate::pod::WidgetPod;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};

/// Tallest share of the screen a sheet takes: 70 percent.
const MAX_HEIGHT_PERCENT: u32 = 70;

/// A sheet over a dimmed screen, anchored to the bottom edge and at most 70
/// percent of the height. The close button closes the window. A tap above the
/// sheet closes it too, unless the sheet is modal.
///
/// The sheet closes its own window through the event context, so the app
/// needs no handler for it. Show it with `Window::new(sheet, ...)` and
/// `ShowOpts { covers_fullscreen: false, ..}` so the screen behind stays visible.
pub struct SheetPanel {
    modal: bool,
    sheet: WidgetPod,
    sheet_top: i32,
}

/// Closes the window that holds the widget, if it has one.
fn close_window(cx: &mut EventCx<'_>) {
    if let Some(id) = cx.window_id() {
        cx.ctx().close(id, None);
    }
}

impl SheetPanel {
    /// A sheet with a title and `content`. The content takes the width of the sheet.
    pub fn new(theme: &Theme, title: impl Into<String>, content: WidgetPod) -> Self {
        let t = theme;
        let title = WidgetPod::new(Label::new(title, t.font.title.clone(), Tone::Ink));
        let close = WidgetPod::new(IconButton::new("close").reaction(Reaction::hook(close_window)))
            .with_key("sheet-close");
        let header = WidgetPod::new(
            Row::new(vec![WidgetPod::new(Expanded::new(title, 1)), close])
                .gap(t.space.md)
                .cross_align(CrossAlign::Center),
        );
        let column = Column::new(vec![header, content])
            .gap(t.space.md)
            .cross_align(CrossAlign::Stretch)
            .shrink();
        // Sheet content lines up with the page margin, as on every page.
        let padded = WidgetPod::new(Padding::new(WidgetPod::new(column), Insets::all(t.space.lg)));
        let sheet = Frame::new(padded)
            .fill(Tone::Surface)
            .border(Tone::Ink, t.line.hairline)
            .radius(t.radius.card);
        Self {
            modal: false,
            sheet: WidgetPod::new(sheet),
            sheet_top: 0,
        }
    }

    /// A modal sheet ignores taps above it. Only its own buttons close it.
    pub fn modal(mut self, yes: bool) -> Self {
        self.modal = yes;
        self
    }
}

impl Widget for SheetPanel {
    crate::impl_as_any!();

    /// The root fills the screen. The sheet sits at the bottom with the full width.
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let w = c.max.w;
        let max_h = c.max.h * MAX_HEIGHT_PERCENT / 100;
        let s = self.sheet.layout(
            cx,
            Constraints {
                min: Size::new(w, 0),
                max: Size::new(w, max_h),
            },
        );
        let y = c.max.h.saturating_sub(s.h) as i32;
        self.sheet.set_origin(Point::new(0, y));
        self.sheet_top = y;
        c.constrain(c.max)
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        cx.dim(cx.bounds(), 96);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        match ev {
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                let above = pos.y < cx.bounds().y + self.sheet_top;
                if above && !self.modal {
                    close_window(cx);
                }
                Handled::Yes
            }
            UiEvent::Gesture(_) => Handled::Yes,
            _ => Handled::No,
        }
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.sheet]
    }
}
