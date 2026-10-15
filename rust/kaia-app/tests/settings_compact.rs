//! Settings geometry, scrolling, selectors and reviewed screen references.
mod common;
use common::*;
use kaia_app::surfaces::settings::{keys, widgets::SettingsPages};
use kaia_events::{Settings, Value};
use kaia_gfx::Point;

#[test]
fn dropdown_persists_and_returns_to_the_same_page() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("compact-select");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-reading");
    app.command("settings:page:1", &[]);
    app.process(vec![]).unwrap();
    tap_key(&mut app, "pick-reading.full_refresh_pages");
    tap_key(&mut app, "settings:pick:option:reading.full_refresh_pages:3");
    assert_eq!(
        Settings::open(&app.services().settings_path())
            .read(keys::READING_FULL_REFRESH_PAGES)
            .as_f64(),
        Some(3.0)
    );
    assert!(app.find_rect("pick-reading.full_refresh_pages").is_some());
    tap_key(&mut app, "set-display.night_mode");
    assert!(app.services().settings.is_true(keys::DISPLAY_NIGHT_MODE));
    assert!(app.find_rect("pick-reading.full_refresh_pages").is_some());
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn header_selection_preserves_defaults_until_changed_and_clears_only_header_flags() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("compact-progress");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    app.services_mut()
        .settings
        .save(keys::READING_HEADER_CLOCK, Value::Bool(true));
    app.services_mut()
        .settings
        .save(keys::READING_HEADER_BATTERY, Value::Bool(true));
    let caller_window = app.router().top_window().unwrap().id();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-reading");
    assert!(app.services().settings.is_true(keys::READING_HEADER_CLOCK));
    tap_key(&mut app, "pick-reading.header_choice");
    tap_key(&mut app, "settings:pick:option:reading.header_choice:2");
    assert!(app.services().settings.is_true(keys::READING_HEADER_CHAPTER));
    assert!(app.services().settings.is_false(keys::READING_HEADER_CLOCK));
    assert!(app.services().settings.is_false(keys::READING_HEADER_BATTERY));
    assert!(app
        .services()
        .settings
        .read(keys::READING_FOOTER_PAGE)
        .as_bool()
        .is_none());
    tap_key(&mut app, "settings-close");
    assert_ne!(app.router().top_window().unwrap().id(), caller_window);
}

#[test]
fn compact_pages_and_power_lock_goldens() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("compact-goldens");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    app.services_mut()
        .settings
        .save(keys::LIBRARY_FOLDERS, Value::array(vec![Value::from("/Books")]));
    tap_key(&mut app, "nav-settings");
    for (section, expected_pages) in [
        ("reading", 3),
        ("device", 2),
        ("home", 1),
        ("library", 1),
        ("fonts", 1),
    ] {
        app.command("nav:settings", &[]);
        app.command(&format!("settings:{section}"), &[]);
        app.process(vec![]).unwrap();
        let mut pages = 0;
        app.update_top::<SettingsPages, _>("settings-entries", |v, _| pages = v.page_count());
        assert_eq!(pages, expected_pages, "{section}");
        for page in 0..pages {
            app.command(&format!("settings:page:{page}"), &[]);
            app.process(vec![]).unwrap();
            let name = format!("settings_compact_{section}_{page}.png");
            let candidate = std::path::PathBuf::from(
                std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| std::env::temp_dir().into_os_string()),
            )
            .join("round3")
            .join(&name);
            std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
            kaia_gfx::png::write_pixmap(app.frame(), &candidate).unwrap();
            if !std::env::var("KAIA_REVIEW_SETTINGS").is_ok_and(|v| v == "1") {
                kaia_gfx::golden::assert_matches_golden(app.frame(), &golden(&name), 14);
            }
        }
    }
}

#[test]
fn scrollbar_pages_and_disabled_pin_does_nothing() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("compact-power");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-device");
    let b = app.find_rect("settings-entries").unwrap();
    app.tap(Point::new(b.right() - 10, b.bottom() - 80)).unwrap();
    tap_key(&mut app, "Change PIN");
    assert!(app.find_rect("Change PIN").is_some());
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn device_drawing_segments_save_both_button_directions() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("compact-buttons");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-reading");
    let b = app.find_rect("settings-page-buttons").unwrap();
    app.tap(Point::new(b.right() - 80, b.bottom() - 60)).unwrap();
    assert!(Settings::open(&app.services().settings_path()).is_true(keys::READING_INVERT_PAGE_BUTTONS));
    app.tap(Point::new(b.x + 80, b.bottom() - 60)).unwrap();
    assert!(Settings::open(&app.services().settings_path()).is_false(keys::READING_INVERT_PAGE_BUTTONS));
}
