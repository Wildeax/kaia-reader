//! Pinch and spread change the text size of a reflowable book, by KOReader's step rule.
//! The gestures come in as two-finger touch frames, as the device sends them.

mod common;

use common::*;
use kaia_app::surfaces::reader::page::PageView;
use kaia_app::{App, HeadlessBackend, ScriptInput, Services};
use kaia_display::InputEvent;
use kaia_events::{Clock, Event, ManualClock, Settings};
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
    const STEPS: i32 = 10;
    app.process(vec![frame(Phase::Down, from, clock)]).unwrap();
    for i in 1..=STEPS {
        clock.advance(Duration::from_millis(20));
        let at =
            |a: Point, b: Point| Point::new(a.x + (b.x - a.x) * i / STEPS, a.y + (b.y - a.y) * i / STEPS);
        app.process(vec![frame(
            Phase::Move,
            [at(from[0], to[0]), at(from[1], to[1])],
            clock,
        )])
        .unwrap();
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
fn pinch_on_a_fixed_layout_book_changes_nothing() {
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
    assert!(matches!(app.top_route(), Some(kaia_app::Route::Reader { .. })));
}
