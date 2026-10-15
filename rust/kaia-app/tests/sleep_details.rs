//! The details sleep screen: the book cover above a band with the title, the author,
//! the progress and the state label. The app sleeps through its power key, on a color
//! panel and on a grayscale panel of the Libra Colour's size. Goldens live in `tests/golden/`.

mod common;

use std::fs;
use std::path::PathBuf;

use common::*;
use kaia_app::surfaces::settings::{commands, entries::Entry, keys, sections};
use kaia_app::{App, AppOptions, HeadlessBackend, ScriptInput, Services};
use kaia_display::PanelSpec;
use kaia_events::{SystemClock, Value};
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::PixelFormat;
use kaia_input::Key;
use kaia_platform::{Env, Platform};

/// An app on a panel of `format` with the three test books scanned into the library.
fn details_app(name: &str, format: PixelFormat) -> (App<HeadlessBackend>, PathBuf) {
    let dir = scratch(name);
    let root = dir.join("platform");
    fs::create_dir_all(&root).unwrap();
    let env = Env {
        home: Some(root.clone()),
        ..Env::default()
    };
    let platform = Platform::detect_in(&root, &env).unwrap();
    pin_english(&dir.join("data"));
    let panel = PanelSpec { format, ..panel() };
    let services = Services::open(platform, panel, Some(dir.join("data"))).unwrap();
    let backend = HeadlessBackend::new(panel, ScriptInput::idle());
    let options = AppOptions {
        library_roots: vec![library_with_books(&dir)],
    };
    let mut app = App::new(backend, services, Box::new(SystemClock::new()), options);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    (app, dir)
}

fn choose_sleep_screen(app: &mut App<HeadlessBackend>, value: &str) {
    app.services_mut()
        .settings
        .save(keys::DEVICE_SLEEP_SCREEN, Value::from(value));
}

/// Sleeps with the power key and runs the loop until the cover is ready on the screen.
fn sleep_with_cover(app: &mut App<HeadlessBackend>) {
    app.key(Key::Power).unwrap();
    assert!(app.sleeping(), "the power key sleeps the device");
    wait_for(app, "the sleep picture", |a| a.sleep_picture_ready());
    app.process(Vec::new()).unwrap();
}

#[test]
fn details_is_a_sleep_screen_choice_after_the_cover() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, _dir) = details_app("sleep-details-choice", PixelFormat::Rgb24);
    assert_eq!(
        commands::value_for_pick(keys::DEVICE_SLEEP_SCREEN, 1),
        Some(Value::from("details"))
    );
    assert!(commands::is_known_key(keys::DEVICE_SLEEP_SCREEN));
    let rows = sections::entries("device", app.services(), 0);
    for key in [
        keys::DEVICE_SHOW_BOOK,
        keys::DEVICE_SLEEP_DETAILS,
        keys::DEVICE_COVER_FULLSCREEN,
    ] {
        assert!(
            rows.iter().any(|row| matches!(row,
                Entry::Toggle { key: found, on: true, .. } if *found == key
            )),
            "the default sleep screen enables {key}"
        );
    }
}

#[test]
fn details_sleep_screen_in_color_shows_the_last_book_with_its_progress() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = details_app("sleep-details-color", PixelFormat::Rgb24);
    choose_sleep_screen(&mut app, "details");
    let juliet = book_id(&app, "juliet.epub");
    app.services_mut().library.record_open(juliet, 0.42).unwrap();
    // No book is open, so the screen shows the book opened last.
    sleep_with_cover(&mut app);
    assert_eq!(app.frame().format(), PixelFormat::Rgb24);
    assert_matches_golden(app.frame(), &golden("sleep_details_color.png"), 14);
}

#[test]
fn details_sleep_screen_in_grayscale_draws_the_cover_in_gray() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = details_app("sleep-details-gray", PixelFormat::Gray8);
    choose_sleep_screen(&mut app, "details");
    let juliet = book_id(&app, "juliet.epub");
    app.services_mut().library.record_open(juliet, 0.42).unwrap();
    sleep_with_cover(&mut app);
    assert_eq!(app.frame().format(), PixelFormat::Gray8);
    assert_matches_golden(app.frame(), &golden("sleep_details_gray.png"), 14);
}

#[test]
fn details_without_any_opened_book_shows_the_blank_default_screen() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = details_app("sleep-details-empty", PixelFormat::Rgb24);
    choose_sleep_screen(&mut app, "details");
    app.key(Key::Power).unwrap();
    assert!(app.sleeping());
    app.process(Vec::new()).unwrap();
    let details = app.frame().data().to_vec();
    app.key(Key::Power).unwrap();
    assert!(!app.sleeping());

    choose_sleep_screen(&mut app, "cover");
    app.key(Key::Power).unwrap();
    assert!(app.sleeping());
    app.process(Vec::new()).unwrap();
    assert_eq!(
        app.frame().data(),
        details.as_slice(),
        "with no book, details falls back to the cover choice's blank screen"
    );
}

fn set_fullscreen(app: &mut App<HeadlessBackend>, on: bool) {
    app.services_mut()
        .settings
        .save(keys::DEVICE_COVER_FULLSCREEN, Value::Bool(on));
}

#[test]
fn fitted_cover_with_band_sleep_screen_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = details_app("sleep-fitted-band", PixelFormat::Rgb24);
    choose_sleep_screen(&mut app, "details");
    set_fullscreen(&mut app, false);
    let juliet = book_id(&app, "juliet.epub");
    app.services_mut().library.record_open(juliet, 0.42).unwrap();
    sleep_with_cover(&mut app);
    assert_matches_golden(app.frame(), &golden("sleep_fitted_band_color.png"), 14);
}

#[test]
fn full_screen_cover_without_details_sleep_screen_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = details_app("sleep-fullscreen-cover", PixelFormat::Rgb24);
    choose_sleep_screen(&mut app, "cover");
    set_fullscreen(&mut app, true);
    assert!(!keys::sleep_details(&app.services().settings));
    open_from_library(&mut app, "juliet.epub");
    sleep_with_cover(&mut app);
    assert_matches_golden(app.frame(), &golden("sleep_fullscreen_cover_color.png"), 14);
}

#[test]
fn blank_sleep_screen_hides_a_recent_book_even_with_details_on() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = details_app("sleep-blank-golden", PixelFormat::Rgb24);
    // Details stays on while the book is hidden. It is remembered, not shown.
    app.services_mut()
        .settings
        .save(keys::DEVICE_SLEEP_DETAILS, Value::Bool(true));
    choose_sleep_screen(&mut app, "blank");
    assert!(
        keys::sleep_details(&app.services().settings),
        "details is remembered while the book is hidden"
    );
    let juliet = book_id(&app, "juliet.epub");
    app.services_mut().library.record_open(juliet, 0.42).unwrap();
    app.key(Key::Power).unwrap();
    assert!(app.sleeping());
    app.process(Vec::new()).unwrap();
    assert_matches_golden(app.frame(), &golden("sleep_blank_color.png"), 14);
}
