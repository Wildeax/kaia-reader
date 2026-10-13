//! [`WidgetPod`]: a widget with its place in the tree, its dirty state and
//! the routing code that walks the tree.
//!
//! Positions are stored relative to the parent, so moving a subtree does not
//! change its children. Absolute bounds are computed while walking.

use std::sync::atomic::{AtomicU64, Ordering};

use kaia_events::Ctx;
use kaia_gfx::text::FontSystem;
use kaia_gfx::{Point, Rect, Size};
use kaia_input::Key;

use crate::geometry::Constraints;
use crate::theme::Theme;
use crate::widget::{EventCx, Handled, LayoutCx, Out, PaintCx, PaintHint, PaintRequest, UiEvent, Widget};
use crate::window::Window;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Identifies a widget in the process. Ids are never reused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct WidgetId(u64);

/// Shared state for one event delivery.
pub(crate) struct Dispatch<'a> {
    pub ctx: &'a mut Ctx,
    pub out: &'a mut Vec<Out>,
    pub theme: &'a Theme,
    pub fonts: &'a mut FontSystem,
    /// The window that routes the event, for widgets that schedule later work.
    pub window: Option<Window>,
    /// Keys that widgets sent to the focused widget. The window routes them after the event.
    pub keys: Vec<Key>,
    pub focus: Option<WidgetId>,
    /// Set by a widget that asked for focus. The window applies it after routing.
    pub focus_request: Option<WidgetId>,
}

/// A widget in the tree. Containers own their children as pods.
pub struct WidgetPod {
    id: WidgetId,
    key: Option<String>,
    widget: Box<dyn Widget>,
    /// Origin relative to the parent, and size from the last layout.
    rect: Rect,
    visible: bool,
    focusable: bool,
    needs_layout: bool,
    paint: Option<PaintRequest>,
}

fn origin_of(r: Rect) -> Point {
    Point::new(r.x, r.y)
}

impl WidgetPod {
    pub fn new(widget: impl Widget + 'static) -> Self {
        Self {
            id: WidgetId(NEXT_ID.fetch_add(1, Ordering::Relaxed)),
            key: None,
            widget: Box::new(widget),
            rect: Rect::default(),
            visible: true,
            focusable: false,
            needs_layout: true,
            paint: None,
        }
    }

    /// Names this widget so [`Window::update`] and [`Window::find_rect`] can reach it.
    /// Keys need not be unique, but the first match in paint order wins.
    pub fn with_key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// The key set with [`WidgetPod::with_key`].
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// Lets this widget take focus through the page-turn keys.
    pub fn focusable(mut self, yes: bool) -> Self {
        self.focusable = yes;
        self
    }

    /// Hidden widgets take no space, paint nothing and get no events.
    pub fn visible(mut self, yes: bool) -> Self {
        self.visible = yes;
        self
    }

    pub fn id(&self) -> WidgetId {
        self.id
    }

    pub(crate) fn is_focusable(&self) -> bool {
        self.focusable
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Changes visibility. The window re-lays out on the next paint.
    pub fn set_visible(&mut self, yes: bool) {
        if self.visible != yes {
            self.visible = yes;
            self.needs_layout = true;
        }
    }

    /// The origin relative to the parent and the size from the last layout.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    pub fn size(&self) -> Size {
        self.rect.size()
    }

    pub fn flex(&self) -> u32 {
        self.widget.flex()
    }

    /// Sets the origin relative to the parent. Called by the parent after layout.
    pub fn set_origin(&mut self, p: Point) {
        self.rect.x = p.x;
        self.rect.y = p.y;
    }

    /// Lays out this widget and returns its size. A hidden widget gets size zero.
    pub fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        if !self.visible {
            self.rect.w = 0;
            self.rect.h = 0;
            self.needs_layout = false;
            return Size::new(0, 0);
        }
        let size = c.constrain(self.widget.layout(cx, c));
        self.rect.w = size.w;
        self.rect.h = size.h;
        self.needs_layout = false;
        size
    }

    /// Asks for a layout pass before the next paint or dispatch.
    pub(crate) fn request_layout(&mut self) {
        self.needs_layout = true;
    }

    /// Asks for a repaint of the whole widget with the default refresh.
    pub fn request_paint(&mut self) {
        self.add_paint(PaintRequest {
            rect: None,
            hint: PaintHint::Ui,
        });
    }

    fn add_paint(&mut self, req: PaintRequest) {
        self.paint = Some(match self.paint {
            Some(prev) => prev.merge(req),
            None => req,
        });
    }

    /// Applies the requests a keyed update made.
    pub(crate) fn apply_update(&mut self, paint: Option<PaintRequest>, layout: bool) {
        if let Some(req) = paint {
            self.add_paint(req);
        }
        if layout {
            self.needs_layout = true;
        }
    }

    /// The widget itself, for downcasting in [`Window::update`].
    pub(crate) fn widget_mut(&mut self) -> &mut dyn Widget {
        self.widget.as_mut()
    }

    /// The absolute bounds, given the absolute origin of the parent.
    pub(crate) fn abs(&self, origin: Point) -> Rect {
        Rect::new(
            origin.x + self.rect.x,
            origin.y + self.rect.y,
            self.rect.w,
            self.rect.h,
        )
    }

    /// True when any widget in the subtree asked for a layout.
    pub(crate) fn any_needs_layout(&mut self) -> bool {
        if self.needs_layout {
            return true;
        }
        self.widget
            .children_mut()
            .into_iter()
            .any(|c| c.any_needs_layout())
    }

    /// True when a repaint or a layout is pending anywhere in this subtree.
    pub(crate) fn has_requests(&mut self) -> bool {
        if self.paint.is_some() || self.needs_layout {
            return true;
        }
        self.widget.children_mut().into_iter().any(|c| c.has_requests())
    }

    /// Drops every pending paint request in the subtree.
    pub(crate) fn clear_paint(&mut self) {
        self.paint = None;
        for child in self.widget.children_mut() {
            child.clear_paint();
        }
    }

    /// Collects the dirty rectangles in absolute coordinates and clears the flags.
    pub(crate) fn collect_dirty(&mut self, origin: Point, out: &mut Vec<(Rect, PaintHint)>) {
        if !self.visible {
            self.paint = None;
            return;
        }
        let abs = self.abs(origin);
        if let Some(req) = self.paint.take() {
            let rect = match req.rect {
                Some(r) => r.translate(abs.x, abs.y).intersect(&abs),
                None => abs,
            };
            if !rect.is_empty() {
                out.push((rect, req.hint));
            }
        }
        let o = origin_of(abs);
        for child in self.widget.children_mut() {
            child.collect_dirty(o, out);
        }
    }

    /// Paints this widget and its children. Subtrees outside the clip are skipped.
    pub(crate) fn paint(&mut self, cx: &mut PaintCx, origin: Point) {
        if !self.visible {
            return;
        }
        let abs = self.abs(origin);
        if abs.intersect(&cx.clip).is_empty() {
            return;
        }
        let saved = (cx.bounds, cx.focused);
        cx.bounds = abs;
        cx.focused = cx.focus == Some(self.id);
        self.widget.paint(cx);
        let o = origin_of(abs);
        for child in self.widget.children_mut() {
            child.paint(cx, o);
        }
        (cx.bounds, cx.focused) = saved;
    }

    /// Calls this widget's handler and applies the requests it made.
    pub(crate) fn deliver(&mut self, d: &mut Dispatch, origin: Point, ev: &UiEvent) -> Handled {
        let bounds = self.abs(origin);
        let mut cx = EventCx {
            ctx: &mut *d.ctx,
            out: &mut *d.out,
            theme: d.theme,
            fonts: &mut *d.fonts,
            window: d.window.clone(),
            id: self.id,
            bounds,
            focused: d.focus == Some(self.id),
            paint: None,
            layout: false,
            focus: false,
            keys: Vec::new(),
        };
        let handled = self.widget.event(&mut cx, ev);
        let EventCx {
            paint,
            layout,
            focus,
            keys,
            ..
        } = cx;
        d.keys.extend(keys);
        if let Some(req) = paint {
            self.add_paint(req);
        }
        if layout {
            self.needs_layout = true;
        }
        if focus {
            d.focus_request = Some(self.id);
        }
        handled
    }

    /// Gesture routing. The deepest widget under `pos` is tried first, then
    /// its ancestors, so a handler that returns `No` lets the parent see the gesture.
    pub(crate) fn route_gesture(
        &mut self,
        d: &mut Dispatch,
        origin: Point,
        pos: Point,
        ev: &UiEvent,
    ) -> Handled {
        if !self.visible {
            return Handled::No;
        }
        let abs = self.abs(origin);
        if !abs.contains(pos) {
            return Handled::No;
        }
        let o = origin_of(abs);
        for child in self.widget.children_mut().into_iter().rev() {
            if child.route_gesture(d, o, pos, ev) == Handled::Yes {
                return Handled::Yes;
            }
        }
        self.deliver(d, origin, ev)
    }

    /// Key routing toward the focused widget. Returns `None` when `focus` is
    /// not in this subtree. Otherwise the key goes to the focused widget and
    /// then to each ancestor until one handles it.
    pub(crate) fn route_focused(
        &mut self,
        d: &mut Dispatch,
        origin: Point,
        focus: WidgetId,
        ev: &UiEvent,
    ) -> Option<Handled> {
        if !self.visible {
            return None;
        }
        if self.id == focus {
            return Some(self.deliver(d, origin, ev));
        }
        let o = origin_of(self.abs(origin));
        let mut found = false;
        for child in self.widget.children_mut() {
            if let Some(h) = child.route_focused(d, o, focus, ev) {
                found = true;
                if h == Handled::Yes {
                    return Some(Handled::Yes);
                }
                break;
            }
        }
        if found {
            Some(self.deliver(d, origin, ev))
        } else {
            None
        }
    }

    /// Named event routing. This widget goes first, then its children in
    /// paint order, until one handles the event.
    pub(crate) fn route_named(&mut self, d: &mut Dispatch, origin: Point, ev: &UiEvent) -> Handled {
        if !self.visible {
            return Handled::No;
        }
        if self.deliver(d, origin, ev) == Handled::Yes {
            return Handled::Yes;
        }
        let o = origin_of(self.abs(origin));
        for child in self.widget.children_mut() {
            if child.route_named(d, o, ev) == Handled::Yes {
                return Handled::Yes;
            }
        }
        Handled::No
    }

    /// Finds a pod by id. Returns it with the origin of its parent.
    pub(crate) fn find_mut(&mut self, id: WidgetId, origin: Point) -> Option<(&mut WidgetPod, Point)> {
        if self.id == id {
            return Some((self, origin));
        }
        let o = origin_of(self.abs(origin));
        for child in self.widget.children_mut() {
            if let Some(found) = child.find_mut(id, o) {
                return Some(found);
            }
        }
        None
    }

    /// Finds the first pod with `key` in paint order. Returns it with the
    /// origin of its parent.
    pub(crate) fn find_key(&mut self, key: &str, origin: Point) -> Option<(&mut WidgetPod, Point)> {
        if self.key.as_deref() == Some(key) {
            return Some((self, origin));
        }
        let o = origin_of(self.abs(origin));
        for child in self.widget.children_mut() {
            if let Some(found) = child.find_key(key, o) {
                return Some(found);
            }
        }
        None
    }

    /// Adds the focusable, visible widgets in paint order.
    pub(crate) fn collect_focusable(&mut self, out: &mut Vec<WidgetId>) {
        if !self.visible {
            return;
        }
        if self.focusable {
            out.push(self.id);
        }
        for child in self.widget.children_mut() {
            child.collect_focusable(out);
        }
    }
}
