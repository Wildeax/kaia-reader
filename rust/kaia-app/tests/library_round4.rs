//! Round 4 local routes and modal interaction checks.
mod common;
use common::*;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::{Event, Value};
use kaia_gfx::{golden::assert_matches_golden, Color};
use kaia_input::Key;
use kaia_platform::wpa::ScanResult;
use kaia_platform::{Network, PlatformError};
use std::sync::{Arc, Mutex};

fn capture(app: &mut App<HeadlessBackend>, name: &str) {
    freeze_status_clock(app);
    assert_matches_golden(app.frame(), &golden(name), 14);
}
fn command(app: &mut App<HeadlessBackend>, id: &str, args: &[Value]) {
    app.command(id, args);
    app.process(vec![]).unwrap();
}
fn indexed(name: &str) -> App<HeadlessBackend> {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    app
}
#[test]
fn details_surround_is_opaque_and_actions_live_in_the_menu() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = indexed("round4-details");
    let id = book_id(&app, "juliet.epub");
    tap_key(&mut app, "nav-library");
    command(&mut app, &format!("library:details:{id}"), &[]);
    for (x, y) in [(0, 0), (10, 1600), (1250, 1670), (600, 1670)] {
        assert_eq!(app.frame().get(x, y), Some(Color::gray(77)));
    }
    assert!(app.find_rect(&format!("library-rating-open-{id}")).is_none());
    tap_key(&mut app, &format!("details-menu-{id}"));
    for action in ["rating-open", "tags-open", "stats-open", "favorite-toggle"] {
        assert!(app.find_rect(&format!("library-{action}-{id}")).is_some());
    }
    capture(&mut app, "library_details_actions.png");
    tap_key(&mut app, &format!("library-favorite-toggle-{id}"));
    assert!(app.services().library.is_favorite(id).unwrap());
    app.key(Key::Back).unwrap();
    tap_key(&mut app, "nav-more");
    tap_key(&mut app, "more-favorites");
    assert!(app.find_rect("library-view-authors").is_none());
    assert!(app.find_rect(&format!("book-{id}")).is_some());
    capture(&mut app, "favorites_populated.png");
}
#[test]
fn local_more_pages_have_no_notebook_creation_or_service_actions() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = make_app(&scratch("round4-more"), vec![]);
    app.start().unwrap();
    capture(&mut app, "home_empty.png");
    tap_key(&mut app, "nav-more");
    capture(&mut app, "more.png");
    tap_key(&mut app, "more-favorites");
    capture(&mut app, "favorites_empty.png");
    tap_key(&mut app, "favorites-back");
    tap_key(&mut app, "more-notebooks");
    capture(&mut app, "notebooks_intro.png");
    tap_key(&mut app, "notebooks-intro-close");
    capture(&mut app, "notebooks_empty.png");
    assert!(app.find_rect("notebooks-new").is_none());
    tap_key(&mut app, "local-back");
    tap_key(&mut app, "more-experimental");
    capture(&mut app, "experiments.png");
    tap_key(&mut app, "experimental.enabled");
    assert_eq!(
        app.services().settings.read("experimental.enabled").as_bool(),
        Some(true)
    );
    tap_key(&mut app, "experiments-large-print");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "display".into()
        })
    );
    assert!(app.unknown_commands().is_empty());
}
#[test]
fn global_search_scopes_results_sort_and_return() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("round4-search");
    let lib = library_with_books(&dir);
    std::fs::copy(lib.join("juliet.epub"), lib.join("juliet-copy.epub")).unwrap();
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    tap_key(&mut app, "status-search");
    capture(&mut app, "global_search.png");
    tap_key(&mut app, "search-scope");
    capture(&mut app, "global_search_scopes.png");
    tap_key(&mut app, "search-scope-0");
    command(&mut app, "global-search:query", &[Value::from("Juliet")]);
    tap_key(&mut app, "search-go");
    assert!(app.find_rect("search-keyboard").is_none());
    assert!(app.find_rect("search-hit-0").is_some());
    capture(&mut app, "global_search_results.png");
    tap_key(&mut app, "search-sort");
    capture(&mut app, "global_search_sort.png");
    tap_key(&mut app, "search-sort-1");
    tap_key(&mut app, "search-hit-0");
    wait_for(&mut app, "book", |a| {
        matches!(a.top_route(), Some(Route::Reader { .. })) && !a.reader_busy()
    });
    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Search));
    assert!(app.find_rect("search-hit-0").is_some());
    assert!(app.find_rect("search-keyboard").is_none());
    tap_key(&mut app, "search-field");
    command(
        &mut app,
        "global-search:query",
        &[Value::from("no-such-title-zzzz")],
    );
    tap_key(&mut app, "search-go");
    capture(&mut app, "global_search_empty.png");
    tap_key(&mut app, "search-scope");
    tap_key(&mut app, "search-scope-2");
    let book = app
        .services()
        .library
        .book(book_id(&app, "2col.pdf"))
        .unwrap()
        .unwrap();
    let table = |pairs: Vec<(&str, Value)>| {
        pairs
            .into_iter()
            .map(|(k, v)| (kaia_events::value::Key::from(k), v))
            .collect()
    };
    let saved = kaia_events::Settings::from_table(table(vec![
        ("format", Value::from(kaia_book::state::STATE_FORMAT)),
        ("version", Value::from(kaia_book::state::STATE_VERSION)),
        (
            "doc",
            Value::Table(table(vec![
                ("partial_md5", Value::from(book.partial_md5.clone())),
                ("title", Value::from("Local notes")),
            ])),
        ),
        (
            "annotations",
            Value::array(vec![Value::Table(table(vec![
                ("kaia_id", Value::from(1i64)),
                ("page", Value::from(1i64)),
                ("pageno", Value::from(1i64)),
                ("drawer", Value::from("lighten")),
                ("text", Value::from("A passage to revisit")),
                ("note", Value::from("A note about the margin")),
                ("datetime", Value::from("2026-10-15 09:00:00")),
            ]))]),
        ),
    ]));
    let folder = app.services().paths.data_dir.join("kaia/books");
    std::fs::create_dir_all(&folder).unwrap();
    saved
        .flush(&folder.join(format!("{}.json", book.partial_md5)))
        .unwrap();
    command(&mut app, "global-search:query", &[Value::from("note")]);
    tap_key(&mut app, "search-go");
    assert!(app.find_rect("search-hit-0").is_some());
    capture(&mut app, "global_search_annotations.png");
    tap_key(&mut app, "search-scope");
    tap_key(&mut app, "search-scope-3");
    command(&mut app, "global-search:query", &[Value::from("word")]);
    tap_key(&mut app, "search-go");
    assert!(app.find_rect("search-hit-0").is_none());
    tap_key(&mut app, "search-scope");
    tap_key(&mut app, "search-scope-1");
    tap_key(&mut app, "search-book");
    let id = book_id(&app, "juliet.epub");
    tap_key(&mut app, &format!("search-book-{id}"));
    command(&mut app, "global-search:query", &[Value::from("Juliet")]);
    tap_key(&mut app, "search-go");
    wait_for(&mut app, "current search", |a| {
        a.find_rect("search-hit-0").is_some()
    });
    capture(&mut app, "global_search_current.png");
    assert!(app.find_rect("search-hit-3").is_some());
    assert!(app.find_rect("search-hit-4").is_none());
    tap_key(&mut app, "search-next");
    assert!(app.find_rect("search-hit-4").is_some());
    assert!(app.find_rect("search-hit-0").is_none());
    tap_key(&mut app, "search-previous");
    assert!(app.find_rect("search-hit-0").is_some());
    assert!(app.unknown_commands().is_empty());
}

#[derive(Default)]
struct Radio {
    on: bool,
    fail: bool,
    joined: bool,
    password_len: usize,
}
struct FakeRadio(Arc<Mutex<Radio>>);
impl Network for FakeRadio {
    fn enable_wifi(&mut self) -> Result<(), PlatformError> {
        self.0.lock().unwrap().on = true;
        Ok(())
    }
    fn disable_wifi(&mut self) -> Result<(), PlatformError> {
        self.0.lock().unwrap().on = false;
        Ok(())
    }
    fn is_wifi_on(&self) -> bool {
        self.0.lock().unwrap().on
    }
    fn is_connected(&self) -> bool {
        self.0.lock().unwrap().joined
    }
    fn obtain_ip(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn release_ip(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn scan(&mut self) -> Result<Vec<ScanResult>, PlatformError> {
        // Empty names keep captures free of network names, including synthetic ones.
        Ok((0..5)
            .map(|n| ScanResult {
                ssid: " ".repeat(n as usize + 1),
                bssid: " ".repeat(n as usize + 1),
                frequency_mhz: 2412,
                signal_dbm: -45 - n * 10,
                flags: if n == 0 {
                    "[WPA2-PSK]".into()
                } else {
                    String::new()
                },
            })
            .collect())
    }
    fn connect(&mut self, _ssid: &str, psk: Option<&str>) -> Result<(), PlatformError> {
        let mut r = self.0.lock().unwrap();
        r.password_len = psk.unwrap_or("").len();
        if r.fail {
            Err(PlatformError::AuthFailed)
        } else {
            r.joined = true;
            Ok(())
        }
    }
}
#[test]
fn wifi_scan_password_failure_retry_and_off() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = make_app(&scratch("round4-wifi"), vec![]);
    let radio = Arc::new(Mutex::new(Radio::default()));
    app.services_mut().platform.network = Box::new(FakeRadio(radio.clone()));
    app.start().unwrap();
    tap_key(&mut app, "status-wifi");
    capture(&mut app, "wifi_off.png");
    app.tap(centre_of(&app, "quick-wifi")).unwrap();
    assert!(app.reader_busy());
    wait_for(&mut app, "radio enabled", |a| {
        a.services().platform.network.is_wifi_on()
    });
    assert_matches_golden(app.frame(), &golden("wifi_scanning.png"), 14);
    wait_for(&mut app, "networks", |a| a.find_rect("wifi-network-0").is_some());
    app.broadcast_app(Event::new("ReaderTick"));
    app.process(vec![]).unwrap();
    capture(&mut app, "wifi_networks.png");
    tap_key(&mut app, "wifi-network-0");
    tap_key(&mut app, "wifi-password");
    for c in "secret12".chars() {
        app.key(Key::Char(c)).unwrap();
    }
    capture(&mut app, "wifi_password.png");
    radio.lock().unwrap().fail = true;
    app.tap(centre_of(&app, "wifi-join")).unwrap();
    assert!(app.reader_busy());
    assert_matches_golden(app.frame(), &golden("wifi_joining.png"), 14);
    wait_for(&mut app, "join failure", |a| a.find_rect("wifi-retry").is_some());
    app.broadcast_app(Event::new("ReaderTick"));
    app.process(vec![]).unwrap();
    assert_eq!(radio.lock().unwrap().password_len, 8);
    capture(&mut app, "wifi_failure.png");
    tap_key(&mut app, "wifi-retry");
    let mut password = String::new();
    app.update_top::<kaia_ui::controls::TextField, _>("wifi-password", |field, _| {
        password = field.text().into()
    });
    assert!(password.is_empty());
    radio.lock().unwrap().fail = false;
    command(&mut app, "wifi:password", &[Value::from("secret12")]);
    tap_key(&mut app, "wifi-join");
    app.broadcast_app(Event::new("ReaderTick"));
    app.process(vec![]).unwrap();
    wait_for(&mut app, "connected", |_| radio.lock().unwrap().joined);
    assert!(radio.lock().unwrap().joined);
    capture(&mut app, "wifi_connected.png");
    tap_key(&mut app, "quick-wifi");
    assert!(!radio.lock().unwrap().on);
    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Home));
    assert!(app.unknown_commands().is_empty());
}
