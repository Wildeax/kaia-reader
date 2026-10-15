mod common;

use std::fs;
use std::path::PathBuf;

use common::*;
use kaia_app::surfaces::settings::{entries::Entry, sections, storage};
use kaia_app::{App, HeadlessBackend, Route};
use kaia_book::{BookSession, OpenOptions};
use kaia_events::Value;
use kaia_library::storage::Source;

fn command(app: &mut App<HeadlessBackend>, command: &str) {
    app.command(command, &[]);
    app.process(Vec::new()).unwrap();
}

fn setup(name: &str) -> (App<HeadlessBackend>, PathBuf) {
    let dir = scratch(name);
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books.clone()]);
    app.services_mut()
        .settings
        .save("network.downloads", Value::from(books.to_str().unwrap()));
    app.start().unwrap();
    wait_for(&mut app, "library scan", |app| !app.scan_running());
    app.process(Vec::new()).unwrap();
    (app, dir)
}

fn open_storage(app: &mut App<HeadlessBackend>) {
    command(app, "nav:settings");
    tap_key(app, "settings-device-info");
    tap_key(app, "settings:storage");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "storage".into()
        })
    );
}

#[test]
fn cancelling_the_dialog_and_a_late_confirm_delete_nothing() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let (mut app, dir) = setup("storage-cancel");
    open_storage(&mut app);
    tap_key(&mut app, "storage-delete-opds");
    assert!(app.find_rect("dialog-button:storage:confirm").is_some());
    tap_key(&mut app, "dialog-button:storage:cancel");
    command(&mut app, "storage:confirm");
    tap_key(&mut app, "storage-delete-opds");
    command(&mut app, "back");
    command(&mut app, "storage:confirm");
    for name in TEST_BOOKS {
        assert!(dir.join("books").join(name).exists());
        assert!(book_id(&app, name) > 0);
    }
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn confirmed_calibre_delete_keeps_other_sources_and_reports_the_result() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let (mut app, dir) = setup("storage-calibre");
    fs::write(dir.join("books/metadata.calibre"), r#"[{"lpath":"juliet.epub"}]"#).unwrap();
    let sources = storage::sources(app.services()).unwrap();
    let groups = app.services().library.storage_groups(&sources).unwrap();
    assert_eq!(groups[0].source, Source::Calibre);
    assert_eq!(groups[0].books.len(), 1);
    assert_eq!(groups[1].books.len(), 2);
    let id = book_id(&app, "juliet.epub");
    open_storage(&mut app);
    kaia_gfx::png::write_pixmap(app.frame(), &std::env::temp_dir().join("kaia-storage-page.png")).unwrap();
    tap_key(&mut app, "storage-delete-calibre");
    tap_key(&mut app, "dialog-button:storage:confirm");
    assert!(!dir.join("books/juliet.epub").exists());
    assert!(dir.join("books/leaves.epub").exists());
    assert!(dir.join("books/2col.pdf").exists());
    assert!(dir.join("books/metadata.calibre").exists());
    assert!(app.services().library.book(id).unwrap().is_none());
    assert!(app
        .notices()
        .iter()
        .any(|text| text == "1 deleted, 0 failed, 0 skipped because open."));
}

#[test]
fn delete_keeps_annotations_and_skips_a_live_reader_session() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let (mut app, dir) = setup("storage-annotations");
    let path = fs::canonicalize(dir.join("books/2col.pdf")).unwrap();
    let original = fs::read(&path).unwrap();
    let data = app.services().paths.data_dir.clone();
    let id = book_id(&app, "2col.pdf");
    let md5 = app.services().library.book(id).unwrap().unwrap().partial_md5;
    let state = data.join("kaia/books").join(format!("{md5}.json"));
    let reservation = kaia_book::session::hold_closed_book(&path, &data, &md5)
        .unwrap()
        .unwrap();
    assert!(kaia_book::session::hold_closed_book(&path, &data, &md5)
        .unwrap()
        .is_none());
    drop(reservation);
    let mut session = BookSession::open(&path, OpenOptions::new(&data)).unwrap();
    assert!(session.toggle_bookmark().unwrap());
    let bookmark = session.bookmarks().unwrap()[0].id;
    session.edit_note(bookmark, Some("Keep this note.")).unwrap();
    session.save().unwrap();
    assert!(!storage::delete_one(app.services_mut(), id).unwrap());
    let sources = storage::sources(app.services()).unwrap();
    let group = app.services().library.storage_groups(&sources).unwrap().remove(1);
    let report = storage::delete_group(app.services_mut(), &group).unwrap();
    assert_eq!((report.deleted, report.failed, report.skipped_open), (2, 0, 1));
    assert!(path.exists());
    drop(session);
    let before = fs::read(&state).unwrap();
    assert!(storage::delete_one(app.services_mut(), id).unwrap());
    assert_eq!(fs::read(&state).unwrap(), before);
    assert!(!path.exists());
    fs::write(&path, original).unwrap();
    app.services_mut().library.index_file(&path).unwrap();
    let mut session = BookSession::open(&path, OpenOptions::new(&data)).unwrap();
    let notes = session.annotations().unwrap();
    assert!(notes
        .iter()
        .any(|note| note.note.as_deref() == Some("Keep this note.")));
}

#[test]
fn page_reports_data_cache_and_volume_sizes_and_refuses_a_bad_source_cache() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let (mut app, dir) = setup("storage-totals");
    let cache = app.services().paths.data_dir.join("cache");
    fs::create_dir(&cache).unwrap();
    fs::write(cache.join("entry"), [0; 123]).unwrap();
    let entries = sections::entries("storage", app.services(), 0);
    for label in [
        "Indexed books",
        "Free space",
        "Total space",
        "Kaia data",
        "Cache, included in Kaia data",
    ] {
        assert!(
            entries
                .iter()
                .any(|entry| matches!(entry, Entry::Row { primary, value: Some(_), .. } if primary == label)),
            "{label}"
        );
    }
    fs::write(dir.join("books/metadata.calibre"), b"broken").unwrap();
    open_storage(&mut app);
    command(&mut app, "storage:delete:opds");
    assert!(app.find_rect("dialog-button:storage:confirm").is_none());
    assert!(dir.join("books/juliet.epub").exists());
}

#[test]
fn delete_dialog_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let (app, _) = setup("storage-dialog-golden");
    let group = app
        .services()
        .library
        .storage_groups(&storage::sources(app.services()).unwrap())
        .unwrap()
        .remove(1);
    let services = app.services();
    let dialog = storage::delete_dialog(&services.theme, &group);
    let pixmap = kaia_ui::render(dialog, app.screen(), &services.theme, &services.assets);
    let preview = std::env::temp_dir().join("kaia-storage-dialog.png");
    kaia_gfx::png::write_pixmap(&pixmap, &preview).unwrap();
    println!("Storage dialog preview: {}", preview.display());
    kaia_gfx::golden::assert_matches_golden(&pixmap, &golden("storage_delete_dialog.png"), 14);
}
