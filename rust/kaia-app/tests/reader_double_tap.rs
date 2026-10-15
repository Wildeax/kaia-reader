//! Double tap lookup in the reader, with `reading.double_tap_lookup` on and off.
//!
//! With the setting on, a double tap on a word opens the dictionary sheet for that
//! word, and a double tap off text does nothing. A single tap turns the page once the
//! double tap window has passed, and the corner bookmark still works on a single tap.
//! With the setting off, a single tap turns the page at once, as it did before the
//! setting.
//!
//! The tests run on a manual clock, so the window is exact: a tap waits until the
//! clock moves past it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kaia_app::surfaces::reader::page::{PageView, PAGE_KEY};
use kaia_app::surfaces::settings::keys::READING_DOUBLE_TAP_LOOKUP;
use kaia_app::{App, AppOptions, HeadlessBackend, Route, ScriptInput, Services};
use kaia_display::InputEvent;
use kaia_events::{Clock, ManualClock, Value};
use kaia_gfx::Point;
use kaia_input::{Key, Phase, TouchEvent};
use kaia_platform::{Env, Platform};
use kaia_ui::components::{Label, Paragraph};

mod common;

use common::*;

/// The double tap window of the recognizer, which the reader uses too.
const WINDOW: Duration = Duration::from_millis(300);

/// Opens juliet.epub on a manual clock. `lookup` sets `reading.double_tap_lookup`.
fn reader(name: &str, lookup: bool) -> (App<HeadlessBackend>, ManualClock, PathBuf) {
    let dir = scratch(name);
    let books = library_with_books(&dir);
    let platform = Platform::detect_in(
        &dir,
        &Env {
            home: Some(dir.clone()),
            ..Env::default()
        },
    )
    .unwrap();
    let data = dir.join("data");
    pin_english(&data);
    let services = Services::open(platform, panel(), Some(data)).unwrap();
    let clock = ManualClock::new();
    let mut app = App::new(
        HeadlessBackend::new(panel(), ScriptInput::idle()),
        services,
        Box::new(clock.clone()),
        AppOptions {
            library_roots: vec![books],
        },
    );
    app.start().unwrap();
    idle(&mut app);
    app.services_mut()
        .settings
        .save(READING_DOUBLE_TAP_LOOKUP, Value::from(lookup));
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    idle(&mut app);
    (app, clock, dir)
}

/// Runs the loop until the scan and the reader's work are done. The clock does not move.
fn idle(app: &mut App<HeadlessBackend>) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        app.process(Vec::new()).unwrap();
        if !app.reader_busy() && !app.scan_running() {
            break;
        }
        assert!(Instant::now() < deadline, "timed out waiting for the reader");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// The time on the clock in microseconds, the unit of the touch samples.
fn now_us(clock: &ManualClock) -> u64 {
    clock.now().as_micros() as u64
}

/// A touch down and up at `p`, 80 ms long, starting now. Runs the loop once the touch lifts.
fn tap(app: &mut App<HeadlessBackend>, clock: &ManualClock, p: Point) {
    let t0 = now_us(clock);
    app.process(vec![
        InputEvent::Touch(TouchEvent {
            slot: 0,
            phase: Phase::Down,
            pos: p,
            time_us: t0,
        }),
        InputEvent::Touch(TouchEvent {
            slot: 0,
            phase: Phase::Up,
            pos: p,
            time_us: t0 + 80_000,
        }),
    ])
    .unwrap();
    clock.advance(Duration::from_micros(80_000));
    idle(app);
}

/// Two taps at `p`, `gap` apart on the clock.
fn double_tap(app: &mut App<HeadlessBackend>, clock: &ManualClock, p: Point, gap: Duration) {
    tap(app, clock, p);
    clock.advance(gap);
    tap(app, clock, p);
}

/// A hold at `p`, long enough for the hold timer. It ends with the touch lifting.
fn hold_at(app: &mut App<HeadlessBackend>, clock: &ManualClock, p: Point) {
    let t0 = now_us(clock);
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Down,
        pos: p,
        time_us: t0,
    })])
    .unwrap();
    clock.advance(Duration::from_millis(600));
    app.process(Vec::new()).unwrap();
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: p,
        time_us: now_us(clock),
    })])
    .unwrap();
    idle(app);
}

/// Turns to a page of juliet with text. Page 0 is the cover.
fn turn_to_text(app: &mut App<HeadlessBackend>) {
    for _ in 0..8 {
        app.key(Key::RPgFwd).unwrap();
        idle(app);
    }
}

/// The page shown (0-based).
fn page(app: &mut App<HeadlessBackend>) -> u32 {
    idle(app);
    let mut position = None;
    assert!(app.update_top::<PageView, _>(PAGE_KEY, |view, _| {
        position = view.position.map(|(page, _)| page);
    }));
    position.expect("the reader window is on top and has a position")
}

/// A point on the right third of the page, outside the centre square. A tap there turns forward.
fn forward_zone(app: &App<HeadlessBackend>) -> Point {
    Point::new(app.screen().w as i32 * 5 / 6, app.screen().h as i32 / 2)
}

/// The word on the text page at this point, as the hold tests use it.
fn word_point() -> Point {
    Point::new(200, 120)
}

/// A point in the left margin, off the text. It is in the back zone, so a single tap there
/// turns back.
fn margin_point() -> Point {
    Point::new(4, 600)
}

/// The top right corner, where a tap toggles the bookmark.
fn corner_point(app: &App<HeadlessBackend>) -> Point {
    Point::new(app.screen().w as i32 - 10, 10)
}

/// The number of dark pixels in the `size` square at the top right corner of the frame.
fn dark_in_corner(app: &App<HeadlessBackend>, size: i32) -> usize {
    let frame = app.frame();
    let x0 = app.screen().w as i32 - size;
    let mut dark = 0;
    for y in 0..size {
        for x in x0..x0 + size {
            if frame.get(x, y).is_some_and(|c| c.luma() < 128) {
                dark += 1;
            }
        }
    }
    dark
}

fn text_of(app: &mut App<HeadlessBackend>, key: &str) -> String {
    let mut text = None;
    app.update_top::<Paragraph, _>(key, |p, _| text = Some(p.text().to_string()));
    if text.is_none() {
        app.update_top::<Label, _>(key, |l, _| text = Some(l.text().to_string()));
    }
    text.unwrap_or_else(|| panic!("no text widget {key:?} on the top window"))
}

/// The first word of a label, without the punctuation around it.
fn word_of(label: &str) -> String {
    label
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_string()
}

/// Writes a one-word StarDict dictionary into `<data>/dict`.
fn write_stardict(data_dir: &Path, word: &str, definition: &str) {
    let dir = data_dir.join("dict");
    fs::create_dir_all(&dir).unwrap();
    let body = definition.as_bytes();
    let mut idx = Vec::new();
    idx.extend_from_slice(word.as_bytes());
    idx.push(0);
    idx.extend_from_slice(&0u32.to_be_bytes());
    idx.extend_from_slice(&(body.len() as u32).to_be_bytes());
    fs::write(dir.join("test.dict"), body).unwrap();
    fs::write(dir.join("test.idx"), &idx).unwrap();
    let ifo = format!(
        "StarDict's dict ifo file\nversion=2.4.2\nbookname=Test Dictionary\nwordcount=1\nidxfilesize={}\nsametypesequence=m\n",
        idx.len()
    );
    fs::write(dir.join("test.ifo"), ifo).unwrap();
}

#[test]
fn a_double_tap_on_a_word_opens_the_dictionary_with_that_word() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock, dir) = reader("double-tap-word", true);
    turn_to_text(&mut app);

    // A hold selects the word and shows its popover. The word tells the test what to define.
    hold_at(&mut app, &clock, word_point());
    let word = word_of(&text_of(&mut app, "sel-word"));
    assert!(!word.is_empty(), "a word is selected");
    app.key(Key::Back).unwrap();
    idle(&mut app);
    write_stardict(&dir.join("data"), &word, "the definition of the word");

    double_tap(&mut app, &clock, word_point(), Duration::from_millis(100));
    clock.advance(WINDOW);
    idle(&mut app);
    assert_eq!(
        app.top_route(),
        Some(&Route::ReaderPanel),
        "the dictionary sheet is open"
    );
    assert!(
        text_of(&mut app, "dict-body").contains("the definition of the word"),
        "the sheet looks up the word under the double tap"
    );
}

#[test]
fn a_double_tap_off_text_does_nothing() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock, _dir) = reader("double-tap-margin", true);
    turn_to_text(&mut app);
    let before = page(&mut app);

    double_tap(&mut app, &clock, margin_point(), Duration::from_millis(100));
    idle(&mut app);
    assert!(
        matches!(app.top_route(), Some(Route::Reader { .. })),
        "the reader stays on top"
    );
    assert_eq!(page(&mut app), before, "the double tap does not turn the page");

    // The first tap of the pair must not turn the page after the window either.
    clock.advance(WINDOW * 2);
    idle(&mut app);
    assert_eq!(page(&mut app), before, "no tap is left waiting");
    assert!(
        !matches!(app.top_route(), Some(Route::ReaderPanel)),
        "no sheet opens for a point off text"
    );
}

#[test]
fn a_single_tap_turns_the_page_once_the_window_has_passed() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock, _dir) = reader("double-tap-single", true);
    turn_to_text(&mut app);
    let before = page(&mut app);

    let point = forward_zone(&app);
    tap(&mut app, &clock, point);
    assert_eq!(page(&mut app), before, "the page waits for the double tap window");

    clock.advance(WINDOW);
    idle(&mut app);
    assert_eq!(
        page(&mut app),
        before + 1,
        "the single tap turns the page after the window"
    );
}

#[test]
fn the_corner_bookmark_still_works_on_a_single_tap() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock, _dir) = reader("double-tap-corner", true);
    turn_to_text(&mut app);
    let before = dark_in_corner(&app, 20);

    let point = corner_point(&app);
    tap(&mut app, &clock, point);
    clock.advance(WINDOW);
    idle(&mut app);
    assert!(
        dark_in_corner(&app, 20) > before,
        "the dog-ear is drawn after the single tap"
    );
    assert!(
        matches!(app.top_route(), Some(Route::Reader { .. })),
        "the corner opens no sheet"
    );
}

#[test]
fn a_double_tap_in_the_corner_does_not_toggle_the_bookmark() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock, _dir) = reader("double-tap-corner-twice", true);
    turn_to_text(&mut app);
    let before = dark_in_corner(&app, 20);
    let page_before = page(&mut app);

    let point = corner_point(&app);
    double_tap(&mut app, &clock, point, Duration::from_millis(100));
    clock.advance(WINDOW);
    idle(&mut app);
    assert_eq!(
        dark_in_corner(&app, 20),
        before,
        "two bookmark toggles would cancel out, so the double tap must not reach the corner"
    );
    assert_eq!(
        page(&mut app),
        page_before,
        "the double tap does not turn the page"
    );
}

#[test]
fn with_lookup_off_a_single_tap_turns_the_page_at_once() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock, _dir) = reader("double-tap-off-single", false);
    turn_to_text(&mut app);
    let before = page(&mut app);

    // No clock movement: the turn happens during the tap, as it did before the setting.
    let point = forward_zone(&app);
    tap(&mut app, &clock, point);
    assert_eq!(page(&mut app), before + 1, "the page turns with no wait");
}

#[test]
fn with_lookup_off_a_double_tap_turns_the_page_twice() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock, _dir) = reader("double-tap-off-double", false);
    turn_to_text(&mut app);
    let before = page(&mut app);

    let point = forward_zone(&app);
    double_tap(&mut app, &clock, point, Duration::from_millis(100));
    assert_eq!(
        page(&mut app),
        before + 2,
        "two taps are two page turns when the setting is off"
    );
}

#[test]
fn the_setting_is_a_known_key_and_is_off_by_default() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("double-tap-setting");
    let mut app = make_app(&dir, vec![library_with_books(&dir)]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    assert!(
        !app.services().settings.is_true(READING_DOUBLE_TAP_LOOKUP),
        "off by default"
    );
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-reading");
    app.command("settings:set:reading.double_tap_lookup", &[Value::Bool(true)]);
    app.process(Vec::new()).unwrap();
    assert!(
        app.services().settings.is_true(READING_DOUBLE_TAP_LOOKUP),
        "the Reading toggle turns it on"
    );
    app.command("settings:set:reading.double_tap_lookup", &[Value::Bool(false)]);
    app.process(Vec::new()).unwrap();
    assert!(
        !app.services().settings.is_true(READING_DOUBLE_TAP_LOOKUP),
        "and off again"
    );
}
