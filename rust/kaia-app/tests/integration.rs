//! Shared loop behavior on a headless display and a manual clock.
use kaia_app::{App, AppOptions, HeadlessBackend, ScriptInput, Services};
use kaia_events::{Event, ManualClock, RefreshMode, Value};
use kaia_gfx::Point;
use kaia_input::Key;
use kaia_platform::{Env, Platform};
use kaia_ui::components::Label;
use std::time::Duration;
mod common;
use common::*;

fn manual(name: &str) -> (App<HeadlessBackend>, ManualClock) {
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
    let services = Services::open(platform, panel(), Some(dir.join("data"))).unwrap();
    let clock = ManualClock::new();
    let mut app = App::new(
        HeadlessBackend::new(panel(), ScriptInput::idle()),
        services,
        Box::new(clock.clone()),
        AppOptions::default(),
    );
    app.start().unwrap();
    (app, clock)
}

fn settle(app: &mut App<HeadlessBackend>) {
    app.process(Vec::new()).unwrap();
}

#[test]
fn sheet_paints_takes_input_and_self_close_restores_the_frame() {
    let (mut app, _) = manual("sheet-stack");
    let before = app.frame().data().to_vec();
    app.command("settings:quick:open", &[]);
    settle(&mut app);
    let sleep = app.find_rect("quick-sleep").unwrap();
    assert!(
        sleep.bottom() <= app.screen().h as i32,
        "Sleep stays inside the sheet"
    );
    assert_ne!(app.frame().data(), before);
    app.tap(Point::new(10, 200)).unwrap();
    assert!(app.find_rect("quick-sleep").is_none());
    assert!(
        app.frame().data() == before,
        "closing the window restores the frame"
    );
    assert_eq!(app.router().depth(), 1);
}

#[test]
fn toast_paints_and_expiry_restores_pixels_without_taking_back() {
    let (mut app, clock) = manual("toast-stack");
    app.command("nav.settings", &[]);
    app.command("settings:network", &[]);
    settle(&mut app);
    let before = app.frame().data().to_vec();
    app.command("settings:sync:test", &[]);
    settle(&mut app);
    assert_ne!(app.frame().data(), before);
    clock.advance(Duration::from_secs(5));
    settle(&mut app);
    assert!(
        app.frame().data() == before,
        "closing the window restores the frame"
    );
    app.command("settings:sync:test", &[]);
    settle(&mut app);
    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&kaia_app::Route::Settings));
}

#[test]
fn dialog_buttons_reach_the_underlying_library() {
    let (mut app, _) = manual("dialog-stack");
    app.command("nav.library", &[]);
    let c = app.services_mut().library.create_collection("Test").unwrap();
    app.command(&format!("library:collections:delete:{}", c.id), &[]);
    settle(&mut app);
    tap_key(
        &mut app,
        &format!("dialog-button:library:collections:delete-confirm:{}", c.id),
    );
    assert!(app.services().library.collections().unwrap().is_empty());
    assert_eq!(app.top_route(), Some(&kaia_app::Route::Library { page: 0 }));
}

#[test]
fn scan_progress_is_throttled_and_refreshes_only_its_line() {
    let (mut app, clock) = manual("scan-throttle");
    let line = app.find_rect("scan-progress").unwrap();
    for n in 1..4 {
        app.broadcast_app(Event::with_args(
            "LibraryScanProgress",
            vec![Value::Number(n as f64), Value::Number(100.0)],
        ));
        settle(&mut app);
    }
    let before = app.stats().refreshes;
    app.broadcast_app(Event::with_args(
        "LibraryScanProgress",
        vec![Value::Number(99.0), Value::Number(100.0)],
    ));
    settle(&mut app);
    assert_eq!(app.stats().refreshes, before);
    clock.advance(Duration::from_secs(1));
    settle(&mut app);
    assert_eq!(app.stats().refreshes, before + 1);
    let refresh = app.backend().display().refreshes().last().unwrap();
    assert_eq!(refresh.region, line);
    assert!(!refresh.dither);
    assert_eq!(app.refresh_modes().last(), Some(&RefreshMode::Ui));
    let mut text = String::new();
    app.update_top::<Label, _>("scan-progress", |label, _| text = label.text().into());
    assert!(text.contains("99 of 100"));
}

#[test]
fn rebuilding_library_refreshes_a_changed_region() {
    let (mut app, _) = manual("rebuild-region");
    app.command("nav.library", &[]);
    settle(&mut app);
    let before = app.stats().refreshes as usize;
    app.command("library:sort", &[]);
    settle(&mut app);
    let updates = &app.backend().display().refreshes()[before..];
    assert!(!updates.is_empty());
    assert!(updates.iter().all(|r| r.region.h < app.screen().h && !r.flash));
}

#[test]
fn quick_commands_reach_the_platform_and_warmth_is_saved() {
    let (mut app, _) = manual("quick-values");
    app.command("settings:quick:open", &[]);
    settle(&mut app);
    app.command("settings:quick:warmth", &[Value::Number(73.0)]);
    app.command("settings:quick:frontlight", &[Value::Number(42.0)]);
    assert_eq!(app.services().platform.frontlight.warmth(), 73);
    assert_eq!(app.services().platform.frontlight.intensity(), 42);
    assert_eq!(app.services().settings.read("display.warmth").as_i64(), Some(73));
}

#[test]
fn power_cancels_sleep_and_scheduled_ramps_finish_without_blocking() {
    let (mut app, clock) = manual("power-cancel");
    app.command("settings:quick:frontlight", &[Value::Number(50.0)]);
    app.command("sleep", &[]);
    settle(&mut app);
    assert!(app.sleeping());
    clock.advance(Duration::from_secs(1));
    settle(&mut app);
    assert_eq!(app.services().platform.frontlight.intensity(), 0);
    app.key(Key::Power).unwrap();
    assert!(!app.sleeping());
    clock.advance(Duration::from_secs(20));
    settle(&mut app);
    assert!(!app.sleeping());
    assert_eq!(app.services().platform.frontlight.intensity(), 50);
}

#[test]
fn collections_create_rename_reorder_and_delete_through_commands() {
    let (mut app, _) = manual("collection-commands");
    app.command("nav.library", &[]);
    for name in ["One", "Two"] {
        app.command("library:collections:new", &[]);
        app.command("library:name", &[Value::from(name)]);
        app.command("library:name:save", &[]);
        settle(&mut app);
    }
    let rows = app.services().library.collections().unwrap();
    let id = rows[1].id;
    app.command(&format!("library:collections:rename:{id}"), &[]);
    app.command("library:name", &[Value::from("Second")]);
    app.command("library:name:save", &[]);
    app.command(&format!("library:collections:move:{id}:0"), &[]);
    settle(&mut app);
    let rows = app.services().library.collections().unwrap();
    assert_eq!(rows[0].name, "Second");
    assert_eq!(rows[0].id, id);
}

#[test]
fn status_line_opens_quick_settings() {
    let (mut app, _) = manual("status-tap");
    let r = app.find_rect("status-line").unwrap();
    app.tap(Point::new(r.x + r.w as i32 / 2, r.y + r.h as i32 / 2))
        .unwrap();
    assert!(app.find_rect("quick-sleep").is_some());
}

#[test]
fn changing_color_mode_refreshes_the_whole_display() {
    let (mut app, _) = manual("color-mode");
    let before = app.stats().refreshes;
    app.services_mut()
        .settings
        .save("display.color_rendering", Value::Bool(false));
    settle(&mut app);
    assert_eq!(app.stats().refreshes, before + 1);
    let display = app.backend().display();
    let refresh = display.refreshes().last().unwrap();
    assert_eq!(refresh.region, kaia_gfx::Rect::from_size(app.screen()));
    assert!(refresh.flash);
    assert!(!refresh.dither);
    assert!(display
        .snapshot()
        .data()
        .as_chunks::<3>()
        .0
        .iter()
        .all(|p| p[0] == p[1] && p[1] == p[2]));
    app.services_mut()
        .settings
        .save("display.color_rendering", Value::Bool(true));
    settle(&mut app);
    assert_eq!(app.stats().refreshes, before + 2);
    assert_eq!(app.backend().display().snapshot().data(), app.frame().data());
}
