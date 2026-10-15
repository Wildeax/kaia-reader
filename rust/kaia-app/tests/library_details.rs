//! The book details of the Library: the series and its number, and the local
//! rating. A rating is set, changed and cleared in the book's state file, with no
//! store request, and it stays after a restart.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use kaia_app::{App, HeadlessBackend, Route};
use kaia_book::state::{read_rating, write_rating};
use kaia_display::InputEvent;
use kaia_events::Event;
use kaia_gfx::golden::assert_matches_golden;
use kaia_input::{Phase, TouchEvent};
use rusqlite::params;

use common::*;

/// The key a button with `action` has in the tree.
fn key(action: &str) -> String {
    action.replace(':', "-")
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

/// Shuts `app` down and starts a new one on the same data folder, with no scan.
fn restart(mut app: App<HeadlessBackend>, dir: &std::path::Path) -> App<HeadlessBackend> {
    app.shutdown();
    drop(app);
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app
}

/// Opens the Library and shows the Books tab as a list. A cover grid lays out
/// only the rows that fit on the page, and a test needs its book on screen.
fn open_library(app: &mut App<HeadlessBackend>) {
    if app.top_route() != Some(&Route::Library { page: 0 }) {
        tap_key(app, "library");
    }
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    tap_key(app, "library-view-list");
}

/// Holds a finger on `key` until the details sheet is up, then lifts it.
fn hold_for_details(app: &mut App<HeadlessBackend>, widget: &str) {
    let p = centre_of(app, widget);
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Down,
        pos: p,
        time_us: 0,
    })])
    .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while app.find_rect(&key("library:panel:close")).is_none() {
        assert!(
            std::time::Instant::now() < deadline,
            "the details sheet never showed"
        );
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: p,
        time_us: 0,
    })])
    .unwrap();
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

/// The state file of a book. It exists once the book has been opened.
fn state_file(app: &App<HeadlessBackend>, id: i64) -> PathBuf {
    let book = library_db(app).book(id).unwrap().unwrap();
    app.services()
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{}.json", book.partial_md5))
}

/// Opens the book in the reader, saves its state and returns to the Library.
fn open_once(app: &mut App<HeadlessBackend>, id: i64) {
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(app);
    app.broadcast_app(Event::new("SaveState"));
    app.command("reader:overlay", &[]);
    tap_key(app, "overlay-home");
    // The book may have been opened from Home, so the back action lands there.
    open_library(app);
}

#[test]
fn details_show_the_series_and_its_number() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("details-series");
    let juliet = book_id(&app, "juliet.epub");
    set_series(&app, juliet, "Dark Tower", 2.0);
    let leaves = book_id(&app, "leaves.epub");
    set_series(&app, leaves, "Dark Tower", 1.5);
    open_library(&mut app);

    hold_for_details(&mut app, &format!("book-{juliet}"));
    assert!(
        app.find_rect("details-series").is_some(),
        "the details show the series line"
    );
    tap_key(&mut app, &key("library:panel:close"));

    hold_for_details(&mut app, &format!("book-{leaves}"));
    assert!(app.find_rect("details-series").is_some());
}

#[test]
fn a_book_with_no_series_shows_no_series_line() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("details-no-series");
    let leaves = book_id(&app, "leaves.epub");
    open_library(&mut app);
    hold_for_details(&mut app, &format!("book-{leaves}"));
    assert!(app.find_rect("details-series").is_none());
}

/// Opens the rating sheet of a book from its details, then taps `action`.
fn rate(app: &mut App<HeadlessBackend>, book: i64, stars: u8) {
    tap_key(app, &format!("details-menu-{book}"));
    tap_key(app, &key(&format!("library:rating:open:{book}")));
    tap_key(app, &key(&format!("library:rating:{book}:{stars}")));
}

#[test]
fn the_rating_needs_the_book_opened_once_and_then_is_set_changed_and_cleared() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("rating");
    let leaves = book_id(&app, "leaves.epub");
    open_library(&mut app);

    hold_for_details(&mut app, &format!("book-{leaves}"));
    tap_key(&mut app, &format!("details-menu-{leaves}"));
    tap_key(&mut app, &key(&format!("library:rating:open:{leaves}")));
    assert!(
        app.find_rect(&key(&format!("library:rating:{leaves}:4")))
            .is_none(),
        "a book with no state file has no star buttons"
    );
    assert!(
        !state_file(&app, leaves).exists(),
        "viewing the rating creates no state file"
    );
    tap_key(&mut app, &key(&format!("library:rating:back:{leaves}")));
    tap_key(&mut app, &key("library:panel:close"));

    open_once(&mut app, leaves);
    let path = state_file(&app, leaves);
    assert_eq!(read_rating(&path).unwrap(), None, "the opened book is not rated");

    hold_for_details(&mut app, &format!("book-{leaves}"));
    rate(&mut app, leaves, 4);
    assert_eq!(read_rating(&path).unwrap(), Some(4), "a tap sets four stars");
    assert!(
        app.find_rect("book-details-dialog").is_some(),
        "the sheet returns to the details with the new rating"
    );

    rate(&mut app, leaves, 2);
    assert_eq!(
        read_rating(&path).unwrap(),
        Some(2),
        "a second tap changes the rating"
    );

    rate(&mut app, leaves, 0);
    assert_eq!(read_rating(&path).unwrap(), None, "Clear removes the rating");

    rate(&mut app, leaves, 5);
    tap_key(&mut app, &key("library:panel:close"));

    let mut app = restart(app, &dir);
    open_library(&mut app);
    hold_for_details(&mut app, &format!("book-{leaves}"));
    assert_eq!(read_rating(&path).unwrap(), Some(5), "the rating is kept on disk");
    tap_key(&mut app, &format!("details-menu-{leaves}"));
    tap_key(&mut app, &key(&format!("library:rating:open:{leaves}")));
    assert!(app
        .find_rect(&key(&format!("library:rating:{leaves}:5")))
        .is_some());
}

#[test]
fn a_rating_written_by_an_import_shows_in_the_details() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("rating-import");
    let juliet = book_id(&app, "juliet.epub");
    open_once(&mut app, juliet);
    let path = state_file(&app, juliet);
    // The Kobo import writes the rating into the same summary key.
    assert!(write_rating(&path, Some(3)).unwrap());

    open_library(&mut app);
    hold_for_details(&mut app, &format!("book-{juliet}"));
    tap_key(&mut app, &format!("details-menu-{juliet}"));
    tap_key(&mut app, &key(&format!("library:rating:open:{juliet}")));
    assert!(
        app.find_rect(&key(&format!("library:rating:{juliet}:3")))
            .is_some(),
        "the imported rating is on the rating sheet"
    );
}

#[test]
fn details_golden_with_the_series_and_an_unopened_rating() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("golden-details-series");
    let juliet = book_id(&app, "juliet.epub");
    set_series(&app, juliet, "Dark Tower", 2.0);
    open_library(&mut app);
    hold_for_details(&mut app, &format!("book-{juliet}"));
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_details_series.png"), 14);
}
