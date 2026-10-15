//! The Authors view of the Library: the list of authors with their counts, the
//! books of an open author in the layout of the Books tab, and the sort rules.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kaia_app::{App, HeadlessBackend, Route};
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Rect;
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

/// The width of a widget in pixels.
fn width(app: &App<HeadlessBackend>, key: &str) -> u32 {
    app.find_rect(key)
        .unwrap_or_else(|| panic!("no widget {key:?} on the top window"))
        .w
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

/// Sets the authors of a book in the index. The field is stored as written, so
/// several names are joined with a line feed, as the engine joins them.
fn set_authors(app: &App<HeadlessBackend>, id: i64, field: Option<&str>) {
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

/// Opens the Authors tab and the author at row `n`.
fn open_author(app: &mut App<HeadlessBackend>, n: usize) {
    tap_key(app, &key("library:view:authors"));
    assert!(
        app.find_rect("author-list").is_some(),
        "the Authors tab lists authors"
    );
    tap_key(app, &format!("author-{n}"));
    assert!(
        app.find_rect("author-up").is_some(),
        "an open author has a row to go back"
    );
}

#[test]
fn authors_tab_lists_each_author_and_opens_its_books() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("authors-view");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    set_authors(&app, juliet, Some("William Shakespeare"));
    set_authors(&app, leaves, Some("Walt Whitman"));
    set_authors(&app, pdf, Some("William Shakespeare\nWalt Whitman"));
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);

    tap_key(&mut app, &key("library:view:authors"));
    assert!(
        app.find_rect("author-0").is_some(),
        "William Shakespeare is the first surname"
    );
    assert!(
        app.find_rect("author-1").is_some(),
        "Walt Whitman is the second surname"
    );
    assert!(app.find_rect("author-2").is_none(), "two authors, no more rows");
    assert!(
        app.find_rect("library-filter").is_none(),
        "the Authors tab has no filter"
    );
    assert!(
        app.find_rect("library-sort").is_none(),
        "the Authors tab has no sort"
    );

    // Shakespeare has two books, including the shared book.
    tap_key(&mut app, "author-0");
    assert!(app.find_rect(&format!("book-{juliet}")).is_some());
    assert!(app.find_rect(&format!("book-{pdf}")).is_some());
    assert!(
        app.find_rect(&format!("book-{leaves}")).is_none(),
        "Whitman's book is not under Shakespeare"
    );

    tap_key(&mut app, "author-up");
    assert!(
        app.find_rect("author-list").is_some(),
        "Back returns to the list of authors"
    );
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn authors_sort_by_name_ignoring_case_and_a_leading_the() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("authors-sort");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    set_authors(&app, juliet, Some("The Bard"));
    set_authors(&app, leaves, Some("zola"));
    set_authors(&app, pdf, Some("Austen"));
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);
    tap_key(&mut app, &key("library:view:authors"));

    // Austen, then The Bard (sorted as "bard"), then Zola.
    let rows = ["author-0", "author-1", "author-2"];
    assert!(rows.iter().all(|r| app.find_rect(r).is_some()));
    assert!(position(&app, "author-0") < position(&app, "author-1"));
    assert!(position(&app, "author-1") < position(&app, "author-2"));
    tap_key(&mut app, "author-1");
    assert!(
        app.find_rect(&format!("book-{juliet}")).is_some(),
        "row 1 is The Bard, whose book is Romeo and Juliet"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_author_lists_its_books_in_series_order() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("authors-series");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    set_authors(&app, juliet, Some("Shared Author"));
    set_authors(&app, leaves, Some("Shared Author"));
    set_series(&app, juliet, "Verona", 2.0);
    set_series(&app, leaves, "Verona", 1.0);
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);
    open_author(&mut app, 0);
    tap_key(&mut app, "library-sort");
    tap_key(&mut app, "sort-series");

    let leaves_key = format!("book-{leaves}");
    let juliet_key = format!("book-{juliet}");
    assert!(
        position(&app, &leaves_key) < position(&app, &juliet_key),
        "book 1 of the series comes before book 2"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn an_author_shows_the_books_in_the_layout_of_the_books_tab() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("authors-layout");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    set_authors(&app, juliet, Some("Shared Author"));
    set_authors(&app, leaves, Some("Shared Author"));
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);

    // Covers: three columns, so each cover is narrower than a list row.
    open_author(&mut app, 0);
    let covers_width = width(&app, &format!("book-{juliet}"));

    // The layout choice is on the Books tab, so switch there, then back to Authors.
    tap_key(&mut app, "library-view-books");
    tap_key(&mut app, &key("library:view:list"));
    open_author(&mut app, 0);
    let list_width = width(&app, &format!("book-{juliet}"));
    assert!(
        list_width > covers_width,
        "the list layout gives each book a full row ({list_width} px against {covers_width} px)"
    );
    assert!(app.find_rect(&format!("book-{leaves}")).is_some());
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn the_authors_tab_keeps_the_route_and_has_no_unknown_commands() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("authors-routes");
    open_library(&mut app);
    tap_key(&mut app, &key("library:view:authors"));
    tap_key(&mut app, "author-0");
    tap_key(&mut app, "author-up");
    tap_key(&mut app, "library-view-books");
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert!(
        app.unknown_commands().is_empty(),
        "every id has an owner: {:?}",
        app.unknown_commands()
    );
}

#[test]
fn authors_list_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("golden-authors");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    set_authors(&app, juliet, Some("William Shakespeare"));
    set_authors(&app, leaves, Some("Walt Whitman"));
    set_authors(&app, pdf, Some("William Shakespeare\nWalt Whitman"));
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);
    tap_key(&mut app, &key("library:view:authors"));
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_authors.png"), 14);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn author_books_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = indexed("golden-author-books");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    set_authors(&app, juliet, Some("William Shakespeare"));
    set_authors(&app, leaves, Some("Walt Whitman"));
    set_authors(&app, pdf, Some("William Shakespeare\nWalt Whitman"));
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);
    open_author(&mut app, 1);
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_author_books.png"), 14);
    let _ = (juliet, leaves, pdf);
    let _ = fs::remove_dir_all(dir);
}
