//! The Calibre view's offline search and browse. The metadata file is written into the
//! downloads folder by the test, the way a wireless transfer leaves it. The library index
//! holds one of its books, so a tap opens that book, and a tap on another book reports
//! that it is not on the device.

mod common;
use common::*;
use kaia_app::surfaces::catalogs::store;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::Value;
use kaia_library::Library;
use std::fs;
use std::path::Path;

/// The book that the library holds. Its path in the metadata file is `LPATH`.
const LPATH: &str = "Shakespeare, William/Romeo and Juliet (1).epub";

/// Four books with non-ASCII text and a missing field. Only Romeo and Juliet has a file.
const METADATA: &str = r#"[
    {"uuid":"u1","lpath":"Shakespeare, William/Romeo and Juliet (1).epub",
     "title":"Romeo and Juliet","authors":["William Shakespeare"],
     "series":"Classics","series_index":1,"tags":["Drama"],"size":1200},
    {"uuid":"u2","lpath":"Herbert, Frank/Dune.epub","title":"Dune",
     "authors":["Frank Herbert"],"series":"Dune","series_index":1,"tags":["Science fiction"]},
    {"uuid":"u3","lpath":"Ångström, Zoë/Café Stories.epub","title":"Café Stories",
     "authors":["Zoë Ångström"],"tags":["Essays"],"series":null},
    {"uuid":"u4","lpath":"Loose/Notes.txt"}
]"#;

fn command(app: &mut App<HeadlessBackend>, id: &str) {
    app.command(id, &[]);
    app.process(Vec::new()).unwrap();
}

fn change(app: &mut App<HeadlessBackend>, name: &str, value: &str) {
    app.command(&format!("catalogs:change:{name}"), &[Value::from(value)]);
}

/// A scratch data folder with the downloads folder set to `downloads`. The library
/// holds the Romeo and Juliet file, copied to its path in the metadata.
fn setup(name: &str, metadata: Option<&[u8]>) -> (std::path::PathBuf, std::path::PathBuf) {
    let dir = scratch(name);
    let downloads = dir.join("downloads");
    fs::create_dir_all(downloads.join("Shakespeare, William")).unwrap();
    let book = downloads.join(LPATH);
    fs::copy(repo_root().join("test/juliet.epub"), &book).unwrap();
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    Library::open(&data.join("library.sqlite3"))
        .unwrap()
        .index_file(&book)
        .unwrap();
    if let Some(bytes) = metadata {
        fs::write(downloads.join("metadata.calibre"), bytes).unwrap();
    }
    (dir, downloads)
}

/// Starts the app and opens the Calibre view. Its downloads folder is `downloads`.
fn open_calibre(dir: &Path, downloads: &Path) -> App<HeadlessBackend> {
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app.services_mut()
        .settings
        .save(store::DOWNLOADS, Value::from(downloads.to_str().unwrap()));
    command(&mut app, "nav:settings");
    command(&mut app, "settings:network");
    command(&mut app, "settings:calibre");
    assert_eq!(app.top_route(), Some(&Route::Calibre));
    app
}

fn search(app: &mut App<HeadlessBackend>, query: &str) {
    command(app, "catalogs:calibre-search");
    change(app, "calibre-query", query);
    command(app, "catalogs:calibre-search-go");
}

#[test]
fn search_opens_the_book_in_the_library_and_reports_the_rest() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, downloads) = setup("calibre-search", Some(METADATA.as_bytes()));
    let mut app = open_calibre(&dir, &downloads);
    assert!(app.find_rect("catalogs:calibre-search").is_some());
    assert!(app.find_rect("catalogs:calibre-browse:author").is_some());
    assert!(app.find_rect("catalogs:calibre-browse:series").is_some());
    assert!(app.find_rect("catalogs:calibre-browse:tag").is_some());

    // Non-ASCII, case-insensitive, and two words that match two fields.
    search(&mut app, "CAFÉ");
    assert!(app.find_rect("catalogs:calibre-book:0").is_some());
    assert!(app.find_rect("catalogs:calibre-book:1").is_none());
    command(&mut app, "back");
    assert_eq!(app.top_route(), Some(&Route::Calibre));

    search(&mut app, "romeo classics");
    assert!(app.find_rect("catalogs:calibre-book:0").is_some());
    assert!(app.find_rect("catalogs:calibre-book:1").is_none());
    // The book is in the library, so the tap opens it in the reader.
    command(&mut app, "catalogs:calibre-book:0");
    assert!(
        matches!(app.top_route(), Some(Route::Reader { .. })),
        "{:?}",
        app.top_route()
    );
    wait_reader(&mut app);
    app.shutdown();
}

#[test]
fn a_matched_book_without_a_file_stays_in_the_view() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, downloads) = setup("calibre-missing", Some(METADATA.as_bytes()));
    let mut app = open_calibre(&dir, &downloads);
    search(&mut app, "dune");
    assert!(app.find_rect("catalogs:calibre-book:0").is_some());
    command(&mut app, "catalogs:calibre-book:0");
    assert_eq!(app.top_route(), Some(&Route::Calibre));
    assert!(
        app.find_rect("catalogs-status").is_some(),
        "the status line says the book is not on the device"
    );
    app.shutdown();
}

#[test]
fn blank_and_empty_searches_keep_the_view_open() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, downloads) = setup("calibre-blank", Some(METADATA.as_bytes()));
    let mut app = open_calibre(&dir, &downloads);
    search(&mut app, "   ");
    assert!(app.find_rect("catalogs:calibre-book:0").is_none());
    assert!(
        app.find_rect("catalogs:calibre-search-go").is_some(),
        "still typing"
    );
    search(&mut app, "zzz-no-such-book");
    assert!(app.find_rect("catalogs:calibre-book:0").is_none());
    app.shutdown();
}

#[test]
fn browse_by_author_lists_groups_then_their_books() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, downloads) = setup("calibre-browse", Some(METADATA.as_bytes()));
    let mut app = open_calibre(&dir, &downloads);
    command(&mut app, "catalogs:calibre-browse:author");
    // Authors sort by name: Frank Herbert, William Shakespeare, Zoë Ångström.
    assert!(app.find_rect("catalogs:calibre-group:2").is_some());
    assert!(app.find_rect("catalogs:calibre-group:3").is_none());
    command(&mut app, "catalogs:calibre-group:1");
    assert!(app.find_rect("catalogs:calibre-book:0").is_some());
    assert!(app.find_rect("catalogs:calibre-book:1").is_none());
    command(&mut app, "catalogs:calibre-book:0");
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));
    wait_reader(&mut app);
    app.shutdown();
}

#[test]
fn browse_by_series_and_tag_use_their_own_groups() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, downloads) = setup("calibre-browse-tags", Some(METADATA.as_bytes()));
    let mut app = open_calibre(&dir, &downloads);
    command(&mut app, "catalogs:calibre-browse:series");
    // Classics, then Dune.
    assert!(app.find_rect("catalogs:calibre-group:1").is_some());
    assert!(app.find_rect("catalogs:calibre-group:2").is_none());
    command(&mut app, "back");
    command(&mut app, "catalogs:calibre-browse:tag");
    // Drama, Essays, Science fiction.
    assert!(app.find_rect("catalogs:calibre-group:2").is_some());
    command(&mut app, "catalogs:calibre-group:1");
    assert!(app.find_rect("catalogs:calibre-book:0").is_some());
    assert!(app.find_rect("catalogs:calibre-book:1").is_none());
    app.shutdown();
}

#[test]
fn no_metadata_file_offers_only_the_wireless_connection() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, downloads) = setup("calibre-none", None);
    let mut app = open_calibre(&dir, &downloads);
    assert!(app.find_rect("catalogs:calibre-search").is_none());
    assert!(app.find_rect("catalogs:discover").is_some());
    app.shutdown();
}

#[test]
fn damaged_metadata_is_reported_and_the_wireless_view_still_works() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let damaged = br#"[{"lpath": "Shakespeare, William/Romeo and Juliet (1).epub", "title": "Romeo"#;
    let (dir, downloads) = setup("calibre-damaged", Some(damaged));
    let mut app = open_calibre(&dir, &downloads);
    assert!(app.find_rect("catalogs:calibre-search").is_none());
    assert!(app.find_rect("catalogs:discover").is_some());
    app.shutdown();
}

#[test]
fn oversized_metadata_is_refused_without_reading_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (dir, downloads) = setup("calibre-large", None);
    // A sparse file one byte over the cap. Nothing is read from it.
    let file = fs::File::create(downloads.join("metadata.calibre")).unwrap();
    file.set_len(kaia_calibre::metadata::MAX_BYTES + 1).unwrap();
    drop(file);
    let mut app = open_calibre(&dir, &downloads);
    assert!(app.find_rect("catalogs:calibre-search").is_none());
    assert!(app.find_rect("catalogs:discover").is_some());
    app.shutdown();
}
