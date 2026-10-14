//! Plugin file copies and moves through the file manager host. A moved book
//! keeps its id, history and collections, and it opens at its new path. A copy
//! is a new book. A call that fails changes nothing.
#![cfg(feature = "lua")]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use kaia_app::plugins::library::Host;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_library::{CoverSize, Filter, Sort, SortKey, Status};
use kaia_lua::LibraryHost;
use rusqlite::Connection;

/// A data folder with juliet.epub in `books`, an empty `archive` folder, and the
/// app scanned over both. Returns the app, the two folders and the book id.
fn setup(name: &str) -> (App<HeadlessBackend>, PathBuf, PathBuf, i64) {
    let data = scratch(name);
    let books = data.join("books");
    let archive = data.join("archive");
    fs::create_dir_all(&books).unwrap();
    fs::create_dir_all(&archive).unwrap();
    fs::copy(repo_root().join("test/juliet.epub"), books.join("juliet.epub")).unwrap();
    let mut app = make_app(&data, vec![books.clone(), archive.clone()]);
    app.start().unwrap();
    assert!(app.scan_running(), "the first scan starts");
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    let id = book_id(&app, "juliet.epub");
    // Reading progress, a history entry and a collection, as a reader leaves them.
    app.services_mut().library.add_to_collection(id, "Keep").unwrap();
    app.services_mut().library.record_open(id, 0.5).unwrap();
    (app, books, archive, id)
}

/// Ids of the books in the collection called `name`.
fn members(app: &App<HeadlessBackend>, name: &str) -> Vec<i64> {
    let lib = library_db(app);
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

/// Rows in the reading history for `id`, read through a second connection.
fn history_rows(app: &App<HeadlessBackend>, id: i64) -> i64 {
    Connection::open(app.services().library_path())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM history WHERE book_id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap()
}

/// Every visible book, by id.
fn all_ids(app: &App<HeadlessBackend>) -> Vec<i64> {
    library_db(app)
        .books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
        .unwrap()
        .into_iter()
        .map(|b| b.id)
        .collect()
}

/// Installs a trigger on the index that makes the writes it names fail. The
/// trigger stays until the test ends. It is created through a second connection.
fn refuse_writes(app: &App<HeadlessBackend>, trigger: &str) {
    Connection::open(app.services().library_path())
        .unwrap()
        .execute_batch(trigger)
        .unwrap();
}

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

#[test]
fn move_keeps_the_book_id_history_and_collection() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, books, archive, id) = setup("plugin-library-move");
    let source = books.join("juliet.epub");
    let target = archive.join("juliet.epub");

    let mut host = Host::new(&app.services().paths);
    host.move_book(text(&source), text(&archive)).unwrap();

    assert!(target.is_file(), "the destination exists");
    assert!(!source.exists(), "the source is gone");
    let lib = library_db(&app);
    let moved = lib
        .book_by_path(&target)
        .unwrap()
        .expect("the index has the destination");
    assert_eq!(moved.id, id, "the book keeps its id");
    assert!(lib.book_by_path(&source).unwrap().is_none());
    assert!((moved.percent - 0.5).abs() < 1e-9, "progress kept");
    assert_eq!(moved.status, Status::Reading);
    assert!(lib.cover(id, CoverSize::Small).unwrap().is_some(), "cover kept");
    assert_eq!(history_rows(&app, id), 1, "history kept");
    assert_eq!(members(&app, "Keep"), vec![id], "collection kept");

    app.command(&format!("open_book:{}", text(&target)), &[]);
    wait_reader(&mut app);
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));

    // The reader records its own open, so count the history again from here on.
    let history = history_rows(&app, id);
    app.command("scan", &[]);
    wait_for(&mut app, "the rescan", |a| !a.scan_running());
    assert_eq!(
        all_ids(&app),
        vec![id],
        "the rescan neither duplicates nor drops the book"
    );
    assert_eq!(
        library_db(&app).book(id).unwrap().unwrap().path,
        fs::canonicalize(&target).unwrap()
    );
    assert_eq!(members(&app, "Keep"), vec![id]);
    assert_eq!(history_rows(&app, id), history, "the rescan keeps the history");
}

#[test]
fn copy_adds_a_new_book_and_keeps_the_source() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, books, archive, id) = setup("plugin-library-copy");
    let source = books.join("juliet.epub");
    let target = archive.join("juliet.epub");

    let mut host = Host::new(&app.services().paths);
    host.copy_book(text(&source), text(&archive)).unwrap();

    assert!(source.is_file(), "the source stays");
    assert!(target.is_file(), "the copy exists");
    let lib = library_db(&app);
    let copy = lib.book_by_path(&target).unwrap().expect("the copy is indexed");
    assert_ne!(copy.id, id, "a copy is a new book");
    assert!(copy.extracted);
    assert_eq!(copy.status, Status::New);
    assert!(copy.last_opened_at.is_none());
    let original = lib.book(id).unwrap().unwrap();
    assert_eq!(original.path, fs::canonicalize(&source).unwrap());
    assert!(
        (original.percent - 0.5).abs() < 1e-9,
        "the source keeps its progress"
    );
    assert_eq!(history_rows(&app, id), 1);
    assert_eq!(
        members(&app, "Keep"),
        vec![id],
        "the copy is not in the collection"
    );

    app.command("scan", &[]);
    wait_for(&mut app, "the rescan", |a| !a.scan_running());
    let mut ids = all_ids(&app);
    ids.sort_unstable();
    let mut expected = vec![id, copy.id];
    expected.sort_unstable();
    assert_eq!(ids, expected, "the rescan adds no third book");
}

#[test]
fn move_to_a_missing_folder_changes_nothing() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, books, archive, id) = setup("plugin-library-missing");
    let source = books.join("juliet.epub");
    let missing = archive.join("no-such-folder");

    let mut host = Host::new(&app.services().paths);
    let err = host.move_book(text(&source), text(&missing)).unwrap_err();

    assert_eq!(err, "Not a folder");
    assert!(source.is_file(), "the source stays");
    assert!(!missing.exists());
    assert_eq!(
        library_db(&app).book(id).unwrap().unwrap().path,
        fs::canonicalize(&source).unwrap()
    );
    assert_eq!(history_rows(&app, id), 1);
    assert_eq!(members(&app, "Keep"), vec![id]);
}

#[test]
fn move_onto_an_existing_file_changes_nothing() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, books, archive, id) = setup("plugin-library-exists");
    let source = books.join("juliet.epub");
    let occupied = archive.join("juliet.epub");
    fs::write(&occupied, b"someone else's file").unwrap();

    let mut host = Host::new(&app.services().paths);
    let err = host.move_book(text(&source), text(&archive)).unwrap_err();

    assert_eq!(err, "The destination already exists");
    assert!(source.is_file());
    assert_eq!(fs::read(&occupied).unwrap(), b"someone else's file");
    assert_eq!(
        library_db(&app).book(id).unwrap().unwrap().path,
        fs::canonicalize(&source).unwrap()
    );
    assert_eq!(members(&app, "Keep"), vec![id]);
}

#[test]
fn failed_index_update_puts_the_book_back() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, books, archive, id) = setup("plugin-library-rollback");
    let source = books.join("juliet.epub");
    let target = archive.join("juliet.epub");
    refuse_writes(
        &app,
        "CREATE TRIGGER refuse_moves BEFORE UPDATE OF path ON books
         BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    );

    let mut host = Host::new(&app.services().paths);
    let err = host.move_book(text(&source), text(&archive)).unwrap_err();

    assert!(err.starts_with("The library could not be updated"), "{err}");
    assert!(source.is_file(), "the source is back in place");
    assert!(!target.exists(), "the destination is removed");
    assert_eq!(
        library_db(&app).book(id).unwrap().unwrap().path,
        fs::canonicalize(&source).unwrap()
    );
    assert_eq!(history_rows(&app, id), 1);
    assert_eq!(members(&app, "Keep"), vec![id]);
}

#[test]
fn failed_copy_index_removes_the_copy() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (app, books, archive, id) = setup("plugin-library-copy-rollback");
    let source = books.join("juliet.epub");
    let target = archive.join("juliet.epub");
    refuse_writes(
        &app,
        "CREATE TRIGGER refuse_books BEFORE INSERT ON books
         BEGIN SELECT RAISE(ABORT, 'refused'); END;",
    );

    let mut host = Host::new(&app.services().paths);
    let err = host.copy_book(text(&source), text(&archive)).unwrap_err();

    assert!(err.starts_with("The library could not be updated"), "{err}");
    assert!(source.is_file());
    assert!(!target.exists(), "the copy is removed");
    assert_eq!(all_ids(&app), vec![id], "no row was added");
    assert_eq!(history_rows(&app, id), 1);
    assert_eq!(members(&app, "Keep"), vec![id]);
}
