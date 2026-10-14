//! The Series view of the Library, and the next-in-series card on Home.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kaia_app::{App, HeadlessBackend, Route};
use kaia_gfx::Rect;
use kaia_library::Status;
use rusqlite::params;

mod common;

use common::*;

/// The key a button or row with `action` has in the tree.
fn key(action: &str) -> String {
    action.replace(':', "-")
}

/// The top-left corner of a widget as (y, x), for ordering rows.
fn position(app: &App<HeadlessBackend>, key: &str) -> (i32, i32) {
    let r: Rect = app
        .find_rect(key)
        .unwrap_or_else(|| panic!("no widget {key:?} on the top window"));
    (r.y, r.x)
}

/// A library app with the three test books indexed and Home on screen.
fn indexed(name: &str) -> (App<HeadlessBackend>, PathBuf) {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    (app, dir)
}

/// Reopens the app on the same data folder with no scan.
fn reopen(dir: &Path) -> App<HeadlessBackend> {
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app
}

fn open_library(app: &mut App<HeadlessBackend>) {
    tap_key(app, "library");
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
}

/// Sets the series of a book in the index. The test books carry no series
/// metadata, so the test writes it, as the scan does for an EPUB that has it.
fn set_series(app: &App<HeadlessBackend>, id: i64, name: &str, index: f64) {
    let conn = rusqlite::Connection::open(app.services().library_path()).unwrap();
    conn.execute(
        "UPDATE books SET series = ?2, series_index = ?3 WHERE id = ?1",
        params![id, name, index],
    )
    .unwrap();
}

#[test]
fn series_view_lists_each_series_and_opens_its_books_in_order() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("series-view");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    set_series(&app, juliet, "Verona", 2.0);
    set_series(&app, leaves, "Verona", 1.0);
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);

    tap_key(&mut app, &key("library:view:series"));
    assert!(
        app.find_rect("series-list").is_some(),
        "the Series view lists the series"
    );
    assert!(app.find_rect("series-0").is_some(), "Verona is the first series");

    tap_key(&mut app, "series-0");
    assert!(
        app.find_rect("series-up").is_some(),
        "an open series has a row to go back"
    );
    let leaves_key = format!("book-{leaves}");
    let juliet_key = format!("book-{juliet}");
    assert!(
        position(&app, &leaves_key) < position(&app, &juliet_key),
        "book 1 comes before book 2"
    );

    tap_key(&mut app, "series-up");
    assert!(
        app.find_rect("series-list").is_some(),
        "Up returns to the list of series"
    );
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn home_offers_the_next_unread_book_after_a_finished_one() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("next-finished");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    set_series(&app, juliet, "Verona", 1.0);
    set_series(&app, leaves, "Verona", 2.0);
    // The PDF is the book to continue, so the hero is not the next book.
    app.services_mut().library.record_open(pdf, 0.3).unwrap();
    app.services_mut().library.record_open(juliet, 1.0).unwrap();
    app.services_mut()
        .library
        .set_status(juliet, Status::Complete)
        .unwrap();
    drop(app);
    let mut app = reopen(&dir);

    assert!(
        app.find_rect("next-in-series").is_some(),
        "the finished book's next number is offered"
    );
    tap_key(&mut app, "next-in-series");
    assert_eq!(
        app.top_route(),
        Some(&Route::Reader { book: leaves }),
        "the card opens the next book"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn home_does_not_offer_a_next_book_that_is_already_started() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("next-started");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    set_series(&app, juliet, "Verona", 1.0);
    set_series(&app, leaves, "Verona", 2.0);
    // Leaves is started first, so juliet is the book read most recently.
    app.services_mut().library.record_open(leaves, 0.2).unwrap();
    app.services_mut().library.record_open(juliet, 1.0).unwrap();
    app.services_mut()
        .library
        .set_status(juliet, Status::Complete)
        .unwrap();
    drop(app);
    let app = reopen(&dir);
    assert!(
        app.find_rect("next-in-series").is_none(),
        "the next book is reading already, so there is nothing to offer"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_book_to_continue_offers_its_next_unread_book() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("next-current");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    set_series(&app, juliet, "Verona", 1.0);
    set_series(&app, leaves, "Verona", 2.0);
    app.services_mut().library.record_open(juliet, 0.4).unwrap();
    drop(app);
    let app = reopen(&dir);
    assert!(
        app.find_rect("continue").is_some(),
        "the book in progress is the one to continue"
    );
    assert!(
        app.find_rect("next-in-series").is_some(),
        "the book to continue offers the next number too"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn home_does_not_repeat_the_next_book_when_it_is_the_hero() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("next-hero");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    set_series(&app, juliet, "Verona", 1.0);
    set_series(&app, leaves, "Verona", 2.0);
    // Nothing is in progress, so Home features the newest book, which is the next one.
    app.services_mut().library.record_open(juliet, 1.0).unwrap();
    app.services_mut()
        .library
        .set_status(juliet, Status::Complete)
        .unwrap();
    app.services_mut()
        .library
        .set_status(leaves, Status::New)
        .unwrap();
    drop(app);
    let app = reopen(&dir);
    assert!(
        app.find_rect("next-in-series").is_none(),
        "a next book that Home already shows is not offered twice"
    );
    let _ = fs::remove_dir_all(dir);
}
