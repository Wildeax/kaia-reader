mod common;
use common::*;
use kaia_app::surfaces::catalogs::{
    store,
    worker::{Reply, Request, Worker},
};
use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::Value;
use kaia_gfx::golden::assert_matches_golden;
use kaia_opds::Catalog;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

const FEED: &[u8] = include_bytes!("fixtures/catalogs/feed.xml");
struct Server {
    url: String,
    hits: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let count = hits.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let quit = stop.clone();
        let thread = std::thread::spawn(move || {
            while !quit.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = listener.accept() else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                stream.set_nonblocking(false).unwrap();
                count.fetch_add(1, Ordering::SeqCst);
                stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
                let mut bytes = Vec::new();
                let mut buf = [0; 2048];
                while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = stream.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    bytes.extend_from_slice(&buf[..n]);
                }
                let request = String::from_utf8_lossy(&bytes);
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let (status, body) = match path {
                    "/auth" if !request.to_lowercase().contains("authorization: basic cmvhzgvyonnly3jlda==") => (401,Vec::new()),
                    "/bad" => (200,b"<html>Not a feed</html>".to_vec()),
                    "/book.epub" => (200,fs::read(repo_root().join("test/juliet.epub")).unwrap()),
                    "/cover" => (200,cover()),
                    "/options" => (200, br#"{"links":[{"rel":"http://opds-spec.org/acquisition/open-access","href":"/book.epub","type":"application/epub+zip"}]}"#.to_vec()),
                    "/page2" => (200,b"<feed xmlns=\"http://www.w3.org/2005/Atom\"><title>More classics</title><link rel=\"previous\" href=\"/feed\" type=\"application/atom+xml\"/></feed>".to_vec()),
                    _ => (200,FEED.to_vec()),
                };
                if path == "/slow" {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        FEED.len()
                    );
                    for byte in FEED {
                        if quit.load(Ordering::Relaxed) {
                            break;
                        }
                        if stream.write_all(&[*byte]).is_err() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                } else {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(&body);
                }
            }
        });
        Self {
            url,
            hits,
            stop,
            thread: Some(thread),
        }
    }
    fn catalog(&self, path: &str) -> Catalog {
        Catalog {
            title: "My local catalog".into(),
            url: format!("{}{path}", self.url),
            username: None,
            password: None,
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn command(app: &mut App<HeadlessBackend>, id: &str) {
    app.command(id, &[]);
    app.process(Vec::new()).unwrap();
}
fn change(app: &mut App<HeadlessBackend>, name: &str, value: &str) {
    app.command(&format!("catalogs:change:{name}"), &[Value::from(value)]);
}
fn idle(app: &mut App<HeadlessBackend>) {
    wait_for(app, "network worker", |a| {
        a.find_rect("catalogs:cancel").is_none()
    });
    app.process(Vec::new()).unwrap();
}
fn open_catalogs(app: &mut App<HeadlessBackend>) {
    command(app, "nav:settings");
    command(app, "settings:network");
    command(app, "settings:catalogs");
}

#[test]
fn catalog_flow_goldens_download_and_open() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let server = Server::start();
    let dir = scratch("catalog-flow");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let suggestions = kaia_opds::default_catalogs();
    store::save(&mut app.services_mut().settings, &suggestions[..2]);
    open_catalogs(&mut app);
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);
    assert_matches_golden(app.frame(), &golden("catalogs_list.png"), 14);
    store::save(&mut app.services_mut().settings, &[server.catalog("/feed")]);
    command(&mut app, "catalogs:browse:0");
    idle(&mut app);
    assert!(app.find_rect("catalogs:entry:0").is_some());
    assert_matches_golden(app.frame(), &golden("catalogs_feed.png"), 14);
    command(&mut app, "catalogs:next");
    idle(&mut app);
    assert!(app.find_rect("catalogs:previous").is_some());
    command(&mut app, "catalogs:previous");
    idle(&mut app);
    command(&mut app, "catalogs:search");
    change(&mut app, "query", "Romeo and Juliet");
    command(&mut app, "catalogs:search-go");
    idle(&mut app);
    command(&mut app, "catalogs:entry:0");
    app.process(Vec::new()).unwrap();
    idle(&mut app);
    assert_eq!(app.top_route(), Some(&Route::CatalogBook));
    assert_matches_golden(app.frame(), &golden("catalogs_book.png"), 14);
    let downloads = dir.join("downloads");
    app.services_mut()
        .settings
        .save(store::DOWNLOADS, Value::from(downloads.to_str().unwrap()));
    command(&mut app, "catalogs:download:0");
    idle(&mut app);
    assert!(app.find_rect("catalogs:open-book").is_some());
    let paths: Vec<_> = fs::read_dir(&downloads)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(paths.len(), 1);
    assert!(app.services().library.book_by_path(&paths[0]).unwrap().is_some());
    command(&mut app, "catalogs:open-book");
    wait_reader(&mut app);
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));
    app.shutdown();
}

#[test]
fn catalog_editor_suggestions_and_plain_password_persist() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("catalog-editor");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    open_catalogs(&mut app);
    command(&mut app, "catalogs:suggestions");
    command(&mut app, "catalogs:suggest:0");
    command(&mut app, "catalogs:save");
    assert_eq!(store::catalogs(&app.services().settings).len(), 1);
    command(&mut app, "catalogs:edit:0");
    change(&mut app, "name", "Home library");
    change(&mut app, "url", "localhost:8080/opds");
    change(&mut app, "username", "reader");
    change(&mut app, "password", "secret");
    command(&mut app, "catalogs:save");
    let saved = kaia_events::Settings::open(&app.services().settings_path());
    let catalogs = store::catalogs(&saved);
    assert_eq!(catalogs[0].password.as_deref(), Some("secret"));
    assert_eq!(catalogs[0].url, "http://localhost:8080/opds");
    command(&mut app, "catalogs:edit:0");
    command(&mut app, "catalogs:delete");
    assert!(store::catalogs(&app.services().settings).is_empty());
    app.shutdown();
}

fn finish(worker: &Worker) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        assert!(Instant::now() < deadline, "worker did not finish");
        if let Some(Reply::Finished(result)) = worker.next() {
            return result;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn bad_feed_auth_failure_and_cancel_use_local_server() {
    let server = Server::start();
    let dir = scratch("catalog-errors");
    for (path, message) in [("/bad", "OPDS feed"), ("/auth", "Authentication failed")] {
        let c = server.catalog(path);
        let w = Worker::start(
            Request::Feed(c.clone(), c.url),
            dir.join("books"),
            dir.join("library.sqlite3"),
            false,
        );
        assert!(finish(&w).unwrap_err().contains(message));
    }
    let mut c = server.catalog("/auth");
    c.username = Some("reader".into());
    c.password = Some("secret".into());
    let w = Worker::start(
        Request::Feed(c.clone(), c.url),
        dir.join("books"),
        dir.join("library.sqlite3"),
        false,
    );
    assert!(finish(&w).is_ok());
    let c = server.catalog("/slow");
    let w = Worker::start(
        Request::Feed(c.clone(), c.url),
        dir.join("books"),
        dir.join("library.sqlite3"),
        false,
    );
    std::thread::sleep(Duration::from_millis(30));
    let start = Instant::now();
    w.cancel();
    assert_eq!(finish(&w).unwrap_err(), "Cancelled.");
    assert!(start.elapsed() < Duration::from_secs(2));
}

fn cover() -> Vec<u8> {
    use kaia_gfx::{Color, PixelFormat, Pixmap, Rect};
    let mut image = Pixmap::new(240, 360, PixelFormat::Rgb24);
    image.fill_rect(Rect::new(0, 0, 240, 360), Color::rgb(31, 111, 92));
    image.fill_rect(Rect::new(16, 16, 208, 328), Color::gray(238));
    image.fill_rect(Rect::new(28, 28, 184, 304), Color::rgb(31, 111, 92));
    for y in [100, 120, 140, 240, 260] {
        image.fill_rect(Rect::new(55, y, 130, 5), Color::gray(238));
    }
    kaia_gfx::png::encode_pixmap(&image).unwrap()
}

fn calibre_send(peer: &mut std::net::TcpStream, opcode: u64, payload: &str) {
    let body = format!("[{opcode},{payload}]");
    write!(peer, "{}{body}", body.len()).unwrap();
}
fn calibre_reply(peer: &mut std::net::TcpStream) -> String {
    let mut length = 0usize;
    let mut byte = [0];
    loop {
        peer.read_exact(&mut byte).unwrap();
        if byte[0] == b'[' {
            break;
        }
        length = length * 10 + usize::from(byte[0] - b'0');
    }
    let mut bytes = vec![0; length];
    bytes[0] = b'[';
    peer.read_exact(&mut bytes[1..]).unwrap();
    String::from_utf8(bytes).unwrap()
}

#[test]
fn calibre_screen_receives_book_and_metadata_then_disconnects() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let connected = Arc::new(AtomicBool::new(false));
    let seen = connected.clone();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        seen.store(true, Ordering::SeqCst);
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        calibre_send(&mut stream, 9, r#"{"passwordChallenge":"salt"}"#);
        assert!(calibre_reply(&mut stream).contains("acceptedExtensions"));
        let book = fs::read(repo_root().join("test/juliet.epub")).unwrap();
        calibre_send(
            &mut stream,
            8,
            &format!(
                r#"{{"lpath":"received.epub","length":{},"metadata":{{"lpath":"received.epub","uuid":"test-1","title":"Romeo and Juliet","authors":["William Shakespeare"],"series":"Classics","series_index":1}}}}"#,
                book.len()
            ),
        );
        assert!(calibre_reply(&mut stream).starts_with("[0,"));
        stream.write_all(&book).unwrap();
        let mut byte = [0];
        assert_eq!(stream.read(&mut byte).unwrap(), 0);
    });
    let dir = scratch("calibre-flow");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let downloads = dir.join("downloads");
    app.services_mut()
        .settings
        .save(store::DOWNLOADS, Value::from(downloads.to_str().unwrap()));
    command(&mut app, "nav:settings");
    command(&mut app, "settings:network");
    command(&mut app, "settings:calibre");
    assert!(!connected.load(Ordering::SeqCst));
    change(&mut app, "address", &address);
    command(&mut app, "catalogs:connect");
    wait_for(&mut app, "Calibre receipt", |a| {
        a.find_rect("catalogs:open-book").is_some()
    });
    assert!(app
        .services()
        .library
        .book_by_path(&downloads.join("received.epub"))
        .unwrap()
        .is_some());
    let metadata = kaia_calibre::read_metadata_cache(&downloads).unwrap();
    assert_eq!(metadata[0]["title"].as_str(), Some("Romeo and Juliet"));
    assert_eq!(metadata[0]["series"].as_str(), Some("Classics"));
    command(&mut app, "catalogs:cancel");
    idle(&mut app);
    peer.join().unwrap();
    app.shutdown();
}

#[test]
fn calibre_cancel_removes_incomplete_book() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let (sent, received) = std::sync::mpsc::channel();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        calibre_send(
            &mut stream,
            8,
            r#"{"lpath":"incomplete.epub","length":1000000,"metadata":{"lpath":"incomplete.epub"}}"#,
        );
        calibre_reply(&mut stream);
        stream.write_all(b"unfinished book").unwrap();
        sent.send(()).unwrap();
        let mut buf = [0; 512];
        let _ = stream.read(&mut buf);
    });
    let dir = scratch("calibre-cancel");
    let root = dir.join("downloads");
    let worker = Worker::start(
        Request::Connect {
            address,
            password: String::new(),
        },
        root.clone(),
        dir.join("library.sqlite3"),
        false,
    );
    received.recv_timeout(Duration::from_secs(5)).unwrap();
    worker.cancel();
    assert_eq!(finish(&worker).unwrap_err(), "Cancelled.");
    peer.join().unwrap();
    assert_eq!(fs::read_dir(root).unwrap().count(), 0);
}

#[test]
fn stalled_feed_cover_and_download_cancel_without_waiting_for_server() {
    for kind in ["feed", "cover", "download"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (ready, received) = std::sync::mpsc::channel();
        let peer = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut buffer = [0; 4096];
            let mut request = Vec::new();
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\nConnection: close\r\n\r\n")
                .unwrap();
            ready.send(()).unwrap();
            let _ = stream.read(&mut buffer);
        });
        let dir = scratch(&format!("stalled-{kind}"));
        let catalog = Catalog {
            title: "Stalled server".into(),
            url: url.clone(),
            username: None,
            password: None,
        };
        let request = match kind {
            "feed" => Request::Feed(catalog, url.clone()),
            "cover" => Request::Cover(catalog, url.clone()),
            _ => Request::Download(
                catalog,
                kaia_opds::parse_feed(FEED, &url).unwrap().entries[0].acquisitions[0].clone(),
            ),
        };
        let root = dir.join("downloads");
        let worker = Worker::start(request, root.clone(), dir.join("library.sqlite3"), false);
        received.recv_timeout(Duration::from_secs(3)).unwrap();
        let start = Instant::now();
        worker.cancel();
        assert_eq!(finish(&worker).unwrap_err(), "Cancelled.");
        assert!(start.elapsed() < Duration::from_secs(1), "{kind}");
        peer.join().unwrap();
        assert!(!root.exists() || fs::read_dir(root).unwrap().count() == 0);
    }
}

#[test]
fn wifi_prompt_and_back_send_no_requests() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let server = Server::start();
    let dir = scratch("catalog-wifi");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    app.services_mut().platform.kind =
        kaia_platform::platform::Kind::Kobo(kaia_platform::device::Model::LibraColour);
    store::save(&mut app.services_mut().settings, &[server.catalog("/feed")]);
    open_catalogs(&mut app);
    command(&mut app, "catalogs:browse:0");
    assert!(app.find_rect("catalogs:wifi").is_some());
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);
    command(&mut app, "catalogs:cancel");
    command(&mut app, "back");
    assert!(app.find_rect("catalogs:add").is_some());
    assert_eq!(server.hits.load(Ordering::SeqCst), 0);
    app.shutdown();
}

#[test]
fn indirect_downloads_are_indexed_and_keep_existing_files() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let server = Server::start();
    let dir = scratch("catalog-indirect");
    let root = dir.join("downloads");
    let database = dir.join("library.sqlite3");
    let catalog = server.catalog("/feed");
    let mut acquisition =
        kaia_opds::parse_feed(FEED, &catalog.url).unwrap().entries[0].acquisitions[0].clone();
    acquisition.indirect = true;
    acquisition.url = format!("{}/options", server.url);
    fs::create_dir_all(&root).unwrap();
    let existing = root.join(acquisition.file_name().unwrap());
    fs::write(&existing, b"Keep this existing book").unwrap();
    for _ in 0..2 {
        let worker = Worker::start(
            Request::Download(catalog.clone(), acquisition.clone()),
            root.clone(),
            database.clone(),
            false,
        );
        assert!(finish(&worker).is_ok());
    }
    assert_eq!(fs::read(&existing).unwrap(), b"Keep this existing book");
    let library = kaia_library::Library::open(&database).unwrap();
    assert_eq!(fs::read_dir(&root).unwrap().count(), 3);
    for path in fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| *p != existing)
    {
        assert!(library.book_by_path(&path).unwrap().is_some());
    }
}
