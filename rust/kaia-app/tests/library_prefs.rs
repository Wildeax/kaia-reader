//! Headless tests for what the Library keeps between runs: the layout of the Books
//! tab, the open tab and the sort. Each test saves the settings, shuts the app down
//! and opens it again on the same data folder.

mod common;

use std::time::Duration;

use kaia_app::{App, HeadlessBackend};

use common::*;

/// Shuts `app` down, which writes the settings, and opens a new app on `dir`.
fn restart(mut app: App<HeadlessBackend>, dir: &std::path::Path) -> App<HeadlessBackend> {
    app.shutdown();
    drop(app);
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app
}

/// A library app with the three test books indexed and Home on screen.
fn indexed(name: &str) -> (App<HeadlessBackend>, std::path::PathBuf) {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    (app, dir)
}

#[test]
fn the_list_layout_survives_a_restart() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("prefs-list");
    tap_key(&mut app, "library");
    tap_key(&mut app, "library-view-list");
    assert!(app.find_rect("list").is_some(), "the list is on show");

    let mut app = restart(app, &dir);
    tap_key(&mut app, "library");
    assert!(
        app.find_rect("list").is_some(),
        "Library opens as the list after a restart"
    );
    assert_eq!(
        app.services().settings.read("library.layout").as_str(),
        Some("list")
    );
}

#[test]
fn the_list_layout_survives_a_restart_while_another_tab_is_open() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("prefs-folders");
    tap_key(&mut app, "library");
    tap_key(&mut app, "library-view-list");
    tap_key(&mut app, "library-view-folders");

    let mut app = restart(app, &dir);
    tap_key(&mut app, "library");
    assert!(app.find_rect("list").is_none(), "Folders is still the open tab");
    assert!(
        app.find_rect("library-sort").is_none(),
        "no Books controls on Folders"
    );

    tap_key(&mut app, "library-view-books");
    assert!(app.find_rect("list").is_some(), "Books comes back as the list");
}

#[test]
fn the_books_layout_defaults_to_covers_and_the_tab_and_sort_survive() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("prefs-defaults");
    tap_key(&mut app, "library");
    assert!(
        app.find_rect("list").is_none(),
        "Books opens as covers by default"
    );

    tap_key(&mut app, "library-sort");
    tap_key(&mut app, "sort-progress");
    tap_key(&mut app, "library-view-authors");

    let mut app = restart(app, &dir);
    assert_eq!(
        app.services().settings.read("library.sort").as_str(),
        Some("progress")
    );
    assert_eq!(
        app.services().settings.read("library.view").as_str(),
        Some("authors")
    );
    tap_key(&mut app, "library");
    assert!(
        app.find_rect("library-view-authors").is_some(),
        "the Authors tab is open"
    );
    tap_key(&mut app, "library-view-books");
    assert!(app.find_rect("list").is_none(), "the Books layout is covers");
    assert!(app.find_rect("library-view-list").is_some());
}
