//! The window stack, mirroring `UIManager`'s `_window_stack` and its
//! `show`, `close`, `sendEvent`, `broadcastEvent`, `topdown_widgets_iter`
//! and the `_repaint` paint plan (`frontend/ui/uimanager.lua`).

use std::collections::BTreeSet;

use crate::ctx::{Ctx, WindowRequest};
use crate::dirty::{DirtyQueue, Refresh};
use crate::handler::EventHandler;
use crate::Event;

/// A window on the stack. Windows are boxed handlers, so the stack holds
/// windows of different types.
pub type Window = Box<dyn EventHandler>;

/// Identifies a window on the stack. Ids are never reused within a `Ctx`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowId(u64);

impl WindowId {
    pub(crate) fn from_raw(raw: u64) -> Self {
        WindowId(raw)
    }

    /// The number behind the id, for logs and the Lua layer.
    pub fn raw(self) -> u64 {
        self.0
    }
}

/// Window flags, and the refresh to request when the window is shown.
///
/// - `modal`: the window stays above normal windows.
/// - `toast`: the window never stops event propagation (KOReader's `Notification`).
/// - `covers_fullscreen`: windows below it need no repaint.
/// - `always_active`: the window receives events below the top window (KOReader's `is_always_active`).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ShowOpts {
    pub modal: bool,
    pub toast: bool,
    pub covers_fullscreen: bool,
    pub always_active: bool,
    pub refresh: Option<Refresh>,
}

struct Entry {
    id: WindowId,
    opts: ShowOpts,
    window: Window,
}

/// Windows from bottom (index 0) to top.
#[derive(Default)]
pub struct WindowStack {
    entries: Vec<Entry>,
}

impl WindowStack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn contains(&self, id: WindowId) -> bool {
        self.index_of(id).is_some()
    }

    /// The topmost window, or `None` when the stack is empty.
    pub fn top(&self) -> Option<WindowId> {
        self.entries.last().map(|entry| entry.id)
    }

    /// All window ids, top first (`topdown_widgets_iter`).
    pub fn ids_top_down(&self) -> Vec<WindowId> {
        self.entries.iter().rev().map(|entry| entry.id).collect()
    }

    pub fn options(&self, id: WindowId) -> Option<ShowOpts> {
        self.index_of(id).map(|index| self.entries[index].opts)
    }

    pub fn window_mut(&mut self, id: WindowId) -> Option<&mut Window> {
        let index = self.index_of(id)?;
        Some(&mut self.entries[index].window)
    }

    /// `UIManager:show(widget, refreshtype, refreshregion)`.
    ///
    /// The window goes where `UIManager:show` puts it. It receives `Show`, and
    /// its dirty flag and refresh are queued. Changes that the window's
    /// handlers queue are applied before this returns.
    pub fn show(&mut self, window: impl EventHandler + 'static, opts: ShowOpts, ctx: &mut Ctx) -> WindowId {
        let id = ctx.alloc_window_id();
        self.insert(id, Box::new(window), opts, ctx);
        self.apply_requests(ctx);
        id
    }

    /// `UIManager:close(widget, refreshtype, refreshregion)`.
    ///
    /// The window receives `FlushSettings` and `CloseWidget`, then leaves the
    /// stack. Windows that are uncovered from the topmost full-screen window
    /// down are marked dirty, and the optional refresh is queued. Returns the
    /// removed window, or `None` when it was not on the stack.
    pub fn close(&mut self, id: WindowId, refresh: Option<Refresh>, ctx: &mut Ctx) -> Option<Window> {
        let removed = self.close_now(id, refresh, ctx);
        self.apply_requests(ctx);
        removed
    }

    /// Applies the show and close requests that handlers queued on `ctx`.
    /// The app calls this after running tasks, and each delivery calls it
    /// after its handler returns.
    pub fn apply_requests(&mut self, ctx: &mut Ctx) {
        while let Some(request) = ctx.requests.pop_front() {
            match request {
                WindowRequest::Show { id, window, opts } => self.insert(id, window, opts, ctx),
                WindowRequest::Close { id, refresh } => {
                    self.close_now(id, refresh, ctx);
                }
            }
        }
    }

    /// `UIManager:setDirty(widget, refreshtype, refreshregion)`.
    ///
    /// Marks the window for repaint when it is on the stack. The refresh is
    /// queued either way, as KOReader does when the widget is not a window.
    pub fn set_dirty(&self, window: Option<WindowId>, refresh: Option<Refresh>, ctx: &mut Ctx) {
        match window {
            Some(id) if self.contains(id) => ctx.dirty.set_dirty(Some(id), refresh),
            _ => ctx.dirty.set_dirty(None, refresh),
        }
    }

    /// `UIManager:setDirty("all", refreshtype, refreshregion)`: marks every window.
    pub fn set_dirty_all(&self, refresh: Option<Refresh>, ctx: &mut Ctx) {
        for entry in &self.entries {
            ctx.dirty.mark(entry.id);
        }
        ctx.dirty.set_dirty(None, refresh);
    }

    /// `UIManager:sendEvent(event)`. Returns true when a handler consumed the event.
    ///
    /// 1. Toasts above the topmost non-toast window get the event, top
    ///    first. Their return value is ignored.
    /// 2. The topmost non-toast window gets it. If it returns true, delivery stops.
    /// 3. Windows below that one get it only when `always_active` is set. The
    ///    first one that returns true stops delivery. Other windows below a
    ///    modal get nothing.
    ///
    /// Windows that a handler closes are skipped. Windows that a handler
    /// shows during step 3 are visited in step 3, as in KOReader. Windows
    /// shown during step 1 are not visited.
    pub fn send(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        let mut checked = BTreeSet::new();
        let mut top = None;
        for id in self.ids_top_down() {
            if !self.contains(id) {
                continue;
            }
            checked.insert(id);
            if self.is_toast(id) {
                self.deliver(id, ev, ctx);
            } else {
                top = Some(id);
                break;
            }
        }
        if let Some(top) = top {
            if self.deliver(top, ev, ctx) {
                return true;
            }
        }
        while let Some(id) = self.topmost_unchecked(&checked) {
            checked.insert(id);
            if self.is_always_active(id) && self.deliver(id, ev, ctx) {
                return true;
            }
        }
        false
    }

    /// `UIManager:broadcastEvent(event)`: every window gets the event, top
    /// first, and nothing stops early. Returns true when any handler returned true.
    pub fn broadcast(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        let mut checked = BTreeSet::new();
        let mut handled = false;
        while let Some(id) = self.topmost_unchecked(&checked) {
            checked.insert(id);
            if self.deliver(id, ev, ctx) {
                handled = true;
            }
        }
        handled
    }

    /// The windows to repaint, bottom to top, as `_repaint` picks them. Painting
    /// starts at the topmost full-screen window. Once one window is painted,
    /// every window above it is painted too. The paint flags of the painted
    /// windows are cleared.
    pub fn paint_plan(&self, dirty: &mut DirtyQueue) -> Vec<WindowId> {
        let start = self
            .entries
            .iter()
            .rposition(|entry| entry.opts.covers_fullscreen)
            .unwrap_or(0);
        let mut painting = false;
        let mut plan = Vec::new();
        for entry in &self.entries[start..] {
            if painting || dirty.is_marked(entry.id) {
                painting = true;
                dirty.unmark(entry.id);
                plan.push(entry.id);
            }
        }
        plan
    }

    fn index_of(&self, id: WindowId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id == id)
    }

    fn is_toast(&self, id: WindowId) -> bool {
        self.index_of(id)
            .is_some_and(|index| self.entries[index].opts.toast)
    }

    fn is_always_active(&self, id: WindowId) -> bool {
        self.index_of(id)
            .is_some_and(|index| self.entries[index].opts.always_active)
    }

    fn topmost_unchecked(&self, checked: &BTreeSet<WindowId>) -> Option<WindowId> {
        self.entries
            .iter()
            .rev()
            .map(|entry| entry.id)
            .find(|id| !checked.contains(id))
    }

    /// Calls the window's handler, then applies the requests it queued.
    fn deliver(&mut self, id: WindowId, ev: &Event, ctx: &mut Ctx) -> bool {
        let Some(index) = self.index_of(id) else {
            return false;
        };
        let consumed = self.entries[index].window.handle(ev, ctx);
        self.apply_requests(ctx);
        consumed
    }

    /// The insertion rule from `UIManager:show`, scanning from the top:
    /// - a toast goes above the topmost toast;
    /// - any other window goes above the topmost non-toast window, unless
    ///   that window is modal and the new one is not.
    ///
    /// Without a match, the window goes to the bottom.
    fn insert_position(&self, opts: &ShowOpts) -> usize {
        for index in (0..self.entries.len()).rev() {
            let top = &self.entries[index].opts;
            if top.toast {
                if opts.toast {
                    return index + 1;
                }
            } else if opts.modal || !top.modal {
                return index + 1;
            }
        }
        0
    }

    fn insert(&mut self, id: WindowId, window: Window, opts: ShowOpts, ctx: &mut Ctx) {
        let position = self.insert_position(&opts);
        self.entries.insert(position, Entry { id, opts, window });
        ctx.dirty.set_dirty(Some(id), opts.refresh);
        self.entries[position].window.handle(&Event::new("Show"), ctx);
    }

    fn close_now(&mut self, id: WindowId, refresh: Option<Refresh>, ctx: &mut Ctx) -> Option<Window> {
        let index = self.index_of(id)?;
        self.entries[index]
            .window
            .handle(&Event::new("FlushSettings"), ctx);
        self.entries[index].window.handle(&Event::new("CloseWidget"), ctx);
        let entry = self.entries.remove(index);
        ctx.dirty.unmark(id);
        let start = self
            .entries
            .iter()
            .rposition(|e| e.opts.covers_fullscreen)
            .unwrap_or(0);
        for uncovered in &self.entries[start..] {
            ctx.dirty.mark(uncovered.id);
        }
        if let Some(refresh) = refresh {
            ctx.dirty.refresh(refresh.mode, refresh.region, refresh.dither);
        }
        Some(entry.window)
    }
}
