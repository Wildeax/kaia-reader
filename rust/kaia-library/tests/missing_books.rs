//! Missing books are kept, never purged. A book removed from disk stays in the
//! index with its history and collections, is hidden from the views while it is
//! missing, and comes back under the same id with its reading data.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use common::*;
use kaia_library::{Filter, Library, Sort, SortKey, Status};

const DAY: i64 = 24 * 60 * 60;
/// The clock starts here, so every stamp in a test is a known time.
const START: i64 = 1_700_000_000;

/// Opens a library whose clock the test moves through the returned atomic.
fn open_with_clock(dir: &Path) -> (Library, Arc<AtomicI64>) {
    let mut lib = open_library(dir);
    let clock = Arc::new(AtomicI64::new(START));
    let source = Arc::clone(&clock);
    lib.set_clock(move || source.load(Ordering::SeqCst));
    (lib, clock)
}

fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    fs::create_dir_all(&root).unwrap();
    (tmp, root)
}

/// The `missing_since` column of a row, read on a second connection.
fn missing_since(dir: &Path, id: i64) -> Option<i64> {
    raw_connection(dir)
        .query_row("SELECT missing_since FROM books WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
}

/// A count of rows for one book, from a query with one `?1` parameter.
fn rows_for(dir: &Path, sql: &str, id: i64) -> i64 {
    raw_connection(dir).query_row(sql, [id], |r| r.get(0)).unwrap()
}

fn all_books(lib: &Library) -> usize {
    lib.books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
        .unwrap()
        .len()
}

#[test]
fn removed_book_is_kept_past_thirty_days_and_comes_back_with_its_state() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let (mut lib, clock) = open_with_clock(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let id = lib.book_by_path(&files[2]).unwrap().unwrap().id;
    lib.record_open(id, 0.4).unwrap();
    lib.set_status(id, Status::Complete).unwrap();
    lib.add_to_collection(id, "Favorites").unwrap();
    let history_rows = rows_for(tmp.path(), "SELECT COUNT(*) FROM history WHERE book_id = ?1", id);
    assert_eq!(history_rows, 1);

    // The file is removed. A scan one day later marks the row missing.
    fs::remove_file(&files[2]).unwrap();
    clock.store(START + DAY, Ordering::SeqCst);
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.removed, 0);
    assert_eq!(
        missing_since(tmp.path(), id),
        Some(START + DAY),
        "the row is stamped with the scan time"
    );
    assert!(lib.book(id).unwrap().is_none(), "a missing book is hidden");
    assert_eq!(all_books(&lib), 3, "the other three books still show");
    assert_eq!(
        lib.collections().unwrap()[0].books,
        0,
        "the collection count hides it"
    );
    assert!(lib.recent(10).unwrap().is_empty(), "recent books hide it");

    // Thirty-one days later another scan keeps the row, its history and its membership.
    clock.store(START + DAY + 31 * DAY, Ordering::SeqCst);
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.removed, 0, "nothing is deleted");
    assert_eq!(report.added, 0);
    assert_eq!(
        missing_since(tmp.path(), id),
        Some(START + DAY),
        "the first missing stamp stays"
    );
    assert_eq!(
        rows_for(tmp.path(), "SELECT COUNT(*) FROM history WHERE book_id = ?1", id),
        history_rows,
        "history keeps the book"
    );
    assert_eq!(
        rows_for(
            tmp.path(),
            "SELECT COUNT(*) FROM collection_books WHERE book_id = ?1",
            id
        ),
        1,
        "the collection keeps the book"
    );
    assert!(lib.book(id).unwrap().is_none(), "still hidden while missing");
    assert_eq!(all_books(&lib), 3);

    // The file is copied back to its old path. The next scan restores the same id and its state.
    fs::copy(repo_test_file("leaves.epub"), &files[2]).unwrap();
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.added, 0, "the copy does not get a new row");
    assert_eq!(report.removed, 0);
    assert_eq!(missing_since(tmp.path(), id), None, "the row is live again");

    let back = lib.book(id).unwrap().expect("the book is back in the library");
    assert_eq!(back.id, id);
    assert_eq!(back.path, files[2].canonicalize().unwrap());
    assert!((back.percent - 0.4).abs() < 1e-9, "progress is kept");
    assert_eq!(back.status, Status::Complete, "status is kept");
    assert_eq!(all_books(&lib), 4);
    let collections = lib.collections().unwrap();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0].name, "Favorites");
    assert_eq!(collections[0].books, 1, "collection membership is kept");
    assert_eq!(lib.recent(10).unwrap()[0].id, id, "history is kept");
}

#[test]
fn removed_book_returns_at_a_new_path_after_thirty_days_with_its_id() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let (mut lib, clock) = open_with_clock(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let id = lib.book_by_path(&files[2]).unwrap().unwrap().id;
    lib.record_open(id, 0.3).unwrap();
    lib.add_to_collection(id, "Favorites").unwrap();
    fs::remove_file(&files[2]).unwrap();
    clock.store(START + DAY, Ordering::SeqCst);
    scan(&mut lib, std::slice::from_ref(&root));

    // A scan with the file still gone runs more than thirty days after the removal.
    clock.store(START + 32 * DAY, Ordering::SeqCst);
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.removed, 0);
    assert_eq!(
        missing_since(tmp.path(), id),
        Some(START + DAY),
        "the row is kept"
    );

    // A copy at a new name is then found by content.
    let returned = root.join("returned.epub");
    fs::copy(repo_test_file("leaves.epub"), &returned).unwrap();
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.moved, 1, "the missing row is matched by content");
    assert_eq!(report.added, 0);
    assert_eq!(report.removed, 0);

    let back = lib
        .book_by_path(&returned)
        .unwrap()
        .expect("found at the new path");
    assert_eq!(back.id, id);
    assert!((back.percent - 0.3).abs() < 1e-9);
    assert_eq!(lib.collections().unwrap()[0].books, 1);
    assert_eq!(lib.recent(10).unwrap()[0].id, id);
}
