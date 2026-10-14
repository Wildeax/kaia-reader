//! R3-H1: the plugin host never writes, moves or deletes KOReader's sidecars
//! next to a book. A stock sidecar stays where KOReader wrote it. DocSettings
//! writes Kaia's copy under the data folder.

mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use common::book::BookReader;
use common::workflow;
use common::{host, lua_ui, repo_root, serial, TempDir};
use kaia_events::Value;
use kaia_lua::{LuaHost, PluginOptions};

/// The metadata that stock KOReader wrote next to the book.
const STOCK: &str = "return { [\"percent_finished\"] = 0.25, [\"koreader_only\"] = \"keep me\" }\n";

/// Every file under `dir`, keyed by its path relative to `dir`. A missing
/// folder has no files.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_path_buf();
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// `folder/juliet.epub` with the sidecar that stock KOReader writes next to it.
fn stock_book(folder: &Path) -> PathBuf {
    let book = folder.join("juliet.epub");
    fs::copy(repo_root().join("test/juliet.epub"), &book).unwrap();
    let sdr = folder.join("juliet.sdr");
    fs::create_dir_all(&sdr).unwrap();
    fs::write(sdr.join("metadata.epub.lua"), STOCK).unwrap();
    fs::write(sdr.join("cover.png"), "not really a png").unwrap();
    fs::write(sdr.join("custom_metadata.lua"), "KOReader metadata").unwrap();
    book
}

/// Asserts that both snapshots hold the same files with the same bytes. A
/// failure names the file that changed or is missing.
fn assert_same_files(actual: &BTreeMap<PathBuf, Vec<u8>>, expected: &BTreeMap<PathBuf, Vec<u8>>) {
    let names = |files: &BTreeMap<PathBuf, Vec<u8>>| -> Vec<String> {
        files.keys().map(|p| p.display().to_string()).collect()
    };
    assert_eq!(names(actual), names(expected), "the files in the folder differ");
    for (path, bytes) in expected {
        assert!(actual[path] == *bytes, "{} changed", path.display());
    }
}

fn set_path(host: &LuaHost, name: &str, path: &Path) {
    host.set_global(name, &Value::Str(path.display().to_string()))
        .unwrap();
}

fn check(host: &LuaHost, name: &str, source: &str) {
    if let Err(err) = host.exec(name, source) {
        panic!("{name} failed: {err}");
    }
}

/// Moves the stock book with the bundled movetoarchive plugin, then runs
/// `DocSettings:open(book):flush()` on the old path. `reader_settings` is the
/// text of `settings.reader.lua` before the host starts, if any.
fn archive_with_stock_sidecar(reader_settings: Option<&str>) {
    let data = TempDir::new("guard-archive");
    if let Some(text) = reader_settings {
        fs::write(data.path().join("settings.reader.lua"), text).unwrap();
    }
    let books = TempDir::new("guard-books");
    let archive = TempDir::new("guard-archive-folder");
    let book = stock_book(books.path());
    let mut expected = snapshot(books.path());

    let (mut ui, _) = lua_ui(&data, false);
    let library = Rc::new(RefCell::new(workflow::Library {
        folder: books.path().to_string_lossy().into(),
        opened: None,
    }));
    ui.set_library_host(library.clone()).unwrap();
    ui.set_reader_host(Rc::new(RefCell::new(BookReader::open_path(&book, &data))))
        .unwrap();
    workflow::prepare(&mut ui);
    ui.load_plugins(PluginOptions {
        names: vec!["movetoarchive".into()],
        ..Default::default()
    })
    .unwrap();
    ui.send_reader_event("ReaderReady", &[]);
    workflow::healthy(&ui);
    ui.exec(
        "archive-folder",
        &format!(
            "local P = KAIA_PLUGIN.instances.movetoarchive; P:loadSettings(); P.data.archive_dir_path = {:?}; P.updated = true",
            format!("{}/", archive.path().display())
        ),
    )
    .unwrap();
    ui.open_plugin_menu("move_to_archive").unwrap();
    workflow::tap(&mut ui, "Move current book to archive");
    workflow::tap(&mut ui, "Open");
    ui.send_reader_event("FlushSettings", &[]);
    workflow::healthy(&ui);

    let target = archive.path().join("juliet.epub");
    assert!(target.is_file(), "the book did not move to the archive");
    assert!(!book.exists(), "the source book still exists");
    assert_eq!(library.borrow().opened.as_deref(), target.to_str());

    set_path(ui.host(), "BOOK", &book);
    set_path(ui.host(), "TARGET", &target);
    ui.exec("flush", "require('docsettings'):open(BOOK):flush()")
        .unwrap();
    workflow::healthy(&ui);

    // The stock sidecar and its files are where KOReader left them. Only the
    // moved book differs.
    expected.remove(Path::new("juliet.epub"));
    assert_same_files(&snapshot(books.path()), &expected);
    assert_eq!(
        fs::read_to_string(books.path().join("juliet.sdr/metadata.epub.lua")).unwrap(),
        STOCK
    );
    // No sidecar appears next to the archived book.
    assert!(!archive.path().join("juliet.sdr").exists());
    assert_eq!(
        snapshot(archive.path()).keys().cloned().collect::<Vec<_>>(),
        vec![PathBuf::from("juliet.epub")]
    );
    // Kaia keeps its own copy under the data folder, and that copy holds the
    // state KOReader wrote.
    assert!(!snapshot(&data.path().join("docsettings")).is_empty());
    let percent = ui
        .eval(
            "read-target",
            "return require('docsettings'):open(TARGET):readSetting('percent_finished')",
        )
        .unwrap();
    assert_eq!(percent.as_f64(), Some(0.25));
    let location = ui
        .eval(
            "read-location",
            "return G_reader_settings:readSetting('document_metadata_folder')",
        )
        .unwrap();
    assert_eq!(location.as_str(), Some("dir"));
}

#[test]
fn movetoarchive_leaves_the_stock_sidecar_in_a_fresh_data_folder() {
    let _guard = serial();
    archive_with_stock_sidecar(None);
}

#[test]
fn movetoarchive_keeps_the_stock_sidecar_when_settings_say_doc() {
    let _guard = serial();
    archive_with_stock_sidecar(Some("return { [\"document_metadata_folder\"] = \"doc\" }\n"));
}

#[test]
fn movetoarchive_keeps_the_stock_sidecar_when_settings_say_hash() {
    let _guard = serial();
    archive_with_stock_sidecar(Some("return { [\"document_metadata_folder\"] = \"hash\" }\n"));
}

#[test]
fn plugin_writes_cannot_switch_the_metadata_folder() {
    let _guard = serial();
    let data = TempDir::new("guard-pin");
    let books = TempDir::new("guard-pin-books");
    let book = stock_book(books.path());
    let before = snapshot(books.path());
    let host = host(&data);
    set_path(&host, "BOOK", &book);
    set_path(&host, "DATA", data.path());
    check(
        &host,
        "pin.lua",
        r#"
        local DocSettings = require("docsettings")
        assert(G_reader_settings:readSetting("document_metadata_folder") == "dir")
        for _, value in ipairs({ "doc", "hash" }) do
            G_reader_settings:saveSetting("document_metadata_folder", value)
            assert(G_reader_settings:readSetting("document_metadata_folder") == "dir")
            G_reader_settings.data.document_metadata_folder = value
            assert(G_reader_settings:readSetting("document_metadata_folder", value) == "dir")
            local ds = DocSettings:open(BOOK)
            ds:saveSetting("percent_finished", 0.5)
            local written = ds:flush()
            assert(written:sub(1, #DATA) == DATA, "written next to the book: " .. tostring(written))
        end
        G_reader_settings:delSetting("document_metadata_folder")
        assert(G_reader_settings:readSetting("document_metadata_folder") == "dir")
        G_reader_settings:flush()
        "#,
    );
    assert_same_files(&snapshot(books.path()), &before);
    let saved = fs::read_to_string(data.path().join("settings.reader.lua")).unwrap();
    assert!(
        saved.contains("[\"document_metadata_folder\"] = \"dir\""),
        "settings.reader.lua after flush: {saved}"
    );
}

#[test]
fn rebinding_the_settings_global_does_not_move_docsettings_writes() {
    let _guard = serial();
    let data = TempDir::new("guard-rebind");
    let books = TempDir::new("guard-rebind-books");
    let book = stock_book(books.path());
    let before = snapshot(books.path());
    let host = host(&data);
    set_path(&host, "BOOK", &book);
    set_path(&host, "DATA", data.path());
    check(
        &host,
        "rebind.lua",
        r#"
        local DocSettings = require("docsettings")
        G_reader_settings = require("luasettings"):open(DATA .. "/plugin-settings.lua")
        G_reader_settings:saveSetting("document_metadata_folder", "doc")
        local written = DocSettings:open(BOOK):flush()
        assert(written:sub(1, #DATA) == DATA, "written next to the book: " .. tostring(written))
        "#,
    );
    assert_same_files(&snapshot(books.path()), &before);
}

/// A valid legacy KOReader file next to the book, `juliet.epub.kpdfview.lua`.
fn legacy_kpdfview(folder: &Path) -> PathBuf {
    let file = folder.join("juliet.epub.kpdfview.lua");
    fs::write(&file, "return { [\"page\"] = 3 }\n").unwrap();
    file
}

#[test]
fn reset_keeps_a_legacy_kpdfview_next_to_the_book() {
    let _guard = serial();
    let data = TempDir::new("guard-reset");
    let books = TempDir::new("guard-reset-books");
    let book = books.path().join("juliet.epub");
    fs::copy(repo_root().join("test/juliet.epub"), &book).unwrap();
    let kpdf = legacy_kpdfview(books.path());
    let before = snapshot(books.path());
    let host = host(&data);
    set_path(&host, "BOOK", &book);
    check(&host, "reset.lua", "require('docsettings'):open(BOOK):purge()");
    assert!(kpdf.is_file(), "the legacy kpdfview file was removed");
    assert_same_files(&snapshot(books.path()), &before);
}

#[test]
fn move_keeps_a_legacy_kpdfview_and_the_stock_sidecar() {
    let _guard = serial();
    let data = TempDir::new("guard-move-kpdf");
    let books = TempDir::new("guard-move-kpdf-books");
    let archive = TempDir::new("guard-move-kpdf-archive");
    let book = stock_book(books.path());
    let kpdf = legacy_kpdfview(books.path());
    let before = snapshot(books.path());
    let target = archive.path().join("juliet.epub");
    let host = host(&data);
    set_path(&host, "BOOK", &book);
    set_path(&host, "TARGET", &target);
    // The native move comes first, as the file manager does it.
    fs::rename(&book, &target).unwrap();
    check(
        &host,
        "move.lua",
        "require('docsettings').updateLocation(BOOK, TARGET)",
    );
    assert!(kpdf.is_file(), "the legacy kpdfview file was removed");
    let mut expected = before;
    expected.remove(Path::new("juliet.epub"));
    assert_same_files(&snapshot(books.path()), &expected);
    assert!(!archive.path().join("juliet.sdr").exists());
}

#[test]
fn move_removes_kaias_old_copy_and_keeps_the_stock_sidecar() {
    let _guard = serial();
    let data = TempDir::new("guard-own-copy");
    let books = TempDir::new("guard-own-copy-books");
    let archive = TempDir::new("guard-own-copy-archive");
    let book = stock_book(books.path());
    let target = archive.path().join("juliet.epub");
    let stock_sidecar = books.path().join("juliet.sdr/metadata.epub.lua");
    let mut expected = snapshot(books.path());
    expected.remove(Path::new("juliet.epub"));
    let host = host(&data);
    set_path(&host, "DATA", data.path());
    set_path(&host, "BOOK", &book);
    set_path(&host, "TARGET", &target);
    set_path(&host, "STOCK", &stock_sidecar);
    // Kaia writes its own copy for the old path, then the book moves.
    check(&host, "own-copy.lua", "require('docsettings'):open(BOOK):flush()");
    fs::rename(&book, &target).unwrap();
    check(
        &host,
        "move.lua",
        "require('docsettings').updateLocation(BOOK, TARGET)",
    );
    assert_same_files(&snapshot(books.path()), &expected);
    // Kaia's copy for the old path is gone, so only the stock sidecar is found
    // there. The copy for the new path is under the data folder.
    check(
        &host,
        "locations.lua",
        r#"
        local DocSettings = require("docsettings")
        local old = DocSettings:findSidecarFile(BOOK)
        assert(old == STOCK, "old path: " .. tostring(old))
        local new = DocSettings:findSidecarFile(TARGET)
        assert(new and new:sub(1, #DATA) == DATA, "new path: " .. tostring(new))
        "#,
    );
}

#[test]
fn unparsable_legacy_kpdfview_is_skipped_not_deleted() {
    let _guard = serial();
    let data = TempDir::new("guard-bad-kpdf");
    let books = TempDir::new("guard-bad-kpdf-books");
    let book = books.path().join("juliet.epub");
    fs::copy(repo_root().join("test/juliet.epub"), &book).unwrap();
    let kpdf = books.path().join("juliet.epub.kpdfview.lua");
    fs::write(&kpdf, "not valid Lua\n").unwrap();
    let host = host(&data);
    set_path(&host, "BOOK", &book);
    check(
        &host,
        "bad-kpdf.lua",
        "local ds = require('docsettings'):open(BOOK); assert(ds:readSetting('percent_finished') == nil)",
    );
    assert!(kpdf.is_file(), "the unparsable legacy kpdfview file was deleted");
    assert_eq!(fs::read_to_string(&kpdf).unwrap(), "not valid Lua\n");
}

/// Deletes a book whose stock sidecar names `cache_path(data, target)` as its
/// cache file. The file in the book folder that the path names must survive.
fn delete_keeps_named_file(label: &str, cache_path: impl Fn(&TempDir, &Path) -> PathBuf) {
    let data = TempDir::new(label);
    let books = TempDir::new(&format!("{label}-books"));
    let book = books.path().join("juliet.epub");
    fs::copy(repo_root().join("test/juliet.epub"), &book).unwrap();
    let sdr = books.path().join("juliet.sdr");
    fs::create_dir_all(&sdr).unwrap();
    let target = books.path().join("leftover.bin");
    fs::write(&target, "book folder").unwrap();
    let named = cache_path(&data, &target);
    fs::write(
        sdr.join("metadata.epub.lua"),
        format!(
            "return {{ [\"cache_file_path\"] = {:?}, [\"percent_finished\"] = 0.25 }}\n",
            named.display().to_string()
        ),
    )
    .unwrap();
    let before = snapshot(books.path());
    let host = host(&data);
    set_path(&host, "BOOK", &book);
    check(&host, "delete.lua", "require('docsettings').updateLocation(BOOK)");
    assert!(
        target.is_file(),
        "a book folder file named by the cache path was deleted"
    );
    assert_same_files(&snapshot(books.path()), &before);
}

#[test]
fn delete_keeps_a_stock_cache_path_outside_the_data_folder() {
    let _guard = serial();
    delete_keeps_named_file("guard-cache-abs", |_, target| target.to_path_buf());
}

#[test]
fn delete_keeps_a_stock_cache_path_that_climbs_out_of_the_data_folder() {
    let _guard = serial();
    // Written as <data>/cache/../../<book folder>/leftover.bin. The cache folder
    // exists once the host starts, so the path names the book folder.
    delete_keeps_named_file("guard-cache-climb", |data, target| {
        let folder = target.parent().unwrap().file_name().unwrap().to_str().unwrap();
        let file = target.file_name().unwrap().to_str().unwrap();
        PathBuf::from(format!("{}/cache/../../{folder}/{file}", data.path().display()))
    });
}
