//! The charging indicator on the sleep screen. The mark shows in the top right corner
//! while the charger is connected and `device.charging_indicator` is on. The layouts of
//! the sleep screen (full-screen cover, fitted cover, blank) are goldens in
//! `sleep_details.rs`.
//!
//! The battery is a fake whose charger state the test sets. The clock is manual, so the
//! battery poll and the suspend timer run only when a test advances it.

mod common;

use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use kaia_app::surfaces::settings::keys;
use kaia_app::{App, AppOptions, HeadlessBackend, ScriptInput, Services};
use kaia_events::{ManualClock, Value};
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Pixmap;
use kaia_input::Key;
use kaia_platform::{Battery, Env, Platform};

/// A battery whose charger state the test sets. The charge is steady at 80 percent.
struct FakeBattery(Arc<AtomicBool>);

impl Battery for FakeBattery {
    fn capacity(&self) -> Option<u8> {
        Some(80)
    }

    fn is_charging(&self) -> Option<bool> {
        Some(self.0.load(Ordering::SeqCst))
    }

    fn is_charged(&self) -> Option<bool> {
        Some(false)
    }
}

/// An app with the three test books scanned in, juliet opened last, and a fake battery.
fn sleep_app(name: &str, charging: Arc<AtomicBool>) -> (App<HeadlessBackend>, ManualClock) {
    let dir = scratch(name);
    let root = dir.join("platform");
    fs::create_dir_all(&root).unwrap();
    let env = Env {
        home: Some(root.clone()),
        ..Env::default()
    };
    let platform = Platform::detect_in(&root, &env).unwrap();
    pin_english(&dir.join("data"));
    let mut services = Services::open(platform, panel(), Some(dir.join("data"))).unwrap();
    services.platform.battery = Box::new(FakeBattery(charging));
    let clock = ManualClock::new();
    let mut app = App::new(
        HeadlessBackend::new(panel(), ScriptInput::idle()),
        services,
        Box::new(clock.clone()),
        AppOptions {
            library_roots: vec![library_with_books(&dir)],
        },
    );
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    let juliet = book_id(&app, "juliet.epub");
    app.services_mut().library.record_open(juliet, 0.42).unwrap();
    (app, clock)
}

fn set_indicator(app: &mut App<HeadlessBackend>, on: bool) {
    app.services_mut()
        .settings
        .save(keys::DEVICE_CHARGING_INDICATOR, Value::Bool(on));
}

/// Sleeps with the power key and runs the loop until the cover is on the screen.
/// Returns the frame the sleep screen shows.
fn sleep_frame(app: &mut App<HeadlessBackend>) -> Pixmap {
    app.key(Key::Power).unwrap();
    assert!(app.sleeping(), "the power key sleeps the device");
    wait_for(app, "the sleep picture", |a| a.sleep_picture_ready());
    app.process(Vec::new()).unwrap();
    app.frame().clone()
}

fn wake(app: &mut App<HeadlessBackend>) {
    app.key(Key::Power).unwrap();
    assert!(!app.sleeping(), "the power key wakes the device");
}

/// The box that holds every pixel that differs between two frames of one size.
/// Returns (left, top, right, bottom), or None when the frames are equal.
fn changed_box(before: &Pixmap, after: &Pixmap) -> Option<(u32, u32, u32, u32)> {
    assert_eq!(before.size(), after.size());
    let width = before.size().w as usize;
    let bpp = before.format().bytes_per_pixel();
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for (i, (a, b)) in before
        .data()
        .chunks(bpp)
        .zip(after.data().chunks(bpp))
        .enumerate()
    {
        if a != b {
            let x = (i % width) as u32;
            let y = (i / width) as u32;
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((left, top, right, bottom)) => (left.min(x), top.min(y), right.max(x), bottom.max(y)),
            });
        }
    }
    bounds
}

#[test]
fn charging_mark_shows_only_while_charging_and_the_setting_is_on() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let charging = Arc::new(AtomicBool::new(true));
    let (mut app, _clock) = sleep_app("sleep-charging-states", charging.clone());

    set_indicator(&mut app, true);
    let on_charging = sleep_frame(&mut app);
    assert_matches_golden(&on_charging, &golden("sleep_charging_on_color.png"), 14);
    wake(&mut app);

    set_indicator(&mut app, false);
    let off_charging = sleep_frame(&mut app);
    wake(&mut app);

    set_indicator(&mut app, true);
    charging.store(false, Ordering::SeqCst);
    let on_not_charging = sleep_frame(&mut app);

    let size = on_charging.size();
    let (left, top, right, bottom) =
        changed_box(&off_charging, &on_charging).expect("the charging mark changes the frame");
    assert!(
        left > size.w / 2 && bottom < size.h / 4,
        "the mark is in the top right corner, got {left},{top} to {right},{bottom}"
    );
    assert!(
        changed_box(&off_charging, &on_not_charging).is_none(),
        "no mark when the charger is off, whether or not the setting is on"
    );
}

#[test]
fn charger_connected_while_the_screen_is_up_draws_the_mark() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let charging = Arc::new(AtomicBool::new(false));
    let (mut app, clock) = sleep_app("sleep-charger-redraw", charging.clone());
    set_indicator(&mut app, true);
    // The loop wakes at the battery poll, 60 seconds in. Sleeping at 50 keeps the suspend
    // timer (15 seconds after the sleep, at 65) after that wake, so the screen stays up.
    clock.advance(Duration::from_secs(50));
    app.process(Vec::new()).unwrap();
    let before = sleep_frame(&mut app);

    charging.store(true, Ordering::SeqCst);
    clock.advance(Duration::from_secs(10));
    app.process(Vec::new()).unwrap();
    assert!(app.sleeping(), "the sleep screen is still up after the poll");
    let after = app.frame().clone();
    assert!(
        changed_box(&before, &after).is_some(),
        "the charging mark appears when the charger connects"
    );
}

#[test]
fn charger_connected_while_the_indicator_is_off_leaves_the_screen_alone() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let charging = Arc::new(AtomicBool::new(false));
    let (mut app, clock) = sleep_app("sleep-charger-off", charging.clone());
    set_indicator(&mut app, false);
    clock.advance(Duration::from_secs(50));
    app.process(Vec::new()).unwrap();
    let before = sleep_frame(&mut app);

    charging.store(true, Ordering::SeqCst);
    clock.advance(Duration::from_secs(10));
    app.process(Vec::new()).unwrap();
    assert!(app.sleeping());
    assert!(
        changed_box(&before, app.frame()).is_none(),
        "with the indicator off, a charger never changes the sleep screen"
    );
}
