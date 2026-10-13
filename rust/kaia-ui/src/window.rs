//! [`Window`]: a widget tree on the `kaia_events` window stack.
//!
//! The window owns focus, the app outbox and the paint state. It lays out
//! when a widget asks for it, paints only the dirty rectangles, and queues one
//! refresh per rectangle in the `DirtyQueue`.
//!
//! A `Window` is a cheap handle (`Rc`), so the app keeps one clone for input
//! and passes another to the window stack. The stack calls back through
//! [`EventHandler`]. Borrows last for one call, and handlers cannot reach the
//! window, so the `RefCell` never sees a re-entrant borrow.

use std::cell::RefCell;
use std::rc::Rc;

use kaia_events::{Ctx, DirtyQueue, Event, EventHandler, ShowOpts, WindowId};
use kaia_gfx::text::FontSystem;
use kaia_gfx::{draw, Pixmap, Point, Rect, Size};
use kaia_input::Key;

use crate::assets::{Assets, IconCache, SharedAssets};
use crate::geometry::Constraints;
use crate::pod::{Dispatch, WidgetId, WidgetPod};
use crate::theme::Theme;
use crate::widget::{Handled, LayoutCx, Out, PaintCx, PaintHint, UiEvent, Widget};

const ORIGIN: Point = Point::new(0, 0);

struct Inner {
    root: WidgetPod,
    theme: Theme,
    assets: SharedAssets,
    screen: Size,
    focus: Option<WidgetId>,
    outbox: Vec<Out>,
    /// Set when the next paint must lay out the tree and repaint everything.
    full: bool,
    id: Option<WindowId>,
}

/// A widget tree that can live on the window stack.
#[derive(Clone)]
pub struct Window {
    inner: Rc<RefCell<Inner>>,
}

impl Window {
    /// A window for a screen of `screen` pixels. The first paint lays out
    /// `root` and repaints the whole screen.
    pub fn new(root: impl Widget + 'static, theme: Theme, assets: SharedAssets, screen: Size) -> Self {
        let mut root = WidgetPod::new(root);
        root.set_origin(ORIGIN);
        Self {
            inner: Rc::new(RefCell::new(Inner {
                root,
                theme,
                assets,
                screen,
                focus: None,
                outbox: Vec::new(),
                full: true,
                id: None,
            })),
        }
    }

    /// Queues this window on the stack and remembers its id. The stack applies
    /// the request after the current handler returns.
    pub fn show(&self, ctx: &mut Ctx, opts: ShowOpts) -> WindowId {
        let id = ctx.show(self.clone(), opts);
        self.inner.borrow_mut().id = Some(id);
        id
    }

    /// The id from [`Window::show`], once it is set.
    pub fn id(&self) -> Option<WindowId> {
        self.inner.borrow().id
    }

    pub fn screen(&self) -> Size {
        self.inner.borrow().screen
    }

    /// The widget with keyboard focus, if any.
    pub fn focus(&self) -> Option<WidgetId> {
        self.inner.borrow().focus
    }

    /// Re-lays out and repaints everything on the next paint.
    pub fn invalidate(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.root.request_layout();
        inner.full = true;
    }

    /// Routes an input event through the tree. Named events come in through
    /// [`EventHandler::handle`].
    pub fn dispatch(&self, ev: UiEvent, ctx: &mut Ctx) -> Handled {
        let mut guard = self.inner.borrow_mut();
        let inner = &mut *guard;
        // Hit testing and focus need bounds, so lay out before routing.
        // The next paint repaints everything, because the layout moved things.
        if inner.root.any_needs_layout() {
            relayout(inner);
            inner.full = true;
        }
        let mut d = Dispatch {
            ctx: &mut *ctx,
            out: &mut inner.outbox,
            focus: inner.focus,
            focus_request: None,
        };
        let handled = match &ev {
            UiEvent::Gesture(g) => inner.root.route_gesture(&mut d, ORIGIN, g.pos(), &ev),
            UiEvent::Key(k) => {
                let routed = d
                    .focus
                    .and_then(|f| inner.root.route_focused(&mut d, ORIGIN, f, &ev));
                match (routed, k.pressed, page_turn(k.key)) {
                    (Some(Handled::Yes), _, _) => Handled::Yes,
                    (_, true, Some(forward)) => {
                        move_focus(&mut inner.root, &mut d, forward);
                        Handled::Yes
                    }
                    _ => Handled::No,
                }
            }
            UiEvent::Named(_) => inner.root.route_named(&mut d, ORIGIN, &ev),
            UiEvent::FocusGained | UiEvent::FocusLost => Handled::No,
        };
        if let Some(id) = d.focus_request.take() {
            set_focus(&mut inner.root, &mut d, id);
        }
        inner.focus = d.focus;
        if let Some(id) = inner.id {
            if inner.root.has_requests() {
                ctx.dirty.mark(id);
            }
        }
        handled
    }

    /// Takes the events and actions that widgets sent to the app.
    pub fn drain_outbox(&self) -> Vec<Out> {
        std::mem::take(&mut self.inner.borrow_mut().outbox)
    }

    /// Paints the dirty rectangles into `target`, the full frame of this
    /// window, and queues one refresh per rectangle in `dirty`. Returns true
    /// when anything was painted.
    pub fn paint(&self, target: &mut Pixmap, dirty: &mut DirtyQueue) -> bool {
        let mut guard = self.inner.borrow_mut();
        let inner = &mut *guard;
        assert_eq!(target.size(), inner.screen, "frame size must match the window");
        let screen = Rect::from_size(inner.screen);
        let mut regions: Vec<(Rect, PaintHint)> = Vec::new();

        if inner.root.any_needs_layout() {
            relayout(inner);
            inner.full = true;
        }
        if inner.full {
            regions.push((screen, PaintHint::Ui));
            inner.full = false;
        }
        inner.root.collect_dirty(ORIGIN, &mut regions);

        if let Some(id) = inner.id {
            dirty.unmark(id);
        }
        let mut assets = inner.assets.borrow_mut();
        let Assets { fonts, icons } = &mut *assets;
        let mut painted = false;
        for (rect, hint) in merge_regions(regions) {
            let clip = rect.intersect(&screen);
            if clip.is_empty() {
                continue;
            }
            paint_clip(
                target,
                clip,
                &inner.theme,
                &mut *fonts,
                &mut *icons,
                &mut inner.root,
                inner.focus,
            );
            dirty.refresh(hint.mode(), Some(to_events(clip)), false);
            painted = true;
        }
        painted
    }
}

impl EventHandler for Window {
    fn handle(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        self.dispatch(UiEvent::Named(ev.clone()), ctx) == Handled::Yes
    }
}

/// Merges rectangles that overlap, so no pixel is painted twice. Blending an
/// anti-aliased edge twice would make it darker. The merged rectangle takes
/// the stronger refresh hint.
fn merge_regions(regions: Vec<(Rect, PaintHint)>) -> Vec<(Rect, PaintHint)> {
    let mut out: Vec<(Rect, PaintHint)> = Vec::new();
    for (mut rect, mut hint) in regions {
        while let Some(i) = out.iter().position(|(r, _)| !r.intersect(&rect).is_empty()) {
            let (other, other_hint) = out.swap_remove(i);
            rect = rect.union(&other);
            if other_hint.mode().rank() > hint.mode().rank() {
                hint = other_hint;
            }
        }
        out.push((rect, hint));
    }
    out
}

/// Lays the whole tree out for the window size and drops pending paint
/// requests, because the caller repaints everything after a layout.
fn relayout(inner: &mut Inner) {
    let mut assets = inner.assets.borrow_mut();
    let mut lc = LayoutCx {
        theme: &inner.theme,
        fonts: &mut assets.fonts,
    };
    inner.root.layout(&mut lc, Constraints::tight(inner.screen));
    inner.root.clear_paint();
}

/// Paints the tree clipped to `clip` (absolute coordinates).
///
/// The clip is a scratch pixmap the size of the region. The region's current
/// pixels are copied in first, so anti-aliased edges blend with what is
/// already on the frame, and the result is copied back. Drawing outside the
/// region writes to nothing.
fn paint_clip(
    target: &mut Pixmap,
    clip: Rect,
    theme: &Theme,
    fonts: &mut FontSystem,
    icons: &mut IconCache,
    root: &mut WidgetPod,
    focus: Option<WidgetId>,
) {
    let mut scratch = Pixmap::new(clip.w, clip.h, target.format());
    draw::blit(&mut scratch, &*target, clip, ORIGIN);
    {
        let mut cx = PaintCx {
            theme,
            fonts,
            icons,
            target: &mut scratch,
            origin: Point::new(clip.x, clip.y),
            clip,
            bounds: clip,
            focused: false,
            focus,
        };
        root.paint(&mut cx, ORIGIN);
    }
    draw::blit(
        target,
        &scratch,
        Rect::from_size(clip.size()),
        Point::new(clip.x, clip.y),
    );
}

fn page_turn(key: Key) -> Option<bool> {
    match key {
        Key::LPgFwd | Key::RPgFwd => Some(true),
        Key::LPgBack | Key::RPgBack => Some(false),
        _ => None,
    }
}

/// Moves focus to the next or previous focusable widget in paint order. Wraps
/// at both ends. With no focus yet, forward starts at the first widget and
/// backward at the last.
fn move_focus(root: &mut WidgetPod, d: &mut Dispatch, forward: bool) {
    let mut order = Vec::new();
    root.collect_focusable(&mut order);
    if order.is_empty() {
        return;
    }
    let len = order.len();
    let current = d.focus.and_then(|f| order.iter().position(|id| *id == f));
    let next = match (current, forward) {
        (None, true) => 0,
        (None, false) => len - 1,
        (Some(i), true) => (i + 1) % len,
        (Some(i), false) => (i + len - 1) % len,
    };
    set_focus(root, d, order[next]);
}

/// Moves focus to `id` if that widget is focusable. Repaints both widgets and
/// sends them the focus events. Focus requests made inside the focus handlers
/// are ignored.
fn set_focus(root: &mut WidgetPod, d: &mut Dispatch, id: WidgetId) {
    if d.focus == Some(id) {
        return;
    }
    if !root
        .find_mut(id, ORIGIN)
        .is_some_and(|(pod, _)| pod.is_focusable())
    {
        return;
    }
    if let Some(old) = d.focus {
        if let Some((pod, parent)) = root.find_mut(old, ORIGIN) {
            pod.request_paint();
            pod.deliver(d, parent, &UiEvent::FocusLost);
        }
    }
    d.focus = Some(id);
    if let Some((pod, parent)) = root.find_mut(id, ORIGIN) {
        pod.request_paint();
        pod.deliver(d, parent, &UiEvent::FocusGained);
    }
    d.focus_request = None;
}

/// Converts a pixel rectangle to the type `DirtyQueue` uses.
pub(crate) fn to_events(r: Rect) -> kaia_events::Rect {
    kaia_events::Rect::new(r.x, r.y, r.w as i32, r.h as i32)
}
