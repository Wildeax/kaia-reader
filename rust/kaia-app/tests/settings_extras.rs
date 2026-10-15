//! Settings extras: large print, the experimental features page, the finished
//! count in the statistics, and airplane mode with its fake Wi-Fi.
//!
//! Desktop has no radio, so the Wi-Fi tests use a fake [`Network`] that records
//! what Kaia asks of it. Renders of the large print screens go to scratch
//! folders, and print their paths for review.

mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use common::*;
use kaia_app::surfaces::settings::entries::Entry;
use kaia_app::surfaces::settings::{facts, keys, sections};
use kaia_app::{Route, Services};
use kaia_events::{Settings, Value};
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::png::write_pixmap;
use kaia_library::Status;
use kaia_platform::network::Network;
use kaia_platform::wpa::ScanResult;
use kaia_platform::{Env, Platform, PlatformError};
use kaia_stats::Stats;
use kaia_ui::theme::LARGE_PRINT_SCALE;
use kaia_ui::Theme;

/// The clock the statistics fixtures use.
const FIXED_NOW: i64 = 1_700_000_000;

/// What the fake radio was asked to do, and whether it is on.
#[derive(Debug, Default)]
struct Radio {
    on: bool,
    enables: usize,
    disables: usize,
    scans: usize,
    joins: usize,
    forgets: usize,
    connected: bool,
    saved: bool,
    fail: bool,
}

/// A Wi-Fi radio that only records calls. Shared with the test through `log`.
struct FakeNetwork {
    log: Arc<Mutex<Radio>>,
}

impl FakeNetwork {
    fn boxed(on: bool) -> (Box<dyn Network>, Arc<Mutex<Radio>>) {
        let log = Arc::new(Mutex::new(Radio {
            on,
            ..Radio::default()
        }));
        (Box::new(FakeNetwork { log: log.clone() }), log)
    }
}

impl Network for FakeNetwork {
    fn enable_wifi(&mut self) -> Result<(), PlatformError> {
        let mut radio = self.log.lock().unwrap();
        radio.enables += 1;
        radio.on = true;
        Ok(())
    }

    fn disable_wifi(&mut self) -> Result<(), PlatformError> {
        let mut radio = self.log.lock().unwrap();
        radio.disables += 1;
        radio.on = false;
        Ok(())
    }

    fn is_wifi_on(&self) -> bool {
        self.log.lock().unwrap().on
    }

    fn is_connected(&self) -> bool {
        self.log.lock().unwrap().connected
    }

    fn obtain_ip(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn release_ip(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn scan(&mut self) -> Result<Vec<ScanResult>, PlatformError> {
        let mut radio = self.log.lock().unwrap();
        radio.scans += 1;
        if radio.fail {
            return Err(PlatformError::AuthFailed);
        }
        Ok(vec![ScanResult {
            ssid: "Test connection".into(),
            bssid: String::new(),
            frequency_mhz: 2400,
            signal_dbm: -50,
            flags: "[WPA2-PSK-CCMP][ESS]".into(),
        }])
    }

    fn connect(&mut self, _ssid: &str, psk: Option<&str>) -> Result<(), PlatformError> {
        let mut radio = self.log.lock().unwrap();
        radio.joins += 1;
        if radio.fail || psk != Some("qqqqqqqq") {
            return Err(PlatformError::AuthFailed);
        }
        radio.connected = true;
        radio.saved = true;
        Ok(())
    }
    fn saved_networks(&mut self) -> Result<Vec<kaia_platform::wpa::SavedNetwork>, PlatformError> {
        Ok(if self.log.lock().unwrap().saved {
            vec![kaia_platform::wpa::SavedNetwork {
                id: 7,
                ssid: "Test connection".into(),
            }]
        } else {
            vec![]
        })
    }
    fn forget_network(&mut self, id: u32) -> Result<(), PlatformError> {
        assert_eq!(id, 7);
        let mut radio = self.log.lock().unwrap();
        radio.forgets += 1;
        radio.saved = false;
        radio.connected = false;
        Ok(())
    }
}

/// Makes the app look like a Kobo to the Wi-Fi code paths, with the fake radio.
fn kobo_with_radio(app: &mut kaia_app::App<kaia_app::HeadlessBackend>, on: bool) -> Arc<Mutex<Radio>> {
    let (network, log) = FakeNetwork::boxed(on);
    app.services_mut().platform.kind =
        kaia_platform::platform::Kind::Kobo(kaia_platform::device::Model::LibraColour);
    app.services_mut().platform.network = network;
    log
}

/// Writes a settings file with `pairs` in the data folder before the app starts.
fn write_settings(data: &Path, pairs: &[(&str, Value)]) {
    fs::create_dir_all(data).unwrap();
    let path = data.join("settings.json");
    let mut settings = Settings::open(&path);
    for (key, value) in pairs {
        settings.save(*key, value.clone());
    }
    settings.flush(&path).unwrap();
}

fn command(app: &mut kaia_app::App<kaia_app::HeadlessBackend>, id: &str, args: &[Value]) {
    app.command(id, args);
    app.process(Vec::new()).unwrap();
}

fn save_png(pixmap: &kaia_gfx::Pixmap, name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("kaia-settings-extras");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}-{}.png", std::process::id()));
    write_pixmap(pixmap, &path).unwrap();
    eprintln!("rendered {}", path.display());
    path
}

/// A library with the test books, scanned. Returns the app.
fn scanned_app(dir: &Path) -> kaia_app::App<kaia_app::HeadlessBackend> {
    let lib = library_with_books(dir);
    let mut app = make_app(dir, vec![lib.clone()]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| {
        !a.scan_running()
            && a.services()
                .library
                .book_by_path(&lib.join("juliet.epub"))
                .ok()
                .flatten()
                .is_some()
    });
    app
}

/// A TCP server on the loopback address that answers 200 and counts the requests.
fn counting_server() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&count);
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            seen.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    (url, count)
}

// Large print

#[test]
fn large_print_setting_scales_the_theme_at_startup() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("large-print-startup");
    write_settings(
        &dir.join("data"),
        &[(keys::DISPLAY_LARGE_PRINT, Value::Bool(true))],
    );
    let app = make_app(&dir, Vec::new());
    let theme = &app.services().theme;
    let normal = Theme::new(theme.scale, theme.color_panel);
    assert_eq!(theme.text_scale, LARGE_PRINT_SCALE);
    assert_eq!(
        theme.font.body.size,
        (normal.font.body.size * LARGE_PRINT_SCALE).round()
    );
    assert_eq!(theme.space, normal.space, "spacing does not grow");
}

#[test]
fn large_print_switch_rebuilds_from_the_settings_root() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("large-print-switch");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    assert_eq!(app.services().theme.text_scale, 1.0);

    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-advanced");
    tap_key(&mut app, "settings:display");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "display".into()
        })
    );
    command(&mut app, "settings:set:display.large_print", &[Value::Bool(true)]);
    assert_eq!(app.services().theme.text_scale, LARGE_PRINT_SCALE);
    assert_eq!(
        app.router().routes().len(),
        2,
        "the settings index keeps its caller underneath"
    );
    assert_eq!(app.top_route(), Some(&Route::Settings));
    assert!(app.services().settings.is_true(keys::DISPLAY_LARGE_PRINT));

    // The root has no switch of its own: open the display section again to turn it off.
    tap_key(&mut app, "settings-advanced");
    tap_key(&mut app, "settings:display");
    command(
        &mut app,
        "settings:set:display.large_print",
        &[Value::Bool(false)],
    );
    assert_eq!(app.services().theme.text_scale, 1.0);
}

#[test]
fn large_print_screens_render_with_the_larger_text() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("large-print-render");
    write_settings(
        &dir.join("data"),
        &[(keys::DISPLAY_LARGE_PRINT, Value::Bool(true))],
    );
    let mut app = scanned_app(&dir);
    assert_eq!(app.services().theme.text_scale, LARGE_PRINT_SCALE);
    save_png(app.frame(), "home");

    tap_key(&mut app, "nav-library");
    save_png(app.frame(), "library");

    tap_key(&mut app, "nav-settings");
    save_png(app.frame(), "settings-root");

    tap_key(&mut app, "settings-advanced");
    tap_key(&mut app, "settings:display");
    save_png(app.frame(), "settings-display");

    for section in ["reading", "network", "statistics", "advanced", "about"] {
        command(&mut app, "nav:settings", &[]);
        command(&mut app, &format!("settings:{section}"), &[]);
        save_png(app.frame(), &format!("settings-{section}"));
    }

    command(&mut app, "settings:quick:open", &[]);
    save_png(app.frame(), "quick-settings");
}

#[test]
fn large_print_display_page_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("large-print-golden");
    write_settings(
        &dir.join("data"),
        &[(keys::DISPLAY_LARGE_PRINT, Value::Bool(true))],
    );
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-advanced");
    tap_key(&mut app, "settings:display");
    assert_matches_golden(app.frame(), &golden("settings_large_print.png"), 14);
}

// Experimental features

#[test]
fn experimental_row_opens_a_page_with_the_master_switch_off() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("experimental");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    tap_key(&mut app, "nav-settings");
    command(&mut app, "settings:advanced", &[]);
    command(&mut app, "settings:experimental", &[]);
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "experimental".into()
        })
    );
    let rows = sections::entries("experimental", app.services(), FIXED_NOW);
    assert!(matches!(
        &rows[0],
        Entry::Toggle {
            key: "experimental.enabled",
            on: false,
            ..
        }
    ));
    assert!(
        rows.iter()
            .any(|e| matches!(e, Entry::Text(t) if t.contains("No experimental features"))),
        "the list is empty in this build"
    );

    command(
        &mut app,
        "settings:set:experimental.enabled",
        &[Value::Bool(true)],
    );
    assert!(app.services().settings.is_true("experimental.enabled"));
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn advanced_lists_the_experimental_row() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("experimental-row");
    let app = make_app(&dir, Vec::new());
    let rows = sections::entries("advanced", app.services(), FIXED_NOW);
    assert!(rows.iter().any(|e| matches!(
        e,
        Entry::Row { primary, action: Some(action), .. }
            if primary == "Experimental features" && action == "settings:experimental"
    )));
}

// Statistics

#[test]
fn statistics_count_finished_books_and_total_time() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("stats-finished");
    let data = dir.join("data");
    let lib = library_with_books(&dir);
    fs::create_dir_all(&data).unwrap();
    let mut stats = Stats::open(data.join("statistics.sqlite3")).unwrap();
    stats
        .start_book(
            "378df3409d69b6994febc92492dbb19e",
            "Juliet",
            "Shakespeare",
            40,
            1,
            FIXED_NOW - 900,
        )
        .unwrap();
    stats.page_turn(2, FIXED_NOW - 600).unwrap();
    stats.end_book(FIXED_NOW).unwrap();

    let mut app = make_app(&dir, vec![lib.clone()]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| {
        !a.scan_running()
            && a.services()
                .library
                .book_by_path(&lib.join("juliet.epub"))
                .ok()
                .flatten()
                .is_some()
    });
    let id = book_id(&app, "juliet.epub");
    app.services_mut()
        .library
        .set_status(id, Status::Complete)
        .unwrap();

    let rows = sections::entries("statistics", app.services(), FIXED_NOW);
    assert!(
        rows.iter().any(|e| matches!(
            e,
            Entry::Row { primary, value: Some(v), .. } if primary == "Books finished" && v == "1"
        )),
        "one finished book"
    );
    // Each stay is capped when it is recorded, so the value comes from the database.
    let all = Stats::open(data.join("statistics.sqlite3"))
        .unwrap()
        .all_time()
        .unwrap();
    assert!(all.seconds > 0);
    let expected = facts::duration(all.seconds);
    assert!(
        rows.iter().any(|e| matches!(
            e,
            Entry::Row { primary, value: Some(v), .. } if primary == "All time" && *v == expected
        )),
        "all-time reading shows {expected}"
    );
}

#[test]
fn statistics_without_a_database_still_count_finished_books() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("stats-no-db");
    let app = make_app(&dir, Vec::new());
    let rows = sections::entries("statistics", app.services(), FIXED_NOW);
    assert!(rows.iter().any(|e| matches!(
        e,
        Entry::Row { primary, value: Some(v), .. } if primary == "Books finished" && v == "0"
    )));
}

// Airplane mode

#[test]
fn airplane_switch_turns_wifi_off_and_keeps_it_off() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("airplane-off");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let radio = kobo_with_radio(&mut app, true);

    command(&mut app, "settings:quick:open", &[]);
    assert!(app.find_rect("quick-airplane").is_some());
    command(&mut app, "settings:quick:airplane", &[Value::Bool(true)]);
    assert!(app.services().settings.is_true(keys::NETWORK_AIRPLANE_MODE));
    // Wi-Fi goes off through the same script job as the Wi-Fi switch.
    assert_eq!(app.wifi_requests(), (0, 1), "one job turns Wi-Fi off");

    // A sheet built before airplane mode went on still sends the plain Wi-Fi id.
    command(&mut app, "settings:quick:wifi", &[Value::Bool(true)]);
    assert_eq!(app.wifi_requests(), (0, 1), "no job turns Wi-Fi on");

    // The Wi-Fi switch refuses to turn the radio on and says why.
    command(&mut app, "settings:quick:wifi-blocked", &[Value::Bool(true)]);
    assert_eq!(app.wifi_requests(), (0, 1));
    assert_eq!(radio.lock().unwrap().enables, 0);
    assert!(
        app.notices().iter().any(|n| n.contains("Airplane mode is on")),
        "{:?}",
        app.notices()
    );

    // Switching airplane mode off leaves Wi-Fi off until the user turns it on.
    command(&mut app, "settings:quick:airplane", &[Value::Bool(false)]);
    assert!(!app.services().settings.is_true(keys::NETWORK_AIRPLANE_MODE));
    assert_eq!(app.wifi_requests(), (0, 1), "nothing turns Wi-Fi back on");
    assert_eq!(radio.lock().unwrap().enables, 0);
}

#[test]
fn airplane_mode_off_turns_nothing_on_by_itself() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("airplane-control");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let radio = kobo_with_radio(&mut app, false);
    command(&mut app, "settings:quick:airplane", &[Value::Bool(true)]);
    command(&mut app, "settings:quick:airplane", &[Value::Bool(false)]);
    assert_eq!(radio.lock().unwrap().enables, 0);
    assert_eq!(radio.lock().unwrap().disables, 0, "nothing was on to turn off");
}

#[test]
fn startup_turns_off_wifi_that_came_up_in_airplane_mode() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("airplane-startup");
    let data = dir.join("data");
    write_settings(
        &data,
        &[
            (keys::NETWORK_AIRPLANE_MODE, Value::Bool(true)),
            (kaia_app::i18n::SETTING, Value::from("en")),
        ],
    );
    let root = dir.join("platform");
    fs::create_dir_all(&root).unwrap();
    let env = Env {
        home: Some(root.clone()),
        ..Env::default()
    };
    let mut platform = Platform::detect_in(&root, &env).unwrap();
    let (network, radio) = FakeNetwork::boxed(true);
    platform.network = network;
    let services = Services::open(platform, panel(), Some(data)).unwrap();
    assert!(!radio.lock().unwrap().on, "the radio is off after startup");
    assert_eq!(radio.lock().unwrap().disables, 1);
    drop(services);
}

#[test]
fn catalogs_do_not_offer_to_turn_wifi_on_in_airplane_mode() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (url, hits) = counting_server();
    let dir = scratch("airplane-catalogs");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let radio = kobo_with_radio(&mut app, false);
    let catalog = kaia_opds::Catalog {
        title: "Local".into(),
        url: format!("{url}/feed"),
        username: None,
        password: None,
    };
    kaia_app::surfaces::catalogs::store::save(&mut app.services_mut().settings, &[catalog]);
    command(&mut app, "settings:quick:airplane", &[Value::Bool(true)]);

    command(&mut app, "nav:settings", &[]);
    command(&mut app, "settings:network", &[]);
    command(&mut app, "settings:catalogs", &[]);
    command(&mut app, "catalogs:browse:0", &[]);
    assert!(
        app.find_rect("catalogs:wifi").is_none(),
        "no Turn on Wi-Fi button"
    );
    assert!(
        app.notices().iter().any(|n| n.contains("Airplane mode is on")),
        "{:?}",
        app.notices()
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "no request leaves the device");
    assert_eq!(radio.lock().unwrap().enables, 0);
}

#[test]
fn catalogs_offer_to_turn_wifi_on_without_airplane_mode() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (url, hits) = counting_server();
    let dir = scratch("airplane-control-catalogs");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let radio = kobo_with_radio(&mut app, false);
    let catalog = kaia_opds::Catalog {
        title: "Local".into(),
        url: format!("{url}/feed"),
        username: None,
        password: None,
    };
    kaia_app::surfaces::catalogs::store::save(&mut app.services_mut().settings, &[catalog]);

    command(&mut app, "nav:settings", &[]);
    command(&mut app, "settings:network", &[]);
    command(&mut app, "settings:catalogs", &[]);
    command(&mut app, "catalogs:browse:0", &[]);
    assert!(
        app.find_rect("catalogs:wifi").is_some(),
        "the prompt offers Wi-Fi"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    assert_eq!(radio.lock().unwrap().enables, 0, "only the button turns Wi-Fi on");
}

#[test]
fn quick_settings_show_airplane_mode_and_fit_the_screen() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("airplane-fit");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    command(&mut app, "settings:quick:open", &[]);
    for name in ["quick-wifi", "quick-airplane", "quick-rotation"] {
        let rect = app.find_rect(name).unwrap_or_else(|| panic!("{name} is missing"));
        assert!(rect.right() <= app.screen().w as i32, "{name}: {rect:?}");
        assert!(rect.bottom() <= app.screen().h as i32, "{name}: {rect:?}");
    }
}

#[test]
fn round5_wifi_scan_keyboard_join_forget_and_privacy() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round5-wifi");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    let (network, radio) = FakeNetwork::boxed(true);
    app.services_mut().platform.network = network;
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-wifi");
    tap_key(&mut app, "wifi-connect");
    wait_for(&mut app, "scan", |a| a.find_rect("wifi:select:0").is_some());
    assert_matches_golden(app.frame(), &golden("settings_wifi_scan.png"), 14);
    tap_key(&mut app, "wifi:select:0");
    tap_key(&mut app, "wifi-password");
    let keyboard = app.find_rect("wifi-keyboard").unwrap();
    let mut point = kaia_gfx::Point::new(0, 0);
    let theme = app.services().theme.clone();
    app.update_top::<kaia_ui::controls::OnScreenKeyboard, _>("wifi-keyboard", |kb, _| {
        let key = kb
            .key_boxes(&theme, keyboard.w)
            .into_iter()
            .find(|k| k.action == kaia_ui::controls::KeyAction::Char('q'))
            .unwrap();
        point = kaia_gfx::Point::new(
            keyboard.x + key.rect.x + key.rect.w as i32 / 2,
            keyboard.y + key.rect.y + key.rect.h as i32 / 2,
        );
    });
    for _ in 0..8 {
        app.tap(point).unwrap();
    }
    assert_matches_golden(app.frame(), &golden("settings_wifi_password.png"), 14);
    radio.lock().unwrap().fail = true;
    tap_key(&mut app, "wifi-join");
    wait_for(&mut app, "refused password", |a| {
        a.find_rect("wifi-password").is_some()
    });
    assert!(!app.services().platform.network.is_connected());
    assert_matches_golden(app.frame(), &golden("settings_wifi_failure.png"), 14);
    radio.lock().unwrap().fail = false;
    tap_key(&mut app, "wifi-password");
    for _ in 0..8 {
        app.tap(point).unwrap();
    }
    tap_key(&mut app, "wifi-join");
    wait_for(&mut app, "join", |a| {
        a.services().platform.network.is_connected() && a.find_rect("wifi-retry").is_some()
    });
    tap_key(&mut app, "settings-back");
    tap_key(&mut app, "wifi-forget");
    wait_for(&mut app, "saved networks", |a| {
        a.find_rect("wifi:select:0").is_some()
    });
    tap_key(&mut app, "wifi:select:0");
    assert_matches_golden(app.frame(), &golden("settings_wifi_forget.png"), 14);
    tap_key(&mut app, "wifi-remove");
    wait_for(&mut app, "forget", |a| {
        a.find_rect("wifi-retry").is_some() && !a.services().platform.network.is_connected()
    });
    assert_eq!(radio.lock().unwrap().forgets, 1);
    assert_eq!(radio.lock().unwrap().joins, 2);
    tap_key(&mut app, "settings-close");
    let saved = fs::read_to_string(app.services().settings_path()).unwrap();
    assert!(!saved.contains("Test connection"));
    assert!(!saved.contains("qqqqqqqq"));
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn round5_wifi_off_airplane_and_failed_scan_do_not_join() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round5-wifi-failure");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    let (network, radio) = FakeNetwork::boxed(false);
    app.services_mut().platform.network = network;
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-wifi");
    tap_key(&mut app, "wifi-connect");
    assert_eq!(radio.lock().unwrap().scans, 0);
    tap_key(&mut app, "settings-back");
    radio.lock().unwrap().on = true;
    app.services_mut()
        .settings
        .save(keys::NETWORK_AIRPLANE_MODE, Value::Bool(true));
    tap_key(&mut app, "wifi-connect");
    assert_eq!(radio.lock().unwrap().scans, 0);
    app.services_mut()
        .settings
        .save(keys::NETWORK_AIRPLANE_MODE, Value::Bool(false));
    radio.lock().unwrap().fail = true;
    tap_key(&mut app, "wifi-retry");
    wait_for(&mut app, "failed scan", |a| {
        a.services().platform.network.is_wifi_on() && a.find_rect("wifi-retry").is_some()
    });
    app.process(vec![]).unwrap();
    assert_eq!(radio.lock().unwrap().joins, 0);
    assert!(app.unknown_commands().is_empty());
}
