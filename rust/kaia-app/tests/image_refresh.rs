//! The full refresh for image pages. A turn to a page with a picture across it
//! is full, and so is the turn after it. Text pages stay partial. The chapter
//! rule still applies, and the test checks both directions.
//!
//! The books are written by the EPUB writer that kaia-book's tests use.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::time::Duration;

use kaia_app::surfaces::reader::book_dialogs::ContentsList;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::RefreshMode;
use kaia_input::Key;

mod common;
#[path = "../../kaia-book/tests/common/epub.rs"]
mod epub;

use common::*;
use RefreshMode::{Full, Partial};

/// Each section starts a page: 0 chapter one, 1 text, 2 picture, 3 text after it,
/// 4 chapter two, 5 text, 6 picture, 7 text after it, 8 text.
const PICTURE_BOOK: &str = r#"<h1 id="ch1">Chapter one</h1>
<p style="page-break-before: always">Still chapter one.</p>
<div style="page-break-before: always"><img src="pic.png" style="width: 100%"/></div>
<p style="page-break-before: always">After the picture.</p>
<h1 id="ch2" style="page-break-before: always">Chapter two</h1>
<p style="page-break-before: always">Still chapter two.</p>
<div style="page-break-before: always"><img src="pic.png" style="width: 100%"/></div>
<p style="page-break-before: always">After the second picture.</p>
<p style="page-break-before: always">Last text.</p>"#;

/// Each section starts a page, and chapter two opens on page 2.
const TEXT_BOOK: &str = r#"<h1 id="ch1">Chapter one</h1>
<p>Text.</p>
<p style="page-break-before: always">Second page.</p>
<h1 id="ch2" style="page-break-before: always">Chapter two</h1>
<p style="page-break-before: always">Third page.</p>
<p style="page-break-before: always">Fourth page.</p>"#;

const CHAPTERS: &[(&str, &str)] = &[("Chapter one", "ch1"), ("Chapter two", "ch2")];

/// Writes `body` as an EPUB named `name` in a fresh library, and opens it in the
/// reader. The cadence is not set, so the reader uses its chapter default.
fn open_epub(name: &str, dir: &Path, body: &str) -> App<HeadlessBackend> {
    let lib = dir.join("books");
    fs::create_dir_all(&lib).unwrap();
    epub::write_epub(&lib.join(name), body, CHAPTERS);
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

/// Sends one page key. Returns the page reached and the one refresh it sent.
fn turn(app: &mut App<HeadlessBackend>, key: Key) -> (u32, RefreshMode) {
    let before = app.refresh_modes().len();
    app.key(key).unwrap();
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
    app.update_top::<ContentsList, _>("contents-list", |list, _| {
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

#[test]
fn picture_pages_and_the_turn_after_them_refresh_fully() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("image-refresh-turns");
    let mut app = open_epub("picture.epub", &dir, PICTURE_BOOK);
    assert_eq!(chapter_pages(&mut app), HashSet::from([0, 4]));

    // Forward: text, picture, after picture, chapter two, text, picture, after, text.
    let forward = [
        (1, Partial),
        (2, Full),
        (3, Full),
        (4, Full),
        (5, Partial),
        (6, Full),
        (7, Full),
        (8, Partial),
    ];
    for (want_page, want_mode) in forward {
        let (reached, mode) = turn(&mut app, Key::RPgFwd);
        assert_eq!(reached, want_page);
        assert_eq!(mode, want_mode, "turn to page {want_page}");
    }

    // Back: the turn away from a picture is full too, in either direction.
    let back = [
        (7, Partial),
        (6, Full),
        (5, Full),
        (4, Full),
        (3, Partial),
        (2, Full),
        (1, Full),
        (0, Full),
    ];
    for (want_page, want_mode) in back {
        let (reached, mode) = turn(&mut app, Key::RPgBack);
        assert_eq!(reached, want_page);
        assert_eq!(mode, want_mode, "turn back to page {want_page}");
    }
}

#[test]
fn text_pages_refresh_partially_except_at_chapter_starts() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("image-refresh-text");
    let mut app = open_epub("text.epub", &dir, TEXT_BOOK);
    assert_eq!(chapter_pages(&mut app), HashSet::from([0, 2]));

    let forward = [(1, Partial), (2, Full), (3, Partial), (4, Partial)];
    for (want_page, want_mode) in forward {
        let (reached, mode) = turn(&mut app, Key::RPgFwd);
        assert_eq!(reached, want_page);
        assert_eq!(mode, want_mode, "turn to page {want_page}");
    }
}
