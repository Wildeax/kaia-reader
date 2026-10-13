//! Headless tests for the Notes and Settings surfaces: the root switches from
//! the nav bar, and a settings section is pushed above the Settings root.

mod common;

use common::*;
use kaia_app::Route;
use kaia_input::Key;

#[test]
fn notes_root_shows_its_placeholder_and_the_nav_bar_returns_home() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("notes");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    tap_key(&mut app, "nav-notes");
    assert_eq!(app.top_route(), Some(&Route::Notes));
    assert_eq!(app.router().routes().len(), 1, "Notes replaces the root");

    tap_key(&mut app, "nav-home");
    assert_eq!(app.top_route(), Some(&Route::Home));
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn settings_opens_a_section_above_the_root_and_back_returns() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    tap_key(&mut app, "nav-settings");
    assert_eq!(app.top_route(), Some(&Route::Settings));

    tap_key(&mut app, "settings-reading");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "reading".to_string()
        })
    );
    assert_eq!(
        app.router().routes().len(),
        2,
        "the section is pushed above the root"
    );

    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Settings));
    assert_eq!(app.router().routes().len(), 1);
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}
