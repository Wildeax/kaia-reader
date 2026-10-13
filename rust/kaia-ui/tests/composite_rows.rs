//! ListRow: behavior through the harness and a golden image.
//! Regenerate with `KAIA_UPDATE_GOLDEN=1 cargo test -p kaia-ui --test composite_rows`.

mod common;

use common::*;
use kaia_events::Value;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Point;
use kaia_ui::components::{Column, CrossAlign, Spacer};
use kaia_ui::composite::{CoverImage, ListRow};
use kaia_ui::widget::{AppAction, Out};
use kaia_ui::{render, Harness, WidgetPod};

fn outs(h: &Harness) -> Vec<Out> {
    h.outbox()
}

fn rows_screen() -> Column {
    Column::new(vec![
        WidgetPod::new(ListRow::new("Reading font").value("Noto Serif").chevron()).with_key("font"),
        WidgetPod::new(
            ListRow::new("Night mode")
                .secondary("Dark pages after sunset")
                .toggle(true),
        )
        .with_key("night"),
        WidgetPod::new(ListRow::new("Show finished books").checkbox(true)).with_key("finished"),
        WidgetPod::new(
            ListRow::new("Frontlight")
                .icon("appbar.contrast")
                .secondary("40 percent")
                .chevron(),
        ),
        WidgetPod::new(
            ListRow::new("The Left Hand of Darkness")
                .secondary("Ursula K. Le Guin")
                .cover(CoverImage::new("The Left Hand of Darkness", None))
                .chevron(),
        ),
        WidgetPod::new(
            ListRow::new(
                "Notes and highlights exported from every book in the library folder on this device",
            )
            .secondary("Markdown")
            .value("Export"),
        )
        .with_key("long"),
        WidgetPod::new(ListRow::new("Sync needs Wi-Fi").enabled(false).chevron()),
        WidgetPod::new(Spacer::new(1)),
    ])
    .cross_align(CrossAlign::Stretch)
}

#[test]
fn a_tap_anywhere_on_a_row_queues_its_action() {
    let t = theme(false);
    let root = Column::new(vec![WidgetPod::new(
        ListRow::new("Reading font")
            .value("Noto Serif")
            .chevron()
            .on_action("font.open"),
    )
    .with_key("row")])
    .cross_align(CrossAlign::Stretch);
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("row").expect("row is laid out");
    // Far right, over the chevron. The row is the target, not only its text.
    assert!(h.tap(Point::new(r.right() - 20, r.y + r.h as i32 / 2)));
    match outs(&h).as_slice() {
        [Out::Action(AppAction { id, args })] => {
            assert_eq!(id, "font.open");
            assert!(args.is_empty());
        }
        other => panic!("expected one action, got {other:?}"),
    }
}

#[test]
fn a_toggle_row_flips_and_reports_the_new_state() {
    let t = theme(false);
    let root = Column::new(vec![WidgetPod::new(
        ListRow::new("Night mode").toggle(false).on_action("night.set"),
    )
    .with_key("row")])
    .cross_align(CrossAlign::Stretch);
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("row").expect("row is laid out");
    let at = Point::new(r.x + 40, r.y + r.h as i32 / 2);
    assert!(h.tap(at));
    assert_eq!(
        outs(&h),
        vec![Out::Action(AppAction {
            id: "night.set".into(),
            args: vec![Value::Bool(true)],
        })]
    );
    let mut on = None;
    h.update::<ListRow, _>("row", |row, _| on = Some(row.is_on()));
    assert_eq!(on, Some(true));
}

#[test]
fn a_disabled_row_does_not_queue_anything() {
    let t = theme(false);
    let root = Column::new(vec![WidgetPod::new(
        ListRow::new("Sync needs Wi-Fi")
            .enabled(false)
            .on_action("sync.open"),
    )
    .with_key("row")])
    .cross_align(CrossAlign::Stretch);
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("row").expect("row is laid out");
    h.tap(Point::new(r.x + 40, r.y + r.h as i32 / 2));
    assert!(outs(&h).is_empty());
}

#[test]
fn a_row_is_at_least_the_row_height_and_its_divider_sits_inside_the_padding() {
    let t = theme(false);
    let mut h = Harness::new(rows_screen(), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("font").expect("row is laid out");
    assert!(r.h >= t.row_height);
    assert_eq!(r.w, SCREEN.w);
}

#[test]
fn a_long_primary_text_takes_two_lines_and_grows_the_row() {
    let t = theme(false);
    let mut h = Harness::new(rows_screen(), SCREEN, &t, &assets());
    h.frame();
    let short = h.find_rect("font").expect("short row").h;
    let long = h.find_rect("long").expect("long row").h;
    assert_eq!(short, t.row_height, "a one-line row keeps the minimum height");
    assert!(
        long > short,
        "two lines of primary text and a secondary line need more height"
    );
}

#[test]
fn list_rows_golden_matches() {
    let t = theme(false);
    let pm = render(rows_screen(), SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("list_row_gray.png"), 0);
}
