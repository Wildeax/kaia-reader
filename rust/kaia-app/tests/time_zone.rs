//! Headless tests for the time zone setting. A saved zone sets the process `TZ`
//! at startup, so chrono's local clock follows it. The Time zone page writes the
//! key and ignores names that are not on offer. The process `TZ` does not change
//! while the app runs, so a picked zone shows after the next start.
//!
//! Every test takes the same lock and restores `TZ` when it ends, because the
//! tests share one process environment.

use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use kaia_app::surfaces::settings::{entries::Entry, keys, sections};
use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::{Settings, Value};
use kaia_platform::timezone;

mod common;

use common::*;

/// The instant the clock checks read: 2023-11-14 22:13:20 UTC.
const INSTANT: i64 = 1_700_000_000;

static LOCK: Mutex<()> = Mutex::new(());

/// Holds the lock. Dropping it puts back the `TZ` the test started with.
struct ZoneGuard {
    _lock: MutexGuard<'static, ()>,
    tz: Option<OsString>,
}

fn zone_guard() -> ZoneGuard {
    let lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ZoneGuard {
        _lock: lock,
        tz: std::env::var_os("TZ"),
    }
}

impl Drop for ZoneGuard {
    fn drop(&mut self) {
        match &self.tz {
            Some(value) => std::env::set_var("TZ", value),
            None => std::env::remove_var("TZ"),
        }
    }
}

/// The local clock at [`INSTANT`], as `HH:MM +HH:MM`. It runs on a new thread,
/// because chrono keeps a zone cache per thread and reads `TZ` only when the
/// cache is new or a second old.
fn local_clock() -> String {
    std::thread::spawn(|| {
        let utc = chrono::DateTime::from_timestamp(INSTANT, 0).unwrap();
        utc.with_timezone(&chrono::Local).format("%H:%M %:z").to_string()
    })
    .join()
    .unwrap()
}

/// Writes `settings.json` in the data folder `dir/data` with the zone `name`.
fn save_zone(dir: &Path, name: &str) {
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    let path = data.join("settings.json");
    let mut settings = Settings::open(&path);
    settings.save(keys::DEVICE_TIME_ZONE, Value::from(name));
    settings.flush(&path).unwrap();
}

/// The zone saved in `dir/data/settings.json`, or None when the key is unset.
fn saved_zone(dir: &Path) -> Option<String> {
    Settings::open(&dir.join("data/settings.json"))
        .read(keys::DEVICE_TIME_ZONE)
        .as_str()
        .map(str::to_string)
}

/// Opens the Time zone page: the Device section from the root, then the page.
fn open_time_zone_page(app: &mut App<HeadlessBackend>) {
    app.start().unwrap();
    tap_key(app, "nav-settings");
    tap_key(app, "settings-device");
    app.command("settings:timezone", &[]);
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "timezone".into()
        })
    );
}

#[test]
fn a_saved_zone_sets_the_clock_at_startup() {
    let _guard = zone_guard();
    let dir = scratch("time-zone-startup");
    // Kolkata is UTC+05:30. The test host can be at any offset, so a zone with a
    // different offset shows that the startup step ran.
    save_zone(&dir, "Asia/Kolkata");
    let _app = make_app(&dir, vec![]);
    assert_eq!(std::env::var("TZ").unwrap(), "IST-5:30");
    assert_eq!(local_clock(), "03:43 +05:30");
}

#[test]
fn a_zone_with_daylight_saving_follows_its_rule() {
    let _guard = zone_guard();
    let dir = scratch("time-zone-berlin");
    save_zone(&dir, "Europe/Berlin");
    let _app = make_app(&dir, vec![]);
    // November is outside summer time, so Berlin is at UTC+01:00.
    assert_eq!(local_clock(), "23:13 +01:00");
}

#[test]
fn an_unknown_saved_zone_keeps_the_system_zone() {
    let _guard = zone_guard();
    let dir = scratch("time-zone-unknown");
    let before = local_clock();
    save_zone(&dir, "Mars/Olympus");
    let _app = make_app(&dir, vec![]);
    assert_eq!(std::env::var_os("TZ"), _guard.tz);
    assert_eq!(local_clock(), before);
}

#[test]
fn the_page_writes_the_zone_and_ignores_unknown_names() {
    let _guard = zone_guard();
    let dir = scratch("time-zone-picker");
    let mut app = make_app(&dir, vec![]);
    open_time_zone_page(&mut app);

    app.command("settings:timezone:Europe/Berlin", &[]);
    assert_eq!(saved_zone(&dir).as_deref(), Some("Europe/Berlin"));
    assert_eq!(
        app.services().settings.read(keys::DEVICE_TIME_ZONE).as_str(),
        Some("Europe/Berlin")
    );

    app.command("settings:timezone:Mars/Olympus", &[]);
    assert_eq!(saved_zone(&dir).as_deref(), Some("Europe/Berlin"));

    app.command("settings:timezone:", &[]);
    assert_eq!(saved_zone(&dir), None);
    assert!(keys::time_zone(&app.services().settings).is_none());

    // The picks do not change the zone of the running process.
    assert_eq!(std::env::var_os("TZ"), _guard.tz);
}

#[test]
fn the_page_lists_system_default_first_then_zones_by_offset() {
    let _guard = zone_guard();
    let dir = scratch("time-zone-list");
    save_zone(&dir, "America/Bogota");
    let app = make_app(&dir, vec![]);

    let rows = sections::entries("timezone", app.services(), 0);
    let choices: Vec<(&str, bool)> = rows
        .iter()
        .filter_map(|row| match row {
            Entry::Choice { label, selected, .. } => Some((label.as_str(), *selected)),
            _ => None,
        })
        .collect();

    assert_eq!(choices[0], ("System default", false));
    let zones = timezone::sorted();
    assert_eq!(choices.len(), zones.len() + 1);
    for (zone, (label, selected)) in zones.iter().zip(&choices[1..]) {
        assert_eq!(*label, format!("{}  {}", timezone::offset_label(zone), zone.name));
        assert_eq!(*selected, zone.name == "America/Bogota");
    }
    assert_eq!(choices[1].0, "UTC-10:00  Pacific/Honolulu");
}

#[test]
fn the_device_section_shows_the_zone_in_use() {
    let _guard = zone_guard();
    let dir = scratch("time-zone-row");
    save_zone(&dir, "America/Bogota");
    let app = make_app(&dir, vec![]);

    let rows = sections::entries("device", app.services(), 0);
    assert!(rows.iter().any(|row| matches!(
        row,
        Entry::Row { primary, action: Some(action), .. }
            if primary == "America/Bogota" && action == "settings:timezone"
    )));
}
