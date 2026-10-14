//! Automatic progress sync. A book pulls on open only when `sync.auto` is on. Airplane
//! mode sends no request. The Network section saves the `sync.auto` switch.

mod common;

use std::io::Read;
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use common::*;
use kaia_app::surfaces::settings::keys;
use kaia_app::{App, HeadlessBackend};
use kaia_events::{Settings, Value};

/// A server on the loopback address that counts the requests it receives. It answers
/// 200 with no body, so a pull ends with a reply the test does not read.
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
            let _ = std::io::Write::write_all(
                &mut stream,
                b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    (url, count)
}

/// The sync switch on, with an account on `url`.
fn sync_on(settings: &mut Settings, url: &str) {
    settings.save(keys::SYNC_ENABLED, Value::Bool(true));
    settings.save(keys::SYNC_SERVER, Value::from(url));
    settings.save(keys::SYNC_USERNAME, Value::from("reader"));
    settings.save(
        keys::SYNC_USERKEY,
        Value::from(kaia_sync::userkey_for("secret").unwrap()),
    );
}

/// Opens juliet.epub from a fresh library. `configure` sets the settings after the app
/// starts and before the book opens.
fn open_juliet(dir: &Path, configure: impl FnOnce(&mut Settings)) -> App<HeadlessBackend> {
    let lib = library_with_books(dir);
    let mut app = make_app(dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    configure(&mut app.services_mut().settings);
    open_from_library(&mut app, "juliet.epub");
    wait_reader(&mut app);
    app
}

/// Runs the app for a while, so a request started by an event would reach the server.
fn settle(app: &mut App<HeadlessBackend>) {
    for _ in 0..40 {
        app.iterate(Some(Duration::from_millis(10))).unwrap();
    }
}

#[test]
fn opening_a_book_sends_no_pull_while_sync_auto_is_off() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sync-auto-off");
    let (url, hits) = counting_server();
    let mut app = open_juliet(&dir, |s| sync_on(s, &url));
    settle(&mut app);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "sync.auto is off by default, so opening sends nothing"
    );
}

#[test]
fn opening_a_book_pulls_once_sync_auto_is_on() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sync-auto-on");
    let (url, hits) = counting_server();
    let mut app = open_juliet(&dir, |s| {
        sync_on(s, &url);
        s.save(keys::SYNC_AUTO, Value::Bool(true));
    });
    wait_for(&mut app, "the pull on open", |_| hits.load(Ordering::SeqCst) >= 1);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "one pull for the open");
}

#[test]
fn airplane_mode_sends_no_request_on_open_or_on_test_connection() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sync-airplane");
    let (url, hits) = counting_server();
    let mut app = open_juliet(&dir, |s| {
        sync_on(s, &url);
        s.save(keys::SYNC_AUTO, Value::Bool(true));
        s.save(keys::NETWORK_AIRPLANE_MODE, Value::Bool(true));
    });
    settle(&mut app);
    assert_eq!(hits.load(Ordering::SeqCst), 0, "no pull on open in airplane mode");

    // Test connection shows the airplane notice and sends nothing.
    let services = app.services_mut();
    let result = services.sync.health(&services.settings);
    assert_eq!(
        result,
        Err("Airplane mode is on. Turn it off to use Wi-Fi.".to_string())
    );
    settle(&mut app);
    assert_eq!(hits.load(Ordering::SeqCst), 0, "the health check sends nothing");
}

#[test]
fn the_network_section_saves_the_sync_auto_switch() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sync-auto-toggle");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-network");
    assert!(
        !app.services().settings.is_true(keys::SYNC_AUTO),
        "off by default"
    );

    tap_key(&mut app, "set-sync.auto");
    assert!(
        app.services().settings.is_true(keys::SYNC_AUTO),
        "the tap turns it on"
    );
    tap_key(&mut app, "set-sync.auto");
    assert!(
        !app.services().settings.is_true(keys::SYNC_AUTO),
        "and a second tap turns it off"
    );
}
