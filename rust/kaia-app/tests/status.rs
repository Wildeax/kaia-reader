//! Status actions, battery data, dismissal and return paths.

use kaia_app::{App, HeadlessBackend, Route};
use kaia_gfx::{golden::assert_matches_golden, png::write_pixmap, Point};
use kaia_input::Key;
use kaia_platform::battery::Battery;

mod common;
use common::*;

struct Charge(u8);
impl Battery for Charge {
    fn capacity(&self) -> Option<u8> {
        Some(self.0)
    }
    fn is_charging(&self) -> Option<bool> {
        Some(false)
    }
    fn is_charged(&self) -> Option<bool> {
        Some(false)
    }
}

fn app(name: &str, charge: Option<u8>) -> App<HeadlessBackend> {
    let mut app = make_app(&scratch(name), Vec::new());
    if let Some(charge) = charge {
        app.services_mut().platform.battery = Box::new(Charge(charge));
    }
    app.start().unwrap();
    freeze_status_clock(&mut app);
    app
}

fn golden_frame(app: &App<HeadlessBackend>, name: &str) {
    if !golden(name).exists() {
        write_pixmap(app.frame(), &golden(&name.replace(".png", ".actual.png"))).unwrap();
    }
    assert_matches_golden(app.frame(), &golden(name), 14);
}

#[test]
fn status_actions_keep_their_caller() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = app("status-actions", None);
    for root in ["nav-home", "nav-library", "nav-more"] {
        tap_key(&mut app, root);
        let caller = app.top_route().cloned();
        for (key, popup, settings, section) in [
            ("status-light", "light-popup", "light-settings", "display"),
            ("status-wifi", "wifi-popup", "wifi-settings", "network"),
        ] {
            tap_key(&mut app, key);
            assert!(app.find_rect(popup).is_some());
            assert_eq!(
                app.top_route(),
                if key == "status-wifi" {
                    Some(&Route::Wifi)
                } else {
                    caller.as_ref()
                }
            );
            app.key(Key::Back).unwrap();
            assert!(app.find_rect(popup).is_none());
            tap_key(&mut app, key);
            tap_key(&mut app, settings);
            assert_eq!(
                app.top_route(),
                Some(&Route::SettingsSection { name: section.into() })
            );
            app.key(Key::Back).unwrap();
            assert_eq!(app.top_route(), caller.as_ref());
        }
        tap_key(&mut app, "status-search");
        assert!(app.find_rect("search-field").is_some());
        app.key(Key::Back).unwrap();
        assert_eq!(app.top_route(), caller.as_ref());
    }
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn battery_popup_dismisses_and_power_returns_to_home() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = app("status-battery-routes", Some(73));
    let battery = centre_of(&app, "status-battery");
    tap_key(&mut app, "status-battery");
    assert!(app.find_rect("battery-popup").is_some());
    app.tap(battery).unwrap();
    assert!(app.find_rect("battery-popup").is_none());
    tap_key(&mut app, "status-battery");
    app.tap(Point::new(50, 900)).unwrap();
    assert!(app.find_rect("battery-popup").is_none());
    tap_key(&mut app, "status-battery");
    tap_key(&mut app, "battery-settings");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "device".into()
        })
    );
    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Home));
    assert!(app.find_rect("battery-popup").is_none());
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn status_controls_have_separate_touch_targets() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = app("status-targets", None);
    let keys = ["status-light", "status-wifi", "status-battery", "status-search"];
    let targets: Vec<_> = keys.iter().map(|key| app.find_rect(key).unwrap()).collect();
    for rect in &targets {
        assert!(rect.w >= app.services().theme.touch_target);
        assert!(rect.h >= app.services().theme.touch_target);
        assert!(rect.x >= 0 && rect.right() <= app.screen().w as i32);
    }
    for pair in targets.windows(2) {
        assert!(pair[0].right() <= pair[1].x);
    }
}

#[test]
fn battery_charge_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = app("status-battery-golden", Some(73));
    tap_key(&mut app, "status-battery");
    golden_frame(&app, "status_battery.png");
}

#[test]
fn battery_unavailable_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = app("status-battery-unavailable", None);
    tap_key(&mut app, "status-battery");
    golden_frame(&app, "status_battery_unavailable.png");
}

#[test]
fn reopening_battery_reads_the_current_charge() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = app("status-charge-update", Some(73));
    for level in [73, 26] {
        app.services_mut().platform.battery = Box::new(Charge(level));
        tap_key(&mut app, "status-battery");
        let mut text = String::new();
        assert!(
            app.update_top::<kaia_ui::components::Label, _>("battery-charge-text", |label, _| {
                text = label.text().to_owned();
            })
        );
        assert_eq!(text, format!("Battery: {level}%"));
        app.key(Key::Back).unwrap();
    }
}
