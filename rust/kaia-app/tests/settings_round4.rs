//! Light panel and settings behavior added in B-147 part 3, round 4.
mod common;
use common::*;
use kaia_app::surfaces::settings::{keys, light::LightPanel, sections};
use kaia_events::{Settings, Value};
use kaia_gfx::{Point, Size};
use kaia_platform::{device::Model, platform::Kind};

fn reference(app: &kaia_app::App<kaia_app::HeadlessBackend>, name: &str) {
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden(name), 14);
}

#[test]
fn light_controls_persist_and_outside_tap_restores_the_caller() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round4-light");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    app.services_mut().platform.kind = Kind::Kobo(Model::LibraColour);
    app.services_mut().platform.frontlight.set_intensity(26).unwrap();
    app.services_mut().platform.frontlight.set_warmth(88).unwrap();
    let caller = app.router().top_window().unwrap().id();
    app.command("settings:light:open", &[]);
    app.process(vec![]).unwrap();
    assert!(!app
        .services()
        .settings
        .is_true(keys::DISPLAY_NATURAL_LIGHT_ENABLED));
    reference(&app, "settings_light_home.png");
    assert!(app.find_rect("quick-wifi").is_none());
    let r = app.find_rect("light-brightness").unwrap();
    assert_eq!(r, kaia_gfx::Rect::new(415, 250, 699, 180));
    app.tap(Point::new(r.x + 75 + 549 / 2, r.bottom() - 50)).unwrap();
    assert_eq!(app.services().platform.frontlight.intensity(), 50);
    // Auto's visible switch and its larger tap area.
    app.tap(Point::new(1065, 560)).unwrap();
    assert!(Settings::open(&app.services().settings_path()).is_true(keys::DISPLAY_NATURAL_LIGHT_ENABLED));
    app.tap(Point::new(700, 830)).unwrap();
    assert_eq!(app.find_rect("light-warmth").unwrap().h, 0);
    reference(&app, "settings_light_bedtime.png");
    app.tap(Point::new(450, 310)).unwrap();
    assert_eq!(
        Settings::open(&app.services().settings_path())
            .read(keys::DISPLAY_NATURAL_LIGHT_BEDTIME)
            .as_i64(),
        Some(1140)
    );
    assert!(app.find_rect("light-warmth").is_some());
    app.tap(Point::new(40, 1200)).unwrap();
    assert!(app.find_rect("light-brightness").is_none());
    assert_eq!(app.router().top_window().unwrap().id(), caller);
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn light_panel_has_reader_geometry_and_fits_landscape() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round4-light-geometry");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    app.services_mut().platform.kind = Kind::Kobo(Model::LibraColour);
    app.services_mut().platform.frontlight.set_intensity(26).unwrap();
    app.services_mut().platform.frontlight.set_warmth(88).unwrap();
    for (screen, name) in [
        (Size::new(1264, 1680), "settings_light_reader.png"),
        (Size::new(1680, 1264), "settings_light_landscape.png"),
    ] {
        let mut theme = app.services().theme.clone();
        theme.scale.width = screen.w;
        theme.scale.height = screen.h;
        let frame = kaia_ui::render(
            LightPanel::new(app.services(), true),
            screen,
            &theme,
            &app.services().assets,
        );
        kaia_gfx::golden::assert_matches_golden(&frame, &golden(name), 14);
    }
}

#[test]
fn sleep_checks_preserve_legacy_choices_and_hidden_details() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for legacy in ["cover", "details", "blank"] {
        let dir = scratch(&format!("round4-sleep-{legacy}"));
        let mut app = make_app(&dir, vec![]);
        app.start().unwrap();
        app.services_mut()
            .settings
            .save(keys::DEVICE_SLEEP_SCREEN, Value::from(legacy));
        tap_key(&mut app, "nav-settings");
        tap_key(&mut app, "settings-device");
        assert_eq!(
            app.services().settings.read(keys::DEVICE_SLEEP_SCREEN).as_str(),
            Some(legacy)
        );
        app.command("settings:set:device.show_book", &[Value::Bool(false)]);
        app.command("settings:set:device.sleep_details", &[Value::Bool(true)]);
        assert_eq!(
            app.services().settings.read(keys::DEVICE_SLEEP_SCREEN).as_str(),
            Some("blank")
        );
        app.command("settings:set:device.show_book", &[Value::Bool(true)]);
        assert_eq!(
            Settings::open(&app.services().settings_path())
                .read(keys::DEVICE_SLEEP_SCREEN)
                .as_str(),
            Some("details")
        );
        app.command("settings:pick:device.sleep_screen", &[Value::Number(0.0)]);
        assert!(!keys::sleep_details(&app.services().settings));
        assert_eq!(keys::DEFAULT_SLEEP_MINUTES, 15.0);
        assert!(!keys::bool_or(
            &app.services().settings,
            keys::DEVICE_PAGE_BUTTON_WAKE,
            false
        ));
    }
}

#[test]
fn direct_reading_entry_closes_to_the_same_book_and_gesture_settings_apply() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round4-reading");
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let book = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{book}"), &[]);
    wait_reader(&mut app);
    app.command("reader:overlay", &[]);
    app.process(vec![]).unwrap();
    tap_key(&mut app, "overlay-light");
    reference(&app, "settings_light_reader_overlay.png");
    app.tap(Point::new(40, 1000)).unwrap();
    tap_key(&mut app, "overlay-reading-settings");
    assert!(app.find_rect("settings-back").is_none());
    app.command("settings:page:2", &[]);
    app.process(vec![]).unwrap();
    tap_key(&mut app, "set-reading.pinch_size");
    tap_key(&mut app, "set-reading.edge_light");
    tap_key(&mut app, "settings-close");
    app.command("back", &[]);
    wait_reader(&mut app);
    assert_eq!(app.top_route(), Some(&kaia_app::Route::Reader { book }));
    let before = app.services().settings.read(keys::READING_FONT_SIZE).clone();
    app.command("reader:size:larger", &[Value::Number(420.0)]);
    app.process(vec![]).unwrap();
    assert_eq!(app.services().settings.read(keys::READING_FONT_SIZE), &before);
    let mut enabled = true;
    assert!(app
        .update_top::<kaia_app::surfaces::reader::page::PageView, _>("reader-page", |view, _| enabled =
            view.brightness_swipe));
    assert!(!enabled);
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn wifi_page_keeps_radio_and_services_separate() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round4-wifi");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-wifi");
    reference(&app, "settings_wifi_off.png");
    app.services_mut()
        .settings
        .save(keys::NETWORK_AIRPLANE_MODE, Value::Bool(true));
    app.command("settings:page:0", &[]);
    app.process(vec![]).unwrap();
    tap_key(&mut app, "settings:quick:wifi-blocked");
    assert!(!app.services().platform.network.is_wifi_on());
    assert!(app.services().settings.read(keys::SYNC_ENABLED).is_nil());
    assert!(sections::entries("wifi", app.services(), 0).iter().any(|row| matches!(row, kaia_app::surfaces::settings::entries::Entry::Row { action: Some(action), .. } if action == "settings:network")));
}

#[test]
fn spanish_reading_pages_and_light_keep_navigation_and_values() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round4-spanish");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-language-dictionaries");
    app.command("settings:language:es", &[]);
    app.process(vec![]).unwrap();
    assert_eq!(app.services().language, "es");
    app.command("nav:settings", &[]);
    app.command("settings:reading", &[]);
    app.command("settings:page:2", &[]);
    app.process(vec![]).unwrap();
    reference(&app, "settings_reading_touch_es.png");
    assert!(app.find_rect("settings-back").is_some());
    assert!(app.find_rect("set-reading.pinch_size").unwrap().bottom() < 1605);
    app.command("settings:close", &[]);
    // The focused light panel shares the same catalog as settings.
    let frame = kaia_ui::render(
        LightPanel::new(app.services(), true),
        app.screen(),
        &app.services().theme,
        &app.services().assets,
    );
    kaia_gfx::golden::assert_matches_golden(&frame, &golden("settings_light_es.png"), 14);
    kaia_app::i18n::activate(&app.services().catalog_root, "en");
}

#[test]
fn frontlight_drag_flushes_on_release() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round4-light-drag");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    app.command(
        "settings:quick:frontlight",
        &[Value::Number(37.0), Value::Bool(true)],
    );
    assert!(Settings::open(&app.services().settings_path())
        .read("display.frontlight")
        .is_nil());
    app.command("settings:quick:frontlight:release", &[Value::Number(37.0)]);
    assert_eq!(
        Settings::open(&app.services().settings_path())
            .read("display.frontlight")
            .as_i64(),
        Some(37)
    );
}

#[test]
fn library_settings_show_scan_completion() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round4-scan");
    let mut app = make_app(&dir, vec![]);
    std::fs::create_dir_all(&app.services().paths.books_dir).unwrap();
    app.start().unwrap();
    wait_for(&mut app, "initial scan", |a| !a.scan_running());
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-library");
    tap_key(&mut app, "settings-scan");
    wait_for(&mut app, "settings scan", |a| !a.scan_running());
    assert!(app
        .find_rect("Scan complete. Added 0, updated 0, removed 0.")
        .is_some());
    assert!(app.unknown_commands().is_empty());
}
