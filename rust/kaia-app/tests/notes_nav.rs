//! Notes keeps the three-tab navigation at the screen edge. Settings opens
//! through More and closes back to More.

mod common;

use std::fs;
use std::path::Path;

use common::*;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_book::state::{STATE_FORMAT, STATE_VERSION};
use kaia_events::value::{Key as TableKey, Table, Value};
use kaia_events::Settings;
use kaia_gfx::Rect;
use kaia_id::partial_md5;
use kaia_input::Key;

fn table(pairs: Vec<(&str, Value)>) -> Table {
    pairs.into_iter().map(|(k, v)| (TableKey::from(k), v)).collect()
}

/// A bookmark: a page with no drawer, text or note.
fn bookmark(id: i64) -> Value {
    Value::Table(table(vec![
        ("kaia_id", Value::from(id)),
        ("datetime", Value::from("2026-10-09 10:00:00")),
        ("page", Value::from(id)),
        ("pageno", Value::from(id)),
    ]))
}

/// A highlight with its text.
fn highlight(id: i64) -> Value {
    Value::Table(table(vec![
        ("kaia_id", Value::from(id)),
        ("datetime", Value::from("2026-10-09 10:00:00")),
        ("page", Value::from(id)),
        ("pageno", Value::from(id)),
        ("drawer", Value::from("lighten")),
        ("text", Value::from(format!("Quote {id}").as_str())),
    ]))
}

/// Writes a per-book state file in the `kaia-book` format.
fn write_state(data: &Path, md5: &str, title: &str, annotations: Vec<Value>) {
    let dir = data.join("kaia").join("books");
    fs::create_dir_all(&dir).unwrap();
    let doc = Value::Table(table(vec![
        ("partial_md5", Value::from(md5)),
        ("file_size", Value::from(1i64)),
        ("title", Value::from(title)),
    ]));
    let root = table(vec![
        ("format", Value::from(STATE_FORMAT)),
        ("version", Value::from(STATE_VERSION)),
        ("doc", doc),
        ("annotations", Value::array(annotations)),
    ]);
    Settings::from_table(root)
        .flush(&dir.join(format!("{md5}.json")))
        .unwrap();
}

/// An app with the test library and one book with `annotations`. Returns the app
/// and the partial MD5 of the book.
fn app_with_book(name: &str, annotations: Vec<Value>) -> (App<HeadlessBackend>, String) {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    let juliet = partial_md5(&lib.join("juliet.epub")).unwrap();
    write_state(&dir.join("data"), &juliet, "Juliet", annotations);
    let mut app = make_app(&dir, vec![lib.clone()]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| {
        !a.scan_running()
            && a.services()
                .library
                .book_by_path(&lib.join("juliet.epub"))
                .ok()
                .flatten()
                .is_some()
    });
    (app, juliet)
}

/// The nav bar rect on the top window.
fn nav_rect(app: &App<HeadlessBackend>) -> Rect {
    app.find_rect("nav-bar").expect("the navigation bar")
}

/// Checks More and its Settings dialog, then returns to Notes.
fn more_nav_rect(app: &mut App<HeadlessBackend>) -> Rect {
    tap_key(app, "nav-more");
    assert_eq!(app.top_route(), Some(&Route::More));
    let rect = nav_rect(app);
    tap_key(app, "more-settings");
    assert!(app.find_rect("nav-bar").is_none(), "settings is a dialog");
    tap_key(app, "settings-close");
    assert_eq!(app.top_route(), Some(&Route::More));
    tap_key(app, "more-notes");
    assert_eq!(app.top_route(), Some(&Route::Notes));
    rect
}

/// The navigation stays aligned with More at the screen edge.
fn assert_bottom(app: &App<HeadlessBackend>, reference: Rect, what: &str) {
    let nav = nav_rect(app);
    assert_eq!(nav.y, reference.y, "{what}: the nav bar is at the More y");
    assert_eq!(
        nav.bottom(),
        app.screen().h as i32,
        "{what}: the nav bar ends at the screen edge"
    );
}

#[test]
fn notes_empty_nav_bar_sits_at_the_bottom_and_opens_settings() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("notes-nav-empty");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    tap_key(&mut app, "nav-notes");
    assert_eq!(app.top_route(), Some(&Route::Notes));
    assert!(app.find_rect("notes-export").is_some(), "the Notes root is shown");

    let reference = more_nav_rect(&mut app);
    assert_bottom(&app, reference, "Notes with no notes");

    tap_key(&mut app, "nav-settings");
    assert_eq!(
        app.top_route(),
        Some(&Route::Settings),
        "the Settings button opens Settings"
    );
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn notes_nav_bar_stays_at_the_bottom_with_few_notes_and_no_match() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, juliet) = app_with_book("notes-nav-few", vec![highlight(1), highlight(2)]);
    tap_key(&mut app, "nav-notes");
    let reference = more_nav_rect(&mut app);
    assert_bottom(&app, reference, "Notes with two highlights");

    // The book has no bookmark, so the Bookmarks filter matches nothing on its page.
    tap_key(&mut app, &format!("notes-book-{juliet}"));
    tap_key(&mut app, "notes-filter-bookmarks");
    assert!(
        app.find_rect("notes-filter-bookmarks").is_some(),
        "the filter row stays on the book page"
    );
    assert_bottom(&app, reference, "a book page with no matching entries");
}

#[test]
fn notes_nav_bar_stays_at_the_bottom_with_a_full_page() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let annotations = (1..=40).map(bookmark).collect();
    let (mut app, _) = app_with_book("notes-nav-full", annotations);
    tap_key(&mut app, "nav-notes");
    let reference = more_nav_rect(&mut app);
    assert_bottom(&app, reference, "Notes with a full page of entries");
}

#[test]
fn other_surfaces_keep_the_nav_bar_at_the_bottom_with_little_content() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("notes-nav-other");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    assert_eq!(app.top_route(), Some(&Route::Home));
    let reference = nav_rect(&app);
    assert_bottom(&app, reference, "Home with no books");
    tap_key(&mut app, "nav-library");
    assert_bottom(&app, reference, "My books with no books");
    tap_key(&mut app, "nav-more");
    assert_bottom(&app, reference, "More");
    tap_key(&mut app, "more-settings");
    assert!(app.find_rect("nav-bar").is_none());
    tap_key(&mut app, "settings-search");
    tap_key(&mut app, "settings-search-field");
    for c in "zzqx".chars() {
        app.key(Key::Char(c)).unwrap();
    }
    app.key(Key::Press).unwrap();
    assert!(app.find_rect("settings-hit-0").is_none(), "the query has no hit");
    assert!(
        app.find_rect("nav-bar").is_none(),
        "search stays in the settings dialog"
    );
    tap_key(&mut app, "settings-close");
    assert_eq!(app.top_route(), Some(&Route::More));
    assert_bottom(&app, reference, "More after settings closes");
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}
