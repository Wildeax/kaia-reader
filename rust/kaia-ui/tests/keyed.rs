//! Keyed updates: reaching a nested widget by key, changing it, and checking
//! which rectangle the change repaints.

mod common;

use common::*;
use kaia_events::{Rect as ERect, Refresh, RefreshMode};
use kaia_gfx::{Rect, Size};
use kaia_ui::components::{Column, Label, Padding, ProgressBar};
use kaia_ui::controls::{Button, ButtonStyle};
use kaia_ui::{Harness, Insets, Tone, WidgetPod};

const SCREEN: Size = Size::new(300, 300);

fn refresh_of(r: Rect) -> Refresh {
    Refresh {
        mode: RefreshMode::Ui,
        region: Some(ERect::new(r.x, r.y, r.w as i32, r.h as i32)),
        dither: false,
    }
}

#[test]
fn a_keyed_label_changes_its_text_and_takes_the_new_size() {
    let t = theme(false);
    let label = Label::new("Short", t.font.body.clone(), Tone::Ink);
    let mut h = Harness::new(
        Column::new(vec![WidgetPod::new(label).with_key("title")]).shrink(),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let before = h.find_rect("title").expect("the keyed label is laid out");

    let found = h.update::<Label, _>("title", |label, cx| {
        label.set_text("A much longer title for the same label");
        cx.request_layout();
    });
    assert!(found);
    h.frame();
    let after = h.find_rect("title").expect("still laid out");
    assert!(after.w > before.w, "{} -> {}", before.w, after.w);
}

#[test]
fn an_update_repaints_only_the_keyed_widget() {
    let t = theme(false);
    let column = Column::new(vec![
        WidgetPod::new(Label::new("Header", t.font.body.clone(), Tone::Ink)),
        WidgetPod::new(ProgressBar::new(0.1)).with_key("bar"),
    ])
    .shrink();
    let mut h = Harness::new(column, SCREEN, &t, &assets());
    h.frame();
    let bar = h.find_rect("bar").expect("the bar is laid out");

    h.update::<ProgressBar, _>("bar", |bar, cx| {
        bar.set_value(0.9);
        cx.request_paint();
    });
    h.frame();
    assert_eq!(h.refresh_requests(), &[refresh_of(bar)]);
}

#[test]
fn a_value_change_repaints_the_new_drawing() {
    let t = theme(false);
    let column = Column::new(vec![WidgetPod::new(ProgressBar::new(0.0)).with_key("bar")]).shrink();
    let mut h = Harness::new(column, SCREEN, &t, &assets());
    h.frame();
    let bar = h.find_rect("bar").expect("laid out");
    let before = h.screen().clone();

    h.update::<ProgressBar, _>("bar", |p, cx| {
        p.set_value(1.0);
        cx.request_paint();
    });
    let after = h.frame().clone();
    // At zero the track shows no fill. At one the fill covers it from the left edge.
    let probe = (bar.x + 2, bar.y + bar.h as i32 / 2);
    assert_ne!(before.get(probe.0, probe.1), Some(t.color(Tone::Accent)));
    assert_eq!(after.get(probe.0, probe.1), Some(t.color(Tone::Accent)));
}

#[test]
fn an_update_with_the_wrong_type_finds_nothing_and_repaints_nothing() {
    let t = theme(false);
    let label = Label::new("Title", t.font.body.clone(), Tone::Ink);
    let column = Column::new(vec![WidgetPod::new(label).with_key("title")]).shrink();
    let mut h = Harness::new(column, SCREEN, &t, &assets());
    h.frame();
    let mut ran = false;
    let found = h.update::<Button, _>("title", |_, _| ran = true);
    assert!(!found, "a label is not a button");
    assert!(!ran, "the closure does not run for the wrong type");
    h.frame();
    assert!(h.refresh_requests().is_empty());
}

#[test]
fn a_missing_key_returns_false() {
    let t = theme(false);
    let column = Column::new(vec![WidgetPod::new(Label::new(
        "x",
        t.font.body.clone(),
        Tone::Ink,
    ))]);
    let mut h = Harness::new(column, SCREEN, &t, &assets());
    h.frame();
    assert!(!h.update::<Label, _>("nope", |_, _| {}));
    assert_eq!(h.find_rect("nope"), None);
}

#[test]
fn find_rect_reports_the_absolute_bounds_of_a_nested_widget() {
    let t = theme(false);
    let label = Label::new("Nested", t.font.body.clone(), Tone::Ink);
    let root = Padding::new(WidgetPod::new(label).with_key("inner"), Insets::all(20));
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("inner").expect("laid out");
    assert_eq!((r.x, r.y), (20, 20), "the padding moves the label to (20, 20)");
    assert!(r.w > 0 && r.h > 0);
}

#[test]
fn a_layout_request_repaints_the_whole_window() {
    let t = theme(false);
    let column = Column::new(vec![WidgetPod::new(Label::new(
        "A",
        t.font.body.clone(),
        Tone::Ink,
    ))
    .with_key("a")])
    .shrink();
    let mut h = Harness::new(column, SCREEN, &t, &assets());
    h.frame();
    h.update::<Label, _>("a", |_, cx| cx.request_layout());
    h.frame();
    assert_eq!(h.refresh_requests(), &[refresh_of(Rect::from_size(SCREEN))]);
}

#[test]
fn an_update_leaves_the_other_widgets_out_of_the_refresh() {
    let t = theme(false);
    let column = Column::new(vec![
        WidgetPod::new(Button::new(ButtonStyle::Secondary).text("Keep")).with_key("b1"),
        WidgetPod::new(ProgressBar::new(0.5)).with_key("p"),
    ])
    .shrink();
    let mut h = Harness::new(column, SCREEN, &t, &assets());
    h.frame();
    let bar = h.find_rect("p").expect("laid out");
    h.update::<ProgressBar, _>("p", |p, cx| {
        p.set_value(0.25);
        cx.request_paint();
    });
    h.frame();
    assert_eq!(h.refresh_requests(), &[refresh_of(bar)]);
}
