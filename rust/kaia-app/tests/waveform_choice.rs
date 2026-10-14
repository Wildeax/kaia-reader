//! The page turn waveform. A page turn sends a partial refresh, and the reader setting
//! `reading.page_turn_waveform` picks the panel waveform for it. Full refreshes keep
//! their waveform. The tests read the refreshes the headless panel recorded.

use std::fs;
use std::path::Path;
use std::time::Duration;

use kaia_app::{App, HeadlessBackend};
use kaia_display::Waveform;
use kaia_events::{RefreshMode, Settings, Value};
use kaia_input::Key;

mod common;

use common::*;

const WAVEFORM_KEY: &str = "reading.page_turn_waveform";
const CADENCE_KEY: &str = "reading.full_refresh_pages";

/// Writes the page turn setting (when given) and the refresh cadence to the data
/// folder of `dir`. The app reads both at startup.
fn write_settings(dir: &Path, waveform: Option<&str>, cadence: f64) {
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    let path = data.join("settings.json");
    let mut settings = Settings::open(&path);
    settings.save(CADENCE_KEY, Value::from(cadence));
    if let Some(waveform) = waveform {
        settings.save(WAVEFORM_KEY, Value::from(waveform));
    }
    settings.flush(&path).unwrap();
}

/// Opens `name` from the library of `dir`. The settings must be written first.
fn open_book(dir: &Path, name: &str) -> App<HeadlessBackend> {
    let lib = library_with_books(dir);
    let mut app = make_app(dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    open_from_library(&mut app, name);
    app
}

/// Turns one page forward. Returns the mode of the refresh the turn sent and the
/// waveform the panel received for it.
fn turn_forward(app: &mut App<HeadlessBackend>) -> (RefreshMode, Waveform) {
    let modes_before = app.refresh_modes().len();
    let panel_before = app.backend().display().refreshes().len();
    app.key(Key::RPgFwd).unwrap();
    wait_reader(app);
    let modes = modes_after(app, modes_before);
    assert_eq!(modes.len(), 1, "one page turn sends one refresh, got {modes:?}");
    let panel = &app.backend().display().refreshes()[panel_before..];
    assert_eq!(panel.len(), 1, "one page turn draws one panel refresh");
    (modes[0], panel[0].waveform)
}

/// Turns `count` pages forward and checks each turn. A partial refresh must use
/// `partial`. A full refresh must keep a full waveform, Gc16 or Gcc16 on the color
/// panel. Returns how many turns were partial.
fn check_turns(app: &mut App<HeadlessBackend>, count: usize, partial: Waveform) -> usize {
    let mut partial_turns = 0;
    for turn in 0..count {
        let (mode, waveform) = turn_forward(app);
        match mode {
            RefreshMode::Partial => {
                partial_turns += 1;
                assert_eq!(waveform, partial, "page turn {turn} is partial");
            }
            RefreshMode::Full => assert!(
                matches!(waveform, Waveform::Gc16 | Waveform::Gcc16),
                "page turn {turn} is full and keeps its waveform, got {waveform:?}"
            ),
            other => panic!("page turn {turn} sent {other:?}"),
        }
    }
    partial_turns
}

#[test]
fn page_turns_use_the_cleaner_waveform_when_the_setting_is_unset() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("waveform-default");
    write_settings(&dir, None, 0.0);
    let mut app = open_book(&dir, "juliet.epub");
    let partial = check_turns(&mut app, 4, Waveform::Glr16);
    assert!(partial > 0, "the first pages of a book include partial turns");
}

#[test]
fn faster_page_turns_request_du_and_full_turns_keep_gc16() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("waveform-faster");
    // A cadence of three makes every third turn a full refresh, so both kinds occur.
    write_settings(&dir, Some("faster"), 3.0);
    let mut app = open_book(&dir, "juliet.epub");
    let partial = check_turns(&mut app, 9, Waveform::Du);
    assert!(
        partial >= 4,
        "a cadence of three leaves most turns partial, got {partial}"
    );
}

#[test]
fn the_settings_row_saves_the_choice_that_the_next_page_turn_uses() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("waveform-row");
    write_settings(&dir, None, 0.0);
    {
        let mut app = make_app(&dir, vec![library_with_books(&dir)]);
        app.start().unwrap();
        app.command("nav:settings", &[]);
        app.command("settings:reading", &[]);
        app.process(vec![]).unwrap();
        // Index 1 is "Faster page turns" in the Reading section.
        app.command("settings:pick:reading.page_turn_waveform", &[Value::Number(1.0)]);
        app.process(vec![]).unwrap();
        assert_eq!(
            app.services().settings.read(WAVEFORM_KEY).as_str(),
            Some("faster")
        );
    }
    // A new app reads the saved file, as after a restart.
    let mut app = open_book(&dir, "juliet.epub");
    let partial = check_turns(&mut app, 4, Waveform::Du);
    assert!(partial > 0);
}
