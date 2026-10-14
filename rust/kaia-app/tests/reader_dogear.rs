//! The dog-ear and the note marks. A tap in the top right corner bookmarks the page,
//! and a bookmarked page shows a folded corner there. A highlight with a note shows
//! a mark in the right margin. The checks read the frame's pixels and the contents
//! list, so they test what the page draws.

use std::time::Duration;

use kaia_app::surfaces::reader::panels::ContentItem;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_display::InputEvent;
use kaia_gfx::Point;
use kaia_input::{Phase, TouchEvent};
use kaia_ui::components::Label;
use kaia_ui::composite::PagedList;
use kaia_ui::controls::TextField;

mod common;

use common::*;

/// Opens the reader on `name` and waits for the library scan.
fn open_reader(name: &str, dir: &std::path::Path) -> App<HeadlessBackend> {
    let lib = library_with_books(dir);
    let mut app = make_app(dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    open_from_library(&mut app, name);
    app
}

/// Turns to a page of juliet with text. Page 0 is the cover.
fn turn_to_text(app: &mut App<HeadlessBackend>) {
    for _ in 0..8 {
        app.key(kaia_input::Key::RPgFwd).unwrap();
        wait_reader(app);
    }
}

/// A hold on a word of the text page. It selects the word and opens the selection popover.
fn hold_at(app: &mut App<HeadlessBackend>, p: Point) {
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Down,
        pos: p,
        time_us: 1,
    })])
    .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    app.iterate(Some(Duration::from_millis(1))).unwrap();
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: p,
        time_us: 2_000_000,
    })])
    .unwrap();
    wait_reader(app);
}

/// The number of dark pixels in the `size` square at the top right corner of the frame.
fn dark_in_corner(app: &App<HeadlessBackend>, size: i32) -> usize {
    let w = app.screen().w as i32;
    dark_in(app, w - size, 0, size, size)
}

/// The number of dark pixels in a rectangle of the frame.
fn dark_in(app: &App<HeadlessBackend>, x0: i32, y0: i32, w: i32, h: i32) -> usize {
    let frame = app.frame();
    let mut dark = 0;
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            if frame.get(x, y).is_some_and(|c| c.luma() < 128) {
                dark += 1;
            }
        }
    }
    dark
}

/// The number of bookmarks in the contents list, read through the bookmark tab.
fn bookmark_count(app: &mut App<HeadlessBackend>) -> usize {
    open_overlay(app);
    tap_key(app, "overlay-contents");
    tap_key(app, "contents-tab-1");
    let mut count = 0;
    app.update_top::<PagedList<ContentItem>, _>("contents-list", |list, _| count = list.items().len());
    // Back to the page: close the contents panel, then the overlay.
    tap_key(app, "panel-close");
    app.tap(Point::new(app.screen().w as i32 / 2, app.screen().h as i32 / 2))
        .unwrap();
    wait_reader(app);
    count
}

fn open_overlay(app: &mut App<HeadlessBackend>) {
    let p = Point::new(app.screen().w as i32 / 2, app.screen().h as i32 / 2);
    app.tap(p).unwrap();
    wait_reader(app);
    assert_eq!(app.top_route(), Some(&Route::Overlay));
}

#[test]
fn a_tap_in_the_top_right_corner_bookmarks_the_page_and_draws_the_dog_ear() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("dogear-tap");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    let (w, _) = (app.screen().w as i32, app.screen().h as i32);
    let before = dark_in_corner(&app, 20);
    assert_eq!(bookmark_count(&mut app), 0, "no bookmark yet");

    app.tap(Point::new(w - 10, 10)).unwrap();
    wait_reader(&mut app);
    assert!(
        matches!(app.top_route(), Some(Route::Reader { .. })),
        "the corner does not open the overlay"
    );
    let marked = dark_in_corner(&app, 20);
    assert!(
        marked > before + 100,
        "the dog-ear is drawn: {before} dark pixels before, {marked} after"
    );
    assert_eq!(bookmark_count(&mut app), 1, "one bookmark on the page");

    // A second tap in the corner removes the bookmark and the dog-ear.
    app.tap(Point::new(w - 10, 10)).unwrap();
    wait_reader(&mut app);
    assert_eq!(dark_in_corner(&app, 20), before, "the corner is plain again");
    assert_eq!(bookmark_count(&mut app), 0, "the bookmark is removed");
}

#[test]
fn the_rest_of_the_top_edge_still_opens_the_overlay() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("dogear-edge");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    let w = app.screen().w as i32;
    // Left of the corner square, the top edge is still the overlay's.
    app.tap(Point::new(w / 2, 10)).unwrap();
    wait_reader(&mut app);
    assert_eq!(app.top_route(), Some(&Route::Overlay));
    // The centre tap closes the overlay again, so the count can open it.
    app.tap(Point::new(w / 2, app.screen().h as i32 / 2)).unwrap();
    wait_reader(&mut app);
    assert_eq!(bookmark_count(&mut app), 0, "a top edge tap bookmarks nothing");
}

#[test]
fn the_bookmark_button_also_draws_the_dog_ear() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("dogear-button");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    let w = app.screen().w as i32;
    let before = dark_in_corner(&app, 20);

    open_overlay(&mut app);
    tap_key(&mut app, "overlay-bookmark");
    // The centre tap closes the overlay, and the page shows the new state.
    app.tap(Point::new(w / 2, app.screen().h as i32 / 2)).unwrap();
    wait_reader(&mut app);
    assert!(
        dark_in_corner(&app, 20) > before + 100,
        "the page shows the dog-ear after the bookmark button"
    );
}

#[test]
fn a_highlight_with_a_note_shows_a_mark_in_the_right_margin() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("dogear-note");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    let (w, h) = (app.screen().w as i32, app.screen().h as i32);
    // The right margin strip, below the top margin and above the bottom bar area.
    let strip = |app: &App<HeadlessBackend>| dark_in(app, w - 30, 40, 30, h - 80);
    let before = strip(&app);

    hold_at(&mut app, Point::new(200, 120));
    tap_key(&mut app, "sel-note");
    app.update_top::<TextField, _>("note-field", |field, cx| {
        field.set_text("check the wording");
        cx.request_paint();
    });
    tap_key(&mut app, "note-save");
    wait_reader(&mut app);
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));

    let after = strip(&app);
    assert!(
        after >= before + 20,
        "the note mark is drawn in the margin: {before} dark pixels before, {after} after"
    );
}

#[test]
fn the_overlay_footer_line_is_a_label_on_the_bars() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("dogear-label");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    open_overlay(&mut app);
    let mut text = None;
    app.update_top::<Label, _>("overlay-footer", |label, _| text = Some(label.text().to_string()));
    let text = text.expect("the overlay has the footer line");
    assert!(
        text.starts_with("Page 9 of "),
        "the default footer is the page count: {text:?}"
    );
}
