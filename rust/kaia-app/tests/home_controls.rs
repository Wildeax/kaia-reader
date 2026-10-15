//! Home controls: each Home section has a switch on the Home page of Settings.
//! A switch that is off hides its section, the choice survives a restart, and
//! with every section off Home shows a note and a button to the Library. The
//! goldens are the Settings Home page, a Home with some sections hidden and a
//! Home with every section hidden.

use std::path::{Path, PathBuf};
use std::time::Duration;

use kaia_app::surfaces::settings::entries::Entry;
use kaia_app::surfaces::settings::{keys, sections};
use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::{Settings, Value};
use kaia_gfx::golden::assert_matches_golden;
use rusqlite::params;

mod common;

use common::*;

/// The switches of the Home page, in the order the page lists them.
const SWITCHES: [&str; 5] = [
    keys::HOME_SHOW_CURRENT,
    keys::HOME_SHOW_NEXT_IN_SERIES,
    keys::HOME_SHOW_MORE_BY_AUTHOR,
    keys::HOME_SHOW_RECENT,
    keys::HOME_SHOW_IN_PROGRESS,
];

/// A library app with the three test books indexed. Returns the app and its folder.
fn scanned(name: &str) -> (App<HeadlessBackend>, PathBuf) {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    (app, dir)
}

/// The app again on the same folder, with no scan, so Home is built from the index.
fn reopen(dir: &Path) -> App<HeadlessBackend> {
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app
}

fn set_authors(app: &App<HeadlessBackend>, id: i64, field: &str) {
    let conn = rusqlite::Connection::open(app.services().library_path()).unwrap();
    conn.execute("UPDATE books SET authors = ?2 WHERE id = ?1", params![id, field])
        .unwrap();
}

fn set_series(app: &App<HeadlessBackend>, id: i64, name: &str, index: f64) {
    let conn = rusqlite::Connection::open(app.services().library_path()).unwrap();
    conn.execute(
        "UPDATE books SET series = ?2, series_index = ?3 WHERE id = ?1",
        params![id, name, index],
    )
    .unwrap();
}

/// Home with three cards: Juliet is the book to continue, Leaves is the next book
/// in its series, and 2col.pdf is in progress. Juliet is read last, so it is the
/// first of the recent books.
struct Fixture {
    app: App<HeadlessBackend>,
    dir: PathBuf,
    juliet: i64,
    pdf: i64,
}

fn series_home(name: &str) -> Fixture {
    let (app, dir) = scanned(name);
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    set_authors(&app, juliet, "Bard");
    set_authors(&app, leaves, "Bard");
    set_authors(&app, pdf, "Someone Else");
    set_series(&app, juliet, "Verona", 1.0);
    set_series(&app, leaves, "Verona", 2.0);
    drop(app);
    let mut app = reopen(&dir);
    app.services_mut().library.record_open(pdf, 0.3).unwrap();
    app.services_mut().library.record_open(juliet, 0.4).unwrap();
    drop(app);
    let app = reopen(&dir);
    Fixture {
        app,
        dir,
        juliet,
        pdf,
    }
}

/// Writes one switch to `settings.json` in `dir`. The app must not be running,
/// because it flushes its own copy of the file.
fn save_switch(dir: &Path, key: &str, on: bool) {
    let path = dir.join("data").join("settings.json");
    let mut settings = Settings::open(&path);
    settings.save(key, Value::Bool(on));
    settings.flush(&path).unwrap();
}

/// Taps the switch of `key` on the Home page of Settings, then goes back to Home.
fn flip(app: &mut App<HeadlessBackend>, key: &str) {
    tap_key(app, "nav-settings");
    tap_key(app, "settings-advanced");
    tap_key(app, "settings:home");
    tap_key(app, &format!("set-{key}"));
    app.command("back", &[]);
    app.command("nav:home", &[]);
    assert_eq!(app.top_route(), Some(&Route::Home));
}

/// The switches the Home page lists, as `(key, on)`.
fn switches(app: &App<HeadlessBackend>) -> Vec<(&'static str, bool)> {
    sections::entries("home", app.services(), 0)
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Toggle { key, on, .. } => Some((key, on)),
            _ => None,
        })
        .collect()
}

#[test]
fn the_home_page_lists_five_switches_and_all_are_on_by_default() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("home-controls-default");
    let app = make_app(&dir, Vec::new());
    let on: Vec<(&str, bool)> = SWITCHES.iter().map(|key| (*key, true)).collect();
    assert_eq!(switches(&app), on);
}

#[test]
fn each_switch_hides_its_own_section_and_no_other() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Fixture {
        mut app,
        juliet: _,
        pdf,
        ..
    } = series_home("home-controls-each");
    let path = app.services().settings_path();
    let continue_pdf = format!("continue-{pdf}");
    let shelf_pdf = format!("shelf-{pdf}");
    assert!(app.find_rect("continue").is_some());
    assert!(app.find_rect("next-in-series").is_some());
    assert!(app.find_rect(&shelf_pdf).is_some());
    assert!(app.find_rect(&continue_pdf).is_some());

    flip(&mut app, keys::HOME_SHOW_NEXT_IN_SERIES);
    assert!(
        app.find_rect("next-in-series").is_none(),
        "the switch hides the next book"
    );
    assert!(app.find_rect("continue").is_some(), "the card stays");
    assert!(app.find_rect(&shelf_pdf).is_some(), "the shelf stays");
    assert!(app.find_rect(&continue_pdf).is_some(), "the other rows stay");
    assert!(Settings::open(&path).is_false(keys::HOME_SHOW_NEXT_IN_SERIES));

    flip(&mut app, keys::HOME_SHOW_RECENT);
    assert!(app.find_rect(&shelf_pdf).is_none(), "the switch hides the shelf");
    assert!(app.find_rect(&continue_pdf).is_some(), "the other rows stay");
    assert!(app.find_rect("continue").is_some(), "the card stays");

    flip(&mut app, keys::HOME_SHOW_IN_PROGRESS);
    assert!(
        app.find_rect(&continue_pdf).is_none(),
        "the switch hides the other rows"
    );
    assert!(app.find_rect("continue").is_some(), "the card stays");

    flip(&mut app, keys::HOME_SHOW_CURRENT);
    assert!(app.find_rect("continue").is_none(), "the switch hides the card");
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn the_author_switch_hides_the_author_tile() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, dir) = scanned("home-controls-author");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    set_authors(&app, juliet, "Bard");
    set_authors(&app, leaves, "Someone Else");
    set_authors(&app, pdf, "Bard");
    drop(app);
    let mut app = reopen(&dir);
    app.services_mut().library.record_open(juliet, 0.4).unwrap();
    drop(app);
    let mut app = reopen(&dir);
    assert!(
        app.find_rect("more-by-author").is_some(),
        "the author tile is on before the switch goes off"
    );

    flip(&mut app, keys::HOME_SHOW_MORE_BY_AUTHOR);
    assert!(
        app.find_rect("more-by-author").is_none(),
        "the switch hides the tile"
    );
    assert!(app.find_rect("continue").is_some(), "the card stays");

    flip(&mut app, keys::HOME_SHOW_MORE_BY_AUTHOR);
    assert!(
        app.find_rect("more-by-author").is_some(),
        "the tile is back when the switch is on"
    );
}

#[test]
fn a_hidden_card_leaves_its_book_on_the_shelf_and_the_offers_still_work() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Fixture {
        app,
        dir,
        juliet,
        pdf,
    } = series_home("home-controls-card");
    drop(app);
    save_switch(&dir, keys::HOME_SHOW_CURRENT, false);
    let app = reopen(&dir);

    assert!(app.find_rect("continue").is_none(), "the card is hidden");
    assert!(
        app.find_rect(&format!("shelf-{juliet}")).is_some(),
        "the book that was the card is on the shelf"
    );
    assert!(app.find_rect(&format!("shelf-{pdf}")).is_some());
    assert!(
        app.find_rect("next-in-series").is_some(),
        "the next book still follows the book to continue"
    );
}

#[test]
fn the_choices_survive_a_restart() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Fixture {
        mut app, dir, pdf, ..
    } = series_home("home-controls-restart");
    let path = app.services().settings_path();
    flip(&mut app, keys::HOME_SHOW_NEXT_IN_SERIES);
    flip(&mut app, keys::HOME_SHOW_RECENT);
    drop(app);

    let app = reopen(&dir);
    assert!(
        app.find_rect("next-in-series").is_none(),
        "the next book stays hidden after the restart"
    );
    assert!(
        app.find_rect(&format!("shelf-{pdf}")).is_none(),
        "the shelf stays hidden"
    );
    assert!(
        app.find_rect("continue").is_some(),
        "the switches left the card on"
    );
    let saved = Settings::open(&path);
    assert!(saved.is_false(keys::HOME_SHOW_NEXT_IN_SERIES));
    assert!(saved.is_false(keys::HOME_SHOW_RECENT));
    assert_eq!(
        switches(&app),
        vec![
            (keys::HOME_SHOW_CURRENT, true),
            (keys::HOME_SHOW_NEXT_IN_SERIES, false),
            (keys::HOME_SHOW_MORE_BY_AUTHOR, true),
            (keys::HOME_SHOW_RECENT, false),
            (keys::HOME_SHOW_IN_PROGRESS, true),
        ],
        "the Home page shows the saved state"
    );
    drop(app);

    let mut app = reopen(&dir);
    flip(&mut app, keys::HOME_SHOW_RECENT);
    drop(app);
    let app = reopen(&dir);
    assert!(
        app.find_rect(&format!("shelf-{pdf}")).is_some(),
        "the shelf comes back once its switch is on, after a restart"
    );
}

#[test]
fn every_section_off_shows_a_note_and_a_library_button() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Fixture {
        mut app, dir, pdf, ..
    } = series_home("home-controls-none");
    for key in SWITCHES {
        flip(&mut app, key);
    }

    assert!(app.find_rect("home-all-hidden").is_some(), "the note shows");
    assert!(
        app.find_rect("home-all-hidden-library").is_some(),
        "the Library button shows"
    );
    assert!(app.find_rect("continue").is_none());
    assert!(app.find_rect("next-in-series").is_none());
    assert!(app.find_rect(&format!("shelf-{pdf}")).is_none());
    assert!(app.find_rect(&format!("continue-{pdf}")).is_none());
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
    drop(app);

    let mut app = reopen(&dir);
    assert!(
        app.find_rect("home-all-hidden").is_some(),
        "the note survives a restart"
    );
    tap_key(&mut app, "home-all-hidden-library");
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
}

#[test]
fn settings_home_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("home-controls-settings-golden");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-advanced");
    tap_key(&mut app, "settings:home");
    assert_matches_golden(app.frame(), &golden("settings_home.png"), 14);
}

#[test]
fn home_hidden_sections_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Fixture { app, dir, .. } = series_home("home-controls-hidden-golden");
    drop(app);
    save_switch(&dir, keys::HOME_SHOW_NEXT_IN_SERIES, false);
    save_switch(&dir, keys::HOME_SHOW_IN_PROGRESS, false);
    let app = reopen(&dir);
    assert_matches_golden(app.frame(), &golden("home_hidden_sections.png"), 14);
}

#[test]
fn home_all_hidden_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let Fixture { app, dir, .. } = series_home("home-controls-all-golden");
    drop(app);
    for key in SWITCHES {
        save_switch(&dir, key, false);
    }
    let app = reopen(&dir);
    assert_matches_golden(app.frame(), &golden("home_all_hidden.png"), 14);
}
