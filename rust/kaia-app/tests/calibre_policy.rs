//! Calibre transfers through the real catalog worker and a local TCP server.
//! These tests cover the app side: the upload and read rules against a real
//! library index, and the message the user sees. The protocol rules are unit
//! tests in kaia-calibre.

mod common;
use common::{repo_root, scratch};
use kaia_app::surfaces::catalogs::worker::{Reply, Request, Worker};
use kaia_library::Library;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// What the user sees when the app refuses a file that the server sent.
const REFUSED: &str = "Calibre sent a file this app does not accept.";

fn send(stream: &mut TcpStream, opcode: u64, payload: &str) {
    let body = format!("[{opcode},{payload}]");
    write!(stream, "{}{body}", body.len()).unwrap();
}

/// Reads one reply. Returns its opcode and its text.
fn reply(stream: &mut TcpStream) -> (u64, String) {
    let mut length = 0usize;
    let mut byte = [0];
    loop {
        stream.read_exact(&mut byte).unwrap();
        if byte[0] == b'[' {
            break;
        }
        length = length * 10 + usize::from(byte[0] - b'0');
    }
    let mut bytes = vec![0; length];
    bytes[0] = b'[';
    stream.read_exact(&mut bytes[1..]).unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let opcode = text[1..].split(',').next().unwrap().parse().unwrap();
    (opcode, text)
}

/// A Calibre server on a local port. The app connects, and `script` runs after
/// the handshake.
fn calibre_server(
    root: &Path,
    database: &Path,
    script: impl FnOnce(&mut TcpStream) + Send + 'static,
) -> (Worker, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let peer = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
        send(&mut stream, 9, r#"{"passwordChallenge":"salt"}"#);
        assert!(reply(&mut stream).1.contains("acceptedExtensions"));
        script(&mut stream);
    });
    let worker = Worker::start(
        Request::Connect {
            address,
            password: String::new(),
        },
        root.to_path_buf(),
        database.to_path_buf(),
        false,
    );
    (worker, peer)
}

/// Waits for the worker to finish and returns its result.
fn finish(worker: &Worker) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        assert!(Instant::now() < deadline, "worker did not finish");
        if let Some(Reply::Finished(result)) = worker.next() {
            return result;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Sends a four-byte book named `lpath`. The app must answer with an error and
/// never with the OK that acknowledges the book. Returns the error text.
fn refused_upload(stream: &mut TcpStream, lpath: &str) -> String {
    send(
        stream,
        8,
        &format!(r#"{{"lpath":"{lpath}","length":4,"metadata":{{"lpath":"{lpath}","uuid":"x"}}}}"#),
    );
    let (op, text) = reply(stream);
    assert_eq!(op, 20, "{lpath}: refused with an error, not acknowledged");
    // The app closes the connection after a refusal, so this write may fail.
    let _ = stream.write_all(b"data");
    text
}

/// A settings file in a hidden folder. The read gets the NOOP that a missing
/// file gets, and the upload is refused before any byte is written.
#[test]
fn settings_are_neither_read_nor_replaced() {
    let dir = scratch("calibre-policy-settings");
    let root = dir.join("downloads");
    fs::create_dir_all(root.join(".kaia")).unwrap();
    let settings = root.join(".kaia/settings.json");
    let original = br#"{"opds":[{"password":"secret"}]}"#;
    fs::write(&settings, original).unwrap();
    let (worker, peer) = calibre_server(&root, &dir.join("library.sqlite3"), |stream| {
        send(stream, 14, r#"{"lpath":".kaia/settings.json"}"#);
        assert_eq!(
            reply(stream).0,
            12,
            "a refused read gets the NOOP that a missing file gets"
        );
        refused_upload(stream, ".kaia/settings.json");
    });
    assert_eq!(finish(&worker).unwrap_err(), REFUSED);
    peer.join().unwrap();
    assert_eq!(fs::read(&settings).unwrap(), original);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn kobo_database_is_not_served() {
    let dir = scratch("calibre-policy-kobo");
    let root = dir.join("downloads");
    fs::create_dir_all(root.join(".kobo")).unwrap();
    fs::write(
        root.join(".kobo/KoboReader.sqlite"),
        b"SQLite format 3\0reading state",
    )
    .unwrap();
    let (worker, peer) = calibre_server(&root, &dir.join("library.sqlite3"), |stream| {
        send(stream, 14, r#"{"lpath":".kobo/KoboReader.sqlite"}"#);
        assert_eq!(
            reply(stream).0,
            12,
            "the read gets the NOOP that a missing file gets"
        );
        // The eject OK comes next. File bytes would have come first.
        send(stream, 12, r#"{"ejecting":true}"#);
        assert_eq!(reply(stream).0, 0);
    });
    assert!(finish(&worker).is_ok());
    peer.join().unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn executables_and_scripts_are_refused_before_any_write() {
    for name in ["Run.exe", "setup.sh"] {
        let dir = scratch(&format!("calibre-policy-{name}"));
        let root = dir.join("downloads");
        let (worker, peer) = calibre_server(&root, &dir.join("library.sqlite3"), move |stream| {
            let text = refused_upload(stream, name);
            assert!(text.contains("The file type is not supported."), "{text}");
        });
        assert_eq!(finish(&worker).unwrap_err(), REFUSED, "{name}");
        peer.join().unwrap();
        // The connection creates the folder, and the refused book leaves it empty.
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0, "{name}");
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn hidden_sidecar_and_driver_names_are_refused_and_create_nothing() {
    for name in [
        ".adds/kaia/kaia-app",
        "Book.sdr/metadata.epub.lua",
        "metadata.calibre",
    ] {
        let dir = scratch(&format!("calibre-policy-names-{}", name.replace(['/', '.'], "-")));
        let root = dir.join("downloads");
        let (worker, peer) = calibre_server(&root, &dir.join("library.sqlite3"), move |stream| {
            refused_upload(stream, name);
        });
        assert_eq!(finish(&worker).unwrap_err(), REFUSED, "{name}");
        peer.join().unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0, "{name}");
        fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn links_in_the_book_path_are_not_followed() {
    let dir = scratch("calibre-policy-link");
    let root = dir.join("downloads");
    let outside = dir.join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("Shelf")).unwrap();
    let (worker, peer) = calibre_server(&root, &dir.join("library.sqlite3"), |stream| {
        refused_upload(stream, "Shelf/Book.epub");
    });
    assert_eq!(finish(&worker).unwrap_err(), REFUSED);
    peer.join().unwrap();
    assert!(!outside.join("Book.epub").exists());
    fs::remove_dir_all(dir).unwrap();
}

/// A book is stored and indexed. The same book sent again replaces the first copy.
#[test]
fn epub_upload_and_resend_still_work() {
    let dir = scratch("calibre-policy-upload");
    let root = dir.join("downloads");
    let database = dir.join("library.sqlite3");
    let first = fs::read(repo_root().join("test/juliet.epub")).unwrap();
    let second = fs::read(repo_root().join("test/leaves.epub")).unwrap();
    let sent = [first, second.clone()];
    let (worker, peer) = calibre_server(&root, &database, move |stream| {
        for book in sent {
            send(
                stream,
                8,
                &format!(
                    r#"{{"lpath":"Author/Juliet.epub","length":{},"metadata":{{"lpath":"Author/Juliet.epub","uuid":"juliet","title":"Romeo and Juliet"}}}}"#,
                    book.len()
                ),
            );
            assert_eq!(reply(stream).0, 0, "the book is acknowledged");
            stream.write_all(&book).unwrap();
        }
        send(stream, 12, r#"{"ejecting":true}"#);
        assert_eq!(reply(stream).0, 0);
    });
    assert!(finish(&worker).is_ok());
    peer.join().unwrap();
    let stored = root.join("Author/Juliet.epub");
    assert_eq!(
        fs::read(&stored).unwrap(),
        second,
        "the re-send replaced the first copy"
    );
    let library = Library::open(&database).unwrap();
    assert!(library
        .book_by_path(&fs::canonicalize(&stored).unwrap())
        .unwrap()
        .is_some());
    assert_eq!(kaia_calibre::read_metadata_cache(&root).unwrap().len(), 1);
    fs::remove_dir_all(dir).unwrap();
}

/// A book that the library indexes can be read and replaced even when the driver
/// list does not name it. A file that the library does not index is neither
/// read nor replaced.
#[test]
fn indexed_books_are_read_and_replaced_and_unindexed_files_are_not() {
    let dir = scratch("calibre-policy-indexed");
    let root = dir.join("downloads");
    let database = dir.join("library.sqlite3");
    fs::create_dir_all(&root).unwrap();
    let juliet = fs::read(repo_root().join("test/juliet.epub")).unwrap();
    let leaves = fs::read(repo_root().join("test/leaves.epub")).unwrap();
    fs::write(root.join("Manual.epub"), &juliet).unwrap();
    fs::write(root.join("Loose.txt"), b"not indexed").unwrap();
    // The library scan indexes Manual.epub. The driver list stays empty.
    let mut library = Library::open(&database).unwrap();
    library
        .scan(std::slice::from_ref(&root), |_| {}, &AtomicBool::new(false))
        .unwrap();
    drop(library);

    let expected_length = juliet.len();
    let replacement = leaves.clone();
    let (worker, peer) = calibre_server(&root, &database, move |stream| {
        send(stream, 14, r#"{"lpath":"Manual.epub"}"#);
        let (op, text) = reply(stream);
        assert_eq!(op, 0, "the indexed book is served");
        assert!(
            text.contains(&format!("\"fileLength\":{expected_length}")),
            "{text}"
        );
        let mut bytes = vec![0; expected_length];
        stream.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, juliet);

        send(
            stream,
            8,
            &format!(
                r#"{{"lpath":"Manual.epub","length":{},"metadata":{{"lpath":"Manual.epub","uuid":"manual"}}}}"#,
                replacement.len()
            ),
        );
        assert_eq!(reply(stream).0, 0, "the indexed book may be replaced");
        stream.write_all(&replacement).unwrap();

        send(stream, 14, r#"{"lpath":"Loose.txt"}"#);
        assert_eq!(reply(stream).0, 12, "an unindexed file is not served");
        send(stream, 12, r#"{"ejecting":true}"#);
        assert_eq!(reply(stream).0, 0);
    });
    assert!(finish(&worker).is_ok());
    peer.join().unwrap();
    assert_eq!(fs::read(root.join("Manual.epub")).unwrap(), leaves);
    assert_eq!(fs::read(root.join("Loose.txt")).unwrap(), b"not indexed");

    let (worker, peer) = calibre_server(&root, &database, |stream| {
        refused_upload(stream, "Loose.txt");
    });
    assert_eq!(finish(&worker).unwrap_err(), REFUSED);
    peer.join().unwrap();
    assert_eq!(fs::read(root.join("Loose.txt")).unwrap(), b"not indexed");
    fs::remove_dir_all(dir).unwrap();
}
