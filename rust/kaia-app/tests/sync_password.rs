//! The progress sync password. Typing it stores only its MD5 user key. A plain
//! password in an older settings file is replaced at startup. A login sends the
//! same x-auth-key that KOReader sends for the same password.

mod common;

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::thread;

use common::*;
use kaia_app::surfaces::settings::keys;
use kaia_events::Value;

/// md5("password"), the value KOReader's `doLogin` stores as `userkey`.
const USER_KEY: &str = "5f4dcc3b5aa765d61d8327deb882cf99";

/// The request headers the server has received, with lowercase names.
type Headers = Arc<Mutex<Vec<(String, String)>>>;

/// A server on the loopback address. It records the headers of each request and
/// answers 200. Returns its address and the record.
fn header_server() -> (String, Headers) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Headers = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&seen);
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            // The request line comes first, then the headers up to the blank line.
            let _ = reader.read_line(&mut line);
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    break;
                }
                if let Some((name, value)) = trimmed.split_once(':') {
                    let entry = (name.trim().to_ascii_lowercase(), value.trim().to_owned());
                    record.lock().unwrap().push(entry);
                }
            }
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    (url, seen)
}

/// The value of the first header named `name`, if the server received one.
fn header(seen: &Headers, name: &str) -> Option<String> {
    let headers = seen.lock().unwrap();
    let found = headers
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.clone());
    found
}

#[test]
fn typing_the_password_keeps_only_its_user_key_on_disk() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sync-password-typed");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-network");
    app.command("settings:set:sync.password", &[Value::from("password")]);
    {
        let settings = &app.services().settings;
        assert_eq!(settings.read(keys::SYNC_USERKEY).as_str(), Some(USER_KEY));
        assert!(!settings.has(keys::SYNC_PASSWORD));
    }

    // Leaving the section writes settings.json.
    app.command("back", &[]);
    let path = app.services().settings_path();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains(USER_KEY), "{text}");
    assert!(!text.contains(keys::SYNC_PASSWORD), "{text}");
    assert!(!text.contains("\"password\""), "{text}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn an_older_plain_password_is_replaced_at_startup() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sync-password-migrate");
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    let older = r#"{"sync.username": "reader", "sync.password": "password"}"#;
    fs::write(data.join("settings.json"), older).unwrap();
    // A flush keeps the previous file as settings.json.old, and that copy holds the password too.
    fs::write(data.join("settings.json.old"), older).unwrap();

    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    let text = fs::read_to_string(data.join("settings.json")).unwrap();
    assert!(text.contains(USER_KEY), "{text}");
    assert!(text.contains("reader"), "{text}");
    assert!(!text.contains(keys::SYNC_PASSWORD), "{text}");
    assert!(!text.contains("\"password\""), "{text}");
    assert!(
        !data.join("settings.json.old").exists(),
        "the old copy held the password"
    );
    assert_eq!(
        app.services().settings.read(keys::SYNC_USERKEY).as_str(),
        Some(USER_KEY)
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_login_sends_the_x_auth_key_koreader_sends() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sync-password-header");
    let (url, seen) = header_server();
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-network");
    app.command("settings:set:sync.server", &[Value::from(url.as_str())]);
    app.command("settings:set:sync.username", &[Value::from("reader")]);
    app.command("settings:set:sync.password", &[Value::from("password")]);

    let client = kaia_app::sync::client(&app.services().settings).unwrap();
    client.authorize().unwrap();
    assert_eq!(header(&seen, "x-auth-user").as_deref(), Some("reader"));
    assert_eq!(header(&seen, "x-auth-key").as_deref(), Some(USER_KEY));
    fs::remove_dir_all(&dir).unwrap();
}
