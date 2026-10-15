use super::*;
use crate::surfaces::notes::data::{Location, Note};
use kaia_events::Value;
use std::io::{BufRead, BufReader, Read};
use std::net::{TcpListener, TcpStream};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Default)]
struct Received {
    requests: Vec<(String, String)>,
    files: BTreeMap<String, String>,
    status: Option<u16>,
    collections: std::collections::BTreeSet<String>,
    fail_json: bool,
}
struct Server {
    url: String,
    received: Arc<Mutex<Received>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let received = Arc::new(Mutex::new(Received::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (seen, stopped) = (received.clone(), stop.clone());
        let worker = thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &seen),
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        Self {
            url,
            received,
            stop,
            worker: Some(worker),
        }
    }
    fn count(&self) -> usize {
        self.received.lock().unwrap().requests.len()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let result = self.worker.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}
fn serve(stream: TcpStream, seen: &Mutex<Received>) {
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap() == 0 {
        return;
    }
    let request = line.trim().split(' ').map(str::to_owned).collect::<Vec<_>>();
    let mut headers = BTreeMap::new();
    loop {
        line.clear();
        assert!(
            reader.read_line(&mut line).unwrap() > 0,
            "request headers ended early"
        );
        if line == "\r\n" {
            break;
        }
        let (key, value) = line.trim().split_once(':').unwrap();
        headers.insert(key.to_ascii_lowercase(), value.trim().to_owned());
    }
    let mut body = vec![0; headers.get("content-length").map_or(0, |n| n.parse().unwrap())];
    reader.read_exact(&mut body).unwrap();
    let mut state = seen.lock().unwrap();
    state.requests.push((request[0].clone(), request[1].clone()));
    let status = if headers.get("authorization").map(String::as_str) != Some("Basic cmVhZGVyOnNlY3JldA==") {
        401
    } else if let Some(status) = state.status {
        status
    } else {
        match request[0].as_str() {
            "PROPFIND" => {
                assert_eq!(headers.get("depth").unwrap(), "0");
                207
            }
            "MKCOL" => {
                if state.collections.insert(request[1].clone()) {
                    201
                } else {
                    405
                }
            }
            "PUT" if state.fail_json && request[1].ends_with(".json") => 503,
            "PUT" => {
                state
                    .files
                    .insert(request[1].clone(), String::from_utf8(body).unwrap());
                201
            }
            _ => panic!("unexpected method {}", request[0]),
        }
    };
    write!(
        reader.get_mut(),
        "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nLocation: /redirect-target\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
}
fn book() -> BookNotes {
    BookNotes {
        md5: "abc123".into(),
        title: "A/B: Book".into(),
        author: Some("An Author".into()),
        library_id: Some(1),
        latest: "2026-10-14".into(),
        ink_counts: vec![(1, 2)],
        notes: vec![Note {
            id: 1,
            sticker: None,
            text: Some("A quote".into()),
            note: Some("A note".into()),
            chapter: Some("Chapter one".into()),
            pageno: Some(1),
            location: Location::Page(0),
            datetime: "2026-10-14 10:00:00".into(),
            orphaned: false,
            highlight: true,
            bookmark: false,
        }],
    }
}
fn setup(server: &Server, dir: &Path) -> (WebDav, Settings) {
    let mut settings = Settings::default();
    settings.save(ENABLED, Value::Bool(true));
    settings.save(SERVER, Value::from(server.url.as_str()));
    settings.save(USERNAME, Value::from("reader"));
    settings.save(FOLDER, Value::from("notes"));
    store_password(dir, "secret").unwrap();
    let mut worker = WebDav::new(dir.into());
    worker.allow_local_http = true;
    (worker, settings)
}
fn settle(worker: &mut WebDav, settings: &Settings, now: i64) -> Vec<String> {
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut messages = worker.tick(settings, true, now);
    while worker.busy() {
        assert!(Instant::now() < deadline, "worker did not finish");
        thread::sleep(Duration::from_millis(5));
        messages.extend(worker.tick(settings, true, now));
    }
    messages
}
#[test]
fn webdav_upload_reuses_local_formats_and_unchanged_notes_send_nothing_after_restart() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let (mut worker, settings) = setup(&server, dir.path());
    let books = [book()];
    worker.enqueue(&settings, &books, 1000, false).unwrap();
    assert!(settle(&mut worker, &settings, 1000).is_empty());
    {
        let state = server.received.lock().unwrap();
        assert_eq!(state.requests.len(), 3);
        assert_eq!(
            state.files["/notes/A_B_%20Book%20-%20An%20Author.md"],
            export::markdown_text(&books, 1000)
        );
        assert_eq!(
            state.files["/notes/A_B_%20Book%20-%20An%20Author.json"],
            export::json_text(&books, 1000)
        );
    }
    drop(worker);
    let (mut worker, settings) = setup(&server, dir.path());
    worker.enqueue(&settings, &books, 2000, true).unwrap();
    settle(&mut worker, &settings, 2000);
    assert_eq!(server.count(), 3);
    let mut empty = book();
    empty.notes.clear();
    empty.ink_counts.clear();
    worker.enqueue(&settings, &[empty], 3000, false).unwrap();
    settle(&mut worker, &settings, 3000);
    assert_eq!(server.count(), 6);
    let state = server.received.lock().unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&state.files["/notes/A_B_%20Book%20-%20An%20Author.json"]).unwrap();
    assert!(parsed["books"][0]["annotations"].as_array().unwrap().is_empty());
}
#[test]
fn webdav_401_reports_once_and_holds_across_restart_until_account_edit() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let (mut worker, settings) = setup(&server, dir.path());
    store_password(dir.path(), "wrong").unwrap();
    worker.enqueue(&settings, &[book()], 0, false).unwrap();
    let messages = settle(&mut worker, &settings, 0);
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("refused the login"));
    assert!(settle(&mut worker, &settings, 1000).is_empty());
    assert_eq!(server.count(), 1);
    drop(worker);
    let (mut worker, settings) = setup(&server, dir.path());
    assert!(settle(&mut worker, &settings, 2000).is_empty());
    assert_eq!(server.count(), 1);
    worker.account_changed().unwrap();
    settle(&mut worker, &settings, 2000);
    assert_eq!(server.count(), 4);
}
#[test]
fn webdav_airplane_and_no_wifi_queue_then_connect_uploads() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let (mut worker, mut settings) = setup(&server, dir.path());
    settings.save("network.airplane_mode", Value::Bool(true));
    worker.enqueue(&settings, &[book()], 0, false).unwrap();
    assert!(worker.tick(&settings, true, 0).is_empty());
    assert_eq!(server.count(), 0);
    assert_eq!(worker.load().unwrap().queue.len(), 1);
    drop(worker);
    let (mut worker, settings) = setup(&server, dir.path());
    assert!(worker.tick(&settings, false, 1000).is_empty());
    assert_eq!(server.count(), 0);
    settle(&mut worker, &settings, 1000);
    assert_eq!(server.count(), 3);
}
#[test]
fn webdav_off_sends_nothing_even_with_a_pending_queue_and_test_tap() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let (mut worker, mut settings) = setup(&server, dir.path());
    worker.enqueue(&settings, &[book()], 0, false).unwrap();
    settings.save(ENABLED, Value::Bool(false));
    worker.enqueue(&settings, &[book()], 0, true).unwrap();
    assert!(worker.test_connection(&settings, true).is_err());
    assert!(settle(&mut worker, &settings, 1000).is_empty());
    assert_eq!(server.count(), 0);
    assert!(!Settings::default().is_true(ENABLED));
}
#[test]
fn webdav_test_is_one_propfind_and_http_requires_a_test_only_loopback_switch() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let (mut worker, settings) = setup(&server, dir.path());
    worker.test_connection(&settings, true).unwrap();
    assert_eq!(
        settle(&mut worker, &settings, 0),
        vec![tr("The WebDAV server answered.")]
    );
    assert_eq!(
        server.received.lock().unwrap().requests,
        vec![("PROPFIND".into(), "/notes/".into())]
    );
    worker.allow_local_http = false;
    assert!(worker
        .test_connection(&settings, true)
        .unwrap_err()
        .contains("HTTP is not allowed"));
    assert_eq!(server.count(), 1);
    let mut config = worker.config(&settings).unwrap();
    config.allow_local_http = true;
    config.server = "http://example.com".into();
    assert_eq!(config.folder_url(), Err(Failure::Address));
    config.server = "https://user:password@example.com".into();
    assert_eq!(config.folder_url(), Err(Failure::Address));
}
#[test]
fn webdav_transient_failure_uses_bounded_backoff_and_reports_once() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    server.received.lock().unwrap().status = Some(503);
    let (mut worker, settings) = setup(&server, dir.path());
    worker.enqueue(&settings, &[book()], 0, false).unwrap();
    assert_eq!(settle(&mut worker, &settings, 0).len(), 1);
    assert_eq!(server.count(), 1);
    assert!(settle(&mut worker, &settings, 29).is_empty());
    assert_eq!(server.count(), 1);
    assert!(settle(&mut worker, &settings, 30).is_empty());
    assert_eq!(server.count(), 2);
    assert!(settle(&mut worker, &settings, 149).is_empty());
    assert_eq!(server.count(), 2);
    assert!(settle(&mut worker, &settings, 150).is_empty());
    assert_eq!(server.count(), 3);
    worker.enqueue(&settings, &[book()], 2000, false).unwrap();
    assert!(settle(&mut worker, &settings, 2000).is_empty());
    assert_eq!(server.count(), 3);
    server.received.lock().unwrap().status = None;
    worker.enqueue(&settings, &[book()], 2000, true).unwrap();
    settle(&mut worker, &settings, 2000);
    assert_eq!(server.count(), 6);
}
#[test]
fn webdav_password_is_private_and_absent_from_queue_and_settings() {
    use std::os::unix::fs::PermissionsExt;
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let (mut worker, settings) = setup(&server, dir.path());
    assert_eq!(
        fs::metadata(dir.path().join("webdav-password"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    worker.enqueue(&settings, &[book()], 0, false).unwrap();
    settings.flush(&dir.path().join("settings.json")).unwrap();
    for file in ["settings.json", "webdav-queue.json"] {
        assert!(!fs::read_to_string(dir.path().join(file))
            .unwrap()
            .contains("secret"));
    }
}

#[test]
fn webdav_nested_folders_and_duplicate_names_keep_separate_files() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let (mut worker, mut settings) = setup(&server, dir.path());
    settings.save(FOLDER, Value::from("notes/books"));
    let first = book();
    let mut second = book();
    second.md5 = "another-book".into();
    worker
        .enqueue(&settings, &[first, second.clone()], 0, true)
        .unwrap();
    settle(&mut worker, &settings, 0);
    let state = server.received.lock().unwrap();
    assert_eq!(state.files.len(), 4);
    assert_eq!(state.collections.len(), 2);
    assert!(state.collections.contains("/notes/"));
    assert!(state.collections.contains("/notes/books/"));
    let suffix = &hash(second.md5.as_bytes())[..12];
    assert!(state.files.contains_key(&format!(
        "/notes/books/A_B_%20Book%20-%20An%20Author%20-%20{suffix}.json"
    )));
}

#[test]
fn webdav_partial_upload_then_reverted_notes_replaces_both_files() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let (mut worker, settings) = setup(&server, dir.path());
    worker.enqueue(&settings, &[book()], 0, true).unwrap();
    settle(&mut worker, &settings, 0);
    let mut changed = book();
    changed.notes[0].note = Some("Changed note".into());
    server.received.lock().unwrap().fail_json = true;
    worker.enqueue(&settings, &[changed], 10, false).unwrap();
    assert_eq!(settle(&mut worker, &settings, 10).len(), 1);
    server.received.lock().unwrap().fail_json = false;
    worker.enqueue(&settings, &[book()], 11, false).unwrap();
    settle(&mut worker, &settings, 11);
    assert_eq!(server.count(), 9);
    assert_eq!(
        server.received.lock().unwrap().files["/notes/A_B_%20Book%20-%20An%20Author.md"],
        export::markdown_text(&[book()], 11)
    );
}

#[test]
fn webdav_bad_queue_stays_untouched_and_redirects_are_not_followed() {
    let server = Server::new();
    let dir = tempfile::tempdir().unwrap();
    let (mut worker, settings) = setup(&server, dir.path());
    fs::write(dir.path().join("webdav-queue.json"), b"broken").unwrap();
    assert_eq!(settle(&mut worker, &settings, 0).len(), 1);
    assert!(settle(&mut worker, &settings, 1).is_empty());
    assert_eq!(fs::read(dir.path().join("webdav-queue.json")).unwrap(), b"broken");
    assert_eq!(server.count(), 0);
    fs::remove_file(dir.path().join("webdav-queue.json")).unwrap();
    server.received.lock().unwrap().status = Some(302);
    worker.enqueue(&settings, &[book()], 0, true).unwrap();
    assert_eq!(settle(&mut worker, &settings, 0).len(), 1);
    assert_eq!(server.count(), 1);
}
