//! Moving a book's path and indexing one file. The plugin host calls these
//! after it writes the file, so a moved book keeps its id, history and
//! collections, and a copy becomes a new book.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use common::*;
use kaia_library::{CoverSize, Filter, Library, LibraryError, Sort, SortKey, Status};

/// A `books` root with juliet.epub, scanned into a fresh index. The caller
/// keeps the temp dir alive.
fn indexed_juliet() -> (tempfile::TempDir, PathBuf, Library, i64) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    fs::create_dir_all(&root).unwrap();
    copy_fixture("juliet.epub", &root, "juliet.epub");
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&root.join("juliet.epub")).unwrap().unwrap().id;
    (tmp, root, lib, id)
}

fn canonical(path: impl AsRef<Path>) -> PathBuf {
    fs::canonicalize(path).unwrap()
}

/// Rows in the reading history for `book_id`, read through a second connection.
fn history_rows(dir: &Path, book_id: i64) -> i64 {
    raw_connection(dir)
        .query_row(
            "SELECT COUNT(*) FROM history WHERE book_id = ?1",
            [book_id],
            |r| r.get(0),
        )
        .unwrap()
}

fn book_rows(dir: &Path) -> i64 {
    raw_connection(dir)
        .query_row("SELECT COUNT(*) FROM books", [], |r| r.get(0))
        .unwrap()
}

/// Ids of the books in the collection called `name`.
fn members(lib: &Library, name: &str) -> Vec<i64> {
    let collection = lib
        .collections()
        .unwrap()
        .into_iter()
        .find(|c| c.name == name)
        .expect("collection exists");
    let filter = Filter {
        collection: Some(collection.id),
        ..Default::default()
    };
    lib.books(Sort::new(SortKey::Title, false), &filter, 0, 100)
        .unwrap()
        .into_iter()
        .map(|b| b.id)
        .collect()
}

/// Copies `juliet.epub` to `folder/name` the way the plugin host does, and
/// returns the new path.
fn write_copy(root: &Path, folder: &str, name: &str) -> PathBuf {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    let to = dir.join(name);
    fs::copy(root.join("juliet.epub"), &to).unwrap();
    to
}

/// The plugin host moves a book while a scan runs. The scan has already walked
/// the folders, so it does not see the book at its new path. The book must stay
/// visible anyway.
#[test]
fn book_moved_while_a_scan_runs_stays_visible() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    let archive = tmp.path().join("archive");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&archive).unwrap();
    copy_fixture("juliet.epub", &root, "juliet.epub");
    copy_fixture("leaves.epub", &archive, "leaves.epub");
    let mut lib = open_library(tmp.path());
    let roots = [root.clone(), archive.clone()];
    scan(&mut lib, &roots);
    let id = lib.book_by_path(&root.join("juliet.epub")).unwrap().unwrap().id;

    // The first progress call comes after the walk and before any book is
    // processed. The move runs there through a second connection, as the plugin
    // host does.
    let to = archive.join("juliet.epub");
    let db = db_path(tmp.path());
    let cancel = AtomicBool::new(false);
    let mut moved = false;
    lib.scan(
        &roots,
        |_| {
            if !moved {
                moved = true;
                fs::rename(root.join("juliet.epub"), &to).unwrap();
                Library::open(&db).unwrap().move_book(id, &to).unwrap();
            }
        },
        &cancel,
    )
    .unwrap();

    let found = lib
        .book_by_path(&to)
        .unwrap()
        .expect("the moved book stays visible");
    assert_eq!(found.id, id);
}

#[test]
fn move_book_keeps_the_id_history_collections_and_cover() {
    let (tmp, root, mut lib, id) = indexed_juliet();
    lib.record_open(id, 0.4).unwrap();
    lib.set_status(id, Status::Reading).unwrap();
    lib.add_to_collection(id, "Keep").unwrap();

    let to = write_copy(&root, "archive", "juliet.epub");
    let moved = lib.move_book(id, &to).unwrap();
    fs::remove_file(root.join("juliet.epub")).unwrap();

    assert_eq!(moved.id, id);
    assert_eq!(moved.path, canonical(&to));
    assert!(lib.book_by_path(&root.join("juliet.epub")).unwrap().is_none());
    let found = lib.book_by_path(&to).unwrap().expect("indexed at the new path");
    assert_eq!(found.id, id);
    assert!((found.percent - 0.4).abs() < 1e-9);
    assert_eq!(found.status, Status::Reading);
    assert!(found.last_opened_at.is_some());
    assert!(lib.cover(id, CoverSize::Small).unwrap().is_some(), "cover kept");
    assert_eq!(history_rows(tmp.path(), id), 1);
    assert_eq!(members(&lib, "Keep"), vec![id]);

    // The next scan finds the file where the row now says it is. It adds no row,
    // matches nothing as moved and extracts nothing.
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.added, 0);
    assert_eq!(report.moved, 0);
    assert_eq!(report.extracted, 0);
    assert_eq!(report.removed, 0);
    let all = lib
        .books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
        .unwrap();
    assert_eq!(all.len(), 1, "no duplicate");
    assert_eq!(all[0].id, id);
    assert_eq!(members(&lib, "Keep"), vec![id]);
}

#[test]
fn move_book_refuses_a_file_that_is_not_the_book() {
    let (_tmp, root, mut lib, id) = indexed_juliet();
    let other = copy_fixture("leaves.epub", &root, "other.epub");

    let err = lib.move_book(id, &other).unwrap_err();
    assert!(matches!(err, LibraryError::InvalidArgument(_)), "{err}");
    assert_eq!(
        lib.book(id).unwrap().unwrap().path,
        canonical(root.join("juliet.epub"))
    );
    assert!(lib.book_by_path(&other).unwrap().is_none());
}

#[test]
fn move_book_refuses_a_path_that_another_book_holds() {
    let (_tmp, root, mut lib, id) = indexed_juliet();
    let copy = write_copy(&root, "copy", "juliet.epub");
    let other = lib.index_file(&copy).unwrap();

    let err = lib.move_book(id, &copy).unwrap_err();
    assert!(matches!(err, LibraryError::InvalidArgument(_)), "{err}");
    assert_eq!(
        lib.book(id).unwrap().unwrap().path,
        canonical(root.join("juliet.epub"))
    );
    assert_eq!(lib.book(other.id).unwrap().unwrap().path, canonical(&copy));
}

#[test]
fn move_book_replaces_a_row_whose_file_is_gone() {
    let (tmp, root, mut lib, id) = indexed_juliet();
    // A copy indexed at archive/juliet.epub. The file is then deleted outside
    // the app, and a scan marks the row missing.
    let stale_path = write_copy(&root, "archive", "juliet.epub");
    let stale = lib.index_file(&stale_path).unwrap();
    lib.record_open(stale.id, 0.9).unwrap();
    fs::remove_file(&stale_path).unwrap();
    scan(&mut lib, std::slice::from_ref(&root));
    assert!(lib.book(stale.id).unwrap().is_none(), "missing rows are hidden");
    assert_eq!(book_rows(tmp.path()), 2, "the missing row is still stored");

    // The book now moves onto that path. The missing row goes with its history.
    let to = write_copy(&root, "archive", "juliet.epub");
    let moved = lib.move_book(id, &to).unwrap();
    fs::remove_file(root.join("juliet.epub")).unwrap();

    assert_eq!(moved.id, id);
    assert_eq!(book_rows(tmp.path()), 1);
    assert_eq!(history_rows(tmp.path(), stale.id), 0);
    assert_eq!(lib.book_by_path(&to).unwrap().unwrap().id, id);
}

#[test]
fn failed_update_leaves_the_row_where_it_was() {
    let (tmp, root, mut lib, id) = indexed_juliet();
    lib.record_open(id, 0.3).unwrap();
    lib.add_to_collection(id, "Keep").unwrap();
    let to = write_copy(&root, "archive", "juliet.epub");
    raw_connection(tmp.path())
        .execute_batch(
            "CREATE TRIGGER refuse_moves BEFORE UPDATE OF path ON books
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .unwrap();

    let err = lib.move_book(id, &to).unwrap_err();
    assert!(matches!(err, LibraryError::Sqlite(_)), "{err}");
    let book = lib.book(id).unwrap().unwrap();
    assert_eq!(book.path, canonical(root.join("juliet.epub")));
    assert!((book.percent - 0.3).abs() < 1e-9);
    assert_eq!(history_rows(tmp.path(), id), 1);
    assert_eq!(members(&lib, "Keep"), vec![id]);
    assert!(lib.book_by_path(&to).unwrap().is_none());
}

#[test]
fn move_book_needs_a_known_id_and_an_existing_file() {
    let (_tmp, root, mut lib, id) = indexed_juliet();
    let err = lib.move_book(id + 100, &root.join("juliet.epub")).unwrap_err();
    assert!(matches!(err, LibraryError::NotFound(_)), "{err}");
    let err = lib.move_book(id, &root.join("absent.epub")).unwrap_err();
    assert!(matches!(err, LibraryError::Io(_)), "{err}");
}

#[test]
fn index_file_gives_a_copy_its_own_book() {
    let (tmp, root, mut lib, id) = indexed_juliet();
    lib.record_open(id, 0.3).unwrap();
    lib.add_to_collection(id, "Keep").unwrap();
    let original = lib.book(id).unwrap().unwrap();

    let copy = write_copy(&root, "copy", "juliet.epub");
    let book = lib.index_file(&copy).unwrap();

    assert_ne!(book.id, id, "a copy is a new book");
    assert_eq!(book.path, canonical(&copy));
    assert!(book.extracted);
    assert!(book.failure.is_none());
    assert_eq!(book.title, original.title, "same metadata as a scan gives");
    assert_eq!(book.partial_md5, original.partial_md5);
    assert!(lib.cover(book.id, CoverSize::Small).unwrap().is_some());
    assert_eq!(book.status, Status::New);
    assert!(book.last_opened_at.is_none());
    assert_eq!(history_rows(tmp.path(), book.id), 0);
    assert_eq!(
        members(&lib, "Keep"),
        vec![id],
        "the copy is not in the collection"
    );
    assert!(
        (lib.book(id).unwrap().unwrap().percent - 0.3).abs() < 1e-9,
        "the source keeps its progress"
    );

    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.added, 0);
    assert_eq!(report.moved, 0);
    assert_eq!(book_rows(tmp.path()), 2, "the scan adds no third row");
}

#[test]
fn index_file_replaces_a_row_whose_file_is_gone() {
    let (tmp, root, mut lib, id) = indexed_juliet();
    let stale_path = write_copy(&root, "archive", "juliet.epub");
    let stale = lib.index_file(&stale_path).unwrap();
    lib.record_open(stale.id, 0.9).unwrap();
    fs::remove_file(&stale_path).unwrap();
    scan(&mut lib, std::slice::from_ref(&root));
    assert!(lib.book(stale.id).unwrap().is_none(), "missing rows are hidden");

    let copy_path = write_copy(&root, "archive", "juliet.epub");
    let copy = lib.index_file(&copy_path).unwrap();

    // SQLite may give the copy the id that the replaced row had. What matters
    // is that the copy starts with no history from the old row.
    assert_eq!(history_rows(tmp.path(), copy.id), 0, "the old history is gone");
    assert!(copy.last_opened_at.is_none());
    assert_eq!(copy.percent, 0.0, "the old progress is gone");
    assert_eq!(book_rows(tmp.path()), 2, "the source and the copy");
    assert_eq!(
        lib.book(id).unwrap().unwrap().path,
        canonical(root.join("juliet.epub"))
    );
}

#[test]
fn index_file_returns_the_existing_row_at_its_path() {
    let (tmp, root, mut lib, id) = indexed_juliet();
    let book = lib.index_file(&root.join("juliet.epub")).unwrap();
    assert_eq!(book.id, id);
    assert_eq!(book_rows(tmp.path()), 1);
}

#[test]
fn index_file_rejects_what_is_not_a_book() {
    let (tmp, root, mut lib, _id) = indexed_juliet();
    fs::write(root.join("notes.txt"), b"not a book").unwrap();
    fs::create_dir_all(root.join("folder.epub")).unwrap();

    let err = lib.index_file(&root.join("notes.txt")).unwrap_err();
    assert!(matches!(err, LibraryError::InvalidArgument(_)), "{err}");
    let err = lib.index_file(&root.join("folder.epub")).unwrap_err();
    assert!(matches!(err, LibraryError::InvalidArgument(_)), "{err}");
    let err = lib.index_file(&root.join("absent.epub")).unwrap_err();
    assert!(matches!(err, LibraryError::Io(_)), "{err}");
    assert_eq!(book_rows(tmp.path()), 1);
}
