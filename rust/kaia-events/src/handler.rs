//! Event handlers, the Rust side of KOReader's `EventListener` and
//! `WidgetContainer` (`frontend/ui/widget/eventlistener.lua`,
//! `frontend/ui/widget/container/widgetcontainer.lua`).
//!
//! KOReader calls `on<Name>` on a widget and stops at the first call that
//! returns true. `Handlers` keeps one closure per event name, and `Container`
//! passes an event to its children in order before it tries its own handler.

use std::collections::BTreeMap;

use crate::ctx::Ctx;
use crate::Event;

/// Something that receives events. Returning true stops propagation.
pub trait EventHandler {
    fn handle(&mut self, ev: &Event, ctx: &mut Ctx) -> bool;
}

impl<T: EventHandler + ?Sized> EventHandler for Box<T> {
    fn handle(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        (**self).handle(ev, ctx)
    }
}

/// A named event handler. Receives the event and the app context.
pub type HandlerFn = Box<dyn FnMut(&Event, &mut Ctx) -> bool>;

/// Handlers keyed by event name. `on("PageUpdate", f)` plays the role of
/// defining `onPageUpdate` on a KOReader widget. Registering a name again
/// replaces the earlier handler, as assigning a method does.
#[derive(Default)]
pub struct Handlers {
    by_name: BTreeMap<String, HandlerFn>,
}

impl Handlers {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers the handler for `name`. Returns `self` so calls can chain.
    pub fn on<F>(&mut self, name: &str, handler: F) -> &mut Self
    where
        F: FnMut(&Event, &mut Ctx) -> bool + 'static,
    {
        self.by_name.insert(name.to_owned(), Box::new(handler));
        self
    }

    pub fn has(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// Removes the handler for `name`. Returns false when there was none.
    pub fn remove(&mut self, name: &str) -> bool {
        self.by_name.remove(name).is_some()
    }
}

impl EventHandler for Handlers {
    /// `EventListener:handleEvent`: calls the handler for the event name, if
    /// there is one, and returns what it returns. A missing handler does not consume the event.
    fn handle(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        match self.by_name.get_mut(&ev.name) {
            Some(handler) => handler(ev, ctx),
            None => false,
        }
    }
}

/// A container with children and its own handlers, like `WidgetContainer`.
///
/// `propagateEvent` runs first: each child gets the event in insertion
/// order, and the first child that returns true stops it. The container's own
/// handlers run only when no child consumed the event.
#[derive(Default)]
pub struct Container {
    children: Vec<Box<dyn EventHandler>>,
    own: Handlers,
}

impl Container {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a child at the end. Children receive events in this order.
    pub fn push(&mut self, child: impl EventHandler + 'static) {
        self.children.push(Box::new(child));
    }

    pub fn len(&self) -> usize {
        self.children.len()
    }

    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Registers one of the container's own handlers.
    pub fn on<F>(&mut self, name: &str, handler: F) -> &mut Self
    where
        F: FnMut(&Event, &mut Ctx) -> bool + 'static,
    {
        self.own.on(name, handler);
        self
    }
}

impl EventHandler for Container {
    /// `WidgetContainer:handleEvent`.
    fn handle(&mut self, ev: &Event, ctx: &mut Ctx) -> bool {
        for child in &mut self.children {
            if child.handle(ev, ctx) {
                return true;
            }
        }
        self.own.handle(ev, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use crate::scheduler::ManualClock;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn ctx() -> Ctx {
        Ctx::new(Box::new(ManualClock::new()), Rect::new(0, 0, 600, 800))
    }

    #[test]
    fn named_handler_is_called_for_its_event_only() {
        let hits = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&hits);
        let mut h = Handlers::new();
        h.on("PageUpdate", move |ev, _| {
            log.borrow_mut().push(ev.name.clone());
            true
        });
        let mut ctx = ctx();
        assert!(h.handle(&Event::new("PageUpdate"), &mut ctx));
        assert!(!h.handle(&Event::new("Close"), &mut ctx));
        assert_eq!(*hits.borrow(), vec!["PageUpdate".to_owned()]);
    }

    #[test]
    fn registering_again_replaces_the_handler() {
        let mut h = Handlers::new();
        h.on("Tap", |_, _| true);
        h.on("Tap", |_, _| false);
        let mut ctx = ctx();
        assert!(!h.handle(&Event::new("Tap"), &mut ctx));
        assert!(h.remove("Tap"));
        assert!(!h.has("Tap"));
    }

    #[test]
    fn container_children_first_then_own_handler() {
        let log = Rc::new(RefCell::new(Vec::new()));
        let mut first = Handlers::new();
        let l1 = Rc::clone(&log);
        first.on("Ping", move |_, _| {
            l1.borrow_mut().push("first");
            false
        });
        let mut second = Handlers::new();
        let l2 = Rc::clone(&log);
        second.on("Ping", move |_, _| {
            l2.borrow_mut().push("second");
            true
        });
        let mut parent = Container::new();
        let l3 = Rc::clone(&log);
        parent.on("Ping", move |_, _| {
            l3.borrow_mut().push("parent");
            true
        });
        parent.push(first);
        parent.push(second);
        let mut ctx = ctx();
        assert!(parent.handle(&Event::new("Ping"), &mut ctx));
        // The second child consumes the event, so the parent's own handler does not run.
        assert_eq!(*log.borrow(), vec!["first", "second"]);
    }

    #[test]
    fn container_own_handler_runs_when_no_child_consumes() {
        let mut parent = Container::new();
        parent.on("Ping", |_, _| true);
        parent.push(Handlers::new());
        let mut ctx = ctx();
        assert!(parent.handle(&Event::new("Ping"), &mut ctx));
    }
}
