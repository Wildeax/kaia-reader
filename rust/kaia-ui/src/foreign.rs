//! [`ForeignWidget`]: a node that hands its rectangle and its events to a
//! renderer outside the toolkit. Phase R7 uses it for legacy Lua widgets.
//! See `docs/kaia/specs/UI_TOOLKIT.md`.

use kaia_gfx::{Pixmap, Rect, Size};

use crate::geometry::Constraints;
use crate::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};

/// The outside renderer. The toolkit owns the position and the size. The
/// renderer owns the pixels inside them.
pub trait ForeignRenderer {
    /// Draws into `target`. `target` is a pixmap the size of the widget, in the
    /// frame's pixel format. `rect` is the area to draw, in `target` coordinates.
    fn paint(&mut self, target: &mut Pixmap, rect: Rect);

    /// Receives an event. Gesture positions are absolute, as on the frame.
    /// Return [`Handled::Yes`] to stop the event here.
    fn event(&mut self, ev: &UiEvent) -> Handled;

    /// The size the renderer wants inside `max`. The widget takes it, limited by the constraints.
    fn preferred_size(&self, max: Size) -> Size;
}

/// Wraps a [`ForeignRenderer`] as a widget. Layout asks the renderer for its
/// size. Paint and events go to the renderer as they arrive.
pub struct ForeignWidget {
    renderer: Box<dyn ForeignRenderer>,
}

impl ForeignWidget {
    pub fn new(renderer: impl ForeignRenderer + 'static) -> Self {
        Self {
            renderer: Box::new(renderer),
        }
    }
}

impl Widget for ForeignWidget {
    crate::impl_as_any!();

    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        let want = self.renderer.preferred_size(c.max);
        c.constrain(want)
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let renderer = &mut self.renderer;
        cx.draw_region(b, |target, rect| renderer.paint(target, rect));
    }

    fn event(&mut self, _cx: &mut EventCx, ev: &UiEvent) -> Handled {
        self.renderer.event(ev)
    }
}
