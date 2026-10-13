//! Headless tests for the Library surface: navigation from Home and its golden.

use kaia_app::Route;
use kaia_events::RefreshMode;
use kaia_gfx::golden::assert_matches_golden;

mod common;

use common::*;

#[test]
fn tapping_library_shows_library() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("tap-library");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let before = app.refresh_modes().len();

    tap_key(&mut app, "library");

    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert!(app.find_rect("next").is_some(), "Library shows the page buttons");
    assert!(app.find_rect("library").is_none(), "Home is gone");
    // The press shows on Home first (Fast), then the root changes (Ui).
    assert_eq!(
        modes_after(&app, before),
        vec![RefreshMode::Fast, RefreshMode::Ui]
    );
    assert_eq!(app.router().routes().len(), 1, "the root is replaced, not pushed");
}

#[test]
fn library_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-library");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    tap_key(&mut app, "library");
    assert_matches_golden(app.frame(), &golden("library.png"), 2);
}
