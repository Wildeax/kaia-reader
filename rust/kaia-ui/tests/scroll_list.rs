//! Whole-row scrolling keeps hidden controls out of focus and hit testing.

mod common;

use common::*;
use kaia_gfx::{Point, Rect, Size};
use kaia_ui::composite::{ScrollList, SettingsIndexRow};
use kaia_ui::{Harness, WidgetPod};

fn list(count: usize, screen: Size) -> Harness {
    let list = ScrollList::new((0..count).collect(), |index, _| {
        WidgetPod::new(SettingsIndexRow::new(
            format!("Setting {index}"),
            format!("pick:{index}"),
        ))
        .with_key(format!("row-{index}"))
        .focusable(true)
    });
    Harness::new(list, screen, &theme(false), &assets())
}

#[test]
fn swipes_reveal_rows_and_stop_at_both_ends() {
    let mut h = list(10, Size::new(600, 400));
    h.frame();
    assert!(h.find_rect("row-0").is_some());
    assert!(h.find_rect("row-3").is_none());
    h.swipe(Point::new(300, 350), Point::new(300, 50));
    h.frame();
    assert!(h.find_rect("row-0").is_none());
    assert!(h.outbox().is_empty(), "swiping must not activate a row");
    for _ in 0..10 {
        h.swipe(Point::new(300, 350), Point::new(300, 50));
        h.frame();
    }
    assert!(h.find_rect("row-9").is_some());
    for index in 0..10 {
        if let Some(row) = h.find_rect(&format!("row-{index}")) {
            assert_eq!(row.intersect(&Rect::new(0, 0, 600, 400)), row);
        }
    }
    for _ in 0..10 {
        h.swipe(Point::new(300, 50), Point::new(300, 350));
        h.frame();
    }
    assert!(h.find_rect("row-0").is_some());
    assert!(h.find_rect("row-9").is_none());
}

#[test]
fn horizontal_swipe_does_not_scroll_and_short_lists_stay_put() {
    let mut h = list(2, Size::new(600, 400));
    h.frame();
    let before = h.screen().clone();
    h.swipe(Point::new(500, 200), Point::new(100, 200));
    assert_eq!(h.frame().data(), before.data());
    h.swipe(Point::new(300, 350), Point::new(300, 50));
    assert_eq!(h.frame().data(), before.data());
}

#[test]
fn scrolling_dialog_golden() {
    use kaia_gfx::golden::assert_matches_golden;
    use kaia_ui::composite::SettingsDialog;
    let list = ScrollList::new((0..25).collect(), |index, _| {
        WidgetPod::new(SettingsIndexRow::new(format!("Setting {}", index + 1), "pick"))
    });
    let dialog = SettingsDialog::new("Settings", "Settings", WidgetPod::new(list), "close");
    let mut h = Harness::new(dialog, SCREEN, &theme(false), &assets());
    h.frame();
    h.swipe(Point::new(600, 1400), Point::new(600, 800));
    let frame = h.frame();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/settings_scrolled.png");
    assert_matches_golden(frame, &path, 14);
}

#[test]
fn page_button_focus_is_visible_and_press_activates_the_row() {
    use kaia_input::Key;
    use kaia_ui::widget::Out;
    let mut h = list(10, Size::new(600, 400));
    let before = h.frame().data().to_vec();
    h.key(Key::RPgFwd);
    assert_ne!(h.frame().data(), before);
    h.key(Key::Press);
    assert!(h
        .outbox()
        .iter()
        .any(|out| matches!(out, Out::Action(action) if action.id == "pick:0")));
}
