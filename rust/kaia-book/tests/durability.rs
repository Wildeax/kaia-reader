mod common;

use std::fs::{self, File};
use std::process::Command;
use std::time::{Duration, Instant};

use kaia_book::{BookError, BookSession, Location};

fn fixture() -> (tempfile::TempDir, tempfile::TempDir, std::path::PathBuf) {
    let work = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let book = common::copy_into(&common::repo_file("tall.pdf"), work.path());
    (work, data, book)
}

#[test]
fn metadata_and_bookmark_changes_save_before_close() {
    let (_work, data, book) = fixture();
    let mut s = common::open(&book, &data);
    s.set_status(Some("reading")).unwrap();
    s.set_rating(4).unwrap();
    assert!(s.toggle_bookmark().unwrap());
    let state = fs::read_to_string(s.state_path()).unwrap();
    assert!(state.contains("reading"));
    assert!(state.contains("rating"));
    assert!(state.contains("annotations"));
    assert!(!s.is_dirty());
}

#[test]
fn highlight_edits_and_deletes_save_before_close() {
    let work = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let book = common::copy_into(&common::repo_file("paper.pdf"), work.path());
    let mut s = common::open(&book, &data);
    let (_, word) = common::first_word(&mut s, kaia_book::Size { w: 900, h: 1200 });
    let id = s
        .add_highlight(&word, kaia_book::Drawer::Lighten, "yellow", None)
        .unwrap();
    assert!(fs::read_to_string(s.state_path()).unwrap().contains(&word.text));
    s.edit_note(id, Some("edited")).unwrap();
    assert!(fs::read_to_string(s.state_path()).unwrap().contains("edited"));
    s.delete_annotation(id).unwrap();
    assert!(!fs::read_to_string(s.state_path()).unwrap().contains("edited"));
}

#[test]
fn position_save_waits_five_seconds_and_only_writes_changed_position() {
    let work = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let book = common::copy_into(&common::repo_file("2col.pdf"), work.path());
    let mut s = common::open(&book, &data);
    s.save().unwrap();
    let old = fs::read(s.state_path()).unwrap();
    s.go_to(Location::Page(1)).unwrap();
    assert!(!s.maybe_save(Instant::now()).unwrap());
    assert_eq!(fs::read(s.state_path()).unwrap(), old);
    assert!(s.maybe_save(Instant::now() + Duration::from_secs(6)).unwrap());
    let new = fs::read(s.state_path()).unwrap();
    assert_ne!(new, old);
    assert!(!s.maybe_save(Instant::now() + Duration::from_secs(12)).unwrap());
}

#[test]
fn same_process_cannot_open_one_book_twice() {
    let (_work, data, book) = fixture();
    let first = common::open(&book, &data);
    common::init_engine();
    let error = BookSession::open(&book, common::options(&data)).err().unwrap();
    assert!(matches!(error, BookError::AlreadyOpen(_)), "{error:?}");
    drop(first);
    common::open(&book, &data);
}

#[test]
fn process_lock_holder() {
    let Ok(book) = std::env::var("KAIA_BOOK_TEST_BOOK") else {
        return;
    };
    let data = std::path::PathBuf::from(std::env::var("KAIA_BOOK_TEST_DATA").unwrap());
    let ready = std::path::PathBuf::from(std::env::var("KAIA_BOOK_TEST_READY").unwrap());
    let stop = std::path::PathBuf::from(std::env::var("KAIA_BOOK_TEST_STOP").unwrap());
    common::init_engine();
    let session = BookSession::open(book, kaia_book::OpenOptions::new(data)).unwrap();
    fs::write(ready, b"ready").unwrap();
    for _ in 0..500 {
        if stop.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(session);
    panic!("parent did not release the lock holder");
}

#[test]
fn another_process_opens_read_only_while_lock_is_held() {
    let (_work, data, book) = fixture();
    let ready = data.path().join("ready");
    let stop = data.path().join("stop");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("process_lock_holder")
        .env("KAIA_BOOK_TEST_BOOK", &book)
        .env("KAIA_BOOK_TEST_DATA", data.path())
        .env("KAIA_BOOK_TEST_READY", &ready)
        .env("KAIA_BOOK_TEST_STOP", &stop)
        .spawn()
        .unwrap();
    for _ in 0..500 {
        if ready.exists() {
            break;
        }
        assert!(child.try_wait().unwrap().is_none(), "lock holder exited early");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(ready.exists());
    let mut second = common::open(&book, &data);
    assert!(matches!(
        second.state_issue(),
        Some(kaia_book::StateIssue::Locked)
    ));
    assert!(matches!(second.save(), Err(BookError::ReadOnly(_))));
    fs::write(stop, b"stop").unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn newer_state_is_read_only_and_untouched() {
    let (_work, data, book) = fixture();
    let md5 = kaia_book::partial_md5(&book).unwrap();
    let dir = data.path().join("kaia/books");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{md5}.json"));
    let original = format!(
        r#"{{"format":"kaia-book-state","version":999,"doc":{{"partial_md5":"{md5}","file_size":1}}}}"#
    );
    fs::write(&path, &original).unwrap();
    let mut s = common::open(&book, &data);
    assert!(s.state_issue().is_some());
    assert!(matches!(s.save(), Err(BookError::ReadOnly(_))));
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn corrupt_state_is_preserved_and_recovery_waits_for_a_change() {
    let (_work, data, book) = fixture();
    let md5 = kaia_book::partial_md5(&book).unwrap();
    let dir = data.path().join("kaia/books");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{md5}.json"));
    fs::write(&path, b"{broken").unwrap();
    let mut s = common::open(&book, &data);
    let recovered = dir.join(format!("{md5}.recovered.json"));
    assert!(!recovered.exists());
    assert_eq!(fs::read(&path).unwrap(), b"{broken");
    s.set_rating(2).unwrap();
    assert!(recovered.exists());
    assert_eq!(fs::read(&path).unwrap(), b"{broken");
}

#[test]
fn transient_state_read_error_fails_open_without_renaming() {
    let (_work, data, book) = fixture();
    let md5 = kaia_book::partial_md5(&book).unwrap();
    let dir = data.path().join("kaia/books");
    fs::create_dir_all(dir.join(format!("{md5}.json"))).unwrap();
    common::init_engine();
    let error = BookSession::open(&book, common::options(&data)).err().unwrap();
    assert!(matches!(error, BookError::Io { .. }), "{error:?}");
    assert!(dir.join(format!("{md5}.json")).is_dir());
}

#[test]
fn locked_sidecar_import_opens_unsaved_and_reports_error() {
    let (work, data, book) = fixture();
    let sidecar = work.path().join("tall.sdr");
    fs::create_dir_all(&sidecar).unwrap();
    fs::write(
        sidecar.join("metadata.pdf.lua"),
        b"return { [\"last_page\"] = 1 }",
    )
    .unwrap();
    let md5 = kaia_book::partial_md5(&book).unwrap();
    let dir = data.path().join("kaia/books");
    fs::create_dir_all(&dir).unwrap();
    let lock = File::create(dir.join(format!("{md5}.lock"))).unwrap();
    lock.try_lock().unwrap();
    common::init_engine();
    let mut opts = common::options(&data);
    opts.sidecars = vec![kaia_book::SidecarLocation::BesideBook];
    let s = BookSession::open(&book, opts).unwrap();
    assert!(s.import_report().is_some());
    assert!(s.is_dirty());
    assert!(s.save_error().is_some());
    assert!(!s.state_path().exists());
}

#[test]
fn changed_book_migrates_state_and_reports_reanchoring() {
    let (_work, data, book) = fixture();
    let old_md5 = kaia_book::partial_md5(&book).unwrap();
    {
        let mut s = common::open(&book, &data);
        s.set_status(Some("reading")).unwrap();
    }
    let old = data.path().join(format!("kaia/books/{old_md5}.json"));
    let mut bytes = fs::read(&book).unwrap();
    bytes[100] ^= 1;
    fs::write(&book, bytes).unwrap();
    let new_md5 = kaia_book::partial_md5(&book).unwrap();
    assert_ne!(old_md5, new_md5);
    let s = common::open(&book, &data);
    assert_eq!(s.status(), Some("reading"));
    assert!(s.migration_report().is_some());
    assert!(data.path().join(format!("kaia/books/{new_md5}.json")).exists());
    assert!(!old.exists());
    assert!(data
        .path()
        .join(format!("kaia/books/{old_md5}.json.migrated"))
        .exists());
}

#[test]
fn file_change_stays_flagged_until_acknowledged() {
    let (_work, data, book) = fixture();
    {
        let mut s = common::open(&book, &data);
        s.save().unwrap();
    }
    let mut bytes = fs::read(&book).unwrap();
    bytes.extend_from_slice(b"\n% size changed\n");
    fs::write(&book, bytes).unwrap();
    {
        let mut s = common::open(&book, &data);
        assert!(s.state_is_stale());
        s.set_rating(3).unwrap();
    }
    let mut s = common::open(&book, &data);
    assert!(s.state_is_stale());
    s.acknowledge_file_change().unwrap();
    assert!(!s.state_is_stale());
}
