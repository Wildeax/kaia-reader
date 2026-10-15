mod common;
use common::*;
use kaia_app::{App, AppOptions, HeadlessBackend, ScriptInput, Services};
use kaia_events::{ManualClock, Value};
use kaia_platform::{Env, Platform};
use std::time::Duration;

#[test]
fn inactivity_sleep_resets_on_input() {
    let dir = scratch("inactivity");
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
    services.settings.save("device.sleep_minutes", Value::Number(1.0));
    let clock = ManualClock::new();
    let mut app = App::new(
        HeadlessBackend::new(panel(), ScriptInput::idle()),
        services,
        Box::new(clock.clone()),
        AppOptions::default(),
    );
    app.start().unwrap();
    clock.advance(Duration::from_secs(50));
    app.key(kaia_input::Key::Back).unwrap();
    clock.advance(Duration::from_secs(50));
    app.process(vec![]).unwrap();
    assert!(!app.sleeping());
    clock.advance(Duration::from_secs(11));
    app.process(vec![]).unwrap();
    assert!(app.sleeping());
    app.key(kaia_input::Key::Power).unwrap();
    app.process(vec![]).unwrap();
    assert!(!app.sleeping());
}

#[test]
fn reader_sheet_refresh_stays_inside_sheet() {
    let dir = scratch("sheet-damage");
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    app.process(vec![]).unwrap();
    wait_reader(&mut app);
    let before = app.backend().display().refreshes().len();
    app.command("reader:panel:footer", &[]);
    app.process(vec![]).unwrap();
    app.command("back", &[]);
    app.process(vec![]).unwrap();
    let updates = &app.backend().display().refreshes()[before..];
    assert!(!updates.is_empty());
    assert!(updates.iter().all(|r| r.region.h < app.screen().h));
}

#[test]
fn slider_drag_is_fast_and_release_is_ui() {
    use kaia_app::slider::Slider;
    use kaia_gfx::{Point, Size};
    use kaia_input::{Direction, Gesture};
    use kaia_ui::{Harness, UiEvent};
    let dir = scratch("slider-drag");
    let app = make_app(&dir, vec![]);
    let mut h = Harness::new(
        Slider::new(0.0, 0.0, 100.0, 1.0).on_action("value"),
        Size::new(800, 100),
        &app.services().theme,
        &app.services().assets,
    );
    h.frame();
    let w = h.window().clone();
    w.dispatch(
        UiEvent::Gesture(Gesture::Pan {
            pos: Point::new(400, 50),
            direction: Some(Direction::East),
            distance: 300.0,
            start_pos: Point::new(100, 50),
            time_us: 1000,
        }),
        h.ctx(),
    );
    h.frame();
    assert_eq!(
        h.refresh_requests().iter().map(|r| r.mode).collect::<Vec<_>>(),
        vec![kaia_events::RefreshMode::Fast]
    );
    w.dispatch(
        UiEvent::Gesture(Gesture::PanRelease {
            pos: Point::new(400, 50),
            time_us: 2000,
        }),
        h.ctx(),
    );
    h.frame();
    assert_eq!(
        h.refresh_requests().iter().map(|r| r.mode).collect::<Vec<_>>(),
        vec![kaia_events::RefreshMode::Ui]
    );
    assert!(w
        .drain_outbox()
        .iter()
        .any(|out| matches!(out, kaia_ui::Out::Action(a) if a.id == "value:release")));
}

#[test]
fn opening_returns_before_the_page_is_ready_and_power_still_works() {
    let dir = scratch("open-responsive");
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let id = book_id(&app, "juliet.epub");
    let start = std::time::Instant::now();
    app.command(&format!("open_book:{id}"), &[]);
    let elapsed = start.elapsed();
    assert!(app.reader_busy());
    assert!(elapsed < Duration::from_millis(100), "open took {elapsed:?}");
    app.key(kaia_input::Key::Power).unwrap();
    assert!(app.sleeping());
    app.key(kaia_input::Key::Power).unwrap();
    assert!(!app.sleeping());
    wait_reader(&mut app);
    let before = app.frame().clone();
    app.command("reader:next", &[]);
    assert!(app.reader_busy());
    assert_eq!(before.data(), app.frame().data(), "the old page stays on screen");
    wait_reader(&mut app);
    assert_ne!(before.data(), app.frame().data());
}

#[test]
fn critical_battery_sleeps_but_charging_does_not() {
    struct Battery(bool);
    impl kaia_platform::battery::Battery for Battery {
        fn capacity(&self) -> Option<u8> {
            Some(3)
        }
        fn is_charging(&self) -> Option<bool> {
            Some(self.0)
        }
        fn is_charged(&self) -> Option<bool> {
            Some(false)
        }
    }
    for charging in [false, true] {
        let dir = scratch(if charging {
            "battery-charging"
        } else {
            "battery-critical"
        });
        let mut app = make_app(&dir, vec![]);
        app.services_mut().platform.battery = Box::new(Battery(charging));
        app.start().unwrap();
        app.process(vec![]).unwrap();
        assert_eq!(app.sleeping(), !charging);
    }
}

#[test]
fn reader_slider_preview_and_release_use_the_requested_modes() {
    let dir = scratch("reader-slider-refresh");
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    app.command("reader:overlay", &[]);
    app.process(vec![]).unwrap();
    let before = app.refresh_modes().len();
    app.command("reader:slider", &[Value::Number(5.0), Value::Bool(true)]);
    wait_reader(&mut app);
    let modes = &app.refresh_modes()[before..];
    assert!(!modes.is_empty());
    assert!(
        modes.iter().all(|m| *m == kaia_events::RefreshMode::Fast),
        "{modes:?}"
    );
    let before = app.refresh_modes().len();
    app.command("reader:slider:release", &[]);
    wait_reader(&mut app);
    let modes = &app.refresh_modes()[before..];
    assert!(!modes.is_empty());
    assert!(
        modes.iter().all(|m| *m == kaia_events::RefreshMode::Ui),
        "{modes:?}"
    );
}
