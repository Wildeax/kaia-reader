//! Gesture replay through the harness: swipes, taps and frame output.

mod common;

use common::*;
use kaia_gfx::{Point, Size};
use kaia_ui::Harness;

const SCREEN: Size = Size::new(300, 300);

#[test]
fn a_swipe_reaches_the_widget_under_its_start_point() {
    let log = new_log();
    let root = Holder {
        children: Vec::new(),
        size: SCREEN,
        name: "root",
        log: log.clone(),
        handle: true,
    };
    let mut h = Harness::new(root, SCREEN, &theme(false), &assets());
    log.borrow_mut().clear();

    assert!(h.swipe(Point::new(20, 20), Point::new(20, 220)));
    let seen = log.borrow().clone();
    assert!(
        seen.iter()
            .any(|l| l.starts_with("root") && (l.ends_with("swipe") || l.ends_with("pan"))),
        "{seen:?}"
    );
}

#[test]
fn a_frame_after_a_tap_queues_nothing_when_the_tap_changed_nothing() {
    let log = new_log();
    let root = Holder {
        children: Vec::new(),
        size: SCREEN,
        name: "root",
        log: log.clone(),
        handle: true,
    };
    let mut h = Harness::new(root, SCREEN, &theme(false), &assets());
    h.frame();
    h.tap(Point::new(5, 5));
    h.frame();
    assert!(h.refresh_requests().is_empty(), "{:?}", h.refresh_requests());
}
