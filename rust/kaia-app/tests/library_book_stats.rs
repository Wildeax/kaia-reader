//! The reading statistics of a book, opened from its details in the Library. The
//! figures come from a fixture statistics database written into the data folder.
//!
//! Fixture for juliet.epub: three sessions over two local days, on pages 1 to 4,
//! with 10 pages in the book. The page shows 4 min read, 4 of 10 pages, 2 days, a
//! 1 min average per page and 6 min left.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use kaia_app::surfaces::library::book_stats;
use kaia_app::{App, HeadlessBackend};
use kaia_display::InputEvent;
use kaia_gfx::golden::assert_matches_golden;
use kaia_input::{Phase, TouchEvent};
use kaia_library::Book;
use rusqlite::params;

use common::*;

/// 13 May 2024 00:00 UTC. The spans are placed so that the days stay the same in
/// any time zone from UTC-8 to UTC+12.
const MAY_13: i64 = 1_715_558_400;
const MAY_14: i64 = MAY_13 + 86_400;

/// Page, start time and seconds. Page 3 is read twice; the cap counts it at 120 s.
fn sessions() -> Vec<(i64, i64, i64)> {
    vec![
        (1, MAY_13 + 8 * 3600, 60),
        (2, MAY_13 + 8 * 3600 + 60, 30),
        (3, MAY_13 + 20 * 3600, 100),
        (3, MAY_14 + 7 * 3600, 60),
        (4, MAY_14 + 7 * 3600 + 60, 40),
    ]
}

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

/// Opens the Library and shows the Books tab as a list.
fn open_library(app: &mut App<HeadlessBackend>) {
    if app.top_route() != Some(&kaia_app::Route::Library { page: 0 }) {
        tap_key(app, "library");
    }
    tap_key(app, "library-view-list");
}

/// Holds a finger on `widget` until the details sheet is up, then lifts it.
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

/// The title KOReader keys the book by: the metadata title, or the file name.
fn stats_title(book: &Book) -> String {
    match &book.title {
        Some(title) if !title.trim().is_empty() => title.clone(),
        _ => book
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

/// Writes a book row and its page records into the statistics database of the app,
/// keyed the way the reader keys it. `spans` are (page, start, seconds).
fn write_statistics(app: &App<HeadlessBackend>, book: &Book, spans: &[(i64, i64, i64)]) {
    let path = app.services().paths.data_dir.join("statistics.sqlite3");
    drop(kaia_stats::Stats::open(&path).unwrap());
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO book (title, authors, notes, last_open, highlights, pages, series, language, md5, \
         total_read_time, total_read_pages) VALUES (?1, ?2, 0, ?3, 0, 10, 'N/A', 'N/A', ?4, 0, 0)",
        params![
            stats_title(book),
            book.authors.as_deref().unwrap_or("N/A"),
            MAY_14 + 7 * 3600 + 120,
            book.partial_md5,
        ],
    )
    .unwrap();
    let id = conn.last_insert_rowid();
    for (page, start, seconds) in spans {
        conn.execute(
            "INSERT INTO page_stat_data (id_book, page, start_time, duration, total_pages) \
             VALUES (?1, ?2, ?3, ?4, 10)",
            params![id, page, start, seconds],
        )
        .unwrap();
    }
}

/// The value of the row `key` in the figures of the statistics database.
fn figure_values(app: &App<HeadlessBackend>, book: &Book) -> Vec<(&'static str, String)> {
    let path = app.services().paths.data_dir.join("statistics.sqlite3");
    let mut stats = kaia_stats::Stats::open(&path).unwrap();
    let found = stats
        .book(
            &book.partial_md5,
            &stats_title(book),
            book.authors.as_deref().unwrap_or("N/A"),
        )
        .unwrap()
        .expect("the book has a row");
    book_stats::figures(&found, MAY_14 + 12 * 3600)
        .into_iter()
        .map(|f| (f.key, f.value))
        .collect()
}

#[test]
fn the_statistics_button_needs_a_row_in_the_statistics_database() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("book-stats-button");
    let juliet = book_id(&app, "juliet.epub");
    open_library(&mut app);

    hold_for_details(&mut app, &format!("book-{juliet}"));
    assert!(
        app.find_rect(&key(&format!("library:stats:open:{juliet}")))
            .is_none(),
        "a book that was never opened has no statistics button"
    );
    tap_key(&mut app, &key("library:panel:close"));

    let book = library_db(&app).book(juliet).unwrap().unwrap();
    write_statistics(&app, &book, &sessions());
    hold_for_details(&mut app, &format!("book-{juliet}"));
    assert!(
        app.find_rect(&key(&format!("library:stats:open:{juliet}")))
            .is_some(),
        "a book with a statistics row has the button next to the rating"
    );
}

#[test]
fn the_page_shows_the_totals_of_the_statistics_database() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("book-stats-page");
    let juliet = book_id(&app, "juliet.epub");
    let book = library_db(&app).book(juliet).unwrap().unwrap();
    write_statistics(&app, &book, &sessions());
    open_library(&mut app);

    hold_for_details(&mut app, &format!("book-{juliet}"));
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_details_statistics.png"), 14);

    tap_key(&mut app, &key(&format!("library:stats:open:{juliet}")));
    for row in [
        "book-stats-time",
        "book-stats-pages",
        "book-stats-days",
        "book-stats-first",
        "book-stats-last",
        "book-stats-average",
        "book-stats-left",
    ] {
        assert!(app.find_rect(row).is_some(), "the page has the row {row}");
    }
    assert!(app.find_rect("book-stats-empty").is_none());

    let values = figure_values(&app, &book);
    let value = |key: &str| {
        values
            .iter()
            .find(|(k, _)| *k == key)
            .unwrap_or_else(|| panic!("no figure {key}"))
            .1
            .clone()
    };
    assert_eq!(value("book-stats-time"), "4 min");
    assert_eq!(value("book-stats-pages"), "4 of 10 (40%)");
    assert_eq!(value("book-stats-days"), "2 days");
    assert_eq!(value("book-stats-average"), "1 min");
    assert_eq!(value("book-stats-left"), "6 min");

    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_book_stats.png"), 14);

    tap_key(&mut app, &key("book-stats-back"));
    assert!(
        app.find_rect(&key(&format!("library:stats:open:{juliet}")))
            .is_some(),
        "Back returns to the details"
    );
}

#[test]
fn a_book_with_no_reading_says_so() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("book-stats-empty");
    let leaves = book_id(&app, "leaves.epub");
    let book = library_db(&app).book(leaves).unwrap().unwrap();
    write_statistics(&app, &book, &[]);
    open_library(&mut app);

    hold_for_details(&mut app, &format!("book-{leaves}"));
    tap_key(&mut app, &key(&format!("library:stats:open:{leaves}")));
    assert!(
        app.find_rect("book-stats-empty").is_some(),
        "the page says that no reading is recorded"
    );
    assert!(app.find_rect("book-stats-time").is_none());
}
