//! Headless tests for the Home surface: its first frame, the library scan and its golden.

use std::time::Duration;

use kaia_app::Route;
use kaia_events::RefreshMode;
use kaia_gfx::golden::assert_matches_golden;

mod common;

use common::*;

#[test]
fn startup_renders_home() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("startup");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    assert_eq!(app.top_route(), Some(&Route::Home));
    assert!(
        app.find_rect("library").is_some(),
        "Home shows the Library button"
    );
    assert!(
        app.find_rect("continue").is_none(),
        "an empty library has nothing to continue"
    );
    assert_eq!(
        app.refresh_modes(),
        &[RefreshMode::Full],
        "the first frame is a full refresh"
    );
    assert_eq!(app.stats().frames, 1);
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
fn home_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-home");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    assert_matches_golden(app.frame(), &golden("home.png"), 2);
}
