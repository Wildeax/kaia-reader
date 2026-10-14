//! Safe mode at launch: the marker file and the failed start count, the notice
//! the user sees, and the clear after a healthy start. The launcher side is in
//! `platform/kobo/kaia/test-kaia-rust.sh`.

mod common;
use common::*;

use std::fs;
use std::path::Path;
use std::time::Duration;

use kaia_app::safe_mode::{self, Reason, COUNTER, HEALTHY_AFTER, MARKER};
use kaia_app::{App, AppOptions, HeadlessBackend, ScriptInput, Services};
use kaia_events::ManualClock;
use kaia_platform::{Env, Platform};

/// An app over `data/` on a manual clock, with safe mode applied the way `main` does it.
fn app_in(data: &Path, requested: bool) -> (App<HeadlessBackend>, ManualClock, Option<Reason>) {
    let root = data.join("platform");
    fs::create_dir_all(&root).unwrap();
    let env = Env {
        home: Some(root.clone()),
        ..Env::default()
    };
    let platform = Platform::detect_in(&root, &env).unwrap();
    pin_english(&data.join("data"));
    let mut services = Services::open(platform, panel(), Some(data.join("data"))).unwrap();
    let reason = safe_mode::begin(&mut services, requested);
    let clock = ManualClock::new();
    let app = App::new(
        HeadlessBackend::new(panel(), ScriptInput::idle()),
        services,
        Box::new(clock.clone()),
        AppOptions::default(),
    );
    (app, clock, reason)
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Moves the clock past the healthy time in two steps, so the test also covers a
/// start that is not yet healthy.
fn run_to_healthy(app: &mut App<HeadlessBackend>, clock: &ManualClock) {
    clock.advance(HEALTHY_AFTER - Duration::from_secs(1));
    app.process(vec![]).unwrap();
    clock.advance(Duration::from_secs(2));
    app.process(vec![]).unwrap();
}

#[test]
fn marker_starts_safe_mode_with_notice_and_keeps_marker_after_healthy_start() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("safe-mode-marker");
    write(&data.join("data").join(MARKER), "");
    write(&data.join("data").join(COUNTER), "1\n");
    let (mut app, clock, reason) = app_in(&data, true);
    assert_eq!(reason, Some(Reason::Marker));
    #[cfg(feature = "lua")]
    assert!(app.services().plugins.startup_safe_mode);
    app.start().unwrap();
    assert!(
        app.notices().iter().any(|n| n.contains("safemode file")),
        "{:?}",
        app.notices()
    );
    assert!(data.join("data").join(COUNTER).is_file());
    run_to_healthy(&mut app, &clock);
    assert!(
        !data.join("data").join(COUNTER).exists(),
        "healthy start must clear the count"
    );
    assert!(
        data.join("data").join(MARKER).is_file(),
        "the marker stays until the user removes it"
    );
    app.shutdown();
}

#[test]
fn repeated_failures_start_safe_mode_until_a_healthy_start_clears_the_count() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("safe-mode-failures");
    // The launcher wrote 4 for this launch: three failed starts before it.
    write(&data.join("data").join(COUNTER), "4\n");
    let (mut app, clock, reason) = app_in(&data, true);
    assert_eq!(reason, Some(Reason::Failures));
    #[cfg(feature = "lua")]
    assert!(app.services().plugins.startup_safe_mode);
    assert_eq!(
        safe_mode::failed_starts(&data.join("data")),
        4,
        "the count stays until this start runs healthy, so a crash here keeps safe mode"
    );
    app.start().unwrap();
    assert!(
        app.notices().iter().any(|n| n.contains("several times in a row")),
        "{:?}",
        app.notices()
    );
    run_to_healthy(&mut app, &clock);
    assert!(!data.join("data").join(COUNTER).exists());
    assert!(!data.join("data").join(MARKER).exists(), "no marker was created");
    app.shutdown();
}

#[test]
fn count_below_limit_starts_normally_and_healthy_start_clears_it() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("safe-mode-below");
    write(&data.join("data").join(COUNTER), "3\n");
    // The launcher passes --safe-mode only above the limit, so a plain start has no flag.
    let (mut app, clock, reason) = app_in(&data, false);
    assert_eq!(reason, None);
    #[cfg(feature = "lua")]
    assert!(!app.services().plugins.startup_safe_mode);
    app.start().unwrap();
    assert!(app.notices().is_empty(), "{:?}", app.notices());
    clock.advance(HEALTHY_AFTER - Duration::from_secs(1));
    app.process(vec![]).unwrap();
    assert!(
        data.join("data").join(COUNTER).is_file(),
        "not healthy yet, so the count stays"
    );
    clock.advance(Duration::from_secs(2));
    app.process(vec![]).unwrap();
    assert!(!data.join("data").join(COUNTER).exists());
    app.shutdown();
}

#[test]
fn manual_safe_mode_flag_shows_notice_without_recovery_files() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("safe-mode-flag");
    let (mut app, _clock, reason) = app_in(&data, true);
    assert_eq!(reason, Some(Reason::Requested));
    app.start().unwrap();
    assert!(
        app.notices()
            .iter()
            .any(|n| n == &safe_mode::notice(Reason::Requested)),
        "{:?}",
        app.notices()
    );
    app.shutdown();
}

#[test]
fn unreadable_count_counts_as_zero() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let data = scratch("safe-mode-garbage");
    write(&data.join("data").join(COUNTER), "not a number");
    assert_eq!(safe_mode::failed_starts(&data.join("data")), 0);
    let (_app, _clock, reason) = app_in(&data, false);
    assert_eq!(reason, None);
}
