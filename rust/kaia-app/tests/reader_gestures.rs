mod common;

use common::*;
use kaia_app::surfaces::reader::page::{PageView, PAGE_KEY};
use kaia_app::{App, AppOptions, HeadlessBackend, ScriptInput, Services};
use kaia_events::{Clock, Ctx, Event, ManualClock, Value};
use kaia_gfx::{Point, Size};
use kaia_input::{Direction, Gesture};
use kaia_platform::{Env, Platform};
use kaia_ui::components::Stack;
use kaia_ui::widget::{AppAction, Out, UiEvent};
use kaia_ui::{WidgetPod, Window};
use std::time::Duration;

fn page_window() -> (Window, Ctx) {
    let dir = scratch("gesture-widget");
    let app = make_app(&dir, vec![]);
    let size = Size::new(1000, 1000);
    let window = Window::new(
        Stack::new(vec![WidgetPod::new(PageView::new()).with_key(PAGE_KEY)]),
        app.services().theme.clone(),
        app.services().assets.clone(),
        size,
    );
    let ctx = Ctx::new(
        Box::new(ManualClock::new()),
        kaia_events::Rect::new(0, 0, size.w as i32, size.h as i32),
    );
    (window, ctx)
}

fn send(window: &Window, ctx: &mut Ctx, event: UiEvent) -> Vec<AppAction> {
    window.dispatch(event, ctx);
    window
        .drain_outbox()
        .into_iter()
        .filter_map(|out| match out {
            Out::Action(action) => Some(action),
            _ => None,
        })
        .collect()
}

fn pan(start: Point, end: Point) -> UiEvent {
    UiEvent::Gesture(Gesture::Pan {
        pos: end,
        start_pos: start,
        direction: None,
        distance: 100.0,
        time_us: 100_000,
    })
}

fn touch(window: &Window, ctx: &mut Ctx, pos: Point) {
    send(window, ctx, UiEvent::Gesture(Gesture::Touch { pos, time_us: 0 }));
}

#[test]
fn brightness_requires_a_vertical_start_inside_thirteen_percent() {
    let _guard = SERIAL.lock().unwrap();
    let (window, mut ctx) = page_window();
    for x in [0, 129] {
        let start = Point::new(x, 800);
        touch(&window, &mut ctx, start);
        let actions = send(&window, &mut ctx, pan(start, Point::new(x, 300)));
        assert_eq!(actions[0].id, "reader:brightness");
        assert_eq!(actions[0].args, vec![Value::Number(50.0), Value::Bool(false)]);
        let actions = send(
            &window,
            &mut ctx,
            UiEvent::Gesture(Gesture::PanRelease {
                pos: Point::new(x, 200),
                time_us: 200_000,
            }),
        );
        assert_eq!(actions[0].args, vec![Value::Number(60.0), Value::Bool(true)]);
    }
    let start = Point::new(130, 800);
    touch(&window, &mut ctx, start);
    assert!(send(&window, &mut ctx, pan(start, Point::new(130, 300))).is_empty());
    let start = Point::new(10, 800);
    touch(&window, &mut ctx, start);
    assert!(send(&window, &mut ctx, pan(start, Point::new(200, 790))).is_empty());
    assert!(send(&window, &mut ctx, pan(start, Point::new(200, 200))).is_empty());
    let actions = send(
        &window,
        &mut ctx,
        UiEvent::Gesture(Gesture::Swipe {
            pos: start,
            end_pos: Point::new(200, 200),
            direction: Some(Direction::East),
            distance: 600.0,
            time_us: 300_000,
        }),
    );
    assert_eq!(actions[0].id, "reader:prev");
}

#[test]
fn bottom_hold_uses_fifths_and_release_does_not_select_text() {
    let _guard = SERIAL.lock().unwrap();
    let (window, mut ctx) = page_window();
    let start = Point::new(100, 950);
    touch(&window, &mut ctx, start);
    let actions = send(
        &window,
        &mut ctx,
        UiEvent::Gesture(Gesture::Hold {
            pos: start,
            time_us: 500_000,
        }),
    );
    assert_eq!(actions[0].id, "reader:flip");
    assert_eq!(actions[0].args, vec![Value::Number(0.0)]);
    for zone in 0..5 {
        let actions = send(
            &window,
            &mut ctx,
            UiEvent::Gesture(Gesture::HoldPan {
                pos: Point::new(zone * 200 + 100, 950),
                start_pos: Some(start),
                end_pos: None,
                direction: None,
                distance: 200.0,
                span: None,
                start_span: None,
                time_us: 600_000,
            }),
        );
        assert_eq!(actions[0].args, vec![Value::Number(f64::from(zone))]);
    }
    let actions = send(
        &window,
        &mut ctx,
        UiEvent::Gesture(Gesture::HoldRelease {
            pos: start,
            time_us: 700_000,
        }),
    );
    assert_eq!(actions[0].id, "reader:flip:stop");
    touch(&window, &mut ctx, Point::new(100, 899));
    let actions = send(
        &window,
        &mut ctx,
        UiEvent::Gesture(Gesture::Hold {
            pos: Point::new(100, 899),
            time_us: 900_000,
        }),
    );
    assert_eq!(actions[0].id, "reader:hold");
}

#[test]
fn key_repeat_starts_on_fifth_repeat_and_release_stops_it() {
    let _guard = SERIAL.lock().unwrap();
    let (window, mut ctx) = page_window();
    let key = |forward, pressed| {
        UiEvent::Named(Event::with_args(
            "ReaderPageKey",
            vec![Value::Bool(forward), Value::Bool(pressed)],
        ))
    };
    for forward in [false, true] {
        let actions = send(&window, &mut ctx, key(forward, true));
        assert_eq!(
            actions.last().unwrap().id,
            if forward { "reader:next" } else { "reader:prev" }
        );
        for repeat in 1..=8 {
            let actions = send(&window, &mut ctx, key(forward, true));
            match repeat {
                1 => assert_eq!(actions[0].id, if forward { "reader:next" } else { "reader:prev" }),
                5 => {
                    assert_eq!(actions[0].id, "reader:flip");
                    assert_eq!(
                        actions[0].args,
                        vec![Value::Number(if forward { 4.0 } else { 0.0 })]
                    );
                }
                _ => assert!(actions.is_empty()),
            }
        }
        let actions = send(&window, &mut ctx, key(forward, false));
        assert_eq!(actions[0].id, "reader:flip:stop");
    }
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
        AppOptions {
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

fn level(app: &App<HeadlessBackend>) -> u8 {
    app.services().platform.frontlight.intensity()
}

#[test]
fn raw_touch_swipes_change_light_and_bottom_holds_flip() {
    let _guard = SERIAL.lock().unwrap();
    let (mut app, clock) = reader("raw-reader-gestures");
    app.command("settings:quick:frontlight", &[Value::Number(20.0)]);
    let height = app.screen().h as i32;
    let width = app.screen().w as i32;
    let sample = |app: &mut App<HeadlessBackend>, phase, point| {
        app.process(vec![kaia_display::InputEvent::Touch(kaia_input::TouchEvent {
            slot: 0,
            phase,
            pos: point,
            time_us: clock.now().as_micros() as u64,
        })])
        .unwrap();
    };
    sample(&mut app, kaia_input::Phase::Down, Point::new(20, height * 3 / 4));
    clock.advance(Duration::from_millis(20));
    sample(&mut app, kaia_input::Phase::Move, Point::new(20, height / 4));
    clock.advance(Duration::from_millis(20));
    sample(&mut app, kaia_input::Phase::Up, Point::new(20, height / 4));
    assert_eq!(level(&app), 70);
    clock.advance(Duration::from_millis(2000));
    app.process(vec![]).unwrap();
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| assert_eq!(view.brightness, None));
    let bottom = Point::new(width - 100, height - 20);
    sample(&mut app, kaia_input::Phase::Down, bottom);
    clock.advance(Duration::from_secs(1));
    app.process(vec![]).unwrap();
    assert!(app.update_top::<PageView, _>(PAGE_KEY, |view, _| assert!(view.flipping)));
    sample(&mut app, kaia_input::Phase::Up, bottom);
    settle(&mut app, &clock);
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| assert!(!view.flipping));
    assert!(matches!(app.top_route(), Some(kaia_app::Route::Reader { .. })));
    app.command("reader:overlay", &[]);
    settle(&mut app, &clock);
    let back = centre_of(&app, "overlay-home");
    app.tap(back).unwrap();
    settle(&mut app, &clock);
    assert_eq!(app.top_route(), Some(&kaia_app::Route::Home));
}

#[test]
fn brightness_throttles_live_updates_commits_release_and_expires_header() {
    let _guard = SERIAL.lock().unwrap();
    let (mut app, clock) = reader("brightness-app");
    app.command("settings:quick:frontlight", &[Value::Number(30.0)]);
    let drag = |app: &mut App<HeadlessBackend>, delta, release| {
        app.command("reader:brightness", &[Value::Number(delta), Value::Bool(release)])
    };
    let refreshes = app.backend().display().refreshes().len();
    drag(&mut app, 10.0, false);
    app.process(vec![]).unwrap();
    assert!(app.backend().display().refreshes()[refreshes..]
        .iter()
        .all(|r| r.region.h <= app.services().theme.touch_target));
    assert_eq!(level(&app), 40);
    clock.advance(Duration::from_millis(49));
    drag(&mut app, 20.0, false);
    assert_eq!(level(&app), 40);
    clock.advance(Duration::from_millis(1));
    drag(&mut app, 30.0, false);
    assert_eq!(level(&app), 60);
    drag(&mut app, 100.0, true);
    assert_eq!(level(&app), 100);
    clock.advance(Duration::from_millis(1999));
    app.process(vec![]).unwrap();
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| assert_eq!(view.brightness, Some(100)));
    clock.advance(Duration::from_millis(1));
    app.process(vec![]).unwrap();
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| assert_eq!(view.brightness, None));
    drag(&mut app, -200.0, true);
    assert_eq!(level(&app), 0);
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn flip_release_during_render_restores_quality_and_does_not_queue_turns() {
    let _guard = SERIAL.lock().unwrap();
    let (mut app, clock) = reader("flip-app");
    let mut before = None;
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| before = view.position);
    let refreshes = app.backend().display().refreshes().len();
    app.command("reader:flip", &[Value::Number(4.0)]);
    assert!(app.reader_busy());
    app.command("reader:flip:stop", &[]);
    settle(&mut app, &clock);
    let mut after = None;
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| {
        after = view.position;
        assert!(!view.flipping);
    });
    assert_eq!(after.unwrap().0, before.unwrap().0 + 1);
    let updates = &app.backend().display().refreshes()[refreshes..];
    assert!(updates
        .iter()
        .any(|r| r.waveform == kaia_display::Waveform::Gc16 && r.flash && r.dither));
    clock.advance(Duration::from_secs(10));
    app.process(vec![]).unwrap();
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| assert_eq!(view.position, after));

    let refreshes = app.backend().display().refreshes().len();
    app.command("reader:flip", &[Value::Number(4.0)]);
    settle(&mut app, &clock);
    let updates = &app.backend().display().refreshes()[refreshes..];
    assert!(updates
        .iter()
        .any(|r| r.waveform == kaia_display::Waveform::Du && !r.dither));
    assert!(!updates.iter().any(|r| r.flash));
    app.command("reader:flip", &[Value::Number(2.0)]);
    settle(&mut app, &clock);
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| after = view.position);
    clock.advance(Duration::from_secs(10));
    app.process(vec![]).unwrap();
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| assert_eq!(view.position, after));
    app.command("reader:flip:stop", &[]);
    app.process(vec![]).unwrap();
    assert_eq!(app.refresh_modes().last(), Some(&kaia_events::RefreshMode::Full));
    assert!(clock.now() > Duration::from_secs(20));
}

fn page_state(app: &mut App<HeadlessBackend>) -> (u32, bool) {
    let mut state = None;
    assert!(app.update_top::<PageView, _>(PAGE_KEY, |view, _| {
        state = Some((view.position.unwrap().0, view.flipping));
    }));
    state.unwrap()
}

// Kobo's decoder reports both value 1 and value 2 as pressed. With the Kobo
// feature, feed the raw events through that decoder as well as the app loop.
fn page_key_input(value: i32, time_us: u64) -> Vec<kaia_display::InputEvent> {
    #[cfg(feature = "kobo")]
    {
        use kaia_display::kobo::decode::{code, key, Calibration, EvdevDecoder, RawEvent};
        let mut decoder = EvdevDecoder::new(Calibration::libra_colour(
            (0, 1679),
            (0, 1263),
            kaia_display::Rotation::Deg0,
        ));
        decoder.feed(&[RawEvent {
            time_us,
            kind: code::EV_KEY,
            code: key::RPG_FWD,
            value,
        }])
    }
    #[cfg(not(feature = "kobo"))]
    vec![kaia_display::InputEvent::Key(kaia_input::KeyEvent {
        key: kaia_input::Key::RPgFwd,
        pressed: value != 0,
        time_us,
    })]
}

#[test]
fn routed_page_key_hold_turns_once_then_flips_and_release_stops_in_both_directions() {
    use kaia_app::surfaces::settings::keys;
    let _guard = SERIAL.lock().unwrap();
    let (mut app, clock) = reader("routed-page-hold");
    let initial = page_state(&mut app).0;
    for inverted in [false, true] {
        app.services_mut()
            .settings
            .save(keys::READING_INVERT_PAGE_BUTTONS, Value::Bool(inverted));
        let start = page_state(&mut app).0 as i64;
        let direction = if inverted { -1 } else { 1 };
        let key = |app: &mut App<HeadlessBackend>, value| {
            app.process(page_key_input(value, clock.now().as_micros() as u64))
                .unwrap();
        };
        // Wait for rendering without advancing the quick flip timer.
        key(&mut app, 1);
        wait_reader(&mut app);
        assert_eq!(page_state(&mut app), ((start + direction) as u32, false));
        for repeat in 1..=5 {
            key(&mut app, 2);
            wait_reader(&mut app);
            let turns = if repeat == 5 { 3 } else { 2 };
            assert_eq!(
                page_state(&mut app),
                ((start + turns * direction) as u32, repeat == 5),
                "repeat {repeat}, inverted {inverted}"
            );
        }
        // Later repeats do not add turns. The flip timer owns the cadence.
        let flipping = page_state(&mut app);
        for _ in 6..=8 {
            key(&mut app, 2);
            assert_eq!(page_state(&mut app), flipping);
        }
        clock.advance(Duration::from_millis(300));
        app.process(vec![]).unwrap();
        wait_reader(&mut app);
        assert_eq!(page_state(&mut app), ((start + 4 * direction) as u32, true));
        key(&mut app, 0);
        wait_reader(&mut app);
        let stopped = page_state(&mut app);
        assert!(!stopped.1);
        clock.advance(Duration::from_secs(2));
        app.process(vec![]).unwrap();
        wait_reader(&mut app);
        assert_eq!(page_state(&mut app), stopped);
    }
    assert_eq!(page_state(&mut app), (initial, false));
    // Release resets repeat counting, including two ordinary taps in succession.
    app.services_mut()
        .settings
        .save(keys::READING_INVERT_PAGE_BUTTONS, Value::Bool(false));
    for offset in 1..=2 {
        app.key(kaia_input::Key::RPgFwd).unwrap();
        wait_reader(&mut app);
        assert_eq!(page_state(&mut app), (initial + offset, false));
    }
}

#[test]
fn page_key_hold_over_quick_settings_keeps_only_the_existing_first_turn() {
    let _guard = SERIAL.lock().unwrap();
    let (mut app, clock) = reader("page-hold-over-sheet");
    let initial = page_state(&mut app);
    app.command("settings:quick:open", &[]);
    app.process(vec![]).unwrap();
    assert!(app.find_rect("quick-light-panel").is_some());
    for value in [1, 2, 2, 2, 2, 2, 2, 0] {
        app.process(page_key_input(value, clock.now().as_micros() as u64))
            .unwrap();
    }
    app.command("back", &[]);
    settle(&mut app, &clock);
    // Quick settings is nonmodal. Its existing PageForward fallback reaches
    // the page underneath, but repeats must not start quick flip there.
    assert_eq!(page_state(&mut app), (initial.0 + 1, false));
    app.key(kaia_input::Key::RPgFwd).unwrap();
    settle(&mut app, &clock);
    assert_eq!(page_state(&mut app), (initial.0 + 2, false));
}

#[test]
fn top_edge_brightness_swipe_finishes_before_quick_settings_but_outside_swipe_opens_it() {
    let _guard = SERIAL.lock().unwrap();
    let (mut app, clock) = reader("brightness-before-sheet");
    app.command("settings:quick:frontlight", &[Value::Number(80.0)]);
    let width = app.screen().w as i32;
    let height = app.screen().h as i32;
    let boundary = (width * 13 + 99) / 100;
    for x in [1, boundary - 1, boundary] {
        let before = level(&app);
        for (phase, y) in [
            (kaia_input::Phase::Down, 1),
            (kaia_input::Phase::Move, height / 4 + 1),
            (kaia_input::Phase::Up, height / 2 + 1),
        ] {
            clock.advance(Duration::from_millis(20));
            app.process(vec![kaia_display::InputEvent::Touch(kaia_input::TouchEvent {
                slot: 0,
                phase,
                pos: Point::new(x, y),
                time_us: clock.now().as_micros() as u64,
            })])
            .unwrap();
        }
        if x < boundary {
            assert!(app.find_rect("quick-light-panel").is_none());
            assert!(matches!(app.top_route(), Some(kaia_app::Route::Reader { .. })));
            assert_eq!(level(&app), before.saturating_sub(50));
            clock.advance(Duration::from_millis(2000));
            app.process(vec![]).unwrap();
            assert!(app.update_top::<PageView, _>(PAGE_KEY, |view, _| {
                assert_eq!(view.brightness, None);
            }));
        } else {
            assert!(app.find_rect("quick-light-panel").is_some());
            assert_eq!(level(&app), before);
        }
    }
}
