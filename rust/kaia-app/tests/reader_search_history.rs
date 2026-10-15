//! Headless tests for the in-book search history. An empty search field lists the
//! book's recent searches, newest first. A tap runs one again. Clear history empties
//! the list. The lists survive a restart, are kept per book, and go with erase.

use std::path::Path;
use std::time::{Duration, Instant};

use kaia_app::surfaces::reader::panels::SearchRow;
use kaia_app::surfaces::reader::search_history;
use kaia_app::App;
use kaia_app::HeadlessBackend;
use kaia_app::Route;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Point;
use kaia_platform::reset;
use kaia_ui::components::{Label, Paragraph};
use kaia_ui::composite::PagedList;
use kaia_ui::controls::TextField;

mod common;

use common::*;

/// Opens `name` from the library in a new app on the data folder `dir`.
fn open_book(dir: &Path, name: &str) -> App<HeadlessBackend> {
    let lib = library_with_books(dir);
    let mut app = make_app(dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    open_from_library(&mut app, name);
    app
}

/// Opens the search panel from the reader overlay.
fn open_search(app: &mut App<HeadlessBackend>) {
    let p = Point::new(app.screen().w as i32 / 2, app.screen().h as i32 / 2);
    app.tap(p).unwrap();
    wait_reader(app);
    assert_eq!(app.top_route(), Some(&Route::Overlay), "the overlay opens");
    app.update_top::<Label, _>("overlay-clock", |text, cx| {
        text.set_text("09:41");
        cx.request_layout();
        cx.request_paint();
    });
    app.iterate(None).unwrap();
    tap_key(app, "overlay-search");
}

fn text_of(app: &mut App<HeadlessBackend>, key: &str) -> String {
    let mut text = None;
    app.update_top::<Paragraph, _>(key, |p, _| text = Some(p.text().to_string()));
    if text.is_none() {
        app.update_top::<Label, _>(key, |l, _| text = Some(l.text().to_string()));
    }
    text.unwrap_or_else(|| panic!("no text widget {key:?} on the top window"))
}

fn field_text(app: &mut App<HeadlessBackend>) -> String {
    let mut text = None;
    app.update_top::<TextField, _>("search-field", |field, _| text = Some(field.text().to_string()));
    text.expect("the search field is on the top window")
}

/// The labels of the rows in the search list, in order.
fn listed(app: &mut App<HeadlessBackend>) -> Vec<String> {
    let mut labels = Vec::new();
    app.update_top::<PagedList<SearchRow>, _>("search-results", |list, _| {
        labels = list.items().iter().map(|row| row.label.clone()).collect();
    });
    labels
}

/// Types `text` into the search field and taps Search. Waits until the search ends.
fn search_for(app: &mut App<HeadlessBackend>, text: &str) {
    app.update_top::<TextField, _>("search-field", |field, cx| {
        field.set_text(text);
        cx.request_paint();
    });
    tap_key(app, "search-go");
    let deadline = Instant::now() + Duration::from_secs(60);
    while text_of(app, "search-status").starts_with("Searching") {
        assert!(Instant::now() < deadline, "the search for {text:?} ends");
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
}

/// Empties the search field and taps Search. An empty field lists the recent searches.
fn show_recent(app: &mut App<HeadlessBackend>) {
    app.update_top::<TextField, _>("search-field", |field, cx| {
        field.set_text("");
        cx.request_paint();
    });
    tap_key(app, "search-go");
}

#[test]
fn three_searches_list_newest_first() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("search-history-order");
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    for text in ["Romeo", "Juliet", "love"] {
        search_for(&mut app, text);
    }
    show_recent(&mut app);
    assert_eq!(listed(&mut app), vec!["love", "Juliet", "Romeo"]);
    assert_eq!(
        text_of(&mut app, "search-status"),
        "Recent searches. Tap one to run it again."
    );
}

#[test]
fn a_repeat_moves_to_the_front_without_a_duplicate() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("search-history-repeat");
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    for text in ["Romeo", "Juliet", "romeo"] {
        search_for(&mut app, text);
    }
    show_recent(&mut app);
    assert_eq!(
        listed(&mut app),
        vec!["romeo", "Juliet"],
        "the repeat takes the front, in the new spelling, and appears once"
    );
}

#[test]
fn the_eleventh_search_drops_the_oldest() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("search-history-cap");
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    for i in 1..=11 {
        search_for(&mut app, &format!("query {i}"));
    }
    show_recent(&mut app);
    let labels = listed(&mut app);
    assert_eq!(labels.len(), 10, "ten searches are kept");
    assert_eq!(labels.first().map(String::as_str), Some("query 11"));
    assert_eq!(labels.last().map(String::as_str), Some("query 2"));
    assert!(!labels.iter().any(|l| l == "query 1"), "the oldest is gone");
}

#[test]
fn the_history_survives_a_restart() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("search-history-restart");
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    search_for(&mut app, "Romeo");
    search_for(&mut app, "Juliet");
    drop(app);

    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    assert_eq!(field_text(&mut app), "", "a new app opens the field empty");
    assert_eq!(listed(&mut app), vec!["Juliet", "Romeo"]);
}

#[test]
fn tapping_an_entry_runs_that_search() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("search-history-tap");
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    search_for(&mut app, "Romeo");
    search_for(&mut app, "Juliet");
    show_recent(&mut app);
    assert_eq!(listed(&mut app), vec!["Juliet", "Romeo"]);

    tap_key(&mut app, "reader-search-recent-1");
    assert_eq!(
        field_text(&mut app),
        "Romeo",
        "the field shows the search that ran"
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    while !text_of(&mut app, "search-status").ends_with("matches.") {
        assert!(Instant::now() < deadline, "the search finds Romeo");
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
    let hits = listed(&mut app);
    assert!(
        hits.first().is_some_and(|l| l.starts_with("Page ")),
        "the panel lists the hits, got {hits:?}"
    );
    show_recent(&mut app);
    assert_eq!(
        listed(&mut app),
        vec!["Romeo", "Juliet"],
        "running a recent search moves it to the front"
    );
}

#[test]
fn clear_history_empties_the_list_and_the_file() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("search-history-clear");
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    search_for(&mut app, "Romeo");
    search_for(&mut app, "Juliet");
    show_recent(&mut app);
    assert_eq!(listed(&mut app), vec!["Juliet", "Romeo"]);

    tap_key(&mut app, "search-history-clear");
    assert!(listed(&mut app).is_empty(), "Clear history empties the list");
    assert_eq!(
        text_of(&mut app, "search-status"),
        "Type a word and press Search."
    );
    drop(app);

    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    assert!(
        listed(&mut app).is_empty(),
        "the cleared list stays empty after a restart"
    );
}

#[test]
fn each_book_has_its_own_list() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("search-history-books");
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    search_for(&mut app, "Romeo");
    drop(app);

    let mut app = open_book(&dir, "leaves.epub");
    open_search(&mut app);
    assert!(listed(&mut app).is_empty(), "a book with no searches lists none");
    search_for(&mut app, "leaf");
    drop(app);

    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    assert_eq!(
        listed(&mut app),
        vec!["Romeo"],
        "the first book keeps its own list"
    );
}

#[test]
fn search_history_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("search-history-golden");
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    for text in ["Romeo", "Juliet", "love"] {
        search_for(&mut app, text);
    }
    drop(app);

    // A new app opens the panel with an empty field, so the list shows and no toast does.
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    assert_eq!(listed(&mut app).len(), 3);
    assert_matches_golden(app.frame(), &golden("reader-search-history.png"), 14);
}

#[test]
fn erase_removes_the_history_and_reset_settings_keeps_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("search-history-reset");
    let data = dir.join("data");
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    search_for(&mut app, "Romeo");
    drop(app);

    reset::reset_settings(&data).unwrap();
    assert!(
        search_history::path(&data).exists(),
        "reset settings keeps the searches"
    );
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    assert_eq!(listed(&mut app), vec!["Romeo"]);
    drop(app);

    reset::erase(&data).unwrap();
    assert!(
        !search_history::path(&data).exists(),
        "erase removes the searches"
    );
    let mut app = open_book(&dir, "juliet.epub");
    open_search(&mut app);
    assert!(listed(&mut app).is_empty(), "after erase the list is empty");
}
