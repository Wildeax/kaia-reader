//! The reader's full refresh cadence, read from `reading.full_refresh_pages`.
//! Each page turn sends one refresh. The tests check the mode of every turn: a
//! chapter start, a count of turns, and every page of a fixed-layout book.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

use kaia_app::surfaces::reader::panels::ContentItem;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::{RefreshMode, Settings, Value};
use kaia_input::Key;
use kaia_ui::composite::PagedList;

mod common;

use common::*;

const CADENCE_KEY: &str = "reading.full_refresh_pages";

/// Writes `value` as the cadence in the data folder of `dir`, or leaves the key
/// unset when `value` is None. The app reads it at startup.
fn write_cadence(dir: &Path, value: Option<f64>) {
    let data = dir.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let path = data.join("settings.json");
    let mut settings = Settings::open(&path);
    if let Some(v) = value {
        settings.save(CADENCE_KEY, Value::from(v));
    }
    settings.flush(&path).unwrap();
}

/// Opens `name` from a library that holds only that test file. `sample.pdf` has 45
/// pages, enough for several turns. `2col.pdf` has two pages, so it cannot show a run.
fn open_test_file(name: &str, dir: &Path, value: Option<f64>) -> App<HeadlessBackend> {
    write_cadence(dir, value);
    let lib = dir.join("books");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::copy(repo_root().join("test").join(name), lib.join(name)).unwrap();
    let mut app = make_app(dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    open_from_library(&mut app, name);
    app
}

/// Opens the reader on `name` with the cadence `value` saved first.
fn open_reader(name: &str, dir: &Path, value: Option<f64>) -> App<HeadlessBackend> {
    write_cadence(dir, value);
    let lib = library_with_books(dir);
    let mut app = make_app(dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    open_from_library(&mut app, name);
    app
}

/// The page the reader shows, 0-based, from the reader window on top.
fn page(app: &mut App<HeadlessBackend>) -> u32 {
    wait_reader(app);
    let mut position = None;
    app.update_top::<kaia_app::surfaces::reader::page::PageView, _>("reader-page", |view, _| {
        position = view.position
    });
    position
        .expect("the reader window is on top and has a position")
        .0
}

/// Turns one page forward. Returns the page turned to and the one refresh it sent.
fn turn_forward(app: &mut App<HeadlessBackend>) -> (u32, RefreshMode) {
    let before = app.refresh_modes().len();
    app.key(Key::RPgFwd).unwrap();
    wait_reader(app);
    let modes = modes_after(app, before);
    assert_eq!(modes.len(), 1, "one page turn sends one refresh, got {modes:?}");
    (page(app), modes[0])
}

/// The pages where a table of contents entry starts, 0-based. Read from the
/// Contents panel, which lists each entry as "Page N". The panel is closed again.
fn chapter_pages(app: &mut App<HeadlessBackend>) -> HashSet<u32> {
    let p = centre(app);
    app.tap(p).unwrap();
    wait_reader(app);
    assert_eq!(app.top_route(), Some(&Route::Overlay));
    tap_key(app, "overlay-contents");
    let mut pages = HashSet::new();
    app.update_top::<PagedList<ContentItem>, _>("contents-list", |list, _| {
        for item in list.items() {
            if let Some(n) = item
                .detail
                .strip_prefix("Page ")
                .and_then(|n| n.parse::<u32>().ok())
            {
                pages.insert(n - 1);
            }
        }
    });
    app.key(Key::Back).unwrap();
    wait_reader(app);
    let p = centre(app);
    app.tap(p).unwrap();
    wait_reader(app);
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));
    pages
}

/// Centre of the screen, for the tap that opens the overlay.
fn centre(app: &App<HeadlessBackend>) -> kaia_gfx::Point {
    kaia_gfx::Point::new(app.screen().w as i32 / 2, app.screen().h as i32 / 2)
}

/// Turns forward until `want` chapter starts have been entered, checking each turn:
/// a turn to a chapter start is full, and every other turn is partial.
fn turn_across_chapters(app: &mut App<HeadlessBackend>, chapters: &HashSet<u32>, want: usize) {
    let mut entered = 0;
    for turn in 0..300 {
        let (page, mode) = turn_forward(app);
        let chapter_start = chapters.contains(&page);
        if chapter_start {
            entered += 1;
            assert_eq!(
                mode,
                RefreshMode::Full,
                "turn {turn} to chapter start page {page} is a full refresh"
            );
        } else {
            assert_eq!(
                mode,
                RefreshMode::Partial,
                "turn {turn} to page {page} inside a chapter is partial"
            );
        }
        if entered >= want {
            return;
        }
    }
    panic!("the book has fewer than {want} chapter starts after the cover");
}

#[test]
fn chapter_setting_refreshes_fully_at_each_chapter_start() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("cadence-chapter");
    let mut app = open_reader("juliet.epub", &dir, Some(0.0));
    let chapters = chapter_pages(&mut app);
    assert!(
        chapters.len() > 2,
        "the test book has chapters, got {}",
        chapters.len()
    );
    turn_across_chapters(&mut app, &chapters, 2);
}

#[test]
fn unset_cadence_is_chapter_by_default() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("cadence-default");
    let mut app = open_reader("juliet.epub", &dir, None);
    let chapters = chapter_pages(&mut app);
    turn_across_chapters(&mut app, &chapters, 2);
}

#[test]
fn page_count_refreshes_every_nth_turn_and_at_chapter_starts() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("cadence-pages");
    let mut app = open_reader("juliet.epub", &dir, Some(3.0));
    let chapters = chapter_pages(&mut app);

    // A turn is full at a chapter start, and on the third turn since the last full one.
    let mut since_full = 0;
    for turn in 0..14 {
        let (page, mode) = turn_forward(&mut app);
        if chapters.contains(&page) || since_full + 1 >= 3 {
            since_full = 0;
            assert_eq!(mode, RefreshMode::Full, "turn {turn} to page {page} is full");
        } else {
            since_full += 1;
            assert_eq!(
                mode,
                RefreshMode::Partial,
                "turn {turn} to page {page} is partial"
            );
        }
    }
}

#[test]
fn saved_count_above_ten_refreshes_every_ten_turns() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("cadence-clamp");
    let mut app = open_reader("juliet.epub", &dir, Some(25.0));
    let chapters = chapter_pages(&mut app);

    let mut since_full = 0;
    let mut count_full = 0;
    for turn in 0..22 {
        let (page, mode) = turn_forward(&mut app);
        let by_count = since_full + 1 >= 10;
        if chapters.contains(&page) || by_count {
            if !chapters.contains(&page) {
                count_full += 1;
            }
            since_full = 0;
            assert_eq!(mode, RefreshMode::Full, "turn {turn} to page {page} is full");
        } else {
            since_full += 1;
            assert_eq!(
                mode,
                RefreshMode::Partial,
                "turn {turn} to page {page} is partial"
            );
        }
    }
    assert!(
        count_full >= 1,
        "the count of 10 gave a full refresh that is not a chapter start"
    );
}

#[test]
fn fixed_layout_pages_refresh_fully_on_every_turn() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("cadence-pdf");
    // Chapter is the default, and a PDF page is a picture, so each turn is full.
    // A fixed-layout page has no text position, so only the refresh mode is checked.
    let mut app = open_test_file("sample.pdf", &dir, None);
    for turn in 0..4 {
        let before = app.refresh_modes().len();
        app.key(Key::RPgFwd).unwrap();
        wait_reader(&mut app);
        let modes = modes_after(&app, before);
        assert_eq!(
            modes,
            vec![RefreshMode::Full],
            "PDF turn {turn} is one full refresh"
        );
    }
}
