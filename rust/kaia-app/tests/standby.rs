//! Standby: the idle state between input and sleep, on a manual clock. The timers are
//! unit tests in `app::power`. These tests set the period through the Settings radio, as a
//! user does, and check that standby neither suspends nor starts Wi-Fi.

mod common;

use std::time::Duration;

use common::*;
use kaia_app::surfaces::settings::{commands, entries::Entry, keys, sections};
use kaia_app::{App, AppOptions, HeadlessBackend, ScriptInput, Services};
use kaia_events::{ManualClock, Value};
use kaia_input::Key;
use kaia_platform::{Env, Platform};

fn manual(name: &str) -> (App<HeadlessBackend>, ManualClock) {
    let dir = scratch(name);
    let env = Env {
        home: Some(dir.clone()),
        ..Env::default()
    };
    let platform = Platform::detect_in(&dir, &env).unwrap();
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
    (app, clock)
}

fn advance(app: &mut App<HeadlessBackend>, clock: &ManualClock, secs: u64) {
    clock.advance(Duration::from_secs(secs));
    app.process(Vec::new()).unwrap();
}

#[test]
fn standby_radio_offers_off_and_three_periods_and_saves_the_pick() {
    let (mut app, _clock) = manual("standby-radio");
    app.start().unwrap();
    let rows = sections::entries("device", app.services(), 0);
    let radio = rows.iter().find_map(|row| match row {
        Entry::Radio {
            key,
            options,
            selected,
        } if *key == keys::DEVICE_STANDBY_MINUTES => Some((options.clone(), *selected)),
        _ => None,
    });
    let (options, selected) = radio.expect("the Device section has the standby radio");
    assert_eq!(options, ["Off", "1 min", "2 min", "5 min"]);
    assert_eq!(selected, 0, "standby is off by default");
    assert_eq!(
        commands::value_for_pick(keys::DEVICE_STANDBY_MINUTES, 2),
        Some(Value::Number(2.0))
    );
    assert_eq!(commands::value_for_pick(keys::DEVICE_STANDBY_MINUTES, 4), None);
    // The pick belongs to the open Device section, as a tap on the radio sends it.
    app.command("nav:settings", &[]);
    app.command("settings:device", &[]);
    app.process(Vec::new()).unwrap();
    app.command("settings:pick:device.standby_minutes", &[Value::Number(2.0)]);
    app.process(Vec::new()).unwrap();
    assert_eq!(
        app.services()
            .settings
            .read(keys::DEVICE_STANDBY_MINUTES)
            .as_f64(),
        Some(2.0),
        "index 2 is two minutes"
    );
}

#[test]
fn standby_wakes_at_once_keeps_wifi_and_leaves_the_sleep_time_in_charge() {
    let (mut app, clock) = manual("standby-wake");
    app.start().unwrap();
    app.services_mut()
        .settings
        .save(keys::DEVICE_SLEEP_MINUTES, Value::Number(5.0));
    app.services_mut()
        .settings
        .save(keys::DEVICE_STANDBY_MINUTES, Value::Number(1.0));
    let wifi = app.wifi_requests();

    advance(&mut app, &clock, 59);
    assert!(!app.standby());
    advance(&mut app, &clock, 1);
    assert!(app.standby(), "one idle minute enters standby");
    assert!(!app.sleeping(), "standby keeps the device awake");
    assert_eq!(app.wifi_requests(), wifi, "standby does not change Wi-Fi");

    app.key(Key::RPgFwd).unwrap();
    assert!(!app.standby(), "a key press wakes the device at once");
    assert!(!app.sleeping());

    // The sleep time counts from the press, and it still suspends the device later.
    advance(&mut app, &clock, 299);
    assert!(!app.sleeping());
    advance(&mut app, &clock, 1);
    assert!(app.sleeping(), "the sleep time suspends after standby");
}
