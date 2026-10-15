//! The screen lock: set, change and turn off from Settings, the pad after a wake and
//! at start, the input block, the wrong-try wait across a restart, and recovery by
//! deleting `.kaia/pin`. Goldens live in `tests/golden/`.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use kaia_app::surfaces::pin;
use kaia_app::surfaces::pin::store;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_display::InputEvent;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Point;
use kaia_input::{Key, Phase, TouchEvent};

fn data_of(dir: &Path) -> PathBuf {
    dir.join("data")
}

fn locked(app: &App<HeadlessBackend>) -> bool {
    app.top_route().is_some_and(pin::is_lock_route)
}

/// Taps the pad key `key`. The pad is the top window, so its keys are on it.
fn tap_pad(app: &mut App<HeadlessBackend>, key: &str) {
    let p = centre_of(app, key);
    app.tap(p).unwrap();
}

fn type_digits(app: &mut App<HeadlessBackend>, digits: &str) {
    for d in digits.chars() {
        tap_pad(app, &format!("pin-digit-{d}"));
    }
}

/// Types `digits` and presses OK.
fn enter(app: &mut App<HeadlessBackend>, digits: &str) {
    type_digits(app, digits);
    tap_pad(app, "pin-submit");
}

/// Sleeps the device, then wakes it with the power key.
fn sleep_and_wake(app: &mut App<HeadlessBackend>) {
    app.key(Key::Power).unwrap();
    assert!(app.sleeping(), "the first press sleeps");
    app.key(Key::Power).unwrap();
    assert!(!app.sleeping(), "the second press wakes");
}

/// Stores `digits` as the PIN of the app in `dir`. The data folder is created first.
fn save_pin(dir: &Path, digits: &str) {
    fs::create_dir_all(data_of(dir)).unwrap();
    store::save(&data_of(dir), digits).unwrap();
}

fn pin_file(dir: &Path) -> PathBuf {
    data_of(dir).join(store::FILE)
}

#[test]
fn wake_shows_the_pad_and_blocks_input_to_the_screen_below() {
    let dir = scratch("lock-wake");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    assert_eq!(app.top_route(), Some(&Route::Home));
    save_pin(&dir, "4826");

    sleep_and_wake(&mut app);
    assert!(locked(&app), "a wake with a PIN set shows the pad");
    assert!(app.find_rect("pin-digit-1").is_some());

    // The Library button of the nav bar sits under the pad. A tap there does nothing.
    let screen = app.screen();
    app.tap(Point::new(screen.w as i32 / 8, screen.h as i32 - 40))
        .unwrap();
    assert!(locked(&app), "a tap under the pad leaves the pad on top");
    assert_ne!(app.top_route(), Some(&Route::Library { page: 0 }));

    // A swipe from the top edge does not open the quick settings sheet over the pad.
    let t0 = 1_000_000;
    let at = |y: i32, us: u64| {
        InputEvent::Touch(TouchEvent {
            slot: 0,
            phase: if us == t0 { Phase::Down } else { Phase::Move },
            pos: Point::new(screen.w as i32 / 2, y),
            time_us: us,
        })
    };
    app.process(vec![at(5, t0), at(200, t0 + 100_000), at(400, t0 + 200_000)])
        .unwrap();
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: Point::new(screen.w as i32 / 2, 400),
        time_us: t0 + 250_000,
    })])
    .unwrap();
    assert!(locked(&app));
    assert!(
        app.find_rect("quick-wifi").is_none(),
        "no quick settings over the pad"
    );

    // Back does not close the pad.
    app.key(Key::Back).unwrap();
    assert!(locked(&app), "Back does not close the pad");
}

#[test]
fn the_power_button_sleeps_from_the_pad() {
    let dir = scratch("lock-power");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    save_pin(&dir, "4826");
    sleep_and_wake(&mut app);
    assert!(locked(&app));
    app.key(Key::Power).unwrap();
    assert!(app.sleeping(), "the power key sleeps the device from the pad");
}

#[test]
fn the_right_pin_unlocks() {
    let dir = scratch("lock-unlock");
    let mut app = make_app(&dir, Vec::new());
    save_pin(&dir, "4826");
    app.start().unwrap();
    assert!(locked(&app), "a start with a PIN set shows the pad first");

    enter(&mut app, "4827");
    assert!(locked(&app), "a wrong PIN keeps the pad");
    enter(&mut app, "4826");
    assert_eq!(
        app.top_route(),
        Some(&Route::Home),
        "the right PIN closes the pad"
    );
}

#[test]
fn wrong_pins_back_off_and_the_wait_survives_a_restart() {
    let dir = scratch("lock-backoff");
    let data = data_of(&dir);
    save_pin(&dir, "4826");
    {
        let mut app = make_app(&dir, Vec::new());
        app.start().unwrap();
        for _ in 0..5 {
            enter(&mut app, "1234");
        }
        assert!(locked(&app));
        let stored = store::load_attempts(&data);
        assert_eq!(stored.wrong, 5);
        assert!(stored.until > 0, "the fifth wrong try starts a wait");

        // The right PIN is refused while the wait runs.
        enter(&mut app, "4826");
        assert!(locked(&app), "the right PIN waits out the back-off");
        assert_eq!(
            store::load_attempts(&data).wrong,
            5,
            "a refused try is not counted"
        );
    }

    // A restart keeps the count, so the wait is not skipped.
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    assert!(locked(&app));
    enter(&mut app, "4826");
    assert!(locked(&app), "the wait survives a restart");
}

#[test]
fn deleting_the_pin_file_removes_the_lock() {
    let dir = scratch("lock-recover");
    let data = data_of(&dir);
    save_pin(&dir, "4826");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    assert!(locked(&app));
    drop(app);

    // The recovery from the pad: delete `.kaia/pin`, then start again.
    fs::remove_file(data.join(store::FILE)).unwrap();
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    assert_eq!(app.top_route(), Some(&Route::Home), "no PIN file, no lock");
    sleep_and_wake(&mut app);
    assert_eq!(app.top_route(), Some(&Route::Home), "no lock after a wake either");
}

#[test]
fn settings_sets_changes_and_turns_off_the_pin() {
    let dir = scratch("lock-settings");
    let data = data_of(&dir);
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    app.command("settings:device", &[]);
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "device".into()
        })
    );

    // Set: the new PIN twice.
    app.command("settings:pin:set", &[]);
    enter(&mut app, "4826");
    assert!(
        !pin_file(&dir).exists(),
        "nothing is stored until the second entry"
    );
    enter(&mut app, "4826");
    assert!(pin_file(&dir).exists());
    assert!(store::load(&data).unwrap().matches("4826"));
    assert!(app.find_rect("pin-dots").is_none(), "the flow closed");

    // Change: the old PIN first, then the new one twice.
    app.command("settings:pin:change", &[]);
    enter(&mut app, "1111");
    assert!(
        app.find_rect("pin-dots").is_some(),
        "a wrong old PIN keeps the flow open"
    );
    enter(&mut app, "4826");
    enter(&mut app, "13579");
    enter(&mut app, "24680");
    assert!(
        app.find_rect("pin-dots").is_some(),
        "the two entries differ, so the flow starts again"
    );
    enter(&mut app, "13579");
    enter(&mut app, "13579");
    assert!(app.find_rect("pin-dots").is_none());
    assert!(store::load(&data).unwrap().matches("13579"));
    assert!(!store::load(&data).unwrap().matches("4826"));

    // Turn off: the PIN first. The file goes, and the device no longer locks.
    app.command("settings:pin:off", &[]);
    enter(&mut app, "4826");
    assert!(pin_file(&dir).exists(), "a wrong PIN does not turn the lock off");
    enter(&mut app, "13579");
    assert!(!pin_file(&dir).exists());
    sleep_and_wake(&mut app);
    assert!(!locked(&app), "the cleared lock does not show on wake");
}

#[test]
fn back_cancels_a_pin_flow_and_keeps_the_lock() {
    let dir = scratch("lock-cancel");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    app.command("settings:device", &[]);
    app.command("settings:pin:set", &[]);
    type_digits(&mut app, "123");
    app.command("back", &[]);
    assert!(app.find_rect("pin-dots").is_none());
    assert!(!pin_file(&dir).exists());
}

#[test]
fn the_hash_is_not_the_digits_and_differs_with_the_salt() {
    let dir = scratch("lock-hash");
    let data = data_of(&dir);
    fs::create_dir_all(&data).unwrap();
    store::save(&data, "4826").unwrap();
    let first = fs::read_to_string(pin_file(&dir)).unwrap();
    store::save(&data, "4826").unwrap();
    let second = fs::read_to_string(pin_file(&dir)).unwrap();
    assert_ne!(first, second, "a new salt gives a new file");
    let hash = |text: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix("device.pin_hash="))
            .unwrap()
            .to_string()
    };
    assert_eq!(hash(&first).len(), 64, "the hash is 32 bytes, not the digits");
    assert_ne!(
        hash(&first),
        hash(&second),
        "the same digits hash differently with each salt"
    );
}

#[test]
fn the_pad_matches_its_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("lock-golden");
    save_pin(&dir, "4826");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    assert!(locked(&app));
    let recovery = app
        .find_rect("pin-recovery")
        .expect("the pad says how to recover");
    eprintln!("recovery rect {recovery:?}, screen {:?}", app.screen());
    assert!(recovery.h > 0 && recovery.y + recovery.h as i32 <= app.screen().h as i32);
    assert_matches_golden(app.frame(), &golden("screen_lock_pad.png"), 14);
}
