mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::*;
use kaia_library::{CoverSize, Filter, Sort, SortKey, Status};

/// A temp dir with a `books` root and the database in its own folder.
fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    fs::create_dir_all(&root).unwrap();
    (tmp, root)
}

#[test]
fn first_scan_extracts_metadata_and_covers() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());

    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.files_seen, 4);
    assert_eq!(report.added, 4);
    assert_eq!(report.extracted, 4);
    assert_eq!(report.failed, 0);
    assert!(!report.cancelled);

    let books = lib
        .books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
        .unwrap();
    assert_eq!(books.len(), 4);
    for b in &books {
        assert!(b.extracted, "{} extracted", b.path.display());
        assert!(b.title.as_deref().is_some_and(|t| !t.is_empty()));
        assert!(b.pages.unwrap_or(0) > 0);
        assert_eq!(b.partial_md5.len(), 32);
        assert!(b
            .partial_md5
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }
    let pdf = lib.book_by_path(&files[0]).unwrap().unwrap();
    assert_eq!(pdf.format, "pdf");
    assert_eq!(lib.book_by_path(&files[2]).unwrap().unwrap().format, "epub");

    let small = lib.cover(pdf.id, CoverSize::Small).unwrap().expect("small cover");
    assert_eq!(small.mime, "image/jpeg");
    let (w, h) = jpeg_size(&small.bytes);
    assert!(w <= 160 && h <= 160 && h > 0, "small cover {w}x{h} fits 160 box");
    let large = lib.cover(pdf.id, CoverSize::Large).unwrap().expect("large cover");
    assert_eq!(large.mime, "image/jpeg");
    let (w, h) = jpeg_size(&large.bytes);
    assert!(
        w <= 1264 && h <= 1680 && h > 480,
        "large cover {w}x{h} fits the 1264 x 1680 box"
    );
    assert!(w > 160 || h > 160, "large is bigger than small");
}

#[test]
fn unchanged_rescan_does_no_extraction() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let second = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(second.extracted, 0, "no document was opened");
    assert_eq!(second.unchanged, 4);
    assert_eq!(second.added + second.changed + second.moved + second.removed, 0);
}

#[test]
fn changed_file_is_extracted_again_and_keeps_its_id() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&files[2]).unwrap().unwrap().id;
    lib.record_open(id, 0.25).unwrap();

    // A different file at the same path: size and content change.
    fs::copy(repo_test_file("juliet.epub"), &files[2]).unwrap();
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.changed, 1);
    assert_eq!(report.extracted, 1);
    let after = lib.book_by_path(&files[2]).unwrap().unwrap();
    assert_eq!(after.id, id);
    assert!((after.percent - 0.25).abs() < 1e-9, "progress kept");
}

#[test]
fn moved_file_keeps_progress_collections_and_history() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let before = lib.book_by_path(&files[2]).unwrap().unwrap();
    lib.record_open(before.id, 0.4).unwrap();
    lib.set_status(before.id, Status::Reading).unwrap();
    lib.add_to_collection(before.id, "Favorites").unwrap();

    let sub = root.join("moved");
    fs::create_dir_all(&sub).unwrap();
    let moved_to = sub.join("leaves-renamed.epub");
    fs::rename(&files[2], &moved_to).unwrap();

    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.moved, 1, "found by content");
    assert_eq!(report.added, 0);
    assert_eq!(report.extracted, 0, "moved book is not extracted again");
    assert_eq!(report.removed, 0);

    assert!(lib.book_by_path(&files[2]).unwrap().is_none());
    let after = lib.book_by_path(&moved_to).unwrap().expect("found at new path");
    assert_eq!(after.id, before.id);
    assert!((after.percent - 0.4).abs() < 1e-9);
    assert_eq!(after.status, Status::Reading);
    assert_eq!(after.title, before.title);
    let coll = lib.collections().unwrap();
    assert_eq!(coll.len(), 1);
    assert_eq!(coll[0].books, 1);
    assert_eq!(
        lib.recent(10).unwrap()[0].id,
        before.id,
        "history follows the move"
    );
}

#[test]
fn case_only_alias_keeps_book_data_when_old_path_still_exists() {
    let (tmp, root) = setup();
    let old = copy_fixture("leaves.epub", &root, "Book.epub");
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&old).unwrap().unwrap().id;
    lib.record_open(id, 0.4).unwrap();
    lib.add_to_collection(id, "Favorites").unwrap();
    let new = root.join("book.epub");
    if fs::hard_link(&old, &new).is_err() {
        fs::rename(&old, &new).unwrap();
    }
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.added, 0);
    assert_eq!(lib.book_by_path(&new).unwrap().unwrap().id, id);
    assert_eq!(lib.collections().unwrap()[0].books, 1);
    assert_eq!(lib.recent(10).unwrap()[0].id, id);
}

#[test]
fn empty_root_keeps_existing_books_and_relations() {
    let (tmp, root) = setup();
    let file = copy_fixture("leaves.epub", &root, "leaves.epub");
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&file).unwrap().unwrap().id;
    lib.record_open(id, 0.5).unwrap();
    lib.add_to_collection(id, "Favorites").unwrap();
    fs::remove_file(&file).unwrap();
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.removed, 0);
    assert_eq!(lib.book(id).unwrap().unwrap().percent, 0.5);
    assert_eq!(lib.collections().unwrap()[0].books, 1);
    assert_eq!(lib.recent(10).unwrap().len(), 1);
}

#[test]
fn vanished_book_is_hidden_then_returns_with_its_data() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&files[1]).unwrap().unwrap().id;
    lib.record_open(id, 0.5).unwrap();
    lib.add_to_collection(id, "Favorites").unwrap();
    fs::remove_file(&files[1]).unwrap();
    scan(&mut lib, std::slice::from_ref(&root));
    assert!(lib.book(id).unwrap().is_none());
    assert_eq!(lib.collections().unwrap()[0].books, 0);
    assert!(lib.recent(10).unwrap().is_empty());
    let raw = raw_connection(tmp.path());
    let missing: Option<i64> = raw
        .query_row("SELECT missing_since FROM books WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(missing.is_some());
    fs::copy(repo_test_file("tall.pdf"), &files[1]).unwrap();
    scan(&mut lib, std::slice::from_ref(&root));
    let book = lib.book(id).unwrap().unwrap();
    assert_eq!(book.percent, 0.5);
    assert_eq!(lib.collections().unwrap()[0].books, 1);
    assert_eq!(lib.recent(10).unwrap()[0].id, id);
}

#[test]
fn missing_book_returns_at_new_path_by_content() {
    let (tmp, root) = setup();
    let old = copy_fixture("leaves.epub", &root, "old.epub");
    let _other = copy_fixture("juliet.epub", &root, "other.epub");
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&old).unwrap().unwrap().id;
    lib.record_open(id, 0.3).unwrap();
    fs::remove_file(&old).unwrap();
    scan(&mut lib, std::slice::from_ref(&root));
    let new = copy_fixture("leaves.epub", &root, "new.epub");
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.moved, 1);
    assert_eq!(lib.book_by_path(&new).unwrap().unwrap().id, id);
    assert_eq!(lib.book(id).unwrap().unwrap().percent, 0.3);
}

#[test]
fn dropped_cover_is_extracted_after_version_three_migration() {
    let (tmp, root) = setup();
    let file = copy_fixture("2col.pdf", &root, "book.pdf");
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&file).unwrap().unwrap().id;
    drop(lib);
    let raw = raw_connection(tmp.path());
    raw.execute(
        "UPDATE covers SET small = NULL, large = NULL WHERE book_id = ?1",
        [id],
    )
    .unwrap();
    raw.execute_batch(
        "DROP INDEX books_missing_since;
         ALTER TABLE books DROP COLUMN missing_since;
         DROP INDEX collections_name_key;
         ALTER TABLE collections DROP COLUMN name_key;
         ALTER TABLE collections DROP COLUMN position;
         DROP TABLE recent_searches;
         DROP TABLE book_tags;
         DROP TABLE tags;",
    )
    .unwrap();
    raw.pragma_update(None, "user_version", 2).unwrap();
    drop(raw);
    let mut lib = open_library(tmp.path());
    assert!(!lib.book(id).unwrap().unwrap().extracted);
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.extracted, 1);
    assert!(lib.cover(id, CoverSize::Small).unwrap().is_some());
}

#[test]
fn fat_timestamp_shift_updates_mtime_without_extraction() {
    let (tmp, root) = setup();
    let file = copy_fixture("leaves.epub", &root, "leaves.epub");
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let book = lib.book_by_path(&file).unwrap().unwrap();
    raw_connection(tmp.path())
        .execute(
            "UPDATE books SET mtime = ?1 WHERE id = ?2",
            rusqlite::params![book.mtime - 3600, book.id],
        )
        .unwrap();
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.extracted, 0);
    assert_eq!(lib.book(book.id).unwrap().unwrap().mtime, book.mtime);
}

#[test]
fn deleted_file_is_hidden_after_a_populated_scan() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let gone = lib.book_by_path(&files[1]).unwrap().unwrap().id;
    lib.record_open(gone, 0.5).unwrap();
    lib.add_to_collection(gone, "Favorites").unwrap();

    fs::remove_file(&files[1]).unwrap();
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.removed, 0);
    assert!(lib.book(gone).unwrap().is_none());
    assert_eq!(
        lib.books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        lib.collections().unwrap()[0].books,
        0,
        "missing book is hidden from collection counts"
    );
    assert!(
        lib.recent(10).unwrap().is_empty(),
        "missing book is hidden from recent books"
    );
}

#[test]
fn missing_root_removes_nothing() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    // The drive is unplugged: the root does not exist.
    let unplugged = tmp.path().join("usb");
    let report = scan(&mut lib, std::slice::from_ref(&unplugged));
    assert_eq!(report.removed, 0);
    assert_eq!(
        lib.books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
            .unwrap()
            .len(),
        4
    );

    // An empty root may be an unavailable mount, so its books stay.
    fs::create_dir_all(&unplugged).unwrap();
    let report = scan(&mut lib, &[root.clone(), unplugged.clone()]);
    assert_eq!(report.removed, 0, "root with the books is still there");
    fs::remove_dir_all(&root).unwrap();
    fs::create_dir_all(&root).unwrap();
    let report = scan(&mut lib, &[root.clone(), unplugged.clone()]);
    assert_eq!(report.removed, 0);
    assert_eq!(
        lib.books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn unreadable_subfolder_makes_its_root_incomplete() {
    use std::os::unix::fs::PermissionsExt;
    let (tmp, root) = setup();
    fixture_set(&root);
    let locked = root.join("locked");
    fs::create_dir_all(&locked).unwrap();
    let inside = copy_fixture("2col.pdf", &locked, "hidden.pdf");
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    assert!(lib.book_by_path(&inside).unwrap().is_some());

    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    // Running as root ignores the mode bits, so skip the check when the read succeeds.
    if fs::read_dir(&locked).is_ok() {
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    fs::remove_file(root.join("two-column.pdf")).unwrap();
    let report = scan(&mut lib, std::slice::from_ref(&root));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(report.removed, 0, "an incomplete root keeps its books");
    assert!(lib.book_by_path(&inside).unwrap().is_some());
}

#[test]
fn corrupt_file_is_recorded_once_and_retried_only_after_it_changes() {
    let (tmp, root) = setup();
    let broken = root.join("broken.epub");
    fs::write(&broken, b"this is not a zip archive, nor an epub").unwrap();
    let mut lib = open_library(tmp.path());

    let first = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(first.extracted, 1);
    assert_eq!(first.failed, 1);
    let b = lib.book_by_path(&broken).unwrap().unwrap();
    assert!(!b.extracted);
    assert!(
        b.failure.as_deref().is_some_and(|m| !m.is_empty()),
        "failure reason stored"
    );
    assert_eq!(b.title.as_deref(), Some("broken"), "file name used as the title");
    assert!(lib.cover(b.id, CoverSize::Small).unwrap().is_none());

    let second = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(second.extracted, 0, "failed book is not opened again");
    assert_eq!(second.failed, 0);

    // A fixed file at the same path is extracted.
    fs::copy(repo_test_file("leaves.epub"), &broken).unwrap();
    let third = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(third.extracted, 1);
    assert_eq!(third.failed, 0);
    let b = lib.book_by_path(&broken).unwrap().unwrap();
    assert!(b.extracted);
    assert!(b.failure.is_none());
}

#[test]
fn interrupted_extraction_is_retried_then_given_up() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&files[0]).unwrap().unwrap().id;

    // Simulate a process killed inside the engine: the attempt was counted but
    // the book never settled.
    let raw = raw_connection(tmp.path());
    raw.execute("UPDATE books SET extracted = 0, attempts = 1 WHERE id = ?1", [id])
        .unwrap();
    let retry = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(retry.extracted, 1, "interrupted book is retried");
    assert!(lib.book(id).unwrap().unwrap().extracted);

    raw.execute("UPDATE books SET extracted = 0, attempts = 3 WHERE id = ?1", [id])
        .unwrap();
    let giving_up = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(
        giving_up.extracted, 0,
        "after three attempts the engine is not called"
    );
    assert_eq!(giving_up.failed, 1);
    let b = lib.book(id).unwrap().unwrap();
    assert!(b.failure.as_deref().unwrap().contains("interrupted"));
}

#[test]
fn extraction_budget_marks_slow_books_as_failed() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let mut lib = open_library(tmp.path());
    lib.set_extract_budget(Duration::ZERO);
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.extracted, 4);
    assert_eq!(report.failed, 4);
    let books = lib
        .books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
        .unwrap();
    for b in &books {
        assert!(
            b.failure.as_deref().unwrap().contains("longer than"),
            "{:?}",
            b.failure
        );
    }

    // The failure is kept until the file changes, even with a larger budget.
    lib.set_extract_budget(Duration::from_secs(10));
    let again = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(again.extracted, 0);
}

#[test]
fn cancelled_scan_stops_and_removes_nothing() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    fs::remove_file(&files[0]).unwrap();

    let cancel = AtomicBool::new(true);
    let report = lib.scan(std::slice::from_ref(&root), |_| {}, &cancel).unwrap();
    assert!(report.cancelled);
    assert_eq!(report.removed, 0);
    assert_eq!(report.extracted, 0);

    // Cancel after the first book: the books already done are kept.
    fs::copy(repo_test_file("2col.pdf"), root.join("extra-1.pdf")).unwrap();
    fs::copy(repo_test_file("2col.pdf"), root.join("extra-2.pdf")).unwrap();
    let cancel = AtomicBool::new(false);
    let report = lib
        .scan(
            std::slice::from_ref(&root),
            |p| {
                if p.done == 1 {
                    cancel.store(true, Ordering::Relaxed)
                }
            },
            &cancel,
        )
        .unwrap();
    assert!(report.cancelled);
    assert!(report.added + report.changed <= 2);
}

#[test]
fn progress_reports_each_book() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let mut lib = open_library(tmp.path());
    let cancel = AtomicBool::new(false);
    let mut seen = Vec::new();
    lib.scan(
        std::slice::from_ref(&root),
        |p| seen.push((p.done, p.total)),
        &cancel,
    )
    .unwrap();
    assert_eq!(seen, vec![(0, 4), (1, 4), (2, 4), (3, 4)]);
}

#[test]
fn hidden_files_and_other_types_are_not_books() {
    let (tmp, root) = setup();
    fixture_set(&root);
    fs::write(root.join(".hidden.epub"), b"x").unwrap();
    fs::write(root.join("._resource.pdf"), b"x").unwrap();
    fs::write(root.join("notes.md"), b"plain text").unwrap();
    fs::write(root.join("cover.jpg"), b"x").unwrap();
    let mut lib = open_library(tmp.path());
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.files_seen, 4);
}

#[test]
fn nested_roots_index_each_file_once() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let inner = root.join("sub");
    fs::create_dir_all(&inner).unwrap();
    copy_fixture("2col.pdf", &inner, "inner.pdf");
    let mut lib = open_library(tmp.path());
    let report = scan(&mut lib, &[root.clone(), inner.clone()]);
    assert_eq!(report.files_seen, 5);
    assert_eq!(report.added, 5);
}

#[test]
fn partial_md5_matches_the_free_function() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let b = lib.book_by_path(&files[3]).unwrap().unwrap();
    assert_eq!(b.partial_md5, kaia_library::partial_md5(&files[3]).unwrap());
}
