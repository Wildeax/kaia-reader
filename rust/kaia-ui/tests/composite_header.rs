//! Header and nav bar: behavior through the harness and golden images.
//! Regenerate with `KAIA_UPDATE_GOLDEN=1 cargo test -p kaia-ui --test composite_header`.

mod common;

use common::*;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::{Point, Rect};
use kaia_ui::components::{Column, CrossAlign, Padding, Spacer};
use kaia_ui::composite::{Header, NavBar, NavItem};
use kaia_ui::widget::{AppAction, Out};
use kaia_ui::{render, Harness, Insets, Theme, WidgetPod};

fn actions_of(h: &Harness) -> Vec<String> {
    h.outbox()
        .into_iter()
        .filter_map(|out| match out {
            Out::Action(AppAction { id, .. }) => Some(id),
            _ => None,
        })
        .collect()
}

/// A header with a back chevron, two actions and status text, keyed "header".
fn header_screen(t: &Theme) -> Column {
    let header = Header::new(t, "Library")
        .back("nav.back")
        .action("appbar.search", "search.open")
        .action("appbar.menu", "menu.open")
        .status("84%");
    Column::new(vec![
        WidgetPod::new(header).with_key("header"),
        WidgetPod::new(Spacer::new(1)),
    ])
}

fn centre_y(r: Rect) -> i32 {
    r.y + r.h as i32 / 2
}

#[test]
fn the_back_chevron_queues_its_action() {
    let t = theme(false);
    let mut h = Harness::new(header_screen(&t), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("header").expect("header is laid out");
    // The chevron is the first square on the leading edge.
    let at = Point::new(r.x + t.space.sm as i32 + t.touch_target as i32 / 2, centre_y(r));
    assert!(h.tap(at));
    assert_eq!(actions_of(&h), vec!["nav.back"]);
}

#[test]
fn the_trailing_actions_queue_their_actions_from_the_right() {
    let t = theme(false);
    let mut h = Harness::new(header_screen(&t), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("header").expect("header is laid out");
    let touch = t.touch_target as i32;
    let side = t.space.sm as i32;
    let rightmost = Point::new(r.right() - side - touch / 2, centre_y(r));
    let second = Point::new(r.right() - side - touch - touch / 2, centre_y(r));
    assert!(h.tap(rightmost));
    assert!(h.tap(second));
    assert_eq!(actions_of(&h), vec!["menu.open", "search.open"]);
}

#[test]
fn rtl_moves_the_back_chevron_and_the_actions_to_the_other_edge() {
    let t = theme(false).with_rtl(true);
    let mut h = Harness::new(header_screen(&t), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("header").expect("header is laid out");
    let touch = t.touch_target as i32;
    let side = t.space.sm as i32;
    // In RTL the chevron is on the right edge.
    let back = Point::new(r.right() - side - touch / 2, centre_y(r));
    // The last action sits nearest the trailing edge, which is the left edge in RTL.
    let trailing = Point::new(r.x + side + touch / 2, centre_y(r));
    assert!(h.tap(back));
    assert!(h.tap(trailing));
    assert_eq!(actions_of(&h), vec!["nav.back", "menu.open"]);
}

#[test]
fn header_golden_matches() {
    let t = theme(false);
    let root = Column::new(vec![
        WidgetPod::new(Padding::new(
            WidgetPod::new(header_screen_top(&t)),
            Insets::all(t.space.lg),
        )),
        WidgetPod::new(Spacer::new(1)),
    ]);
    let pm = render(root, SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("header_gray.png"), 0);
}

#[test]
fn header_rtl_golden_matches() {
    let t = theme(false).with_rtl(true);
    let root = Column::new(vec![
        WidgetPod::new(Padding::new(
            WidgetPod::new(header_screen_top(&t)),
            Insets::all(t.space.lg),
        )),
        WidgetPod::new(Spacer::new(1)),
    ]);
    let pm = render(root, SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("header_rtl_gray.png"), 0);
}

/// Three headers stacked: a plain title, one with a long title that wraps,
/// and the full header. Each keeps its own settings.
fn header_screen_top(t: &Theme) -> Column {
    let plain = Header::new(t, "Settings");
    let long = Header::new(
        t,
        "The Left Hand of Darkness and Other Stories From the Archipelago",
    )
    .back("nav.back")
    .action("appbar.search", "search.open");
    let full = Header::new(t, "Library")
        .back("nav.back")
        .action("appbar.search", "search.open")
        .action("appbar.menu", "menu.open")
        .status("84%");
    Column::new(vec![
        WidgetPod::new(plain),
        WidgetPod::new(long),
        WidgetPod::new(full),
    ])
}

fn nav_screen(active: NavItem) -> Column {
    Column::new(vec![
        WidgetPod::new(Spacer::new(1)),
        WidgetPod::new(NavBar::new(active)).with_key("nav"),
    ])
    .cross_align(CrossAlign::Stretch)
}

#[test]
fn nav_bar_taps_queue_the_item_actions() {
    let t = theme(false);
    let mut h = Harness::new(nav_screen(NavItem::Home), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("nav").expect("nav bar is laid out");
    assert_eq!(r.h, t.nav_bar, "the bar is the DESIGN.md height");
    let cell = r.w as i32 / 4;
    for (slot, id) in ["nav.home", "nav.library", "nav.notes", "nav.settings"]
        .iter()
        .enumerate()
    {
        let at = Point::new(r.x + slot as i32 * cell + cell / 2, r.y + r.h as i32 / 2);
        assert!(h.tap(at));
        assert_eq!(actions_of(&h), vec![id.to_string()]);
    }
}

#[test]
fn nav_bar_rtl_reverses_the_items() {
    let t = theme(false).with_rtl(true);
    let mut h = Harness::new(nav_screen(NavItem::Home), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("nav").expect("nav bar is laid out");
    let cell = r.w as i32 / 4;
    let leftmost = Point::new(r.x + cell / 2, r.y + r.h as i32 / 2);
    assert!(h.tap(leftmost));
    assert_eq!(actions_of(&h), vec!["nav.settings"]);
}

#[test]
fn a_drag_on_the_nav_bar_is_not_a_tap() {
    let t = theme(false);
    let mut h = Harness::new(nav_screen(NavItem::Home), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("nav").expect("nav bar is laid out");
    let y = r.y + r.h as i32 / 2;
    h.swipe(Point::new(r.x + 50, y), Point::new(r.x + 600, y));
    assert!(actions_of(&h).is_empty());
}

#[test]
fn nav_bar_golden_matches() {
    let t = theme(false);
    let pm = render(nav_screen(NavItem::Library), SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("navbar_gray.png"), 0);
}
