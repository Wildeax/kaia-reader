//! More destinations, return paths and footer placement.

use kaia_app::{App, HeadlessBackend, Route};
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::png::write_pixmap;
use kaia_input::Key;

mod common;
use common::*;

fn assert_footer(app: &App<HeadlessBackend>) {
    let bar = app.find_rect("nav-bar").expect("bottom navigation");
    assert_eq!(bar.x, 0);
    assert_eq!(bar.w, app.screen().w);
    assert_eq!(bar.bottom(), app.screen().h as i32);
    assert_eq!(bar.h, 126);
}

fn check_golden(app: &App<HeadlessBackend>, name: &str) {
    // Missing baselines still produce a candidate for visual review.
    if !golden(name).exists() {
        write_pixmap(app.frame(), &golden(&name.replace(".png", ".actual.png"))).unwrap();
    }
    assert_matches_golden(app.frame(), &golden(name), 14);
}

#[test]
fn more_destinations_return_to_more() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("more-routes");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    assert_footer(&app);
    tap_key(&mut app, "nav-more");
    assert_eq!(app.top_route(), Some(&Route::More));
    assert_footer(&app);
    for (key, route) in [
        ("more-notes", Route::Notes),
        ("more-settings", Route::Settings),
        (
            "more-statistics",
            Route::SettingsSection {
                name: "statistics".into(),
            },
        ),
        (
            "more-experimental",
            Route::SettingsSection {
                name: "experimental".into(),
            },
        ),
    ] {
        tap_key(&mut app, key);
        assert_eq!(app.top_route(), Some(&route));
        app.key(Key::Back).unwrap();
        assert_eq!(app.top_route(), Some(&Route::More));
    }
    tap_key(&mut app, "more-help");
    app.command("dialog:dismiss", &[]);
    assert_eq!(app.top_route(), Some(&Route::More));
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn empty_notes_filters_and_settings_keep_the_footer_at_the_bottom() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("more-footer");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-more");
    tap_key(&mut app, "more-notes");
    for filter in ["all", "highlights", "notes", "bookmarks", "stickers"] {
        app.command(&format!("notes:filter:{filter}"), &[]);
        assert_footer(&app);
    }
    tap_key(&mut app, "nav-more");
    assert_eq!(app.top_route(), Some(&Route::More));
    tap_key(&mut app, "more-settings");
    assert_footer(&app);
    tap_key(&mut app, "nav-library");
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert_footer(&app);
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn favorites_opens_local_books_without_inheriting_other_filters() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("more-favorites");
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let favorite = book_id(&app, "juliet.epub");
    let other = book_id(&app, "leaves.epub");
    app.services_mut().library.set_favorite(favorite, true).unwrap();
    app.services_mut().settings.save("library.view", "authors".into());
    app.services_mut()
        .settings
        .save("library.tag", "unrelated".into());
    tap_key(&mut app, "nav-more");
    tap_key(&mut app, "more-favorites");
    assert!(app.find_rect(&format!("book-{favorite}")).is_some());
    assert!(app.find_rect(&format!("book-{other}")).is_none());
    assert_footer(&app);
    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&Route::More));
    tap_key(&mut app, "more-favorites");
    assert!(app.find_rect(&format!("book-{favorite}")).is_some());
    assert!(app.find_rect(&format!("book-{other}")).is_none());
}

#[test]
fn more_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-more");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-more");
    freeze_status_clock(&mut app);
    check_golden(&app, "more.png");
}

#[test]
fn empty_notes_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-notes-empty");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-notes");
    check_golden(&app, "notes_empty.png");
}

#[test]
fn favorites_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-more-favorites");
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let id = book_id(&app, "juliet.epub");
    app.services_mut().library.set_favorite(id, true).unwrap();
    tap_key(&mut app, "nav-more");
    tap_key(&mut app, "more-favorites");
    check_golden(&app, "more_favorites.png");
}
