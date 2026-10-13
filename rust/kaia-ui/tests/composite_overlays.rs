//! Dialog, Toast and SheetPanel: behavior through the harness and golden images.
//! Regenerate with `KAIA_UPDATE_GOLDEN=1 cargo test -p kaia-ui --test composite_overlays`.

mod common;

use std::time::Duration;

use common::*;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Point;
use kaia_ui::components::{Column, CrossAlign};
use kaia_ui::composite::{Dialog, DialogButton, ListRow, PagedList, SheetPanel, Toast};
use kaia_ui::widget::{AppAction, Out};
use kaia_ui::{render, Harness, Theme, WidgetPod, Window};

fn actions_of(h: &Harness) -> Vec<String> {
    h.outbox()
        .into_iter()
        .filter_map(|out| match out {
            Out::Action(AppAction { id, .. }) => Some(id),
            _ => None,
        })
        .collect()
}

fn sample_dialog(t: &Theme) -> Dialog {
    Dialog::new(
        t,
        "Delete this book?",
        "The file is removed from the device. Notes and highlights are kept in the notes folder.",
        vec![
            DialogButton::new("Cancel", "dialog.cancel"),
            DialogButton::new("Keep", "dialog.keep"),
            DialogButton::new("Delete", "book.delete"),
        ],
    )
}

/// The library behind the overlays: a page of list rows.
fn background_root() -> Column {
    let rows = PagedList::new((0..12u32).collect(), |n: &u32, _t: &Theme| {
        WidgetPod::new(ListRow::new(format!("Book {n}")).secondary("An author").chevron())
    });
    Column::new(vec![WidgetPod::new(rows)])
}

fn sheet_content(t: &Theme) -> Column {
    Column::new(vec![
        WidgetPod::new(ListRow::new("Font size").value("20").chevron()),
        WidgetPod::new(ListRow::new("Justify text").toggle(true)),
        WidgetPod::new(ListRow::new("Hyphenate").toggle(false)),
    ])
    .gap(t.space.sm)
    .cross_align(CrossAlign::Stretch)
}

#[test]
fn a_tap_on_the_scrim_does_nothing() {
    let t = theme(false);
    let mut h = Harness::new(sample_dialog(&t), SCREEN, &t, &assets());
    h.frame();
    assert!(h.tap(Point::new(5, 5)), "the scrim takes the tap");
    assert!(actions_of(&h).is_empty());
    h.frame();
    assert!(!h.stack().is_empty(), "the modal dialog stays open");
}

#[test]
fn each_button_queues_its_action_and_the_main_action_is_last_on_the_right() {
    let t = theme(false);
    let mut h = Harness::new(sample_dialog(&t), SCREEN, &t, &assets());
    h.frame();
    let cancel = h.find_rect("dialog-button:dialog.cancel").expect("cancel button");
    let delete = h.find_rect("dialog-button:book.delete").expect("main button");
    assert!(delete.x > cancel.x, "the main action sits to the right");
    assert!(h.tap(Point::new(
        delete.x + delete.w as i32 / 2,
        delete.y + delete.h as i32 / 2
    )));
    assert_eq!(actions_of(&h), vec!["book.delete"]);
}

#[test]
fn show_queues_a_modal_window_on_top() {
    let t = theme(false);
    let a = assets();
    let mut h = Harness::new(background_root(), SCREEN, &t, &a);
    h.frame();
    let window = sample_dialog(&t).show(h.ctx(), &t, &a, SCREEN);
    h.frame();
    let id = window.id().expect("the window has an id once shown");
    assert!(h.stack().contains(id));
    assert_eq!(h.stack().options(id).map(|o| o.modal), Some(true));
    assert_eq!(h.stack().top(), Some(id), "the dialog is on top");
}

#[test]
fn a_toast_closes_itself_after_its_delay() {
    let t = theme(false);
    let a = assets();
    let mut h = Harness::new(background_root(), SCREEN, &t, &a);
    h.frame();
    let id = Toast::new(&t, "Saved").show(h.ctx(), &t, &a, SCREEN, 2.0);
    h.frame();
    assert!(h.stack().contains(id));
    assert_eq!(h.stack().options(id).map(|o| o.toast), Some(true));

    h.ctx().run_due(Duration::from_secs(1));
    h.frame();
    assert!(h.stack().contains(id), "the toast waits for its delay");

    h.ctx().run_due(Duration::from_secs(3));
    h.frame();
    assert!(!h.stack().contains(id), "the toast closes after two seconds");
}

#[test]
fn the_close_button_closes_the_sheet_window() {
    let t = theme(false);
    let sheet = SheetPanel::new(&t, "Text and layout", WidgetPod::new(sheet_content(&t)));
    let mut h = Harness::new(sheet, SCREEN, &t, &assets());
    h.frame();
    let close = h.find_rect("sheet-close").expect("close button");
    assert!(h.tap(Point::new(
        close.x + close.w as i32 / 2,
        close.y + close.h as i32 / 2
    )));
    h.frame();
    assert!(h.stack().is_empty(), "the sheet's window is gone");
}

#[test]
fn a_tap_above_the_sheet_closes_it_unless_it_is_modal() {
    let t = theme(false);
    let sheet = SheetPanel::new(&t, "Text and layout", WidgetPod::new(sheet_content(&t)));
    let mut h = Harness::new(sheet, SCREEN, &t, &assets());
    h.frame();
    h.tap(Point::new(600, 100));
    h.frame();
    assert!(h.stack().is_empty(), "a tap on the scrim closes a plain sheet");

    let modal = SheetPanel::new(&t, "Text and layout", WidgetPod::new(sheet_content(&t))).modal(true);
    let mut h = Harness::new(modal, SCREEN, &t, &assets());
    h.frame();
    assert!(h.tap(Point::new(600, 100)));
    h.frame();
    assert!(!h.stack().is_empty(), "a modal sheet ignores the scrim");
}

#[test]
fn a_tap_inside_the_sheet_keeps_it_open() {
    let t = theme(false);
    let sheet = SheetPanel::new(&t, "Text and layout", WidgetPod::new(sheet_content(&t)));
    let mut h = Harness::new(sheet, SCREEN, &t, &assets());
    h.frame();
    let close = h.find_rect("sheet-close").expect("close button");
    // Below the header, inside the sheet, left of the close button.
    h.tap(Point::new(close.x - 200, close.bottom() + 80));
    h.frame();
    assert!(!h.stack().is_empty());
}

#[test]
fn dialog_golden_matches() {
    let t = theme(false);
    let a = assets();
    let list = Window::new(background_root(), t.clone(), a.clone(), SCREEN);
    let dialog = Window::new(sample_dialog(&t), t.clone(), a.clone(), SCREEN);
    let pm = paint_stack(&[&list, &dialog], &t);
    assert_matches_golden(&pm, &golden("dialog_gray.png"), 0);
}

#[test]
fn toast_golden_matches() {
    let t = theme(false);
    let a = assets();
    let list = Window::new(background_root(), t.clone(), a.clone(), SCREEN);
    let toast = Window::new(
        Toast::new(&t, "Saved to the notes folder").above(t.nav_bar + t.touch_target + t.space.md),
        t.clone(),
        a.clone(),
        SCREEN,
    );
    let pm = paint_stack(&[&list, &toast], &t);
    assert_matches_golden(&pm, &golden("toast_gray.png"), 0);
}

#[test]
fn sheet_golden_matches() {
    let t = theme(false);
    let a = assets();
    let list = Window::new(background_root(), t.clone(), a.clone(), SCREEN);
    let sheet = Window::new(
        SheetPanel::new(&t, "Text and layout", WidgetPod::new(sheet_content(&t))),
        t.clone(),
        a.clone(),
        SCREEN,
    );
    let pm = paint_stack(&[&list, &sheet], &t);
    assert_matches_golden(&pm, &golden("sheet_gray.png"), 0);
}

#[test]
fn a_dialog_alone_renders_over_a_plain_background() {
    let t = theme(false);
    let pm = render(sample_dialog(&t), SCREEN, &t, &assets());
    // The scrim darkens the plain background, so the corner is no longer white.
    let corner = pm.get(2, 2).expect("corner is on screen");
    assert!(corner.luma() < t.color(kaia_ui::Tone::Surface).luma());
}
