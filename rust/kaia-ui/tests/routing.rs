//! Gesture hit testing, bubbling, named event order and key focus.

mod common;

use common::*;
use kaia_events::Event;
use kaia_gfx::{Point, Size};
use kaia_input::Key;
use kaia_ui::widget::Handled;
use kaia_ui::{AppAction, Harness, Out, WidgetPod};

const SCREEN: Size = Size::new(300, 300);

fn taps(log: &Log) -> Vec<String> {
    log.borrow()
        .iter()
        .filter(|l| l.ends_with("tap"))
        .cloned()
        .collect()
}

/// A 300 x 300 root holding two 100 x 100 children stacked vertically.
/// Returns the harness and the ids of the children.
fn stacked(
    log: &Log,
    a_handles: bool,
    b_handles: bool,
    root_handles: bool,
) -> (Harness, kaia_ui::WidgetId, kaia_ui::WidgetId) {
    let mut a = Tapper::new("A", 100, 100, log);
    a.handle = a_handles;
    let mut b = Tapper::new("B", 100, 100, log);
    b.handle = b_handles;
    let a_pod = WidgetPod::new(a).focusable(true);
    let b_pod = WidgetPod::new(b).focusable(true);
    let (a_id, b_id) = (a_pod.id(), b_pod.id());
    let root = Holder {
        children: vec![a_pod, b_pod],
        size: SCREEN,
        name: "root",
        log: log.clone(),
        handle: root_handles,
    };
    let h = Harness::new(root, SCREEN, &theme(false), &assets());
    // The window sends a Show event when it joins the stack. Tests ignore it.
    log.borrow_mut().clear();
    (h, a_id, b_id)
}

#[test]
fn tap_reaches_the_deepest_widget_then_bubbles_to_the_parent() {
    let log = new_log();
    let (mut h, _, _) = stacked(&log, false, false, false);
    assert!(!h.tap(point(50, 50)));
    assert_eq!(taps(&log), vec!["A tap", "root tap"]);
}

#[test]
fn a_handled_tap_stops_at_the_child() {
    let log = new_log();
    let (mut h, _, _) = stacked(&log, true, false, false);
    assert!(h.tap(point(50, 50)));
    assert_eq!(taps(&log), vec!["A tap"]);
}

#[test]
fn tap_below_the_first_child_hits_the_second() {
    let log = new_log();
    let (mut h, _, _) = stacked(&log, false, false, true);
    assert!(h.tap(point(50, 150)));
    assert_eq!(taps(&log), vec!["B tap", "root tap"]);
}

#[test]
fn tap_outside_every_child_reaches_only_the_root() {
    let log = new_log();
    let (mut h, _, _) = stacked(&log, false, false, false);
    h.tap(point(250, 250));
    assert_eq!(taps(&log), vec!["root tap"]);
}

#[test]
fn named_events_go_in_paint_order_until_handled() {
    let log = new_log();
    let (mut h, _, _) = stacked(&log, false, false, false);
    assert!(!h.send(Event::new("ping")));
    let named: Vec<String> = log
        .borrow()
        .iter()
        .filter(|l| l.contains("named"))
        .cloned()
        .collect();
    assert_eq!(named, vec!["root named ping", "A named ping", "B named ping"]);

    log.borrow_mut().clear();
    let log2 = new_log();
    let (mut h2, _, _) = stacked(&log2, true, false, false);
    assert!(h2.send(Event::new("ping")));
    let named: Vec<String> = log2
        .borrow()
        .iter()
        .filter(|l| l.contains("named"))
        .cloned()
        .collect();
    assert_eq!(named, vec!["root named ping", "A named ping"]);
}

#[test]
fn keys_go_to_the_focused_widget_and_bubble_up() {
    let log = new_log();
    let (mut h, a, _) = stacked(&log, false, false, false);
    // Page-turn with no focus starts at the first focusable widget.
    assert_eq!(h.key(Key::LPgFwd), Handled::Yes);
    assert_eq!(h.focus(), Some(a));
    log.borrow_mut().clear();

    assert_eq!(h.key(Key::Press), Handled::No);
    assert_eq!(*log.borrow(), vec!["A key Press", "root key Press"]);
}

#[test]
fn page_turn_moves_focus_forward_and_back_with_wrap() {
    let log = new_log();
    let (mut h, a, b) = stacked(&log, false, false, false);
    h.key(Key::RPgFwd);
    assert_eq!(h.focus(), Some(a));
    h.key(Key::RPgFwd);
    assert_eq!(h.focus(), Some(b));
    h.key(Key::LPgFwd);
    assert_eq!(h.focus(), Some(a), "forward wraps to the first widget");
    h.key(Key::RPgBack);
    assert_eq!(h.focus(), Some(b), "back wraps to the last widget");
    assert!(log.borrow().iter().any(|l| l == "A lost"));
    assert!(log.borrow().iter().any(|l| l == "B gained"));
}

#[test]
fn a_handled_key_keeps_focus_where_it_is() {
    let log = new_log();
    let (mut h, a, _) = stacked(&log, true, false, false);
    h.key(Key::LPgFwd);
    assert_eq!(h.focus(), Some(a));
    // A handles the page-turn key itself, so focus does not move.
    assert_eq!(h.key(Key::RPgFwd), Handled::Yes);
    assert_eq!(h.focus(), Some(a));
}

#[test]
fn a_widget_can_take_focus_on_tap() {
    let log = new_log();
    let mut b = Tapper::new("B", 300, 300, &log);
    b.focusable = true;
    b.focus_on_tap = true;
    let b_pod = WidgetPod::new(b).focusable(true);
    let b_id = b_pod.id();
    let root = Holder {
        children: vec![b_pod],
        size: SCREEN,
        name: "root",
        log: log.clone(),
        handle: false,
    };
    let mut h = Harness::new(root, SCREEN, &theme(false), &assets());
    log.borrow_mut().clear();
    assert_eq!(h.focus(), None);
    h.tap(Point::new(10, 10));
    assert_eq!(h.focus(), Some(b_id));
    assert!(log.borrow().iter().any(|l| l == "B gained"));
}

/// Emits an event and an app action on tap, for the outbox test.
struct Emitter;

impl kaia_ui::Widget for Emitter {
    fn layout(&mut self, _cx: &mut kaia_ui::LayoutCx, c: kaia_ui::Constraints) -> Size {
        c.constrain(SCREEN)
    }

    fn event(&mut self, cx: &mut kaia_ui::EventCx, ev: &kaia_ui::UiEvent) -> Handled {
        if let kaia_ui::UiEvent::Gesture(kaia_input::Gesture::Tap { .. }) = ev {
            cx.emit(Event::new("book.open"));
            cx.action(AppAction::new("open"));
            return Handled::Yes;
        }
        Handled::No
    }
}

#[test]
fn widgets_queue_events_and_actions_for_the_app() {
    let mut h = Harness::new(Emitter, SCREEN, &theme(false), &assets());
    assert!(h.tap(Point::new(5, 5)));
    let out = h.outbox();
    assert_eq!(out.len(), 2, "{out:?}");
    assert_eq!(out[0], Out::Event(Event::new("book.open")));
    assert_eq!(out[1], Out::Action(AppAction::new("open")));
    assert!(h.outbox().is_empty(), "the outbox is drained");
}
