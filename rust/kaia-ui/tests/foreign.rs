//! ForeignWidget: a fake renderer checks layout, paint and event routing.
//! Regenerate with `KAIA_UPDATE_GOLDEN=1 cargo test -p kaia-ui --test foreign`.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::*;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::{Color, Pixmap, Point, Rect, Size};
use kaia_ui::components::{Column, Spacer};
use kaia_ui::foreign::{ForeignRenderer, ForeignWidget};
use kaia_ui::widget::{Handled, UiEvent};
use kaia_ui::{render, Harness, WidgetPod};

/// Fills its rectangle with a gray level and records every event it gets.
struct Fake {
    level: u8,
    size: Size,
    events: Rc<RefCell<Vec<String>>>,
    consume: bool,
}

impl ForeignRenderer for Fake {
    fn paint(&mut self, target: &mut Pixmap, rect: Rect) {
        target.fill_rect(rect, Color::gray(self.level));
    }

    fn event(&mut self, ev: &UiEvent) -> Handled {
        let label = match ev {
            UiEvent::Gesture(g) => format!("gesture {}", g.name()),
            other => format!("{other:?}"),
        };
        self.events.borrow_mut().push(label);
        if self.consume {
            Handled::Yes
        } else {
            Handled::No
        }
    }

    fn preferred_size(&self, max: Size) -> Size {
        Size::new(self.size.w.min(max.w), self.size.h.min(max.h))
    }
}

fn fake(level: u8, size: Size, events: &Rc<RefCell<Vec<String>>>, consume: bool) -> Fake {
    Fake {
        level,
        size,
        events: Rc::clone(events),
        consume,
    }
}

#[test]
fn the_widget_takes_the_size_the_renderer_prefers_within_the_constraints() {
    let t = theme(false);
    let events = Rc::new(RefCell::new(Vec::new()));
    let root = Column::new(vec![WidgetPod::new(ForeignWidget::new(fake(
        0x40,
        Size::new(300, 200),
        &events,
        false,
    )))
    .with_key("foreign")]);
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("foreign").expect("foreign widget is laid out");
    assert_eq!(r.size(), Size::new(300, 200));
}

#[test]
fn the_renderer_paints_its_rectangle() {
    let t = theme(false);
    let events = Rc::new(RefCell::new(Vec::new()));
    let root = Column::new(vec![WidgetPod::new(ForeignWidget::new(fake(
        0x40,
        Size::new(300, 200),
        &events,
        false,
    )))
    .with_key("foreign")]);
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    let pm = h.frame().clone();
    let r = h.find_rect("foreign").expect("foreign widget is laid out");
    assert_eq!(pm.get(r.x + 150, r.y + 100), Some(Color::gray(0x40)));
}

#[test]
fn gestures_reach_the_renderer_and_a_handled_one_stops_there() {
    let t = theme(false);
    let events = Rc::new(RefCell::new(Vec::new()));
    let root = Column::new(vec![WidgetPod::new(ForeignWidget::new(fake(
        0x40,
        Size::new(300, 200),
        &events,
        true,
    )))
    .with_key("foreign")]);
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("foreign").expect("foreign widget is laid out");
    assert!(h.tap(Point::new(r.x + 10, r.y + 10)));
    // The renderer also sees the touch that starts the tap, and the window's Show event before it.
    assert_eq!(events.borrow().last().map(String::as_str), Some("gesture tap"));
}

#[test]
fn an_unhandled_gesture_reaches_the_renderer_and_is_reported_unhandled() {
    let t = theme(false);
    let events = Rc::new(RefCell::new(Vec::new()));
    let root = Column::new(vec![WidgetPod::new(ForeignWidget::new(fake(
        0x40,
        Size::new(300, 200),
        &events,
        false,
    )))
    .with_key("foreign")]);
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("foreign").expect("foreign widget is laid out");
    let handled = h.tap(Point::new(r.x + 10, r.y + 10));
    assert!(
        !handled,
        "nothing handled the tap, so the window reports it unhandled"
    );
    let taps = events.borrow().iter().filter(|e| *e == "gesture tap").count();
    assert_eq!(taps, 1, "the renderer still saw the tap");
}

#[test]
fn foreign_widget_golden_matches() {
    let t = theme(false);
    let events = Rc::new(RefCell::new(Vec::new()));
    let root = Column::new(vec![
        WidgetPod::new(ForeignWidget::new(fake(
            0x60,
            Size::new(600, 400),
            &events,
            false,
        ))),
        WidgetPod::new(Spacer::new(1)),
    ]);
    let pm = render(root, SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("foreign_gray.png"), 0);
}
