//! Reading, power and manual clock settings from B-147 part 3, round 5.
mod common;
use common::*;
use kaia_app::surfaces::settings::keys;
use kaia_events::{Settings, Value};
use kaia_gfx::Point;

#[test]
fn illustrated_choices_persist_without_rewriting_legacy_values() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round5-reading");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    app.services_mut()
        .settings
        .save(keys::READING_TAP_ZONES, Value::from("page_only"));
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-reading");
    assert!(app.services().settings.read(keys::READING_PROGRESS_BAR).is_nil());
    tap_key(&mut app, "set-reading.progress_bar");
    app.command("settings:page:1", &[]);
    app.process(vec![]).unwrap();
    tap_key(&mut app, "set-reading.adobe_numbers");
    tap_key(&mut app, "set-reading.reduce_rainbow");
    app.command("settings:page:2", &[]);
    app.process(vec![]).unwrap();
    assert_eq!(
        app.services().settings.read(keys::READING_TAP_ZONES).as_str(),
        Some("page_only")
    );
    for (i, value) in keys::ILLUSTRATED_TAP_ZONES.iter().enumerate() {
        let r = app.find_rect("settings-tap-zones").unwrap();
        app.tap(Point::new(r.x + (r.w / 4 * i as u32 + r.w / 8) as i32, r.y + 240))
            .unwrap();
        assert_eq!(
            Settings::open(&app.services().settings_path())
                .read(keys::READING_TAP_ZONES)
                .as_str(),
            Some(*value)
        );
    }
    tap_key(&mut app, "pick-reading.turn_gesture");
    tap_key(&mut app, "settings:pick:option:reading.turn_gesture:1");
    assert_eq!(
        Settings::open(&app.services().settings_path())
            .read(keys::READING_TURN_GESTURE)
            .as_str(),
        Some("swipe")
    );
    assert!(app.find_rect("settings-tap-zones").is_some());
    assert!(app.services().settings.is_true(keys::READING_PROGRESS_BAR));
    assert!(app.services().settings.is_true(keys::READING_ADOBE_NUMBERS));
    assert!(app.services().settings.is_true(keys::READING_REDUCE_RAINBOW));
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn sleep_preferences_remain_independent_and_keep_existing_defaults() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round5-power");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-device");
    for key in [
        keys::DEVICE_COVER_FULLSCREEN,
        keys::DEVICE_CHARGING_INDICATOR,
        keys::DEVICE_SLEEP_SCREEN,
    ] {
        assert!(app.services().settings.read(key).is_nil());
    }
    tap_key(&mut app, "set-device.cover_fullscreen");
    tap_key(&mut app, "set-device.charging_indicator");
    tap_key(&mut app, "set-device.sleep_details");
    tap_key(&mut app, "set-device.show_book");
    tap_key(&mut app, "set-device.show_book");
    let saved = Settings::open(&app.services().settings_path());
    assert!(saved.is_false(keys::DEVICE_COVER_FULLSCREEN));
    assert!(saved.is_true(keys::DEVICE_CHARGING_INDICATOR));
    assert_eq!(saved.read(keys::DEVICE_SLEEP_SCREEN).as_str(), Some("details"));
    assert_eq!(keys::DEFAULT_SLEEP_MINUTES, 15.);
    assert!(saved.read(keys::DEVICE_PAGE_BUTTON_WAKE).is_nil());
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn manual_clock_editors_validate_cancel_and_never_set_the_desktop_clock() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round5-clock");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-datetime");
    tap_key(&mut app, "settings:clock:date");
    for (key, value) in [("year", 2023.), ("month", 2.), ("day", 29.)] {
        app.command(&format!("settings:set:clock.{key}"), &[Value::Number(value)]);
    }
    app.process(vec![]).unwrap();
    tap_key(&mut app, "clock-save");
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden("settings_date_invalid.png"), 14);
    app.command("settings:set:clock.year", &[Value::Number(2024.)]);
    app.process(vec![]).unwrap();
    tap_key(&mut app, "clock-save");
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden("settings_date_editor.png"), 14);
    tap_key(&mut app, "clock-cancel");
    tap_key(&mut app, "settings:clock:time");
    for (key, value) in [("hour", 13.), ("minute", 45.)] {
        app.command(&format!("settings:set:clock.{key}"), &[Value::Number(value)]);
    }
    app.process(vec![]).unwrap();
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden("settings_time_editor.png"), 14);
    tap_key(&mut app, "clock-cancel");
    tap_key(&mut app, "set-device.clock_24h");
    assert!(Settings::open(&app.services().settings_path()).is_false(keys::DEVICE_CLOCK_24H));
    assert!(app.services().settings.read(keys::DEVICE_NETWORK_TIME).is_nil());
    let saved = std::fs::read_to_string(app.services().settings_path()).unwrap();
    assert!(!saved.contains("clock.year"));
    assert!(app.unknown_commands().is_empty());
}
