//! Reading status in batches: one transaction for many books, missing ids skipped,
//! and what unread clears.

mod common;

use std::fs;
use std::path::PathBuf;

use common::*;
use kaia_library::{LibraryError, Status};

fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    fs::create_dir_all(&root).unwrap();
    (tmp, root)
}

#[test]
fn set_status_many_marks_each_book_and_skips_missing_ids() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let ids: Vec<i64> = files
        .iter()
        .map(|f| lib.book_by_path(f).unwrap().unwrap().id)
        .collect();

    let changed = lib
        .set_status_many(&[ids[0], ids[1], 9999], Status::Complete)
        .unwrap();
    assert_eq!(changed, 2, "the missing id changes nothing");
    assert_eq!(lib.book(ids[0]).unwrap().unwrap().status, Status::Complete);
    assert_eq!(lib.book(ids[1]).unwrap().unwrap().status, Status::Complete);
    assert_eq!(lib.book(ids[2]).unwrap().unwrap().status, Status::New);
    assert_eq!(lib.set_status_many(&[], Status::Reading).unwrap(), 0);
}

#[test]
fn set_status_many_is_all_or_nothing_for_a_failing_write() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let first = lib.book_by_path(&files[0]).unwrap().unwrap().id;
    let second = lib.book_by_path(&files[1]).unwrap().unwrap().id;
    // A trigger that fails the second write rolls the whole batch back.
    raw_connection(tmp.path())
        .execute_batch(&format!(
            "CREATE TRIGGER fail_mark BEFORE UPDATE OF status ON books
             WHEN old.id = {second} BEGIN SELECT RAISE(ABORT, 'refused'); END;"
        ))
        .unwrap();
    assert!(lib.set_status_many(&[first, second], Status::Complete).is_err());
    assert_eq!(
        lib.book(first).unwrap().unwrap().status,
        Status::New,
        "the first book keeps its status when the batch fails"
    );
}

#[test]
fn marking_unread_clears_recent_and_keeps_progress() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&files[2]).unwrap().unwrap().id;
    lib.record_open(id, 0.4).unwrap();
    assert_eq!(lib.book(id).unwrap().unwrap().status, Status::Reading);
    assert_eq!(lib.recent(10).unwrap().len(), 1);

    lib.set_status_many(&[id], Status::New).unwrap();

    let book = lib.book(id).unwrap().unwrap();
    assert_eq!(book.status, Status::New);
    assert_eq!(book.last_opened_at, None, "the book is no longer recent");
    assert_eq!(book.percent, 0.4, "the reading position is kept");
    assert!(lib.recent(10).unwrap().is_empty());
    let history: i64 = raw_connection(tmp.path())
        .query_row("SELECT COUNT(*) FROM history WHERE book_id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(history, 0, "the history rows go with it");
}

#[test]
fn finishing_keeps_the_last_opened_time() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&files[2]).unwrap().unwrap().id;
    lib.record_open(id, 0.9).unwrap();
    lib.set_status_many(&[id], Status::Complete).unwrap();
    let book = lib.book(id).unwrap().unwrap();
    assert_eq!(book.status, Status::Complete);
    assert!(
        book.last_opened_at.is_some(),
        "only unread clears the reading history"
    );
}

#[test]
fn set_status_on_a_missing_book_is_not_found() {
    let (tmp, _root) = setup();
    let mut lib = open_library(tmp.path());
    assert!(matches!(
        lib.set_status(9999, Status::Complete),
        Err(LibraryError::NotFound(9999))
    ));
}
