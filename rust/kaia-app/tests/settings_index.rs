//! Settings index layout and navigation through its controls.

mod common;

use common::*;
use kaia_app::surfaces::settings::commands::INDEX_SECTIONS;
use kaia_app::Route;

#[test]
fn index_rows_fit_and_close_restores_home() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-index");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    app.command("nav:settings", &[]);
    app.process(Vec::new()).unwrap();
    assert!(app.find_rect("nav-bar").is_none());
    for (index, section) in INDEX_SECTIONS.iter().enumerate() {
        let row = app.find_rect(&format!("settings-{section}")).unwrap();
        assert_eq!(row.x, 100);
        assert_eq!(row.y, 280 + 107 * index as i32);
        assert_eq!(row.w, 1056);
        assert_eq!(row.h, 107);
    }
    tap_key(&mut app, "settings-advanced");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "advanced".into()
        })
    );
    app.command("back", &[]);
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-close");
    assert_eq!(app.top_route(), Some(&Route::Home));
}

#[test]
fn close_restores_each_caller_and_back_only_leaves_one_settings_page() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for (name, command, caller) in [
        ("home", "nav:home", Route::Home),
        ("library", "nav:library", Route::Library { page: 0 }),
        ("notes", "nav:notes", Route::Notes),
        ("more", "nav:more", Route::More),
    ] {
        let dir = scratch(&format!("settings-caller-{name}"));
        let mut app = make_app(&dir, Vec::new());
        app.start().unwrap();
        app.command(command, &[]);
        app.process(Vec::new()).unwrap();
        let caller_window = app.router().top_window().unwrap().id();
        app.command("nav:settings", &[]);
        app.process(Vec::new()).unwrap();
        app.command("nav:settings", &[]);
        app.process(Vec::new()).unwrap();
        assert_eq!(app.router().routes(), vec![&caller, &Route::Settings]);
        tap_key(&mut app, "settings-reading");
        tap_key(&mut app, "settings-back");
        assert_eq!(app.top_route(), Some(&Route::Settings));
        tap_key(&mut app, "settings-reading");
        app.command("settings:dictionaries", &[]);
        app.process(Vec::new()).unwrap();
        tap_key(&mut app, "settings-close");
        assert_eq!(app.router().routes(), vec![&caller]);
        assert_eq!(app.router().top_window().unwrap().id(), caller_window);
    }
}

#[test]
fn search_replacement_and_close_preserve_the_library_caller() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-search-caller");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    app.command("nav:library", &[]);
    app.process(Vec::new()).unwrap();
    let caller_window = app.router().top_window().unwrap().id();
    app.command("nav:settings", &[]);
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-search");
    app.command("settings:search", &[kaia_events::Value::from("refresh")]);
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-hit-0");
    tap_key(&mut app, "settings-close");
    assert_eq!(app.router().routes(), vec![&Route::Library { page: 0 }]);
    assert_eq!(app.router().top_window().unwrap().id(), caller_window);
}

#[test]
fn close_returns_to_the_open_reader_without_reopening_the_book() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-reader-caller");
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "library scan", |a| !a.scan_running());
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    let caller_window = app.router().top_window().unwrap().id();
    let routes: Vec<_> = app.router().routes().into_iter().cloned().collect();
    app.command("nav:settings", &[]);
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-reading");
    tap_key(&mut app, "settings-close");
    assert_eq!(app.router().routes(), routes.iter().collect::<Vec<_>>());
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));
    assert_eq!(app.router().top_window().unwrap().id(), caller_window);
}

#[test]
fn grouped_settings_keep_secondary_pages_reachable() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-groups");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    app.command("nav:settings", &[]);
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-language-dictionaries");
    tap_key(&mut app, "settings:language");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "language".into()
        })
    );
    tap_key(&mut app, "settings-back");
    tap_key(&mut app, "settings:dictionaries");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "dictionaries".into()
        })
    );
    app.command("nav:settings", &[]);
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-datetime");
    tap_key(&mut app, "settings:timezone");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "timezone".into()
        })
    );
    app.command("nav:settings", &[]);
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-device-info");
    tap_key(&mut app, "settings:about");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection { name: "about".into() })
    );
    app.command("nav:settings", &[]);
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-advanced");
    tap_key(&mut app, "settings:home");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection { name: "home".into() })
    );
    tap_key(&mut app, "settings-close");
    assert_eq!(app.router().routes(), vec![&Route::Home]);
}

#[test]
fn language_group_golden() {
    settings_group_golden("language-dictionaries");
}

#[test]
fn date_time_group_golden() {
    settings_group_golden("datetime");
}

#[test]
fn device_info_group_golden() {
    settings_group_golden("device-info");
}

fn settings_group_golden(name: &str) {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch(&format!("settings-groups-golden-{name}"));
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    app.command("nav:settings", &[]);
    app.process(Vec::new()).unwrap();
    app.command(&format!("settings:{name}"), &[]);
    app.process(Vec::new()).unwrap();
    // Pin the clock and volume facts in visual fixtures; production reads live values.
    let mut entries = kaia_app::surfaces::settings::sections::entries(name, app.services(), 1_700_000_000);
    for entry in &mut entries {
        if let kaia_app::surfaces::settings::entries::Entry::Row { primary, value, .. } = entry {
            match primary.as_str() {
                "Date" => *value = Some("2026-10-15".into()),
                "Time" => *value = Some("02:51".into()),
                "Storage" => *value = Some("12.00 GB free of 32.00 GB".into()),
                _ => (),
            }
        }
    }
    let content = kaia_ui::WidgetPod::new(kaia_app::surfaces::settings::widgets::SettingsPages::new(
        entries, 0,
    ));
    let dialog = kaia_ui::composite::SettingsDialog::new(
        "Settings",
        kaia_app::surfaces::settings::sections::title(name),
        content,
        "settings:close",
    )
    .back("back")
    .content_header();
    let frame = kaia_ui::render(
        dialog,
        app.screen(),
        &app.services().theme,
        &app.services().assets,
    );
    kaia_gfx::golden::assert_matches_golden(&frame, &golden(&format!("settings_{name}.png")), 14);
}

#[test]
fn changing_a_setting_rebuilds_the_caller_when_settings_closes() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-rebuild-caller");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let caller_window = app.router().top_window().unwrap().id();
    app.command("nav:settings", &[]);
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-advanced");
    tap_key(&mut app, "settings:home");
    app.command(
        "settings:set:home.show_current",
        &[kaia_events::Value::Bool(false)],
    );
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "settings-close");
    assert_eq!(app.router().routes(), vec![&Route::Home]);
    assert_ne!(app.router().top_window().unwrap().id(), caller_window);
}
