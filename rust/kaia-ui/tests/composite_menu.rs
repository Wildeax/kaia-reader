//! Menu: behavior through the harness and a golden image of a sub-page.
//! Regenerate with `KAIA_UPDATE_GOLDEN=1 cargo test -p kaia-ui --test composite_menu`.

mod common;

use common::*;
use kaia_events::Value;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::{Point, Rect};
use kaia_ui::components::Column;
use kaia_ui::composite::Menu;
use kaia_ui::widget::{AppAction, Out};
use kaia_ui::{Harness, WidgetPod};

fn menu_root() -> Column {
    let menu = Menu::new("Settings", settings_build())
        .toggle("justify", true)
        .toggle("hyphenate", false)
        .value("font_size", "20");
    Column::new(vec![WidgetPod::new(menu).with_key("menu")])
}

fn outs(h: &Harness) -> Vec<Out> {
    h.outbox()
}

fn centre(r: Rect) -> Point {
    Point::new(r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
}

#[test]
fn sections_become_headings_and_leaves_become_rows() {
    let t = theme(false);
    let mut h = Harness::new(menu_root(), SCREEN, &t, &assets());
    h.frame();
    assert!(h.find_rect("row:font_size").is_some());
    assert!(h.find_rect("row:justify").is_some());
    assert!(
        h.find_rect("row:dark").is_some(),
        "the leaf after the section is a row too"
    );
    assert!(
        h.find_rect("push:[0, 3]").is_some(),
        "a submenu inside a section is a row with a chevron"
    );
    assert!(
        h.find_rect("row:wifi").is_none(),
        "the device tab is not on screen"
    );
}

#[test]
fn a_switch_row_reports_its_new_state_and_the_menu_remembers_the_app_value() {
    let t = theme(false);
    let mut h = Harness::new(menu_root(), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("row:justify").expect("justify row");
    assert!(h.tap(centre(r)));
    assert_eq!(
        outs(&h),
        vec![Out::Action(AppAction {
            id: "settings.justify".into(),
            args: vec![Value::Bool(false)],
        })],
        "the switch was on, so the tap reports off"
    );
    // The app keeps the setting and reports it back.
    let found = h.update::<Menu, _>("menu", |menu, cx| menu.set_toggle("justify", true, cx));
    assert!(found);
    h.frame();
    let r = h.find_rect("row:justify").expect("justify row");
    assert!(h.tap(centre(r)));
    assert_eq!(
        outs(&h).last().and_then(|o| match o {
            Out::Action(AppAction { args, .. }) => args.first().cloned(),
            _ => None,
        }),
        Some(Value::Bool(false))
    );
}

#[test]
fn a_submenu_row_opens_a_page_with_a_back_header_and_back_returns() {
    let t = theme(false);
    let mut h = Harness::new(menu_root(), SCREEN, &t, &assets());
    h.frame();
    let push = h.find_rect("push:[0, 3]").expect("margins row");
    assert!(h.tap(centre(push)));
    h.frame();
    assert!(h.find_rect("row:top").is_some(), "the margins page is on screen");
    assert!(h.find_rect("row:font_size").is_none(), "the tab page is gone");

    let header = h.find_rect("menu-header").expect("header");
    let back = Point::new(
        header.x + t.space.sm as i32 + t.touch_target as i32 / 2,
        centre(header).y,
    );
    assert!(h.tap(back));
    h.frame();
    assert!(
        h.find_rect("row:font_size").is_some(),
        "back returns to the tab page"
    );
}

#[test]
fn tab_buttons_switch_the_tab_and_clear_the_path() {
    let t = theme(false);
    let mut h = Harness::new(menu_root(), SCREEN, &t, &assets());
    h.frame();
    let tab = h.find_rect("menu-tab:1").expect("device tab button");
    assert!(h.tap(centre(tab)));
    h.frame();
    assert!(h.find_rect("row:wifi").is_some());
    assert!(h.find_rect("row:font_size").is_none());
}

#[test]
fn a_leaf_row_without_state_queues_its_callback() {
    let t = theme(false);
    let mut h = Harness::new(menu_root(), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("row:dark").expect("night mode row");
    assert!(h.tap(centre(r)));
    assert_eq!(
        outs(&h),
        vec![Out::Action(AppAction {
            id: "settings.night".into(),
            args: vec![],
        })]
    );
}

#[test]
fn margins_sub_page_golden_matches() {
    let t = theme(false);
    let mut h = Harness::new(menu_root(), SCREEN, &t, &assets());
    h.frame();
    let push = h.find_rect("push:[0, 3]").expect("margins row");
    h.tap(centre(push));
    let pm = h.frame().clone();
    assert_matches_golden(&pm, &golden("menu_margins_gray.png"), 0);
}
