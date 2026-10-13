//! A fake app with two full-screen windows, a menu, a modal dialog, a toast,
//! timers and a broadcast. Each expectation names the KOReader code it
//! mirrors. Paths are relative to the KOReader tree, and the functions are in
//! `frontend/ui/uimanager.lua` unless a path says otherwise.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use kaia_events::{
    Clock, Ctx, Event, EventHandler, Handlers, ManualClock, Rect, Refresh, RefreshMode, ShowOpts, WindowId,
    WindowStack,
};

type Log = Rc<RefCell<Vec<String>>>;

fn new_log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

fn take(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.borrow_mut())
}

fn screen() -> Rect {
    Rect::new(0, 0, 600, 800)
}

/// A window that records "name:event" for every event and consumes the events in `consume`.
struct Probe {
    name: &'static str,
    log: Log,
    consume: &'static [&'static str],
}

impl EventHandler for Probe {
    fn handle(&mut self, ev: &Event, _ctx: &mut Ctx) -> bool {
        self.log.borrow_mut().push(format!("{}:{}", self.name, ev.name));
        self.consume.contains(&ev.name.as_str())
    }
}

fn probe(name: &'static str, log: &Log, consume: &'static [&'static str]) -> Probe {
    Probe {
        name,
        log: Rc::clone(log),
        consume,
    }
}

/// One frame of the fake loop: run every task due at `now`, including tasks
/// that those tasks schedule for the same instant, then apply window changes.
/// This matches `UIManager:handleInput`, which loops `_checkTasks` while the task queue is dirty.
fn frame(ctx: &mut Ctx, stack: &mut WindowStack, clock: &ManualClock, at: f64) {
    clock.set(Duration::from_secs_f64(at));
    while ctx.run_due(clock.now()) > 0 {}
    stack.apply_requests(ctx);
}

#[test]
fn z_order_follows_uimanager_show() {
    let clock = ManualClock::new();
    let mut ctx = Ctx::new(Box::new(clock.clone()), screen());
    let mut stack = WindowStack::new();
    let log = new_log();

    let full = ShowOpts {
        covers_fullscreen: true,
        refresh: Some(Refresh::new(RefreshMode::Full)),
        ..Default::default()
    };
    let reader = stack.show(probe("reader", &log, &[]), full, &mut ctx);
    let menu = stack.show(probe("menu", &log, &[]), ShowOpts::default(), &mut ctx);
    let dialog = stack.show(
        probe("dialog", &log, &["Tap"]),
        ShowOpts {
            modal: true,
            ..Default::default()
        },
        &mut ctx,
    );
    // A toast shown while a modal is on top goes below the modal. UIManager:show
    // only puts a toast above a modal when a toast is already above that modal.
    let note = stack.show(
        probe("note", &log, &["Tap"]),
        ShowOpts {
            toast: true,
            ..Default::default()
        },
        &mut ctx,
    );

    assert_eq!(stack.ids_top_down(), vec![dialog, note, menu, reader]);
    assert_eq!(stack.top(), Some(dialog));
    // UIManager:show sends "Show" to each window it adds.
    assert_eq!(
        take(&log),
        vec!["reader:Show", "menu:Show", "dialog:Show", "note:Show"]
    );
}

#[test]
fn send_stops_at_the_modal_and_skips_windows_below() {
    let clock = ManualClock::new();
    let mut ctx = Ctx::new(Box::new(clock.clone()), screen());
    let mut stack = WindowStack::new();
    let log = new_log();
    stack.show(
        probe("reader", &log, &[]),
        ShowOpts {
            covers_fullscreen: true,
            ..Default::default()
        },
        &mut ctx,
    );
    stack.show(probe("menu", &log, &[]), ShowOpts::default(), &mut ctx);
    stack.show(
        probe("dialog", &log, &["Tap"]),
        ShowOpts {
            modal: true,
            ..Default::default()
        },
        &mut ctx,
    );
    take(&log);

    // UIManager:sendEvent: the topmost non-toast window gets the event first.
    // It consumes Tap, so delivery stops there.
    assert!(stack.send(&Event::new("Tap"), &mut ctx));
    assert_eq!(take(&log), vec!["dialog:Tap"]);

    // The dialog does not consume PageUpdate. Windows below a modal are
    // skipped unless they are always_active, so the reader and the menu get nothing.
    assert!(!stack.send(&Event::new("PageUpdate"), &mut ctx));
    assert_eq!(take(&log), vec!["dialog:PageUpdate"]);
}

#[test]
fn always_active_windows_below_a_modal_receive_events_until_one_consumes() {
    let clock = ManualClock::new();
    let mut ctx = Ctx::new(Box::new(clock.clone()), screen());
    let mut stack = WindowStack::new();
    let log = new_log();
    stack.show(
        probe("reader", &log, &[]),
        ShowOpts {
            covers_fullscreen: true,
            ..Default::default()
        },
        &mut ctx,
    );
    stack.show(probe("menu", &log, &[]), ShowOpts::default(), &mut ctx);
    stack.show(
        probe("dialog", &log, &[]),
        ShowOpts {
            modal: true,
            ..Default::default()
        },
        &mut ctx,
    );
    // A normal window inserted over the menu lands below the modal.
    stack.show(
        probe("clock", &log, &["PageUpdate"]),
        ShowOpts {
            always_active: true,
            ..Default::default()
        },
        &mut ctx,
    );
    take(&log);

    // UIManager:sendEvent, last loop: windows below the top are visited only when is_always_active.
    assert!(stack.send(&Event::new("PageUpdate"), &mut ctx));
    assert_eq!(take(&log), vec!["dialog:PageUpdate", "clock:PageUpdate"]);
}

#[test]
fn toasts_see_events_but_never_stop_them() {
    let clock = ManualClock::new();
    let mut ctx = Ctx::new(Box::new(clock.clone()), screen());
    let mut stack = WindowStack::new();
    let log = new_log();
    stack.show(
        probe("base", &log, &["Tap"]),
        ShowOpts {
            covers_fullscreen: true,
            ..Default::default()
        },
        &mut ctx,
    );
    stack.show(probe("menu", &log, &["Tap"]), ShowOpts::default(), &mut ctx);
    stack.show(
        probe("toast", &log, &["Tap"]),
        ShowOpts {
            toast: true,
            ..Default::default()
        },
        &mut ctx,
    );
    take(&log);

    // UIManager:sendEvent, first loop: the toast gets Tap and returns true,
    // which is ignored. The menu then gets it and consumes it, so the base window does not.
    assert!(stack.send(&Event::new("Tap"), &mut ctx));
    assert_eq!(take(&log), vec!["toast:Tap", "menu:Tap"]);
}

#[test]
fn broadcast_reaches_every_window_top_down() {
    let clock = ManualClock::new();
    let mut ctx = Ctx::new(Box::new(clock.clone()), screen());
    let mut stack = WindowStack::new();
    let log = new_log();
    stack.show(
        probe("reader", &log, &[]),
        ShowOpts {
            covers_fullscreen: true,
            ..Default::default()
        },
        &mut ctx,
    );
    stack.show(probe("menu", &log, &["Broadcast"]), ShowOpts::default(), &mut ctx);
    stack.show(
        probe("dialog", &log, &[]),
        ShowOpts {
            modal: true,
            ..Default::default()
        },
        &mut ctx,
    );
    take(&log);

    // UIManager:broadcastEvent: no window is skipped and no return value stops the walk.
    assert!(stack.broadcast(&Event::new("Broadcast"), &mut ctx));
    assert_eq!(
        take(&log),
        vec!["dialog:Broadcast", "menu:Broadcast", "reader:Broadcast"]
    );
    assert!(!stack.broadcast(&Event::new("Nobody"), &mut ctx));
}

#[test]
fn timers_run_in_order_and_close_windows_through_the_context() {
    let clock = ManualClock::new();
    let mut ctx = Ctx::new(Box::new(clock.clone()), screen());
    let mut stack = WindowStack::new();
    let log = new_log();
    let reader = stack.show(
        probe("reader", &log, &[]),
        ShowOpts {
            covers_fullscreen: true,
            ..Default::default()
        },
        &mut ctx,
    );
    let dialog = stack.show(
        probe("dialog", &log, &[]),
        ShowOpts {
            modal: true,
            ..Default::default()
        },
        &mut ctx,
    );
    // Get the start-up refreshes out of the way, as _repaint does at the end of a frame.
    let _ = stack.paint_plan(&mut ctx.dirty);
    ctx.dirty.take_frame(true);
    take(&log);

    let events = Rc::new(RefCell::new(Vec::new()));
    let push = |label: &'static str| {
        let events = Rc::clone(&events);
        Box::new(move |_ctx: &mut Ctx| events.borrow_mut().push(label))
    };
    // Two tasks due at the same time run in the order they were scheduled.
    ctx.scheduler.schedule_in(1.0, push("one"));
    ctx.scheduler.schedule_in(1.0, push("one-b"));
    // tickAfterNext runs in the second loop at the same instant.
    ctx.scheduler.tick_after_next(push("after-next"));
    // The dialog closes two seconds in. The task asks through Ctx, so the stack applies it.
    ctx.scheduler.schedule_in(
        2.0,
        Box::new(move |ctx: &mut Ctx| {
            ctx.close(dialog, Some(Refresh::new(RefreshMode::FlashUi)));
        }),
    );

    frame(&mut ctx, &mut stack, &clock, 0.0);
    assert_eq!(*events.borrow(), vec!["after-next"]);

    frame(&mut ctx, &mut stack, &clock, 1.0);
    assert_eq!(*events.borrow(), vec!["after-next", "one", "one-b"]);

    frame(&mut ctx, &mut stack, &clock, 2.0);
    assert_eq!(stack.ids_top_down(), vec![reader]);
    // CloseWidget and FlushSettings reach the window before it leaves the stack.
    assert_eq!(take(&log), vec!["dialog:FlushSettings", "dialog:CloseWidget"]);

    // The close marks the uncovered windows dirty, from the topmost full-screen window down.
    let painted = stack.paint_plan(&mut ctx.dirty);
    assert_eq!(painted, vec![reader]);
    let frame_refreshes = ctx.dirty.take_frame(!painted.is_empty());
    assert_eq!(
        frame_refreshes,
        vec![Refresh {
            mode: RefreshMode::FlashUi,
            region: Some(screen()),
            dither: false,
        }]
    );

    // Closing a window that is already gone changes nothing, but its refresh is still queued.
    assert!(stack.close(dialog, None, &mut ctx).is_none());
    stack.set_dirty(Some(dialog), Some(Refresh::new(RefreshMode::Ui)), &mut ctx);
    assert_eq!(ctx.dirty.queued().len(), 1);
    assert!(!ctx.dirty.is_marked(dialog));
}

#[test]
fn show_from_a_handler_waits_until_the_handler_returns() {
    let clock = ManualClock::new();
    let mut ctx = Ctx::new(Box::new(clock.clone()), screen());
    let mut stack = WindowStack::new();
    let log = new_log();

    let pending_during_handler = Rc::new(Cell::new(0usize));
    let child_id: Rc<Cell<Option<WindowId>>> = Rc::new(Cell::new(None));
    let mut launcher = Handlers::new();
    {
        let pending = Rc::clone(&pending_during_handler);
        let child = Rc::clone(&child_id);
        let child_log = Rc::clone(&log);
        launcher.on("Open", move |_ev, ctx| {
            let id = ctx.show(probe("child", &child_log, &[]), ShowOpts::default());
            child.set(Some(id));
            pending.set(ctx.pending_window_requests());
            true
        });
    }
    stack.show(
        launcher,
        ShowOpts {
            covers_fullscreen: true,
            ..Default::default()
        },
        &mut ctx,
    );
    take(&log);

    assert!(stack.send(&Event::new("Open"), &mut ctx));
    // While the handler ran, the window was queued and not yet on the stack.
    assert_eq!(pending_during_handler.get(), 1);
    let id = child_id.get().expect("handler ran");
    assert!(stack.contains(id));
    assert_eq!(stack.top(), Some(id));
    assert_eq!(take(&log), vec!["child:Show"]);
    assert_eq!(ctx.pending_window_requests(), 0);
}
