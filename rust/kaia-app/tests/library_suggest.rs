//! Suggestions on the Library search page: the rows above the books while the
//! reader types, the recent searches for a short field, and what a tap on a
//! suggestion does.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use kaia_app::{App, HeadlessBackend, Route};
use kaia_gfx::golden::assert_matches_golden;
use kaia_input::Key;

mod common;

use common::*;

/// Types `text` into the focused field, one key at a time.
fn type_text(app: &mut App<HeadlessBackend>, text: &str) {
    for c in text.chars() {
        app.key(Key::Char(c)).unwrap();
    }
}

/// The top-left corner of a widget as (y, x), for ordering rows.
fn position(app: &App<HeadlessBackend>, key: &str) -> (i32, i32) {
    let r = app
        .find_rect(key)
        .unwrap_or_else(|| panic!("no widget {key:?} on the top window"));
    (r.y, r.x)
}

/// Opens local search through the My books overflow menu.
fn open_local_search(app: &mut App<HeadlessBackend>) {
    tap_key(app, "library-options");
    tap_key(app, "library-search-open");
}

fn open_library(app: &mut App<HeadlessBackend>) {
    tap_key(app, "library");
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
}

/// The library app with the test books indexed. `extra` names files copied into
/// the book folder as well.
fn indexed(name: &str, extra: &[(&str, &str)]) -> (App<HeadlessBackend>, PathBuf) {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    for (from, to) in extra {
        fs::copy(lib.join(from), lib.join(to)).unwrap();
    }
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    (app, dir)
}

/// Opens the search page and focuses its field.
fn open_search(app: &mut App<HeadlessBackend>) {
    open_library(app);
    open_local_search(app);
    tap_key(app, "search-field");
}

#[test]
fn a_short_field_shows_the_recent_searches_and_no_suggestions() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("suggest-short", &[]);
    let leaves = book_id(&app, "leaves.epub");
    let juliet = book_id(&app, "juliet.epub");
    open_search(&mut app);
    type_text(&mut app, "leaves");
    wait_for(&mut app, "the suggestion", |a| a.find_rect("suggest-0").is_some());
    tap_key(&mut app, "library-search-close");
    open_local_search(&mut app);
    tap_key(&mut app, "search-field");

    type_text(&mut app, "l");
    assert!(
        app.find_rect("recent-0").is_some(),
        "one character shows the recent searches"
    );
    assert!(
        app.find_rect("suggest-0").is_none(),
        "no suggestions for one character"
    );
    assert!(app.find_rect(&format!("book-{leaves}")).is_none());
    assert!(app.find_rect(&format!("book-{juliet}")).is_none());
}

#[test]
fn suggestions_sit_above_the_books_they_name() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("suggest-order", &[]);
    let leaves = book_id(&app, "leaves.epub");
    open_search(&mut app);
    type_text(&mut app, "walt");
    wait_for(&mut app, "the suggestion", |a| a.find_rect("suggest-0").is_some());
    wait_for(&mut app, "the book", |a| {
        a.find_rect(&format!("book-{leaves}")).is_some()
    });

    assert!(
        position(&app, "suggest-0").0 < position(&app, &format!("book-{leaves}")).0,
        "the author suggestion comes before the books"
    );
    assert!(app.find_rect("suggest-1").is_none(), "one author, one suggestion");
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
}

#[test]
fn tapping_an_author_suggestion_searches_it_and_records_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("suggest-author", &[]);
    let leaves = book_id(&app, "leaves.epub");
    let juliet = book_id(&app, "juliet.epub");
    open_search(&mut app);
    type_text(&mut app, "walt");
    wait_for(&mut app, "the suggestion", |a| a.find_rect("suggest-0").is_some());

    tap_key(&mut app, "suggest-0");

    assert!(
        app.find_rect(&format!("book-{leaves}")).is_some(),
        "the author's book is in the results"
    );
    assert!(
        app.find_rect(&format!("book-{juliet}")).is_none(),
        "other books are not"
    );
    assert!(
        app.find_rect("suggest-0").is_none(),
        "the suggestions go once one is chosen"
    );
    assert_eq!(
        library_db(&app).recent_searches(10).unwrap(),
        ["Walt Whitman"],
        "the chosen suggestion is the newest recent search"
    );
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
}

#[test]
fn tapping_a_title_with_one_book_opens_the_book() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("suggest-title", &[]);
    let juliet = book_id(&app, "juliet.epub");
    open_search(&mut app);
    type_text(&mut app, "romeo");
    wait_for(&mut app, "the suggestion", |a| a.find_rect("suggest-0").is_some());

    tap_key(&mut app, "suggest-0");

    assert_eq!(app.top_route(), Some(&Route::Reader { book: juliet }));
    assert_eq!(
        library_db(&app).recent_searches(10).unwrap(),
        ["Romeo and Juliet"],
        "the search is recorded before the book opens"
    );
}

#[test]
fn tapping_a_title_with_two_books_searches_for_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("suggest-twins", &[("juliet.epub", "juliet copy.epub")]);
    let juliet = book_id(&app, "juliet.epub");
    let copy = book_id(&app, "juliet copy.epub");
    open_search(&mut app);
    type_text(&mut app, "romeo");
    wait_for(&mut app, "the suggestion", |a| a.find_rect("suggest-0").is_some());
    assert!(
        app.find_rect("suggest-1").is_none(),
        "two books, one title, one suggestion"
    );

    tap_key(&mut app, "suggest-0");

    assert_eq!(
        app.top_route(),
        Some(&Route::Library { page: 0 }),
        "no book opens"
    );
    assert!(app.find_rect(&format!("book-{juliet}")).is_some());
    assert!(app.find_rect(&format!("book-{copy}")).is_some());
    assert_eq!(
        library_db(&app).recent_searches(10).unwrap(),
        ["Romeo and Juliet"]
    );
}

#[test]
fn library_search_with_suggestions_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("golden-suggest", &[]);
    open_search(&mut app);
    type_text(&mut app, "walt");
    let leaves = book_id(&app, "leaves.epub");
    wait_for(&mut app, "the suggestion", |a| a.find_rect("suggest-0").is_some());
    wait_for(&mut app, "the book", |a| {
        a.find_rect(&format!("book-{leaves}")).is_some()
    });
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_suggestions.png"), 14);
}
