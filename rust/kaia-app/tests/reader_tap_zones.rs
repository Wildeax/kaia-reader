//! Tap zones and turn gestures in the reader.
//!
//! `reading.tap_zones` picks one of four layouts. The old values `left_handed` and
//! `page_only` map to the layout with the same page turn zones. `reading.turn_gesture`
//! decides whether taps, swipes or both turn pages. Taps that open the menu work in
//! every mode. The checks send touches through the app loop and read the page
//! position and the top route, so they test what the reader does with each touch.

use std::time::Duration;

use kaia_app::surfaces::reader::page::{PageView, PAGE_KEY};
use kaia_app::surfaces::settings::keys::{READING_TAP_ZONES, READING_TURN_GESTURE};
use kaia_app::{App, AppOptions, HeadlessBackend, Route, ScriptInput, Services};
use kaia_display::InputEvent;
use kaia_events::{Clock, ManualClock, Value};
use kaia_gfx::Point;
use kaia_input::{Key, Phase, TouchEvent};
use kaia_platform::{Env, Platform};

mod common;

use common::*;

/// Where a tap goes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Expect {
    Back,
    Menu,
    Forward,
}

use Expect::{Back, Forward, Menu};

/// Sample points as fractions of the screen width and height, with the zone each
/// one must give. The points keep clear of the top right corner, which bookmarks.
const DEFAULT_SAMPLES: &[(f64, f64, Expect)] = &[
    (1.0 / 6.0, 0.5, Back),
    (1.0 / 6.0, 0.9, Back),
    (0.5, 0.5, Menu),
    (0.5, 0.1, Menu),
    (0.5, 0.9, Menu),
    (5.0 / 6.0, 0.5, Forward),
    (5.0 / 6.0, 0.9, Forward),
];

const FORWARD_RIGHT_SAMPLES: &[(f64, f64, Expect)] = &[
    (1.0 / 6.0, 0.5, Back),
    (1.0 / 6.0, 0.9, Back),
    (0.5, 0.5, Forward),
    (0.5, 0.7, Forward),
    (0.5, 0.9, Menu),
    (0.38, 0.9, Menu),
    (5.0 / 6.0, 0.5, Forward),
    (5.0 / 6.0, 0.9, Forward),
];

const FORWARD_LEFT_SAMPLES: &[(f64, f64, Expect)] = &[
    (5.0 / 6.0, 0.5, Back),
    (5.0 / 6.0, 0.9, Back),
    (1.0 / 6.0, 0.5, Forward),
    (1.0 / 6.0, 0.9, Forward),
    (0.5, 0.5, Forward),
    (0.5, 0.7, Forward),
    (0.5, 0.9, Menu),
    (0.62, 0.9, Menu),
];

const VERTICAL_SAMPLES: &[(f64, f64, Expect)] = &[
    (0.5, 1.0 / 6.0, Back),
    (1.0 / 6.0, 0.2, Back),
    (0.5, 0.5, Menu),
    (1.0 / 6.0, 0.5, Menu),
    (5.0 / 6.0, 0.5, Menu),
    (0.5, 5.0 / 6.0, Forward),
    (5.0 / 6.0, 0.8, Forward),
];

/// Opens juliet.epub with `settings` saved first, then turns to a page of text. Page 0
/// is the cover, so a back zone has a page to go back to.
fn open(name: &str, settings: &[(&str, &str)]) -> (App<HeadlessBackend>, ManualClock) {
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
    for (key, value) in settings {
        app.services_mut().settings.save(*key, Value::from(*value));
    }
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    idle(&mut app);
    for _ in 0..8 {
        app.key(Key::RPgFwd).unwrap();
        idle(&mut app);
    }
    (app, clock)
}

/// Runs the loop until the scan and the reader's work are done. The clock does not move.
fn idle(app: &mut App<HeadlessBackend>) {
    for _ in 0..5_000 {
        app.process(Vec::new()).unwrap();
        if !app.reader_busy() && !app.scan_running() {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("timed out waiting for the reader");
}

/// The time on the clock in microseconds, the unit of the touch samples.
fn now_us(clock: &ManualClock) -> u64 {
    clock.now().as_micros() as u64
}

/// A point at fractions `fx` and `fy` of the screen.
fn at(app: &App<HeadlessBackend>, fx: f64, fy: f64) -> Point {
    Point::new(
        (f64::from(app.screen().w) * fx) as i32,
        (f64::from(app.screen().h) * fy) as i32,
    )
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

/// A touch down and up at `p`, 80 ms long. The loop runs once it lifts.
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

/// A quick flick from `from` to `to`: ten moves of 20 ms, well inside the swipe window.
fn swipe(app: &mut App<HeadlessBackend>, clock: &ManualClock, from: Point, to: Point) {
    const STEPS: i32 = 10;
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Down,
        pos: from,
        time_us: now_us(clock),
    })])
    .unwrap();
    for i in 1..=STEPS {
        clock.advance(Duration::from_millis(20));
        let pos = Point::new(
            from.x + (to.x - from.x) * i / STEPS,
            from.y + (to.y - from.y) * i / STEPS,
        );
        app.process(vec![InputEvent::Touch(TouchEvent {
            slot: 0,
            phase: Phase::Move,
            pos,
            time_us: now_us(clock),
        })])
        .unwrap();
    }
    clock.advance(Duration::from_millis(20));
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: to,
        time_us: now_us(clock),
    })])
    .unwrap();
    idle(app);
}

/// Taps each sample and checks the page turn or the overlay it gives. The overlay is
/// closed with Back before the next sample.
fn check_samples(app: &mut App<HeadlessBackend>, clock: &ManualClock, samples: &[(f64, f64, Expect)]) {
    for &(fx, fy, expect) in samples {
        let p = at(app, fx, fy);
        let before = page(app);
        tap(app, clock, p);
        let label = format!("tap at ({fx:.2}, {fy:.2}) expected {expect:?}");
        match expect {
            Back => {
                assert!(before > 0, "{label}: a back zone needs a previous page");
                assert_eq!(page(app), before - 1, "{label}: the page goes back");
            }
            Forward => assert_eq!(page(app), before + 1, "{label}: the page goes forward"),
            Menu => {
                assert_eq!(app.top_route(), Some(&Route::Overlay), "{label}: the menu opens");
                app.key(Key::Back).unwrap();
                idle(app);
                assert!(
                    matches!(app.top_route(), Some(Route::Reader { .. })),
                    "{label}: Back closes the menu"
                );
                assert_eq!(page(app), before, "{label}: the page stays");
            }
        }
        if expect != Menu {
            assert!(
                matches!(app.top_route(), Some(Route::Reader { .. })),
                "{label}: no menu opens"
            );
        }
    }
}

/// The number of dark pixels in the `size` square at the top right corner, below the header.
fn dark_in_corner(app: &mut App<HeadlessBackend>, size: i32) -> usize {
    let mut top = 0;
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| top = view.header_height as i32);
    let w = app.screen().w as i32;
    let frame = app.frame();
    let mut dark = 0;
    for y in top..top + size {
        for x in w - size..w {
            if frame.get(x, y).is_some_and(|c| c.luma() < 128) {
                dark += 1;
            }
        }
    }
    dark
}

#[test]
fn default_layout_sends_the_left_third_back_and_the_right_third_forward() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open("tap-zones-default", &[(READING_TAP_ZONES, "default")]);
    check_samples(&mut app, &clock, DEFAULT_SAMPLES);
}

#[test]
fn unset_zones_are_the_default_layout() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open("tap-zones-unset", &[]);
    check_samples(&mut app, &clock, DEFAULT_SAMPLES);
}

#[test]
fn forward_right_has_a_back_strip_a_bottom_menu_box_and_forward_elsewhere() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open("tap-zones-forward-right", &[(READING_TAP_ZONES, "forward_right")]);
    check_samples(&mut app, &clock, FORWARD_RIGHT_SAMPLES);
}

#[test]
fn forward_left_mirrors_forward_right() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open("tap-zones-forward-left", &[(READING_TAP_ZONES, "forward_left")]);
    check_samples(&mut app, &clock, FORWARD_LEFT_SAMPLES);
}

#[test]
fn vertical_sends_the_top_third_back_and_the_bottom_third_forward() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open("tap-zones-vertical", &[(READING_TAP_ZONES, "vertical")]);
    check_samples(&mut app, &clock, VERTICAL_SAMPLES);
}

#[test]
fn legacy_left_handed_maps_to_forward_left() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open("tap-zones-left-handed", &[(READING_TAP_ZONES, "left_handed")]);
    check_samples(&mut app, &clock, FORWARD_LEFT_SAMPLES);
}

#[test]
fn legacy_page_only_maps_to_forward_right() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open("tap-zones-page-only", &[(READING_TAP_ZONES, "page_only")]);
    check_samples(&mut app, &clock, FORWARD_RIGHT_SAMPLES);
}

#[test]
fn an_unknown_zone_value_is_the_default_layout() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open("tap-zones-unknown", &[(READING_TAP_ZONES, "sideways")]);
    check_samples(&mut app, &clock, DEFAULT_SAMPLES);
}

#[test]
fn the_corner_bookmark_works_in_every_layout() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for zones in ["default", "forward_right", "forward_left", "vertical"] {
        let (mut app, clock) = open(
            &format!("tap-zones-corner-{zones}"),
            &[(READING_TAP_ZONES, zones)],
        );
        let before = dark_in_corner(&mut app, 20);
        let page_before = page(&mut app);
        let w = app.screen().w as i32;
        tap(&mut app, &clock, Point::new(w - 10, 10));
        assert!(
            dark_in_corner(&mut app, 20) > before + 100,
            "{zones}: the corner tap draws the dog-ear"
        );
        assert_eq!(
            page(&mut app),
            page_before,
            "{zones}: the corner does not turn the page"
        );
        assert!(
            matches!(app.top_route(), Some(Route::Reader { .. })),
            "{zones}: the corner opens no menu"
        );
        tap(&mut app, &clock, Point::new(w - 10, 10));
        assert_eq!(
            dark_in_corner(&mut app, 20),
            before,
            "{zones}: a second tap removes it"
        );
    }
}

#[test]
fn tap_mode_turns_pages_on_taps_and_ignores_swipes() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open(
        "tap-zones-gesture-tap",
        &[(READING_TAP_ZONES, "default"), (READING_TURN_GESTURE, "tap")],
    );
    let start = page(&mut app);
    let (left, right) = (at(&app, 1.0 / 6.0, 0.5), at(&app, 5.0 / 6.0, 0.5));

    swipe(&mut app, &clock, right, left);
    assert_eq!(
        page(&mut app),
        start,
        "a swipe to the left does not turn the page"
    );
    swipe(&mut app, &clock, left, right);
    assert_eq!(
        page(&mut app),
        start,
        "a swipe to the right does not turn the page"
    );

    tap(&mut app, &clock, right);
    assert_eq!(page(&mut app), start + 1, "a tap on the right turns forward");
    tap(&mut app, &clock, left);
    assert_eq!(page(&mut app), start, "a tap on the left turns back");

    let centre = at(&app, 0.5, 0.5);
    tap(&mut app, &clock, centre);
    assert_eq!(
        app.top_route(),
        Some(&Route::Overlay),
        "the centre tap still opens the menu"
    );
}

#[test]
fn swipe_mode_turns_pages_on_swipes_and_ignores_taps_that_turn() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open(
        "tap-zones-gesture-swipe",
        &[(READING_TAP_ZONES, "default"), (READING_TURN_GESTURE, "swipe")],
    );
    let start = page(&mut app);
    let (left, right) = (at(&app, 1.0 / 6.0, 0.5), at(&app, 5.0 / 6.0, 0.5));

    tap(&mut app, &clock, right);
    assert_eq!(page(&mut app), start, "a tap on the right does not turn the page");
    tap(&mut app, &clock, left);
    assert_eq!(page(&mut app), start, "a tap on the left does not turn the page");

    let centre = at(&app, 0.5, 0.5);
    tap(&mut app, &clock, centre);
    assert_eq!(
        app.top_route(),
        Some(&Route::Overlay),
        "the centre tap still opens the menu"
    );
    app.key(Key::Back).unwrap();
    idle(&mut app);

    swipe(&mut app, &clock, right, left);
    assert_eq!(page(&mut app), start + 1, "a swipe to the left turns forward");
    swipe(&mut app, &clock, left, right);
    assert_eq!(page(&mut app), start, "a swipe to the right turns back");
}

#[test]
fn tap_swipe_mode_turns_pages_on_both() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, clock) = open(
        "tap-zones-gesture-both",
        &[
            (READING_TAP_ZONES, "default"),
            (READING_TURN_GESTURE, "tap_swipe"),
        ],
    );
    let start = page(&mut app);
    let (left, right) = (at(&app, 1.0 / 6.0, 0.5), at(&app, 5.0 / 6.0, 0.5));

    tap(&mut app, &clock, right);
    assert_eq!(page(&mut app), start + 1, "a tap turns forward");
    swipe(&mut app, &clock, right, left);
    assert_eq!(page(&mut app), start + 2, "a swipe turns forward");
    swipe(&mut app, &clock, left, right);
    assert_eq!(page(&mut app), start + 1, "a swipe turns back");
    tap(&mut app, &clock, left);
    assert_eq!(page(&mut app), start, "a tap turns back");
}

#[test]
fn the_turn_gesture_does_not_block_the_menu_in_any_layout() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    for zones in ["default", "forward_right", "forward_left", "vertical"] {
        let (mut app, clock) = open(
            &format!("tap-zones-menu-swipe-{zones}"),
            &[(READING_TAP_ZONES, zones), (READING_TURN_GESTURE, "swipe")],
        );
        // The bottom box of the forward layouts, the centre of the others.
        let menu = if zones == "forward_right" || zones == "forward_left" {
            at(&app, 0.5, 0.9)
        } else {
            at(&app, 0.5, 0.5)
        };
        tap(&mut app, &clock, menu);
        assert_eq!(
            app.top_route(),
            Some(&Route::Overlay),
            "{zones}: the menu zone opens the menu in swipe mode"
        );
    }
}
