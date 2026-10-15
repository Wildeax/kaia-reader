//! Pinch and spread change the text size of a reflowable book, by KOReader's step rule.
//! The gestures come in as two-finger touch frames, as the device sends them.

mod common;

use common::*;
use kaia_app::surfaces::reader::page::PageView;
use kaia_app::{App, HeadlessBackend, ScriptInput, Services};
use kaia_display::InputEvent;
use kaia_events::{Clock, Event, ManualClock, Settings, Value};
use kaia_gfx::Point;
use kaia_input::{Phase, TouchEvent};
use kaia_platform::{Env, Platform};
use std::path::PathBuf;
use std::time::Duration;

/// The Libra Colour panel is 1264 by 1680. KOReader scales units by the shorter side.
const SHORT_SIDE: f64 = 1264.0;

/// The pixels of a size in KOReader units, as the app computes them (rounded up).
fn px_for(units: f64) -> f64 {
    (units * SHORT_SIDE / 600.0).ceil()
}

fn reader(name: &str) -> (App<HeadlessBackend>, ManualClock) {
    if std::env::var_os("FONTDIR").is_none() {
        std::env::set_var("FONTDIR", repo_root().join("resources/fonts"));
    }
    let dir = scratch(name);
    let books = library_with_books(&dir);
    std::fs::copy(repo_root().join("test/sample.pdf"), books.join("sample.pdf")).unwrap();
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
        kaia_app::AppOptions {
            library_roots: vec![books],
        },
    );
    app.start().unwrap();
    settle(&mut app, &clock);
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    settle(&mut app, &clock);
    (app, clock)
}

fn settle(app: &mut App<HeadlessBackend>, clock: &ManualClock) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while app.reader_busy() || app.scan_running() {
        assert!(std::time::Instant::now() < deadline);
        clock.advance(Duration::from_millis(20));
        app.process(vec![]).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    app.process(vec![]).unwrap();
}

fn state_path(app: &App<HeadlessBackend>, name: &str) -> PathBuf {
    let id = book_id(app, name);
    let b = app.services().library.book(id).unwrap().unwrap();
    app.services()
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{}.json", b.partial_md5))
}

/// The saved text size of the open book, in pixels, after the reader saves its state.
fn saved_px(app: &mut App<HeadlessBackend>, name: &str) -> f64 {
    app.broadcast_app(Event::new("SaveState"));
    let path = state_path(app, name);
    Settings::open(&path)
        .read("layout")
        .get("font_size")
        .as_f64()
        .expect("a saved font size")
}

/// One input frame with both fingers, as the Kobo driver sends it: a touch frame
/// per report, with one entry per slot. Sent as one frame, the detector sees both
/// fingers at once, so a lift reads as a pinch and not as a rotate.
fn frame(phase: Phase, points: [Point; 2], clock: &ManualClock) -> InputEvent {
    let time_us = clock.now().as_micros() as u64;
    InputEvent::TouchFrame(
        points
            .into_iter()
            .enumerate()
            .map(|(slot, pos)| TouchEvent {
                slot: slot as u8,
                phase,
                pos,
                time_us,
            })
            .collect(),
    )
}

/// Two fingers land, move in equal steps and lift. `from` and `to` hold the
/// start and end point of each finger, for slot 0 and slot 1.
fn two_finger(app: &mut App<HeadlessBackend>, clock: &ManualClock, from: [Point; 2], to: [Point; 2]) {
    two_finger_timed(app, clock, from, to, 20);
}

fn two_finger_timed(
    app: &mut App<HeadlessBackend>,
    clock: &ManualClock,
    from: [Point; 2],
    to: [Point; 2],
    step_ms: u64,
) {
    let mut fixed = false;
    app.update_top::<PageView, _>(kaia_app::surfaces::reader::page::PAGE_KEY, |view, _| {
        fixed = view.fixed_layout
    });
    let before = app.frame().data().to_vec();
    const STEPS: i32 = 10;
    app.process(vec![frame(Phase::Down, from, clock)]).unwrap();
    for i in 1..=STEPS {
        clock.advance(Duration::from_millis(step_ms));
        let at =
            |a: Point, b: Point| Point::new(a.x + (b.x - a.x) * i / STEPS, a.y + (b.y - a.y) * i / STEPS);
        app.process(vec![frame(
            Phase::Move,
            [at(from[0], to[0]), at(from[1], to[1])],
            clock,
        )])
        .unwrap();
        if fixed {
            assert!(
                app.frame().data() == before,
                "pinch should render only on release"
            );
        }
    }
    clock.advance(Duration::from_millis(20));
    app.process(vec![frame(Phase::Up, to, clock)]).unwrap();
    settle(app, clock);
}

/// The fingers move toward each other. One travels 300 px down and the other 120 px
/// up, so the travel adds up to 420 px and the span shrinks. Each finger moves past
/// the pan threshold (66 px at 300 dpi). KOReader's step is then
/// `ceil(30 * 420 / 1680) = 8`, which is 2 units.
fn pinch(app: &mut App<HeadlessBackend>, clock: &ManualClock) {
    two_finger(
        app,
        clock,
        [Point::new(400, 300), Point::new(800, 1200)],
        [Point::new(400, 600), Point::new(800, 1080)],
    );
}

/// The same movement reversed: the fingers move apart, and the travel adds up to 420 px.
fn spread(app: &mut App<HeadlessBackend>, clock: &ManualClock) {
    two_finger(
        app,
        clock,
        [Point::new(400, 600), Point::new(800, 1080)],
        [Point::new(400, 300), Point::new(800, 1200)],
    );
}

#[test]
fn pinch_shrinks_and_spread_grows_the_text_in_two_unit_steps() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = reader("pinch-steps");
    // The default is 22 units, which the app stores as 47 px on this panel.
    assert_eq!(saved_px(&mut app, "juliet.epub"), px_for(22.0));

    pinch(&mut app, &clock);
    assert_eq!(saved_px(&mut app, "juliet.epub"), px_for(20.0));
    pinch(&mut app, &clock);
    assert_eq!(saved_px(&mut app, "juliet.epub"), px_for(18.0));

    spread(&mut app, &clock);
    assert_eq!(saved_px(&mut app, "juliet.epub"), px_for(20.0));
    spread(&mut app, &clock);
    assert_eq!(saved_px(&mut app, "juliet.epub"), px_for(22.0));
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn pinch_stops_at_twelve_units_and_spread_stops_at_forty() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = reader("pinch-limits");

    // 22 to 12 is five steps of two. Further pinches leave the size at 12.
    for _ in 0..7 {
        pinch(&mut app, &clock);
    }
    assert_eq!(saved_px(&mut app, "juliet.epub"), px_for(12.0));

    // 12 to 40 is fourteen steps of two. Further spreads leave the size at 40.
    for units in (14..=40).step_by(2) {
        spread(&mut app, &clock);
        assert_eq!(saved_px(&mut app, "juliet.epub"), px_for(f64::from(units)));
    }
    for _ in 0..3 {
        spread(&mut app, &clock);
    }
    assert_eq!(saved_px(&mut app, "juliet.epub"), px_for(40.0));
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn pdf_pinch_changes_zoom_without_turning_the_page() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = reader("pinch-pdf");
    let id = book_id(&app, "2col.pdf");
    app.command(&format!("open_book:{id}"), &[]);
    settle(&mut app, &clock);
    let mut before = None;
    app.update_top::<PageView, _>(kaia_app::surfaces::reader::page::PAGE_KEY, |view, _| {
        before = view.position
    });
    pinch(&mut app, &clock);
    spread(&mut app, &clock);
    let mut after = None;
    app.update_top::<PageView, _>(kaia_app::surfaces::reader::page::PAGE_KEY, |view, _| {
        after = view.position
    });
    assert_eq!(before, after);
    let saved = saved_state(&mut app, "2col.pdf");
    assert!(saved.read("zoom_scale").as_f64().unwrap() > 1.4);
    let before_frame = app.frame().data().to_vec();
    pinch(&mut app, &clock);
    assert!(before_frame != app.frame().data());
    assert!(matches!(app.top_route(), Some(kaia_app::Route::Reader { .. })));
}

fn saved_state(app: &mut App<HeadlessBackend>, name: &str) -> Settings {
    app.broadcast_app(Event::new("SaveState"));
    Settings::open(&state_path(app, name))
}

fn single_drag(app: &mut App<HeadlessBackend>, clock: &ManualClock, from: Point, to: Point, slow: bool) {
    for (phase, pos, ms) in [
        (Phase::Down, from, 10),
        (Phase::Move, to, 100),
        (Phase::Up, to, if slow { 1000 } else { 20 }),
    ] {
        clock.advance(Duration::from_millis(ms));
        app.process(vec![InputEvent::Touch(TouchEvent {
            slot: 0,
            phase,
            pos,
            time_us: clock.now().as_micros() as u64,
        })])
        .unwrap();
    }
    settle(app, clock);
}

#[test]
fn sample_pdf_pinch_pan_reset_and_reopen_with_lookup_enabled() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = reader("zoom-sample");
    let id = book_id(&app, "sample.pdf");
    app.command(&format!("open_book:{id}"), &[]);
    settle(&mut app, &clock);
    app.command("reader:slider", &[Value::Number(2.0)]);
    settle(&mut app, &clock);
    app.services_mut()
        .settings
        .save("reading.double_tap_lookup", Value::Bool(true));
    app.command("reader:tap", &[Value::Number(1100.0), Value::Number(800.0)]);
    let before = app.frame().data().to_vec();
    two_finger(
        &mut app,
        &clock,
        [Point::new(400, 600), Point::new(800, 1000)],
        [Point::new(200, 400), Point::new(1000, 1200)],
    );
    let state = saved_state(&mut app, "sample.pdf");
    assert!((state.read("zoom_scale").as_f64().unwrap() - 2.0).abs() < 0.001);
    assert_eq!(state.read("last_page").as_i64(), Some(2));
    assert!(
        before != app.frame().data(),
        "zoom must change the visible region"
    );
    two_finger_timed(
        &mut app,
        &clock,
        [Point::new(200, 400), Point::new(1000, 1200)],
        [Point::new(400, 600), Point::new(800, 1000)],
        100,
    );
    let smaller = saved_state(&mut app, "sample.pdf");
    assert!((smaller.read("zoom_scale").as_f64().unwrap() - 1.0).abs() < 0.001);
    two_finger(
        &mut app,
        &clock,
        [Point::new(400, 600), Point::new(800, 1000)],
        [Point::new(200, 400), Point::new(1000, 1200)],
    );
    let state = saved_state(&mut app, "sample.pdf");
    let pan = state.read("zoom_pan_x").as_i64().unwrap();
    single_drag(&mut app, &clock, Point::new(800, 900), Point::new(600, 800), true);
    let state = saved_state(&mut app, "sample.pdf");
    assert_eq!(state.read("zoom_pan_x").as_i64(), Some(pan + 200));
    app.command("nav:home", &[]);
    app.command(&format!("open_book:{id}"), &[]);
    settle(&mut app, &clock);
    let reopened = saved_state(&mut app, "sample.pdf");
    for key in ["zoom_scale", "zoom_pan_x", "zoom_pan_y"] {
        assert_eq!(reopened.read(key), state.read(key));
    }
    app.command("reader:zoom:reset", &[]);
    settle(&mut app, &clock);
    let reset = saved_state(&mut app, "sample.pdf");
    assert_eq!(reset.read("zoom").get("mode").as_str(), Some("fit_page"));
    assert_eq!(reset.read("zoom_pan_x").as_i64(), Some(0));
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn zoom_sheet_area_drag_and_page_keys() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = reader("zoom-area");
    let id = book_id(&app, "2col.pdf");
    app.command(&format!("open_book:{id}"), &[]);
    settle(&mut app, &clock);
    app.command("reader:overlay", &[]);
    app.command("reader:panel:layout", &[]);
    assert_eq!(app.top_route(), Some(&kaia_app::Route::ReaderPanel));
    app.command("reader:zoom:area", &[]);
    assert!(matches!(app.top_route(), Some(kaia_app::Route::Reader { .. })));
    single_drag(
        &mut app,
        &clock,
        Point::new(200, 300),
        Point::new(650, 900),
        false,
    );
    let state = saved_state(&mut app, "2col.pdf");
    assert_eq!(state.read("zoom").get("mode").as_str(), Some("region"));
    assert!(state.read("zoom_scale").as_f64().unwrap() > 1.9);
    let x = state.read("zoom_pan_x").as_i64().unwrap();
    app.key(kaia_input::Key::RPgFwd).unwrap();
    settle(&mut app, &clock);
    let next = saved_state(&mut app, "2col.pdf");
    assert_eq!(next.read("last_page").as_i64(), Some(1));
    assert!(next.read("zoom_pan_x").as_i64().unwrap() > x);
    app.command("reader:tap", &[Value::Number(1100.0), Value::Number(800.0)]);
    settle(&mut app, &clock);
    let mut down = saved_state(&mut app, "2col.pdf");
    for _ in 0..3 {
        if down.read("zoom_pan_x").as_i64() == Some(0) {
            break;
        }
        app.command("reader:tap", &[Value::Number(1100.0), Value::Number(800.0)]);
        settle(&mut app, &clock);
        down = saved_state(&mut app, "2col.pdf");
    }
    assert_eq!(down.read("last_page").as_i64(), Some(1));
    assert_eq!(down.read("zoom_pan_x").as_i64(), Some(0));
    assert!(down.read("zoom_pan_y").as_i64() > next.read("zoom_pan_y").as_i64());
}
