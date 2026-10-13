//! The widget trait, the events widgets receive, and the contexts they use
//! to lay out, paint and react.
//!
//! A widget draws only its own content in [`Widget::paint`]. The pod that holds
//! it paints the children afterwards, so a parent never paints for its
//! children. See `docs/kaia/specs/UI_TOOLKIT.md`.

use kaia_events::{Ctx, Event, RefreshMode, Value};
use kaia_gfx::draw;
use kaia_gfx::text::{FontSystem, TextLayout};
use kaia_gfx::{AlphaPixmap, Color, Pixmap, Point, Rect, Size};
use kaia_input::{Gesture, KeyEvent};

use crate::assets::IconCache;
use crate::geometry::Constraints;
use crate::pod::{WidgetId, WidgetPod};
use crate::theme::{Theme, Tone};

/// A UI element. Containers hold their children as [`WidgetPod`]s and return
/// them from [`Widget::children_mut`].
pub trait Widget {
    /// Places children and returns this widget's size, which must lie inside
    /// `c`. Children get their size from [`WidgetPod::layout`] and their
    /// position from [`WidgetPod::set_origin`].
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size;

    /// Draws this widget's own content inside [`PaintCx::bounds`]. Children
    /// are painted after this call, on top of it.
    fn paint(&mut self, _cx: &mut PaintCx) {}

    /// Handles an event. Return [`Handled::Yes`] to stop propagation.
    fn event(&mut self, _cx: &mut EventCx, _ev: &UiEvent) -> Handled {
        Handled::No
    }

    /// Children in paint order, first painted first. Hit testing walks them
    /// in reverse, so the last child is the topmost.
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        Vec::new()
    }

    /// Flex weight inside a [`Column`](crate::components::Column) or
    /// [`Row`](crate::components::Row). Zero means the widget takes its natural size.
    fn flex(&self) -> u32 {
        0
    }
}

/// Whether a handler used an event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handled {
    Yes,
    No,
}

/// What a finished frame needs from the panel. Maps to `kaia_events::RefreshMode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaintHint {
    #[default]
    Ui,
    Partial,
    Fast,
    Full,
    Flash,
}

impl PaintHint {
    pub fn mode(self) -> RefreshMode {
        match self {
            PaintHint::Ui => RefreshMode::Ui,
            PaintHint::Partial => RefreshMode::Partial,
            PaintHint::Fast => RefreshMode::Fast,
            PaintHint::Full => RefreshMode::Full,
            PaintHint::Flash => RefreshMode::FlashUi,
        }
    }
}

/// An input or named event as a widget sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum UiEvent {
    /// Routed by position: the deepest widget under the point gets it first.
    Gesture(Gesture),
    /// Routed to the focused widget, then up through its ancestors.
    Key(KeyEvent),
    /// Routed to every widget in paint order until one handles it.
    Named(Event),
    /// Sent to a widget when it gains focus.
    FocusGained,
    /// Sent to a widget when it loses focus.
    FocusLost,
}

/// An action for the app, such as "open this book". The app drains these
/// from the window outbox, as it does for named events.
#[derive(Clone, Debug, PartialEq)]
pub struct AppAction {
    pub id: String,
    pub args: Vec<Value>,
}

impl AppAction {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            args: Vec::new(),
        }
    }
}

/// Something a widget sent to the app.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Event(Event),
    Action(AppAction),
}

/// A paint request on one widget. `rect` is relative to the widget. `None`
/// means the whole widget.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PaintRequest {
    pub rect: Option<Rect>,
    pub hint: PaintHint,
}

impl PaintRequest {
    /// The union of two requests. The stronger refresh hint wins.
    pub fn merge(self, other: PaintRequest) -> PaintRequest {
        let rect = match (self.rect, other.rect) {
            (Some(a), Some(b)) => Some(a.union(&b)),
            _ => None,
        };
        let hint = if other.hint.mode().rank() > self.hint.mode().rank() {
            other.hint
        } else {
            self.hint
        };
        PaintRequest { rect, hint }
    }
}

/// Context for [`Widget::layout`].
pub struct LayoutCx<'a> {
    pub theme: &'a Theme,
    pub fonts: &'a mut FontSystem,
}

/// Context for [`Widget::paint`].
///
/// Coordinates in the methods are absolute (screen pixels). Everything drawn
/// outside [`PaintCx::clip`] is dropped, so a widget can draw its full bounds
/// without checking the clip.
pub struct PaintCx<'a> {
    pub theme: &'a Theme,
    pub fonts: &'a mut FontSystem,
    pub icons: &'a mut IconCache,
    pub(crate) target: &'a mut Pixmap,
    /// Absolute position of the target's top-left pixel.
    pub(crate) origin: Point,
    pub(crate) clip: Rect,
    pub(crate) bounds: Rect,
    pub(crate) focused: bool,
    pub(crate) focus: Option<WidgetId>,
}

impl PaintCx<'_> {
    /// The widget's absolute bounds.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The area being repainted, in absolute coordinates.
    pub fn clip(&self) -> Rect {
        self.clip
    }

    /// True when this widget has focus.
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn color(&self, tone: Tone) -> Color {
        self.theme.color(tone)
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        let local = self.local_rect(rect);
        self.target.fill_rect(local, color);
    }

    pub fn fill_rrect(&mut self, rect: Rect, radius: u32, color: Color) {
        let local = self.local_rect(rect);
        draw::fill_rrect(self.target, local, radius, color);
    }

    /// A border drawn inside `rect`.
    pub fn border(&mut self, rect: Rect, radius: u32, width: u32, color: Color) {
        let local = self.local_rect(rect);
        draw::border(self.target, local, radius, width, color);
    }

    /// Draws a laid out text with its top-left corner at `at`.
    pub fn text(&mut self, layout: &TextLayout, at: Point) {
        let local = self.local(at);
        self.fonts.draw_layout(&mut *self.target, layout, local);
    }

    /// Copies `src` with its top-left corner at `at`.
    pub fn blit_pixmap(&mut self, src: &Pixmap, at: Point) {
        let local = self.local(at);
        draw::blit(&mut *self.target, src, Rect::from_size(src.size()), local);
    }

    /// Composites `src` over the target with its top-left corner at `at`.
    pub fn blit_alpha(&mut self, src: &AlphaPixmap, at: Point) {
        let local = self.local(at);
        draw::blit_alpha(&mut *self.target, src, Rect::from_size(src.size()), local);
    }

    fn local(&self, p: Point) -> Point {
        Point::new(p.x - self.origin.x, p.y - self.origin.y)
    }

    fn local_rect(&self, r: Rect) -> Rect {
        let p = self.local(Point::new(r.x, r.y));
        Rect::new(p.x, p.y, r.w, r.h)
    }
}

/// Context for [`Widget::event`]. Requests made here take effect after the
/// handler returns.
pub struct EventCx<'a> {
    pub(crate) ctx: &'a mut Ctx,
    pub(crate) out: &'a mut Vec<Out>,
    pub(crate) bounds: Rect,
    pub(crate) focused: bool,
    pub(crate) paint: Option<PaintRequest>,
    pub(crate) layout: bool,
    pub(crate) focus: bool,
}

impl EventCx<'_> {
    /// The widget's absolute bounds.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    /// Repaints the whole widget.
    pub fn request_paint(&mut self) {
        self.request_paint_with(PaintHint::Ui);
    }

    /// Repaints the whole widget with the given refresh hint.
    pub fn request_paint_with(&mut self, hint: PaintHint) {
        self.add_paint(PaintRequest { rect: None, hint });
    }

    /// Repaints part of the widget. `rect` is relative to the widget.
    pub fn request_paint_rect(&mut self, rect: Rect, hint: PaintHint) {
        self.add_paint(PaintRequest {
            rect: Some(rect),
            hint,
        });
    }

    /// Re-runs layout for the window before the next paint.
    pub fn request_layout(&mut self) {
        self.layout = true;
    }

    /// Moves focus to this widget after the event.
    pub fn request_focus(&mut self) {
        self.focus = true;
    }

    /// Queues a named event for the app to deliver.
    pub fn emit(&mut self, ev: Event) {
        self.out.push(Out::Event(ev));
    }

    /// Queues an app action.
    pub fn action(&mut self, action: AppAction) {
        self.out.push(Out::Action(action));
    }

    /// The core context: scheduler, dirty queue and settings.
    pub fn ctx(&mut self) -> &mut Ctx {
        self.ctx
    }

    fn add_paint(&mut self, req: PaintRequest) {
        self.paint = Some(match self.paint {
            Some(prev) => prev.merge(req),
            None => req,
        });
    }
}
