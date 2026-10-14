//! The Books tab filters: status, collection, source and file type. Each one
//! narrows the list, they combine with each other, and they survive a restart.
//!
//! The test library has two EPUBs outside the download folder (Other), a PDF in
//! the download folder (OPDS), and a text file that the Calibre cache in the
//! download folder lists (Calibre).

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::{Settings, Value};
use kaia_library::Status;

use common::*;

/// The folders of one test library.
struct Layout {
    dir: PathBuf,
    books: PathBuf,
    downloads: PathBuf,
}

fn layout(name: &str) -> Layout {
    let dir = scratch(name);
    let books = dir.join("books");
    let downloads = dir.join("downloads");
    fs::create_dir_all(&books).unwrap();
    fs::create_dir_all(downloads.join("Calibre")).unwrap();
    for file in ["juliet.epub", "leaves.epub"] {
        fs::copy(repo_root().join("test").join(file), books.join(file)).unwrap();
    }
    fs::copy(repo_root().join("test/2col.pdf"), downloads.join("2col.pdf")).unwrap();
    fs::write(
        downloads.join("Calibre/notes.txt"),
        "Notes from the Calibre cache.\n",
    )
    .unwrap();
    fs::write(
        downloads.join("metadata.calibre"),
        r#"[{"lpath": "Calibre/notes.txt", "title": "Notes"}]"#,
    )
    .unwrap();
    // The download folder is a setting, so the app reads it at start.
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    let path = data.join("settings.json");
    let mut settings = Settings::open(&path);
    settings.save("network.downloads", Value::from(downloads.display().to_string()));
    settings.flush(&path).unwrap();
    Layout {
        dir,
        books,
        downloads,
    }
}

/// Starts the app on the layout, with both folders as scan roots, and waits for the scan.
fn start(layout: &Layout) -> App<HeadlessBackend> {
    let mut app = make_app(&layout.dir, vec![layout.books.clone(), layout.downloads.clone()]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    app
}

/// Shuts `app` down and starts a new one on the same data folder, with no scan.
fn restart(mut app: App<HeadlessBackend>, dir: &Path) -> App<HeadlessBackend> {
    app.shutdown();
    drop(app);
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app
}

/// Opens the Library and shows the Books tab as a list. A cover grid lays out
/// only the rows that fit on the page, and a test needs every book on screen.
fn open_library(app: &mut App<HeadlessBackend>) {
    tap_key(app, "library");
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    tap_key(app, "library-view-list");
}

/// The Books tab shows the book with `id`.
fn shows(app: &App<HeadlessBackend>, id: i64) -> bool {
    app.find_rect(&format!("book-{id}")).is_some()
}

/// Opens the filter sheet and taps `key` in it.
fn pick(app: &mut App<HeadlessBackend>, key: &str) {
    tap_key(app, "library-filter");
    tap_key(app, key);
}

/// Opens the file type picker from the filter sheet and taps `key`.
fn pick_type(app: &mut App<HeadlessBackend>, key: &str) {
    tap_key(app, "library-filter");
    tap_key(app, "library-filter-type");
    tap_key(app, key);
}

struct Books {
    juliet: i64,
    leaves: i64,
    pdf: i64,
    notes: i64,
}

fn books(app: &App<HeadlessBackend>) -> Books {
    Books {
        juliet: book_id(app, "juliet.epub"),
        leaves: book_id(app, "leaves.epub"),
        pdf: book_id(app, "2col.pdf"),
        notes: book_id(app, "notes.txt"),
    }
}

#[test]
fn the_source_and_type_filters_each_narrow_the_list() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let layout = layout("filters-source-type");
    let mut app = start(&layout);
    let b = books(&app);
    open_library(&mut app);
    let missing: Vec<&str> = [
        ("juliet", b.juliet),
        ("leaves", b.leaves),
        ("pdf", b.pdf),
        ("notes", b.notes),
    ]
    .iter()
    .filter(|(_, id)| !shows(&app, *id))
    .map(|(name, _)| *name)
    .collect();
    assert!(
        missing.is_empty(),
        "All books shows every file, missing {missing:?}"
    );

    pick(&mut app, "library-filter-source-calibre");
    assert!(shows(&app, b.notes), "Calibre shows the file its cache lists");
    assert!(!shows(&app, b.pdf) && !shows(&app, b.juliet));

    pick(&mut app, "library-filter-source-other");
    assert!(shows(&app, b.juliet) && shows(&app, b.leaves));
    assert!(
        !shows(&app, b.pdf) && !shows(&app, b.notes),
        "Other leaves out the download folder"
    );

    pick(&mut app, "library-filter-source-opds");
    assert!(
        shows(&app, b.pdf),
        "OPDS downloads shows the file in the download folder"
    );
    assert!(!shows(&app, b.notes), "the Calibre file is not an OPDS download");

    pick(&mut app, "library-filter-source-all");
    assert!(shows(&app, b.juliet) && shows(&app, b.pdf) && shows(&app, b.notes));

    pick_type(&mut app, "format-epub");
    assert!(shows(&app, b.juliet) && shows(&app, b.leaves));
    assert!(
        !shows(&app, b.pdf) && !shows(&app, b.notes),
        "EPUB leaves out the PDF and the text"
    );

    pick_type(&mut app, "format-txt");
    assert!(shows(&app, b.notes), "the text type is txt");
    assert!(!shows(&app, b.juliet));
}

#[test]
fn the_filters_combine_with_status_and_collection_and_survive_a_restart() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let layout = layout("filters-combine");
    let mut app = start(&layout);
    let b = books(&app);
    app.services_mut().library.record_open(b.juliet, 0.5).unwrap();
    app.services_mut()
        .library
        .add_to_collection(b.juliet, "Shelf")
        .unwrap();
    app.services_mut()
        .library
        .add_to_collection(b.leaves, "Shelf")
        .unwrap();
    let shelf = app
        .services()
        .library
        .collections()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "Shelf")
        .unwrap()
        .id;
    open_library(&mut app);

    pick(&mut app, "library-filter-reading");
    pick(&mut app, "library-filter-source-other");
    pick_type(&mut app, "format-epub");
    assert!(shows(&app, b.juliet), "reading, other and epub keep Juliet");
    assert!(!shows(&app, b.leaves), "leaves is not in progress");
    assert_eq!(
        library_db(&app).book(b.juliet).unwrap().unwrap().status,
        Status::Reading
    );

    // A collection narrows the same list. Picking it keeps the status and the other filters.
    tap_key(&mut app, "library-filter");
    tap_key(&mut app, "library-filter-collection");
    tap_key(&mut app, &format!("collection-{shelf}"));
    assert!(shows(&app, b.juliet), "Shelf and reading keep Juliet");
    assert!(!shows(&app, b.leaves));

    let settings = &app.services().settings;
    assert_eq!(settings.read("library.chip").as_str(), Some("reading"));
    assert_eq!(settings.read("library.source").as_str(), Some("other"));
    assert_eq!(settings.read("library.format").as_str(), Some("epub"));
    assert_eq!(settings.read("library.collection").as_i64(), Some(shelf));

    let mut app = restart(app, &layout.dir);
    open_library(&mut app);
    assert!(shows(&app, b.juliet), "the filters come back after a restart");
    assert!(!shows(&app, b.leaves) && !shows(&app, b.pdf) && !shows(&app, b.notes));

    // All books clears the status and keeps the others. Source all clears the source.
    pick(&mut app, "library-filter-all");
    assert!(
        shows(&app, b.juliet) && shows(&app, b.leaves),
        "Shelf, other and epub stay"
    );
    assert!(
        !shows(&app, b.pdf) && !shows(&app, b.notes),
        "the collection still limits the list"
    );
    pick(&mut app, "library-filter-source-all");
    pick_type(&mut app, "format-all");
    assert!(shows(&app, b.juliet) && shows(&app, b.leaves));
    assert!(
        !shows(&app, b.pdf) && !shows(&app, b.notes),
        "Shelf alone still limits the list"
    );
    tap_key(&mut app, "library-filter");
    tap_key(&mut app, "library-filter-collection");
    tap_key(&mut app, "library-collection-none");
    assert!(shows(&app, b.juliet) && shows(&app, b.leaves) && shows(&app, b.pdf) && shows(&app, b.notes));
    assert_eq!(app.services().settings.read("library.format").as_str(), Some(""));
}
