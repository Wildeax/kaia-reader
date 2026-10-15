//! Device input and idle policy with a manual clock and a headless display.

mod common;
use common::*;
use kaia_app::surfaces::settings::{commands, entries::Entry, keys, sections};
use kaia_app::{App, AppOptions, Flow, HeadlessBackend, ScriptInput, Services};
use kaia_display::{InputEvent, Rotation};
use kaia_events::{ManualClock, Value};
use kaia_input::{Key, KeyEvent};
use kaia_platform::{usb::UsbEvent, Env, Platform};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn manual(name: &str) -> (App<HeadlessBackend>, ManualClock, PathBuf) {
    let dir = scratch(name);
    let platform = Platform::detect_in(
        &dir,
        &Env {
            home: Some(dir.clone()),
            ..Env::default()
        },
    )
    .unwrap();
    pin_english(&dir.join("data"));
    let services = Services::open(platform, panel(), Some(dir.join("data"))).unwrap();
    let clock = ManualClock::new();
    let app = App::new(
        HeadlessBackend::new(panel(), ScriptInput::idle()),
        services,
        Box::new(clock.clone()),
        AppOptions {
            library_roots: vec![dir.join("books")],
        },
    );
    (app, clock, dir)
}

fn key(key: Key, pressed: bool, time_us: u64) -> InputEvent {
    InputEvent::Key(KeyEvent {
        key,
        pressed,
        time_us,
    })
}

fn tick(app: &mut App<HeadlessBackend>, clock: &ManualClock, millis: u64) -> Flow {
    clock.advance(Duration::from_millis(millis));
    app.process(vec![]).unwrap()
}

fn wait_for_exit(app: &mut App<HeadlessBackend>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if app.process(vec![]).unwrap() == Flow::Exit {
            break;
        }
        assert!(Instant::now() < deadline, "power-off worker did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn power_sleeps_on_release_and_wake_release_is_consumed() {
    let (mut app, clock, _) = manual("power-edges");
    app.start().unwrap();
    app.process(vec![key(Key::Power, true, 100)]).unwrap();
    assert!(!app.sleeping());
    tick(&mut app, &clock, 749);
    app.process(vec![key(Key::Power, false, 749_100)]).unwrap();
    assert!(app.sleeping());
    app.process(vec![key(Key::Power, true, 800_000)]).unwrap();
    assert!(!app.sleeping());
    app.process(vec![key(Key::Power, false, 850_000)]).unwrap();
    assert!(!app.sleeping());
    // A duplicate release cannot toggle sleep.
    app.process(vec![key(Key::Power, false, 860_000)]).unwrap();
    assert!(!app.sleeping());
}

#[test]
fn hold_powers_off_at_750_ms_and_repeated_press_does_not_restart_it() {
    let (mut app, clock, _) = manual("power-hold");
    app.start().unwrap();
    app.process(vec![key(Key::Power, true, 0)]).unwrap();
    assert_eq!(tick(&mut app, &clock, 500), Flow::Continue);
    app.process(vec![key(Key::Power, true, 500_000)]).unwrap();
    assert_eq!(tick(&mut app, &clock, 249), Flow::Continue);
    assert!(!app.sleeping());
    tick(&mut app, &clock, 1);
    app.process(vec![key(Key::Power, false, 750_000)]).unwrap();
    wait_for_exit(&mut app);
    assert!(!app.sleeping());
}

#[test]
fn release_with_a_long_input_timestamp_also_powers_off() {
    let (mut app, _, _) = manual("power-batched-hold");
    app.start().unwrap();
    app.process(vec![
        key(Key::Power, true, 1_000_000),
        key(Key::Power, false, 1_750_000),
    ])
    .unwrap();
    wait_for_exit(&mut app);
    assert!(!app.sleeping());
}

#[test]
fn light_fades_use_1500_and_750_ms_and_manual_changes_cancel_old_steps() {
    let (mut app, clock, _) = manual("light-fades");
    app.start().unwrap();
    app.process(vec![InputEvent::Light]).unwrap();
    tick(&mut app, &clock, 1499);
    assert!(app.services().platform.frontlight.is_ramping());
    tick(&mut app, &clock, 1);
    assert_eq!(app.services().platform.frontlight.intensity(), 20);
    assert!(!app.services().platform.frontlight.is_ramping());
    app.process(vec![InputEvent::Light]).unwrap();
    tick(&mut app, &clock, 749);
    assert!(app.services().platform.frontlight.is_ramping());
    tick(&mut app, &clock, 1);
    assert_eq!(app.services().platform.frontlight.intensity(), 0);
    app.process(vec![InputEvent::Light]).unwrap();
    tick(&mut app, &clock, 500);
    app.command("settings:quick:frontlight", &[Value::Number(63.0)]);
    assert_eq!(app.services().platform.frontlight.intensity(), 63);
    tick(&mut app, &clock, 2000);
    assert_eq!(app.services().platform.frontlight.intensity(), 63);
}

#[test]
fn light_toggle_reverses_an_unfinished_fade() {
    let (mut app, clock, _) = manual("light-reverse");
    app.start().unwrap();
    app.command("settings:quick:frontlight", &[Value::Number(80.0)]);
    app.process(vec![InputEvent::Light]).unwrap();
    tick(&mut app, &clock, 400);
    app.process(vec![InputEvent::Light]).unwrap();
    tick(&mut app, &clock, 1500);
    assert_eq!(app.services().platform.frontlight.intensity(), 80);
    assert!(!app.services().platform.frontlight.is_ramping());
}

#[test]
fn sleep_cover_defaults_on_and_is_ignored_when_disabled_or_usb_owns_storage() {
    let (mut app, _, _) = manual("cover-policy");
    app.start().unwrap();
    app.key(Key::SleepCoverClosed).unwrap();
    assert!(app.sleeping());
    app.key(Key::SleepCoverOpened).unwrap();
    assert!(!app.sleeping());
    app.services_mut()
        .settings
        .save(keys::DEVICE_SLEEP_COVER, Value::Bool(false));
    app.key(Key::SleepCoverClosed).unwrap();
    assert!(!app.sleeping());
    app.command("sleep", &[]);
    app.key(Key::SleepCoverOpened).unwrap();
    assert!(app.sleeping());
    app.command("resume", &[]);
    app.services_mut()
        .settings
        .save(keys::DEVICE_SLEEP_COVER, Value::Bool(true));
    app.usb_event(UsbEvent::PluggedIntoHost);
    app.key(Key::SleepCoverClosed).unwrap();
    assert!(!app.sleeping());
    app.command("sleep", &[]);
    app.key(Key::SleepCoverOpened).unwrap();
    assert!(app.sleeping());
}

#[test]
fn idle_power_off_uses_activity_and_continues_after_auto_sleep() {
    let (mut app, clock, _) = manual("idle-off");
    app.start().unwrap();
    app.services_mut()
        .settings
        .save(keys::DEVICE_SLEEP_MINUTES, Value::Number(5.0));
    app.services_mut()
        .settings
        .save(keys::DEVICE_POWER_OFF_MINUTES, Value::Number(10.0));
    tick(&mut app, &clock, 299_000);
    app.key(Key::RPgFwd).unwrap();
    tick(&mut app, &clock, 299_000);
    assert!(!app.sleeping());
    tick(&mut app, &clock, 1000);
    assert!(app.sleeping());
    // Ignored touch and page input must not extend an idle shutdown while asleep.
    app.key(Key::RPgFwd).unwrap();
    tick(&mut app, &clock, 299_000);
    tick(&mut app, &clock, 1000);
    wait_for_exit(&mut app);
}

#[test]
fn idle_never_and_usb_storage_do_not_power_off() {
    let (mut app, clock, _) = manual("idle-never");
    app.start().unwrap();
    app.services_mut()
        .settings
        .save(keys::DEVICE_SLEEP_MINUTES, Value::Number(0.0));
    assert_eq!(tick(&mut app, &clock, 3_600_000), Flow::Continue);
    assert!(!app.sleeping());
    app.services_mut()
        .settings
        .save(keys::DEVICE_POWER_OFF_MINUTES, Value::Number(5.0));
    app.usb_event(UsbEvent::PluggedIntoHost);
    assert_eq!(tick(&mut app, &clock, 3_600_000), Flow::Continue);
    assert!(!app.sleeping());
}

#[test]
fn page_wake_is_disabled_without_a_verified_platform_capability() {
    let (mut app, _, _) = manual("page-wake");
    app.start().unwrap();
    assert!(!app.services().platform.supports_page_button_wake());
    app.command("settings:set:device.page_button_wake", &[Value::Bool(true)]);
    assert!(!app.services().settings.is_true(keys::DEVICE_PAGE_BUTTON_WAKE));
    app.services_mut()
        .settings
        .save(keys::DEVICE_PAGE_BUTTON_WAKE, Value::Bool(true));
    app.command("sleep", &[]);
    app.key(Key::RPgFwd).unwrap();
    assert!(app.sleeping());
    assert!(sections::entries("device", app.services(), 0)
        .iter()
        .any(|row| matches!(
            row,
            Entry::DeviceToggle {
                key: keys::DEVICE_PAGE_BUTTON_WAKE,
                on: false,
                enabled: false,
                ..
            }
        )));
}

#[test]
fn orientation_lock_blocks_sensor_changes_but_allows_manual_picks_and_persists() {
    let (mut app, _, dir) = manual("orientation");
    app.start().unwrap();
    let portrait = app.screen();
    app.command("settings:quick:rotation-lock", &[Value::Bool(true)]);
    app.process(vec![InputEvent::Orientation(Rotation::Deg90)])
        .unwrap();
    app.process(vec![]).unwrap();
    assert_eq!(app.screen(), portrait);
    app.command("settings:quick:orientation-pick", &[Value::Number(1.0)]);
    app.process(vec![]).unwrap();
    assert_eq!(app.screen(), kaia_gfx::Size::new(portrait.h, portrait.w));
    assert_eq!(app.frame().size(), app.screen());
    assert_eq!(
        app.services().settings.read(keys::DISPLAY_ROTATION),
        &Value::Number(90.0)
    );
    let saved = kaia_events::Settings::open(&dir.join("data/settings.json"));
    assert_eq!(saved.read(keys::DISPLAY_ROTATION), &Value::Number(90.0));
    app.command("settings:quick:rotation-lock", &[Value::Bool(false)]);
    app.process(vec![InputEvent::Orientation(Rotation::Deg0)])
        .unwrap();
    app.process(vec![]).unwrap();
    assert_eq!(app.screen(), portrait);
}

#[test]
fn both_idle_pickers_offer_the_same_numeric_values() {
    for key in [keys::DEVICE_SLEEP_MINUTES, keys::DEVICE_POWER_OFF_MINUTES] {
        for (index, minutes) in [0, 5, 10, 15, 30, 45, 60].iter().enumerate() {
            assert_eq!(
                commands::value_for_pick(key, index),
                Some(Value::Number(f64::from(*minutes)))
            );
        }
        assert_eq!(commands::value_for_pick(key, 7), None);
    }
    let (app, _, _) = manual("device-defaults");
    let rows = sections::entries("device", app.services(), 0);
    assert!(rows.iter().any(|row| matches!(
        row,
        Entry::Radio {
            key: keys::DEVICE_SLEEP_MINUTES,
            selected: 1,
            ..
        }
    )));
    assert!(rows.iter().any(|row| matches!(
        row,
        Entry::Radio {
            key: keys::DEVICE_POWER_OFF_MINUTES,
            selected: 0,
            ..
        }
    )));
}

#[test]
fn usb_unplug_scans_new_files_and_keeps_recent_missing_rows() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, dir) = manual("usb-rescan");
    let books = library_with_books(&dir);
    app.start().unwrap();
    wait_for(&mut app, "initial scan", |a| !a.scan_running());
    let old = app
        .services()
        .library
        .book_by_path(&books.join("juliet.epub"))
        .unwrap()
        .unwrap();
    app.usb_event(UsbEvent::PluggedIntoHost);
    std::fs::remove_file(books.join("juliet.epub")).unwrap();
    let added = books.join("new.pdf");
    std::fs::copy(books.join("2col.pdf"), &added).unwrap();
    app.usb_event(UsbEvent::UnpluggedFromHost);
    assert!(app.scan_running());
    // A second unplug during the scan queues another pass.
    app.usb_event(UsbEvent::UnpluggedFromHost);
    wait_for(&mut app, "USB scan", |a| !a.scan_running());
    assert!(app.services().library.book_by_path(&added).unwrap().is_some());
    let db = rusqlite::Connection::open(app.services().library_path()).unwrap();
    let missing: Option<i64> = db
        .query_row("SELECT missing_since FROM books WHERE id = ?1", [old.id], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(missing.is_some());
    // The normal book lookup hides missing rows, but the database retains them.
    assert!(app.services().library.book(old.id).unwrap().is_none());
}

#[test]
fn reading_settings_golden_after_adding_page_button_inversion() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, _) = manual("reading-device-golden");
    app.start().unwrap();
    app.command("nav:settings", &[]);
    app.command("settings:reading", &[]);
    app.process(vec![]).unwrap();
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden("settings_reading.png"), 14);
}

#[test]
fn quick_settings_controls_fit_portrait_and_landscape() {
    let (mut app, _, _) = manual("quick-orientations");
    app.start().unwrap();
    for index in 0..4 {
        app.command(
            "settings:pick:display.rotation",
            &[Value::Number(f64::from(index))],
        );
        app.process(vec![]).unwrap();
        app.command("settings:quick:open", &[]);
        app.process(vec![]).unwrap();
        for name in [
            "quick-frontlight",
            "quick-warmth",
            "quick-wifi",
            "quick-rotation",
            "quick-orientation",
            "quick-toggle-light",
            "quick-sleep",
        ] {
            let rect = app.find_rect(name).unwrap();
            assert!(
                rect.y >= 0 && rect.bottom() <= app.screen().h as i32,
                "{name} at {index}: {rect:?}"
            );
            assert!(
                rect.x >= 0 && rect.right() <= app.screen().w as i32,
                "{name} at {index}: {rect:?}"
            );
        }
        app.command("back", &[]);
    }
}

#[test]
fn page_button_inversion_changes_reader_navigation() {
    use kaia_app::surfaces::reader::page::PageView;
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, dir) = manual("page-button-reader");
    library_with_books(&dir);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    open_from_library(&mut app, "2col.pdf");
    let position = |app: &mut App<HeadlessBackend>| {
        wait_reader(app);
        let mut page = None;
        app.update_top::<PageView, _>("reader-page", |view, _| page = view.position);
        page.unwrap().0
    };
    let first = position(&mut app);
    app.key(Key::RPgFwd).unwrap();
    assert_eq!(position(&mut app), first + 1);
    app.services_mut()
        .settings
        .save(keys::READING_INVERT_PAGE_BUTTONS, Value::Bool(true));
    app.key(Key::RPgFwd).unwrap();
    assert_eq!(position(&mut app), first);
    app.key(Key::RPgBack).unwrap();
    assert_eq!(position(&mut app), first + 1);
}

#[test]
fn sleep_during_a_light_fade_preserves_the_requested_state_on_wake() {
    for on in [false, true] {
        let (mut app, clock, _) = manual(if on { "sleep-fading-on" } else { "sleep-fading-off" });
        app.start().unwrap();
        if !on {
            app.command("settings:quick:frontlight", &[Value::Number(20.0)]);
        }
        app.process(vec![InputEvent::Light]).unwrap();
        tick(&mut app, &clock, 400);
        assert_eq!(app.services().platform.frontlight.target_is_on(), on);
        app.command("sleep", &[]);
        tick(&mut app, &clock, 1500);
        assert!(!app.services().platform.frontlight.is_on());
        app.command("resume", &[]);
        tick(&mut app, &clock, 1500);
        assert_eq!(app.services().platform.frontlight.is_on(), on);
    }
}

#[test]
fn rotation_lays_out_an_open_book_for_the_new_screen() {
    use kaia_app::surfaces::reader::page::PageView;
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, dir) = manual("rotation-reader");
    library_with_books(&dir);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    open_from_library(&mut app, "juliet.epub");
    let shown = |app: &mut App<HeadlessBackend>| {
        wait_reader(app);
        let mut shown = None;
        app.update_top::<PageView, _>("reader-page", |view, _| {
            shown = view.page.as_ref().map(|p| p.size()).zip(view.position)
        });
        shown.unwrap()
    };
    let portrait = app.screen();
    let (size, (_, pages)) = shown(&mut app);
    // The page fills the screen between the chapter header and footer.
    assert_eq!(size.w, portrait.w);
    let strip = portrait.h - size.h;
    app.update_top::<PageView, _>("reader-page", |view, _| {
        assert!(view.header_height > 0 && view.footer_height > 0);
        assert_eq!(strip, view.header_height + view.footer_height);
    });
    // The settings picker rotates without closing a sheet, so the reader stays on top.
    app.command("settings:pick:display.rotation", &[Value::Number(1.0)]);
    app.process(vec![]).unwrap();
    let landscape = kaia_gfx::Size::new(portrait.h, portrait.w);
    assert_eq!(app.screen(), landscape);
    let (size, (_, rotated_pages)) = shown(&mut app);
    assert_eq!(size, kaia_gfx::Size::new(landscape.w, landscape.h - strip));
    // Wider, shorter pages hold different text, so the count changes.
    assert_ne!(rotated_pages, pages);
}
