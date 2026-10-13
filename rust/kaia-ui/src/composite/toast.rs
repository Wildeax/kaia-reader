//! [`Toast`]: a short message at the bottom of the screen that closes itself.

use kaia_events::{Ctx, ShowOpts, WindowId};
use kaia_gfx::{Point, Size};

use crate::assets::SharedAssets;
use crate::components::{Frame, Padding, Paragraph};
use crate::geometry::{Constraints, Insets};
use crate::pod::WidgetPod;
use crate::theme::{Theme, Tone};
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};
use crate::window::Window;

/// A message in a dark box. The window is a toast: it never takes events
/// from the window below, and it does not stop them.
pub struct Toast {
    panel: WidgetPod,
    bottom: Option<u32>,
}

impl Toast {
    pub fn new(theme: &Theme, message: impl Into<String>) -> Self {
        let text = WidgetPod::new(Paragraph::new(message, theme.font.body.clone(), Tone::Surface));
        let panel = Frame::new(WidgetPod::new(Padding::new(text, Insets::all(theme.space.md))))
            .fill(Tone::Ink)
            .radius(theme.radius.card);
        Self {
            panel: WidgetPod::new(panel),
            bottom: None,
        }
    }

    /// Puts the bottom edge of the toast `px` above the screen edge. Use it to
    /// clear a nav bar: `Toast::new(..).above(theme.nav_bar)`.
    pub fn above(mut self, px: u32) -> Self {
        self.bottom = Some(px);
        self
    }

    /// Shows the toast on `ctx` and schedules its close after `seconds`.
    /// Returns the window id.
    pub fn show(
        self,
        ctx: &mut Ctx,
        theme: &Theme,
        assets: &SharedAssets,
        screen: Size,
        seconds: f64,
    ) -> WindowId {
        let window = Window::new(self, theme.clone(), assets.clone(), screen);
        let id = window.show(
            ctx,
            ShowOpts {
                toast: true,
                ..ShowOpts::default()
            },
        );
        ctx.scheduler
            .schedule_in(seconds, Box::new(move |ctx: &mut Ctx| ctx.close(id, None)));
        id
    }
}

impl Widget for Toast {
    crate::impl_as_any!();

    /// The box is centered at the bottom, `space.xl` above the edge unless
    /// [`Toast::above`] sets another offset, and at most four fifths wide.
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let max_w = c.max.w * 4 / 5;
        let s = self.panel.layout(
            cx,
            Constraints {
                min: Size::new(0, 0),
                max: Size::new(max_w, c.max.h),
            },
        );
        let x = (c.max.w.saturating_sub(s.w) / 2) as i32;
        let bottom = self.bottom.unwrap_or(cx.theme.space.xl);
        let y = c.max.h.saturating_sub(s.h + bottom) as i32;
        self.panel.set_origin(Point::new(x, y));
        c.constrain(c.max)
    }

    fn event(&mut self, _cx: &mut EventCx, _ev: &UiEvent) -> Handled {
        Handled::No
    }

    fn paint(&mut self, _cx: &mut PaintCx) {}

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.panel]
    }
}
