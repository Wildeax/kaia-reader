//! [`Window`]: a widget tree on the `kaia_events` window stack.
//!
//! The window owns focus, the app outbox and the paint state. It lays out
//! when a widget asks for it, paints only the dirty rectangles, and queues one
//! refresh per rectangle in the `DirtyQueue`.
//!
//! # Painting a stack
//!
//! The app paints the visible windows together with [`Window::paint_stack`],
//! bottom to top. Each window reports what it needs:
//!
//! - A *layout change* (a widget moved, resized, appeared or asked for layout)
//!   dirties the old and new rectangles of the widgets involved. Every window
//!   repaints those rectangles, because what was behind a vacated area belongs to
//!   the windows below.
//! - A *paint request* dirties a rectangle in that window. The window and the
//!   windows above it repaint the rectangle, clipped to it.
//! - A window that closes leaves its *footprint*, the area it had drawn on, to the
//!   windows that remain. They repaint it on the next pass.
//!
//! Every window repaints its whole tree inside each dirty rectangle, so a dialog
//! above a repainted page is drawn again over it, and its scrim dims fresh pixels
//! once. A window that has never painted draws without disturbing the ones below
//! and refreshes only what it drew.
//!
//! # Keys
//!
//! The app should send every key to [`Window::dispatch`] as [`UiEvent::Key`]
//! first. The page-turn keys move focus between focusable widgets when no widget
//! handles them. They return [`Handled::No`] when the window has no focusable
//! widget, and only then should the app turn them into its own `PageForward` and
//! `PageBack` events.
//!
//! A `Window` is a cheap handle (`Rc`), so the app keeps one clone for input
//! and passes another to the window stack. The stack calls back through
//! [`EventHandler`]. Borrows last for one call, and handlers cannot reach the
//! window, so the `RefCell` never sees a re-entrant borrow.

use std::cell::{Cell, RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;

use kaia_events::{Ctx, DirtyQueue, Event, EventHandler, ShowOpts, WindowId, WindowStack};
use kaia_gfx::text::FontSystem;
use kaia_gfx::{draw, Pixmap, Point, Rect, Size};
use kaia_input::{Gesture, Key, KeyEvent};

use crate::assets::{Assets, IconCache, SharedAssets};
use crate::geometry::Constraints;
use crate::pod::{Dispatch, WidgetId, WidgetPod};
use crate::theme::{Theme, Tone};
use crate::widget::{Handled, LayoutCx, Out, PaintCx, PaintHint, UiEvent, UpdateCx, Widget};

const ORIGIN: Point = Point::new(0, 0);

struct Inner {
    root: WidgetPod,
    theme: Theme,
    assets: SharedAssets,
    screen: Size,
    focus: Option<WidgetId>,
    outbox: Vec<Out>,
    /// The widget that got the touch a drag started on. Pans, hold pans and
    /// swipes go to it until the finger lifts, wherever the finger moves.
    captured: Option<WidgetId>,
    /// True until the first paint. The first paint draws on top of the frame
    /// without repainting the windows below.
    fresh: bool,
    /// Set by [`Window::invalidate`] and [`Window::resize`]: the next paint
    /// repaints the whole window and the windows below it.
    full: bool,
    /// Layout damage found while an event was routed, kept for the next paint.
    damage: Vec<Rect>,
    /// The bounding box of what this window has drawn on the frame.
    footprint: Option<Rect>,
}

/// A widget tree that can live on the window stack.
#[derive(Clone)]
pub struct Window {
    inner: Rc<RefCell<Inner>>,
    /// The id from `show`. It lives outside `inner` so a handler can read it
    /// while the window is delivering an event, when `inner` is borrowed.
    id: Rc<Cell<Option<WindowId>>>,
}

impl Window {
    /// A window for a screen of `screen` pixels. The first paint lays out
    /// `root` and draws the whole tree.
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
                captured: None,
                fresh: true,
                full: false,
                damage: Vec::new(),
                footprint: None,
            })),
            id: Rc::new(Cell::new(None)),
        }
    }

    /// Queues this window on the stack and remembers its id. The stack applies
    /// the request after the current handler returns.
    pub fn show(&self, ctx: &mut Ctx, opts: ShowOpts) -> WindowId {
        let id = ctx.show(self.clone(), opts);
        self.id.set(Some(id));
        id
    }

    /// The id from [`Window::show`], once it is set.
    pub fn id(&self) -> Option<WindowId> {
        self.id.get()
    }

    pub fn screen(&self) -> Size {
        self.inner.borrow().screen
    }

    /// The widget with keyboard focus, if any.
    pub fn focus(&self) -> Option<WidgetId> {
        self.inner.borrow().focus
    }

    /// Re-lays out and repaints everything on the next paint, including the
    /// windows below this one.
    pub fn invalidate(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.root.request_layout();
        inner.full = true;
    }

    /// Changes the screen size the window lays out for, for example after a
    /// rotation. The next paint repaints everything, and the frame it paints into
    /// must have the new size.
    pub fn resize(&self, screen: Size) {
        let mut inner = self.inner.borrow_mut();
        inner.screen = screen;
        inner.root.request_layout();
        inner.full = true;
        inner.footprint = None;
    }

    /// Routes an input event through the tree. Named events come in through
    /// [`EventHandler::handle`].
    ///
    /// Gestures go to the deepest widget under the point. A drag is captured by
    /// the widget under its `Touch`: the pans, hold pans and the swipe that follow
    /// go to that widget and its ancestors, wherever the finger is, until the
    /// finger lifts. Keys go to the focused widget and up through its ancestors.
    /// A page-turn key that nobody handles moves focus to the next focusable
    /// widget and is handled. With no focusable widget it is not handled, and the
    /// app can use it to turn the page.
    pub fn dispatch(&self, ev: UiEvent, ctx: &mut Ctx) -> Handled {
        let mut guard = self.inner.borrow_mut();
        let inner = &mut *guard;
        // Hit testing and focus need bounds, so lay out before routing. The
        // widgets that moved are repainted by the next paint.
        if inner.root.any_needs_layout() {
            let moved = relayout(inner);
            inner.damage.extend(moved);
        }
        let mut assets = inner.assets.borrow_mut();
        let mut d = Dispatch {
            ctx: &mut *ctx,
            out: &mut inner.outbox,
            theme: &inner.theme,
            fonts: &mut assets.fonts,
            window: Some(self.clone()),
            keys: Vec::new(),
            focus: inner.focus,
            focus_request: None,
        };
        let handled = match &ev {
            UiEvent::Gesture(g) => route_gesture(&mut inner.root, &mut inner.captured, &mut d, g, &ev),
            UiEvent::Key(k) => {
                let routed = d
                    .focus
                    .and_then(|f| inner.root.route_focused(&mut d, ORIGIN, f, &ev));
                match (routed, k.pressed, page_turn(k.key)) {
                    (Some(Handled::Yes), _, _) => Handled::Yes,
                    (_, true, Some(forward)) if move_focus(&mut inner.root, &mut d, forward) => Handled::Yes,
                    _ => Handled::No,
                }
            }
            UiEvent::Named(_) => inner.root.route_named(&mut d, ORIGIN, &ev),
            UiEvent::FocusGained | UiEvent::FocusLost => Handled::No,
        };
        if let Some(id) = d.focus_request.take() {
            set_focus(&mut inner.root, &mut d, id);
        }
        // Keys that widgets sent (the on-screen keyboard) go to the focused widget.
        for key in std::mem::take(&mut d.keys) {
            if let Some(f) = d.focus {
                let key_ev = UiEvent::Key(KeyEvent {
                    key,
                    pressed: true,
                    time_us: 0,
                });
                inner.root.route_focused(&mut d, ORIGIN, f, &key_ev);
            }
        }
        inner.focus = d.focus;
        drop(assets);
        mark_if_requested(self.id.get(), inner, ctx);
        handled
    }

    /// Runs `f` on the first widget with `key` that has type `T`. The closure
    /// returns nothing; it requests paint or layout through [`UpdateCx`], and
    /// the window applies those requests. Returns false when no widget with
    /// that key has type `T`. The `ctx` is the app context, for the dirty queue.
    pub fn update<T, F>(&self, ctx: &mut Ctx, key: &str, f: F) -> bool
    where
        T: Widget + 'static,
        F: FnOnce(&mut T, &mut UpdateCx<'_>),
    {
        let mut guard = self.inner.borrow_mut();
        let inner = &mut *guard;
        let done = match inner.root.find_key(key, ORIGIN) {
            Some((pod, _)) => run_update::<T, F>(pod, ctx, f),
            None => false,
        };
        mark_if_requested(self.id.get(), inner, ctx);
        done
    }

    /// Like [`Window::update`], for the widget with `id`. Used by scheduled
    /// work that holds a widget id rather than a key.
    pub(crate) fn update_id<T, F>(&self, ctx: &mut Ctx, id: WidgetId, f: F) -> bool
    where
        T: Widget + 'static,
        F: FnOnce(&mut T, &mut UpdateCx<'_>),
    {
        let mut guard = self.inner.borrow_mut();
        let inner = &mut *guard;
        let done = match inner.root.find_mut(id, ORIGIN) {
            Some((pod, _)) => run_update::<T, F>(pod, ctx, f),
            None => false,
        };
        mark_if_requested(self.id.get(), inner, ctx);
        done
    }

    /// The absolute bounds of the first widget with `key`, from the last layout.
    /// Returns `None` when no widget has the key.
    pub fn find_rect(&self, key: &str) -> Option<Rect> {
        let mut guard = self.inner.borrow_mut();
        let (pod, origin) = guard.root.find_key(key, ORIGIN)?;
        Some(pod.abs(origin))
    }

    /// Takes the events and actions that widgets sent to the app.
    pub fn drain_outbox(&self) -> Vec<Out> {
        std::mem::take(&mut self.inner.borrow_mut().outbox)
    }

    /// Paints this window alone. Same as [`Window::paint_stack`] with one window.
    /// Use it for a single window and for tests. An app with overlays should
    /// paint the whole stack, or the windows above and below will not follow.
    pub fn paint(&self, target: &mut Pixmap, dirty: &mut DirtyQueue) -> bool {
        Self::paint_stack(std::slice::from_ref(self), target, dirty)
    }

    /// Paints the visible windows, bottom to top, into `target` (the full
    /// frame) and queues one refresh per changed region in `dirty`. Returns true
    /// when anything was painted. See the module docs for what repaints what.
    ///
    /// `windows` is the stack from the bottom window that needs painting to the
    /// top: [`Window::visible_stack`] builds it. Pass the same list every time
    /// the stack has dirty windows, not only the dirty ones.
    pub fn paint_stack(windows: &[Window], target: &mut Pixmap, dirty: &mut DirtyQueue) -> bool {
        let mut list: Vec<&Window> = Vec::new();
        for w in windows {
            if !list.iter().any(|o| Rc::ptr_eq(&o.inner, &w.inner)) {
                list.push(w);
            }
        }
        let mut guards: Vec<RefMut<'_, Inner>> = list.iter().map(|w| w.inner.borrow_mut()).collect();
        let screen = Rect::from_size(target.size());
        for g in &guards {
            assert_eq!(target.size(), g.screen, "frame size must match the window");
        }

        // Phase 1: lay out, and find what each window needs repainted.
        let mut plans: Vec<Plan> = guards.iter_mut().map(|g| prepare(g, screen)).collect();
        let mut wide: Vec<(Rect, PaintHint)> = Vec::new();
        let mut boards: Vec<SharedAssets> = Vec::new();
        for g in &guards {
            if !boards.iter().any(|a| Rc::ptr_eq(a, &g.assets)) {
                boards.push(Rc::clone(&g.assets));
            }
        }
        for assets in &boards {
            for r in assets.borrow_mut().closed.drain(..) {
                let r = r.intersect(&screen);
                if !r.is_empty() {
                    wide.push((r, PaintHint::Ui));
                }
            }
        }
        for plan in &plans {
            wide.extend(plan.wide.iter().map(|r| (*r, PaintHint::Ui)));
        }
        let wide = merge_regions(wide);

        // Phase 2: paint bottom to top. `below` holds what the windows painted so
        // far changed, which the windows above must draw over again.
        let mut below: Vec<(Rect, PaintHint)> = Vec::new();
        let mut changed: Vec<(Rect, PaintHint)> = wide.clone();
        for (index, (inner, plan)) in guards.iter_mut().zip(plans.drain(..)).enumerate() {
            let regions = if plan.fresh {
                vec![(screen, PaintHint::Ui)]
            } else {
                let mut all = wide.clone();
                all.extend(below.iter().copied());
                all.extend(plan.own.iter().copied());
                merge_regions(all)
            };
            let reaches = inner
                .footprint
                .is_some_and(|f| regions.iter().any(|(r, _)| !r.intersect(&f).is_empty()));
            if !plan.fresh && !plan.full && plan.own.is_empty() && !reaches {
                continue;
            }
            let touched = {
                let inner = &mut **inner;
                let mut assets = inner.assets.borrow_mut();
                let Assets { fonts, icons, .. } = &mut *assets;
                let mut touched = Rect::default();
                for (rect, _) in &regions {
                    let t = paint_clip(
                        target,
                        *rect,
                        &inner.theme,
                        &mut *fonts,
                        &mut *icons,
                        &mut inner.root,
                        inner.focus,
                    );
                    touched = touched.union(&t);
                }
                touched
            };
            inner.footprint = match inner.footprint {
                Some(f) if !plan.full && !touched.is_empty() => Some(f.union(&touched)),
                _ if touched.is_empty() => inner.footprint.filter(|_| !plan.full),
                _ => Some(touched),
            };
            if plan.fresh {
                if !touched.is_empty() {
                    below.push((touched, PaintHint::Ui));
                    // The bottom window is the base of the screen: refresh all of it.
                    changed.push((if index == 0 { screen } else { touched }, PaintHint::Ui));
                }
            } else {
                for (rect, hint) in plan.own {
                    below.push((rect, hint));
                    changed.push((rect, hint));
                }
            }
        }

        for w in &list {
            if let Some(id) = w.id.get() {
                dirty.unmark(id);
            }
        }
        let changed = merge_regions(changed);
        for (rect, hint) in &changed {
            dirty.refresh(hint.mode(), Some(to_events(*rect)), false);
        }
        !changed.is_empty()
    }

    /// The windows to paint for the stack: from the topmost full-screen window
    /// up, bottom first. `lookup` finds the `Window` for an id on the stack.
    pub fn visible_stack(
        stack: &WindowStack,
        mut lookup: impl FnMut(WindowId) -> Option<Window>,
    ) -> Vec<Window> {
        let mut ids = stack.ids_top_down();
        let start = ids
            .iter()
            .position(|id| stack.options(*id).is_some_and(|o| o.covers_fullscreen))
            .unwrap_or(ids.len().saturating_sub(1));
        ids.truncate(start + 1);
        ids.reverse();
        ids.into_iter().filter_map(&mut lookup).collect()
    }

    /// Hands the area this window drew on to the windows below, which repaint
    /// it on the next pass. Called when the window closes.
    fn leave_footprint(&self) {
        let inner = self.inner.borrow();
        if let Some(f) = inner.footprint {
            inner.assets.borrow_mut().closed.push(f);
        }
    }
}

impl EventHandler for Window {
    fn handle(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        if ev.name == "CloseWidget" {
            self.leave_footprint();
        }
        self.dispatch(UiEvent::Named(ev.clone()), ctx) == Handled::Yes
    }
}

/// Runs a keyed update on `pod` when its widget has type `T`, then applies the
/// requests the closure made. Returns false when the type does not match.
fn run_update<T, F>(pod: &mut WidgetPod, ctx: &mut Ctx, f: F) -> bool
where
    T: Widget + 'static,
    F: FnOnce(&mut T, &mut UpdateCx<'_>),
{
    let Some(widget) = pod.widget_mut().as_any_mut().downcast_mut::<T>() else {
        return false;
    };
    let mut cx = UpdateCx::new(ctx);
    f(widget, &mut cx);
    let (paint, layout) = cx.finish();
    pod.apply_update(paint, layout);
    true
}

/// Marks the window for the app when the tree has pending work.
fn mark_if_requested(id: Option<WindowId>, inner: &mut Inner, ctx: &mut Ctx) {
    if let Some(id) = id {
        if inner.full || !inner.damage.is_empty() || inner.root.has_requests() {
            ctx.dirty.mark(id);
        }
    }
}

/// What one window needs painted in a pass.
struct Plan {
    /// Rectangles every window repaints: the old and new places of widgets
    /// that moved, and the whole screen after `invalidate`.
    wide: Vec<Rect>,
    /// Rectangles from this window's paint requests, with their refresh hints.
    /// This window and the windows above repaint them.
    own: Vec<(Rect, PaintHint)>,
    /// The first paint of the window.
    fresh: bool,
    /// The whole window repaints, so its footprint starts over.
    full: bool,
}

/// Lays the window out if needed and collects what it needs painted.
fn prepare(inner: &mut Inner, screen: Rect) -> Plan {
    let mut plan = Plan {
        wide: Vec::new(),
        own: Vec::new(),
        fresh: inner.fresh,
        full: false,
    };
    let moved = if inner.root.any_needs_layout() {
        relayout(inner)
    } else {
        Vec::new()
    };
    let pending = std::mem::take(&mut inner.damage);
    if inner.fresh {
        // The first paint draws the whole tree. Its pending requests are covered.
        inner.fresh = false;
        inner.full = false;
        inner.root.clear_paint();
        return plan;
    }
    if inner.full {
        inner.full = false;
        plan.full = true;
        plan.wide.push(screen);
        inner.root.clear_paint();
        return plan;
    }
    plan.wide = moved
        .into_iter()
        .chain(pending)
        .map(|r| r.intersect(&screen))
        .filter(|r| !r.is_empty())
        .collect();
    let mut own = Vec::new();
    inner.root.collect_dirty(ORIGIN, &mut own);
    plan.own = own
        .into_iter()
        .map(|(r, h)| (r.intersect(&screen), h))
        .filter(|(r, _)| !r.is_empty())
        .collect();
    plan
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

/// Lays the whole tree out for the window size. Returns the rectangles to
/// repaint because of the layout: the old and new bounds of every widget that
/// moved, resized, appeared or disappeared, and of every widget that asked for
/// the layout. Pending paint requests stay.
fn relayout(inner: &mut Inner) -> Vec<Rect> {
    let mut before = HashMap::new();
    inner.root.snapshot(ORIGIN, &mut before);
    {
        let mut assets = inner.assets.borrow_mut();
        let mut lc = LayoutCx {
            theme: &inner.theme,
            fonts: &mut assets.fonts,
        };
        inner.root.layout(&mut lc, Constraints::tight(inner.screen));
    }
    let mut after = HashMap::new();
    inner.root.snapshot(ORIGIN, &mut after);
    let mut moved = Vec::new();
    let mut add = |r: Rect| {
        if !r.is_empty() {
            moved.push(r);
        }
    };
    for (id, (rect, _)) in &after {
        match before.get(id) {
            Some((old, requested)) => {
                if old != rect || *requested {
                    add(*old);
                    add(*rect);
                }
            }
            None => add(*rect),
        }
    }
    for (id, (old, _)) in &before {
        if !after.contains_key(id) {
            add(*old);
        }
    }
    moved
}

/// Paints the tree clipped to `clip` (absolute coordinates). Returns the area
/// that widgets drew on, inside `clip`.
///
/// The clip is a scratch pixmap the size of the region. The region's current
/// pixels are copied in first, so anti-aliased edges blend with what is
/// already on the frame, and the drawn part is copied back. Widgets draw only
/// inside their own bounds, so nothing outside the returned area changes.
fn paint_clip(
    target: &mut Pixmap,
    clip: Rect,
    theme: &Theme,
    fonts: &mut FontSystem,
    icons: &mut IconCache,
    root: &mut WidgetPod,
    focus: Option<WidgetId>,
) -> Rect {
    let mut scratch = Pixmap::new(clip.w, clip.h, target.format());
    draw::blit(&mut scratch, &*target, clip, ORIGIN);
    let touched = {
        let mut cx = PaintCx {
            theme,
            fonts,
            icons,
            target: &mut scratch,
            origin: Point::new(clip.x, clip.y),
            region: clip,
            clip,
            bounds: clip,
            background: Tone::Surface,
            focused: false,
            focus,
            drew: false,
            touched: Rect::default(),
        };
        root.paint(&mut cx, ORIGIN);
        cx.touched.intersect(&clip)
    };
    if !touched.is_empty() {
        let src = Rect::new(touched.x - clip.x, touched.y - clip.y, touched.w, touched.h);
        draw::blit(target, &scratch, src, Point::new(touched.x, touched.y));
    }
    touched
}

/// Routes a gesture. A touch records the widget under it. The gestures of a
/// drag go to that widget until the finger lifts. A gesture without a captured
/// widget goes to the widget under the point where its sequence started.
fn route_gesture(
    root: &mut WidgetPod,
    captured: &mut Option<WidgetId>,
    d: &mut Dispatch,
    g: &Gesture,
    ev: &UiEvent,
) -> Handled {
    let (follows_touch, ends) = match g {
        Gesture::Pan { .. } | Gesture::HoldPan { .. } => (true, false),
        Gesture::PanRelease { .. }
        | Gesture::HoldRelease { .. }
        | Gesture::Swipe { .. }
        | Gesture::Multiswipe { .. } => (true, true),
        _ => (false, false),
    };
    match g {
        Gesture::Touch { pos, .. } => *captured = root.deepest_at(ORIGIN, *pos),
        Gesture::Tap { .. } | Gesture::DoubleTap { .. } => *captured = None,
        _ => {}
    }
    if follows_touch {
        let target = *captured;
        if ends {
            *captured = None;
        }
        if let Some(id) = target {
            if let Some(handled) = root.route_focused(d, ORIGIN, id, ev) {
                return handled;
            }
        }
    }
    let at = match g {
        Gesture::Pan { start_pos, .. } => *start_pos,
        Gesture::HoldPan {
            start_pos: Some(start),
            ..
        } => *start,
        other => other.pos(),
    };
    root.route_gesture(d, ORIGIN, at, ev)
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
/// backward at the last. Returns false when no widget is focusable.
fn move_focus(root: &mut WidgetPod, d: &mut Dispatch, forward: bool) -> bool {
    let mut order = Vec::new();
    root.collect_focusable(&mut order);
    if order.is_empty() {
        return false;
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
    true
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
