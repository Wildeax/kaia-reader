//! The author tile on Home: "More by <author>" for the book on Home, when that
//! author has another unread book in the library. The next book in series comes
//! first, and the two tiles never open the same book.

use std::fs;
use std::path::Path;
use std::time::Duration;

use kaia_app::{App, HeadlessBackend, Route};
use kaia_gfx::golden::assert_matches_golden;
use rusqlite::params;

mod common;

use common::*;

/// A library app with the three test books indexed.
fn indexed(name: &str) -> (App<HeadlessBackend>, std::path::PathBuf) {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    (app, dir)
}

/// Reopens the app on the same data folder with no scan, so Home is built from
/// the index as it is now.
fn reopen(dir: &Path) -> App<HeadlessBackend> {
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app
}

/// Sets the authors of a book in the index.
fn set_authors(app: &App<HeadlessBackend>, id: i64, field: &str) {
    let conn = rusqlite::Connection::open(app.services().library_path()).unwrap();
    conn.execute("UPDATE books SET authors = ?2 WHERE id = ?1", params![id, field])
        .unwrap();
}

/// Sets the series of a book in the index.
fn set_series(app: &App<HeadlessBackend>, id: i64, name: &str, index: f64) {
    let conn = rusqlite::Connection::open(app.services().library_path()).unwrap();
    conn.execute(
        "UPDATE books SET series = ?2, series_index = ?3 WHERE id = ?1",
        params![id, name, index],
    )
    .unwrap();
}

#[test]
fn home_offers_another_unread_book_by_the_author_on_home() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("home-author-offer");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    set_authors(&app, juliet, "Bard");
    set_authors(&app, leaves, "Bard");
    set_authors(&app, pdf, "Someone Else");
    drop(app);
    let mut app = reopen(&dir);
    // Juliet is the book to continue, so Home shows its author.
    app.services_mut().library.record_open(juliet, 0.4).unwrap();
    drop(app);
    let mut app = reopen(&dir);

    assert!(
        app.find_rect("more-by-author").is_some(),
        "the author of the book on Home has another unread book"
    );
    tap_key(&mut app, "more-by-author");
    assert_eq!(
        app.top_route(),
        Some(&Route::Reader { book: leaves }),
        "the tile opens the unread book by that author"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn home_hides_the_author_tile_when_no_other_book_by_the_author_is_unread() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("home-author-none");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    set_authors(&app, juliet, "Bard");
    set_authors(&app, leaves, "Bard");
    drop(app);
    let mut app = reopen(&dir);
    app.services_mut().library.record_open(juliet, 0.4).unwrap();
    // Leaves is started, so it is not unread.
    app.services_mut().library.record_open(leaves, 0.2).unwrap();
    drop(app);
    let app = reopen(&dir);

    assert!(
        app.find_rect("continue").is_some(),
        "Home shows the book to continue"
    );
    assert!(
        app.find_rect("more-by-author").is_none(),
        "no other unread book by the author, so no tile"
    );
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn home_shows_the_next_in_series_before_the_author_tile_and_never_the_same_book() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("home-author-next");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    set_authors(&app, juliet, "Bard");
    set_authors(&app, leaves, "Bard");
    set_authors(&app, pdf, "Bard");
    set_series(&app, juliet, "Verona", 1.0);
    set_series(&app, leaves, "Verona", 2.0);
    drop(app);
    let mut app = reopen(&dir);
    app.services_mut().library.record_open(juliet, 0.4).unwrap();
    drop(app);
    let mut app = reopen(&dir);

    assert!(
        app.find_rect("next-in-series").is_some(),
        "the next number of the series is offered"
    );
    assert!(
        app.find_rect("more-by-author").is_some(),
        "the author has a third unread book"
    );
    let next = app.find_rect("next-in-series").unwrap();
    let author = app.find_rect("more-by-author").unwrap();
    assert!(next.x < author.x, "the next book in series comes first");

    tap_key(&mut app, "more-by-author");
    assert_eq!(
        app.top_route(),
        Some(&Route::Reader { book: pdf }),
        "the author tile skips the book the series tile offers"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn home_leaves_out_the_author_tile_when_the_only_book_is_the_next_in_series() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("home-author-same");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    set_authors(&app, juliet, "Bard");
    set_authors(&app, leaves, "Bard");
    set_series(&app, juliet, "Verona", 1.0);
    set_series(&app, leaves, "Verona", 2.0);
    drop(app);
    let mut app = reopen(&dir);
    app.services_mut().library.record_open(juliet, 0.4).unwrap();
    drop(app);
    let app = reopen(&dir);

    assert!(
        app.find_rect("next-in-series").is_some(),
        "the next book in series is offered"
    );
    assert!(
        app.find_rect("more-by-author").is_none(),
        "the only other book by the author is already offered, so no second tile"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn home_next_and_author_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("home-author-golden");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    set_authors(&app, juliet, "Bard");
    set_authors(&app, leaves, "Bard");
    set_authors(&app, pdf, "Bard");
    set_series(&app, juliet, "Verona", 1.0);
    set_series(&app, leaves, "Verona", 2.0);
    drop(app);
    let mut app = reopen(&dir);
    app.services_mut().library.record_open(juliet, 0.4).unwrap();
    drop(app);
    let mut app = reopen(&dir);
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("home_next_author.png"), 14);
    let _ = fs::remove_dir_all(dir);
}
