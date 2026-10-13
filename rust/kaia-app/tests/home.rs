//! Headless tests for the Home surface: the empty and the populated library,
//! the scan progress line, the hero that opens the reader, and the goldens.

use std::time::{Duration, Instant};

use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::RefreshMode;
use kaia_gfx::golden::assert_matches_golden;

mod common;

use common::*;

/// A fresh app on the same data folder, with no scan. Home is built from the
/// library as it is now, so tests change the index before they reopen.
fn reopen(dir: &std::path::Path) -> App<HeadlessBackend> {
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app
}

#[test]
fn empty_library_shows_the_usb_instructions() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("home-empty");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    assert_eq!(app.top_route(), Some(&Route::Home));
    assert!(
        app.find_rect("library").is_some(),
        "Home shows the library counts"
    );
    assert!(
        app.find_rect("continue").is_none(),
        "an empty library has nothing to continue"
    );
    assert!(app.find_rect("shelf-1").is_none());
    assert_eq!(
        app.refresh_modes(),
        &[RefreshMode::Full],
        "the first frame is a full refresh"
    );
    assert_eq!(app.stats().frames, 1);
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
}

#[test]
fn scan_progress_line_shows_while_the_library_is_indexed() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("home-progress");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();

    let deadline = Instant::now() + Duration::from_secs(120);
    let mut seen = false;
    while app.scan_running() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the library scan"
        );
        if app.find_rect("scan-progress").is_some() {
            seen = true;
        }
        app.iterate(Some(Duration::from_millis(5))).unwrap();
    }
    assert!(seen, "Home shows the progress line while the first scan runs");
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    let mut text = String::new();
    app.update_top::<kaia_ui::components::Label, _>("scan-progress", |label, _| {
        text = label.text().to_owned()
    });
    assert!(text.is_empty(), "the progress text clears when the scan ends");
    assert!(
        app.find_rect("continue").is_some(),
        "the scan adds the book to Home"
    );
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
}

#[test]
fn the_scan_thread_updates_home() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("scan-home");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    assert!(app.find_rect("continue").is_none(), "Home starts with no book");
    assert!(app.scan_running());

    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();

    assert_eq!(app.top_route(), Some(&Route::Home));
    assert!(
        app.find_rect("continue").is_some(),
        "Home features the newest book after the scan"
    );
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
}

#[test]
fn tapping_the_hero_opens_the_reader() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("home-hero");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    let id = book_id(&app, "juliet.epub");
    app.services_mut().library.record_open(id, 0.25).unwrap();
    let mut app = reopen(&dir);

    tap_key(&mut app, "continue");

    assert_eq!(
        app.top_route(),
        Some(&Route::Reader { book: id }),
        "the hero opens its book in the reader"
    );
}

#[test]
fn home_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-home");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    let lib = &mut app.services_mut().library;
    lib.record_open(juliet, 0.42).unwrap();
    lib.record_open(leaves, 0.08).unwrap();
    lib.record_open(pdf, 0.0).unwrap();
    lib.set_status(pdf, kaia_library::Status::Complete).unwrap();
    drop(app);
    let app = reopen(&dir);
    assert_matches_golden(app.frame(), &golden("home.png"), 2);
}

#[test]
fn home_empty_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-home-empty");
    let app = reopen(&dir);
    assert_matches_golden(app.frame(), &golden("home_empty.png"), 2);
}
