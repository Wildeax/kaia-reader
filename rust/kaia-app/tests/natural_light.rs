//! Natural light uses a manual app clock and local date and time.

mod common;

use chrono::{NaiveDate, NaiveDateTime};
use common::*;
use kaia_app::surfaces::settings::{
    entries::{Entry, StepUnit},
    keys, sections,
};
use kaia_app::{App, AppOptions, HeadlessBackend, ScriptInput, Services};
use kaia_events::{Clock, ManualClock, Value};
use kaia_platform::{device::Model, platform::Kind, Env, Platform};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

fn local(hour: u32, minute: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, 14)
        .unwrap()
        .and_hms_opt(hour, minute, 0)
        .unwrap()
}

fn app_at(name: &str, start: NaiveDateTime, warmth: u8) -> (App<HeadlessBackend>, ManualClock) {
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
    let mut services = Services::open(platform, panel(), Some(dir.join("data"))).unwrap();
    services
        .settings
        .save(keys::DEVICE_SLEEP_MINUTES, Value::Number(0.0));
    services
        .settings
        .save(keys::DISPLAY_NATURAL_LIGHT_ENABLED, Value::Bool(true));
    services.platform.frontlight.set_intensity(20).unwrap();
    services.platform.frontlight.set_warmth(warmth).unwrap();
    let clock = ManualClock::new();
    let wall = clock.clone();
    let mut app = App::new(
        HeadlessBackend::new(panel(), ScriptInput::idle()),
        services,
        Box::new(clock.clone()),
        AppOptions::default(),
    );
    app.set_natural_light_clock(move || start + chrono::Duration::from_std(wall.now()).unwrap());
    app.start().unwrap();
    tick(&mut app, &clock, 0);
    (app, clock)
}

fn tick(app: &mut App<HeadlessBackend>, clock: &ManualClock, millis: u64) {
    clock.advance(Duration::from_millis(millis));
    app.process(vec![]).unwrap();
}

fn warmth(app: &App<HeadlessBackend>) -> u8 {
    app.services().platform.frontlight.warmth()
}

#[test]
fn warmth_moves_after_1800_on_half_hour_boundaries() {
    let (mut app, clock) = app_at("natural-evening", local(17, 59), 0);
    tick(&mut app, &clock, 60_000);
    assert_eq!(warmth(&app), 0);
    tick(&mut app, &clock, 29 * 60_000);
    assert_eq!(warmth(&app), 0);
    tick(&mut app, &clock, 60_000);
    assert_eq!(warmth(&app), 0);
    tick(&mut app, &clock, 1000);
    assert_eq!(warmth(&app), 1);
    tick(&mut app, &clock, 5000);
    assert_eq!(warmth(&app), 9);
    tick(&mut app, &clock, 10 * 60_000);
    assert_eq!(warmth(&app), 9);
}

#[test]
fn warmth_returns_to_daytime_at_0700() {
    let (mut app, clock) = app_at("natural-morning", local(5, 0), 92);
    for expected in [69, 46, 23, 0] {
        tick(&mut app, &clock, 30 * 60_000);
        tick(&mut app, &clock, 15_000);
        assert_eq!(warmth(&app), expected);
    }
}

#[test]
fn full_day_to_bedtime_fade_takes_sixty_seconds_in_small_steps() {
    let (mut app, clock) = app_at("natural-fade", local(23, 0), 0);
    for second in 1..=60 {
        let before = warmth(&app);
        tick(&mut app, &clock, 1000);
        let after = warmth(&app);
        assert!((1..=2).contains(&(after - before)));
        assert_eq!(after, (92 * second / 60) as u8);
    }
    assert_eq!(warmth(&app), 92);
    assert_eq!(app.services().settings.read("display.warmth").as_i64(), Some(92));
}

#[test]
fn manual_warmth_cancels_the_fade_until_the_next_boundary() {
    let (mut app, clock) = app_at("natural-manual", local(23, 0), 0);
    tick(&mut app, &clock, 10_000);
    app.command("settings:quick:warmth", &[Value::Number(35.0)]);
    tick(&mut app, &clock, 0);
    tick(&mut app, &clock, 1_789_000);
    assert_eq!(warmth(&app), 35);
    tick(&mut app, &clock, 1000);
    assert_eq!(warmth(&app), 35);
    tick(&mut app, &clock, 60_000);
    assert_eq!(warmth(&app), 92);
}

#[test]
fn manual_warmth_during_daytime_holds_until_1800() {
    let (mut app, clock) = app_at("natural-day-manual", local(12, 0), 0);
    app.command("settings:quick:warmth", &[Value::Number(35.0)]);
    tick(&mut app, &clock, 0);
    tick(&mut app, &clock, 6 * 3_600_000 - 1000);
    assert_eq!(warmth(&app), 35);
    tick(&mut app, &clock, 1000);
    tick(&mut app, &clock, 60_000);
    assert_eq!(warmth(&app), 0);
}

#[test]
fn reasserting_current_warmth_also_pauses_the_fade() {
    let (mut app, clock) = app_at("natural-same-value", local(23, 0), 0);
    tick(&mut app, &clock, 10_000);
    let current = warmth(&app);
    app.command("settings:quick:warmth", &[Value::Number(f64::from(current))]);
    tick(&mut app, &clock, 0);
    tick(&mut app, &clock, 60_000);
    assert_eq!(warmth(&app), current);
}

#[test]
fn disabling_or_turning_off_the_light_cancels_pending_steps() {
    let (mut app, clock) = app_at("natural-off", local(23, 0), 0);
    tick(&mut app, &clock, 10_000);
    let current = warmth(&app);
    app.services_mut().platform.frontlight.set_intensity(0).unwrap();
    tick(&mut app, &clock, 60_000);
    assert_eq!(warmth(&app), current);
    app.services_mut().platform.frontlight.set_intensity(20).unwrap();
    tick(&mut app, &clock, 0);
    tick(&mut app, &clock, 1000);
    assert!(warmth(&app) > current);
    let current = warmth(&app);
    app.services_mut()
        .settings
        .save(keys::DISPLAY_NATURAL_LIGHT_ENABLED, Value::Bool(false));
    tick(&mut app, &clock, 60_000);
    assert_eq!(warmth(&app), current);
}

#[test]
fn overnight_bedtime_and_clock_changes_recompute_the_target() {
    let (mut app, clock) = app_at("natural-clock", local(23, 59), 0);
    app.services_mut()
        .settings
        .save(keys::DISPLAY_NATURAL_LIGHT_BEDTIME, Value::Number(30.0));
    tick(&mut app, &clock, 0);
    tick(&mut app, &clock, 60_000);
    tick(&mut app, &clock, 60_000);
    assert_eq!(warmth(&app), 85);
    tick(&mut app, &clock, 29 * 60_000);
    tick(&mut app, &clock, 60_000);
    assert_eq!(warmth(&app), 92);
    let wall = Rc::new(Cell::new(local(23, 0)));
    let read = wall.clone();
    app.set_natural_light_clock(move || read.get());
    tick(&mut app, &clock, 0);
    wall.set(local(12, 0));
    tick(&mut app, &clock, 0);
    tick(&mut app, &clock, 60_000);
    assert_eq!(warmth(&app), 0);
}

#[test]
fn settings_rows_follow_warmth_support_and_commands_persist_all_four_keys() {
    let (mut app, _) = app_at("natural-settings", local(12, 0), 0);
    assert!(!sections::entries("display", app.services(), 0)
        .iter()
        .any(|entry| {
            matches!(
                entry,
                Entry::Toggle {
                    key: keys::DISPLAY_NATURAL_LIGHT_ENABLED,
                    ..
                }
            )
        }));
    app.services_mut().platform.kind = Kind::Kobo(Model::LibraColour);
    let rows = sections::entries("display", app.services(), 0);
    assert!(rows.iter().any(|entry| matches!(
        entry,
        Entry::Toggle {
            key: keys::DISPLAY_NATURAL_LIGHT_ENABLED,
            ..
        }
    )));
    assert!(rows.iter().any(|entry| matches!(
        entry,
        Entry::Stepper {
            key: keys::DISPLAY_NATURAL_LIGHT_BEDTIME,
            min: 0.0,
            max: 1410.0,
            step: 30.0,
            unit: StepUnit::ClockTime,
            ..
        }
    )));
    app.command("nav:settings", &[]);
    app.command("settings:display", &[]);
    app.process(vec![]).unwrap();
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden("settings_natural_light.png"), 14);
    for (key, value) in [
        (keys::DISPLAY_NATURAL_LIGHT_ENABLED, Value::Bool(false)),
        (keys::DISPLAY_NATURAL_LIGHT_BEDTIME, Value::Number(30.0)),
        (keys::DISPLAY_NATURAL_LIGHT_DAY_WARMTH, Value::Number(10.0)),
        (keys::DISPLAY_NATURAL_LIGHT_BEDTIME_WARMTH, Value::Number(80.0)),
    ] {
        app.command(&format!("settings:set:{key}"), std::slice::from_ref(&value));
        let saved = kaia_events::Settings::read_exact(&app.services().settings_path())
            .unwrap()
            .unwrap();
        assert_eq!(saved.read(key), &value);
    }
}
