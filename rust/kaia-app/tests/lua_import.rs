//! A Lua profile imports through the same plan used by Settings.
mod common;

use kaia_app::lua_import::Plan;
use kaia_events::{Settings, Value};
use kaia_library::Library;
use kaia_storage::{Anchor, DocumentId, Sticker, StickerSize, StickerState, Store};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};

struct Fixture {
    dir: tempfile::TempDir,
    source: PathBuf,
    data: PathBuf,
    book: PathBuf,
    library: Library,
    settings: Settings,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let books = common::library_with_books(dir.path());
        let book = fs::canonicalize(books.join("juliet.epub")).unwrap();
        let source = books.join(".adds/kaia");
        let data = dir.path().join("data");
        fs::create_dir_all(source.join("settings")).unwrap();
        fs::create_dir_all(&data).unwrap();
        fs::write(
            source.join("settings.reader.lua"),
            r#"return {
            cre_font = "Noto Serif", copt_font_size = 26, copt_line_spacing = 120,
            copt_h_page_margins = {12, 14}, frontlight_intensity = 35,
            frontlight_warmth = 40, input_invert_page_turn_keys = true,
            highlight_drawing_style = "underscore", highlight_color = "green",
            readtimer = { duration = 25 }, unknown_key = "must stay out",
            dicts_order = { ["missing.ifo"] = 1 }
        }"#,
        )
        .unwrap();
        fs::write(
            source.join("history.lua"),
            format!(
                "return {{ {{ file = {:?}, time = 1700000000 }} }}",
                book.to_str().unwrap()
            ),
        )
        .unwrap();
        fs::write(
            source.join("settings/collection.lua"),
            format!(
                "return {{ Favorites = {{ {{ file = {:?}, order = 1 }} }} }}",
                book.to_str().unwrap()
            ),
        )
        .unwrap();
        fs::write(
            source.join("settings/profiles.lua"),
            "return { Quiet = { font_size = 25 } }",
        )
        .unwrap();
        let sidecar = source
            .join("docsettings")
            .join(book.strip_prefix("/").unwrap())
            .with_extension("sdr")
            .join("metadata.epub.lua");
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        fs::write(
            sidecar,
            r#"return {
            last_xpointer = "/body/DocFragment[1]/body/p[1].0", percent_finished = 0.42,
            summary = { status = "reading", rating = 4, note = "Review" },
            annotations = {
                { page = "/body/DocFragment[1]/body/p[1].0", pos0 = "/body/DocFragment[1]/body/p[1].0",
                  pos1 = "/body/DocFragment[1]/body/p[1].5", drawer = "lighten", color = "yellow",
                  text = "Romeo", note = "Keep this", datetime = "2026-01-01 10:00:00" },
                { page = "/body/DocFragment[1]/body/p[2].0", datetime = "2026-01-01 10:01:00" }
            }
        }"#,
        )
        .unwrap();
        let stats = Connection::open(source.join("settings/statistics.sqlite3")).unwrap();
        stats.execute_batch("PRAGMA user_version=20221111;
            CREATE TABLE book (id INTEGER PRIMARY KEY,title TEXT,authors TEXT,md5 TEXT,notes INTEGER,last_open INTEGER,highlights INTEGER,pages INTEGER,series TEXT,language TEXT);
            CREATE TABLE page_stat_data (id_book INTEGER,page INTEGER,start_time INTEGER,duration INTEGER,total_pages INTEGER);
            INSERT INTO book VALUES (1,'Juliet','Shakespeare','abc',1,1700000000,1,100,NULL,'en');
            INSERT INTO page_stat_data VALUES (1,1,1700000000,30,100),(1,2,1700000040,40,100);").unwrap();
        drop(stats);
        let mut store = Store::open(&source.join("kaia")).unwrap();
        let image = dir.path().join("image.png");
        kaia_gfx::png::write_pixmap(&kaia_gfx::Pixmap::new(2, 2, kaia_gfx::PixelFormat::Rgb24), &image)
            .unwrap();
        let asset = store.add_asset_file(&image, "CC0").unwrap();
        store
            .put_sticker(&Sticker {
                id: None,
                doc: DocumentId {
                    partial_md5: kaia_book::partial_md5(&book).unwrap(),
                    file_size: fs::metadata(&book).unwrap().len(),
                    title: Some("Juliet".into()),
                },
                anchor: Anchor::Book {},
                asset: Some(asset.sha256),
                text: Some("Remember".into()),
                size: StickerSize { w: 0.1, h: 0.1 },
                rotation: 0,
                state: StickerState::Attached,
                created_at: 1700000000,
                updated_at: 1700000000,
            })
            .unwrap();
        store
            .add_word("Romeo", Some("Juliet"), Some("A context"), 1700000000)
            .unwrap();
        drop(store);
        let library = Library::open(&data.join("library.sqlite3")).unwrap();
        Self {
            dir,
            source,
            data,
            book,
            library,
            settings: Settings::new(),
        }
    }
    fn plan(&self) -> Plan {
        Plan::prepare(&self.source, &self.data, &self.library).unwrap()
    }
    fn state_path(&self) -> PathBuf {
        self.data.join(format!(
            "kaia/books/{}.json",
            kaia_book::partial_md5(&self.book).unwrap()
        ))
    }
}

fn snapshot(root: &Path) -> Vec<(PathBuf, Vec<u8>, std::time::SystemTime)> {
    let mut out = Vec::new();
    for e in fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(snapshot(&p));
        } else {
            out.push((
                p.clone(),
                fs::read(&p).unwrap(),
                fs::metadata(p).unwrap().modified().unwrap(),
            ));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn lua_plan_import_repeat_and_source_preservation() {
    let mut f = Fixture::new();
    let before = snapshot(&f.source);
    let plan = f.plan();
    for (kind, count) in [
        ("Settings", 10),
        ("History entries", 1),
        ("Collection entries", 1),
        ("Book states", 1),
        ("Highlights", 1),
        ("Bookmarks", 1),
        ("Notes", 1),
        ("Stickers", 1),
        ("Assets", 1),
        ("Statistics books", 1),
        ("Page records", 2),
        ("Plugin data", 2),
        ("Words", 1),
    ] {
        assert_eq!(plan.counts.get(kind), count, "{kind}");
    }
    assert!(!f.state_path().exists());
    assert!(!f.data.join("statistics.sqlite3").exists());
    assert_eq!(snapshot(&f.source), before);
    let counts = plan.counts.clone();
    let report = plan.import(&f.data, &mut f.library, &mut f.settings).unwrap();
    assert_eq!(report.imported, counts);
    assert_eq!(report.skipped.total(), 0);
    let indexed = f.library.book_by_path(&f.book).unwrap().unwrap();
    assert_eq!(indexed.percent, 0.42);
    assert_eq!(indexed.status, kaia_library::Status::Reading);
    let state = Settings::read_exact(&f.state_path()).unwrap().unwrap();
    assert_eq!(state.read("layout").get("font_size").as_f64(), Some(55.0));
    assert_eq!(
        state.read("layout").get("line_spacing").as_f64(),
        Some(f64::from(1.2f32))
    );
    assert_eq!(state.read("annotations").array_len(), 2);
    assert_eq!(state.read("percent_finished").as_f64(), Some(0.42));
    assert_eq!(
        state.read("last_xpointer").as_str(),
        Some("/body/DocFragment[1]/body/p[1].0")
    );
    assert_eq!(state.read("summary").get("note").as_str(), Some("Review"));
    assert_eq!(f.settings.read("reading.font_size").as_f64(), Some(26.0));
    assert_eq!(f.settings.read("reading.line_spacing").as_f64(), Some(1.2));
    assert_eq!(f.settings.read("reading.margin_right").as_f64(), Some(14.0));
    assert!(!f.settings.has("unknown_key"));
    assert_eq!(
        state.read("annotations").get(1i64).get("note").as_str(),
        Some("Keep this")
    );
    assert!(state.read("annotations").get(2i64).get("drawer").is_nil());
    let stickers = Store::open(&f.data.join("kaia")).unwrap().list_all().unwrap();
    assert_eq!(stickers.len(), 1);
    assert_eq!(stickers[0].text.as_deref(), Some("Remember"));
    for asset in fs::read_dir(f.source.join("kaia/assets")).unwrap() {
        let asset = asset.unwrap();
        assert_eq!(
            fs::read(asset.path()).unwrap(),
            fs::read(f.data.join("kaia/assets").join(asset.file_name())).unwrap()
        );
    }
    let words = Store::open(&f.data.join("kaia")).unwrap().list_words().unwrap();
    assert_eq!(words[0].context.as_deref(), Some("A context"));
    let mut stats = kaia_stats::Stats::open(f.data.join("statistics.sqlite3")).unwrap();
    assert_eq!(stats.books().unwrap()[0].seconds, 70);
    drop(stats);
    assert_eq!(snapshot(&f.source), before);
    let second = f.plan().import(&f.data, &mut f.library, &mut f.settings).unwrap();
    assert_eq!(second.imported.total(), 0);
    assert_eq!(second.skipped, counts);
    assert_eq!(snapshot(&f.source), before);
}

#[test]
fn lua_snapshot_is_fixed_and_existing_rust_state_wins() {
    let mut f = Fixture::new();
    let plan = f.plan();
    fs::write(
        f.source.join("settings.reader.lua"),
        "return { copt_font_size = 19 }",
    )
    .unwrap();
    plan.import(&f.data, &mut f.library, &mut f.settings).unwrap();
    assert_eq!(f.settings.read("reading.font_size").as_f64(), Some(26.0));
    f.settings.save("reading.font_size", 31.0.into());
    let mut state = Settings::open(&f.state_path());
    state.save("annotations", Value::array([]));
    state.save("percent_finished", 0.75.into());
    state.flush(&f.state_path()).unwrap();
    f.plan().import(&f.data, &mut f.library, &mut f.settings).unwrap();
    assert_eq!(f.settings.read("reading.font_size").as_f64(), Some(31.0));
    let state = Settings::open(&f.state_path());
    assert_eq!(state.read("annotations").array_len(), 0);
    assert_eq!(state.read("percent_finished").as_f64(), Some(0.75));
}

#[test]
fn lua_shared_desktop_store_is_not_written() {
    let f = Fixture::new();
    let before = snapshot(&f.source.join("kaia"));
    let plan = Plan::prepare(&f.source, &f.source, &f.library).unwrap();
    assert!(plan.shared_store);
    let mut library = Library::open(&f.source.join("library.sqlite3")).unwrap();
    let mut settings = Settings::new();
    let report = plan.import(&f.source, &mut library, &mut settings).unwrap();
    assert_eq!(report.imported.get("Stickers"), 0);
    assert_eq!(report.skipped.get("Stickers"), 1);
    // Book JSON files are new Rust data. The existing SQLite and assets keep their bytes and times.
    for (p, bytes, time) in before {
        assert_eq!(fs::read(&p).unwrap(), bytes);
        assert_eq!(fs::metadata(p).unwrap().modified().unwrap(), time);
    }
}

#[test]
fn lua_settings_preview_cancel_import_and_report() {
    let _serial = common::SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let f = Fixture::new();
    let before = snapshot(&f.source);
    let mut app = common::make_app(f.dir.path(), Vec::new());
    app.services_mut().paths.books_dir = f.dir.path().join("books");
    app.start().unwrap();
    let has_import = |app: &kaia_app::App<kaia_app::HeadlessBackend>| {
        kaia_app::surfaces::settings::sections::entries("data", app.services(), 0).iter().any(|e| matches!(e, kaia_app::surfaces::settings::entries::Entry::Button {action,..} if action == "settings:data:lua"))
    };
    assert!(has_import(&app));
    app.services_mut().paths.books_dir = f.dir.path().join("absent");
    assert!(!has_import(&app));
    app.services_mut().paths.books_dir = f.dir.path().join("books");
    app.command("nav:settings", &[]);
    app.command("settings:data", &[]);
    app.command("settings:data:lua", &[]);
    app.process(Vec::new()).unwrap();
    assert!(app.notices().is_empty(), "{:?}", app.notices());
    let preview = common::golden("settings_lua_plan.png");
    kaia_gfx::golden::assert_matches_golden(app.frame(), &preview, 14);
    assert!(!f.state_path().exists());
    app.command("settings:data:lua:close", &[]);
    assert!(!f.state_path().exists());
    app.command("settings:data:lua", &[]);
    app.command("settings:data:lua:confirm", &[]);
    app.process(Vec::new()).unwrap();
    assert!(app.notices().is_empty(), "{:?}", app.notices());
    assert!(f.state_path().is_file());
    let report = common::golden("settings_lua_report.png");
    kaia_gfx::golden::assert_matches_golden(app.frame(), &report, 14);
    assert_eq!(snapshot(&f.source), before);
}

#[test]
fn lua_statistics_wal_and_plugin_data_keep_source_files() {
    let mut f = Fixture::new();
    let source = Connection::open(f.source.join("settings/statistics.sqlite3")).unwrap();
    source.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; INSERT INTO page_stat_data VALUES (1,3,1700000080,20,100);").unwrap();
    let before = snapshot(&f.source);
    assert!(before
        .iter()
        .any(|(p, bytes, _)| p.to_string_lossy().ends_with("-wal") && !bytes.is_empty()));
    let plan = f.plan();
    assert_eq!(plan.counts.get("Page records"), 3);
    let report = plan.import(&f.data, &mut f.library, &mut f.settings).unwrap();
    assert_eq!(report.imported.get("Page records"), 3);
    let plugin =
        kaia_luadata::parse(&fs::read_to_string(f.data.join("settings.reader.lua")).unwrap()).unwrap();
    assert_eq!(plugin.get("readtimer").get("duration").as_i64(), Some(25));
    assert!(plugin.get("cre_font").is_nil());
    assert_eq!(
        fs::read(f.data.join("settings/profiles.lua")).unwrap(),
        fs::read(f.source.join("settings/profiles.lua")).unwrap()
    );
    assert_eq!(snapshot(&f.source), before);
    drop(source);
}

#[test]
fn lua_dictionary_order_uses_booknames_and_supported_settings_only() {
    let mut f = Fixture::new();
    let folder = f.source.join("data/dict");
    fs::create_dir_all(&folder).unwrap();
    for (stem, name) in [("a", "First dictionary"), ("b", "Second dictionary")] {
        fs::write(folder.join(format!("{stem}.ifo")), format!("StarDict's dict ifo file\nversion=2.4.2\nbookname={name}\nwordcount=1\nidxfilesize=13\nsametypesequence=m\n")).unwrap();
        let mut index = b"word\0".to_vec();
        index.extend_from_slice(&0u32.to_be_bytes());
        index.extend_from_slice(&4u32.to_be_bytes());
        fs::write(folder.join(format!("{stem}.idx")), index).unwrap();
        fs::write(folder.join(format!("{stem}.dict")), b"text").unwrap();
    }
    fs::write(
        f.source.join("settings.reader.lua"),
        r#"return {
        dicts_order = { ["data/dict/b.ifo"] = 1, ["data/dict/a.ifo"] = 2 },
        copt_t_page_margin = 19, copt_b_page_margin = 23, full_refresh_count = 6,
        copt_font_size = "bad", unknown = true, highlight_color = "invalid"
    }"#,
    )
    .unwrap();
    let before = snapshot(&f.source);
    f.plan().import(&f.data, &mut f.library, &mut f.settings).unwrap();
    assert_eq!(
        f.settings.read("dictionary.order"),
        &Value::array(["Second dictionary".into(), "First dictionary".into()])
    );
    assert_eq!(f.settings.read("reading.margin_top").as_i64(), Some(19));
    assert_eq!(f.settings.read("reading.margin_bottom").as_i64(), Some(23));
    assert_eq!(f.settings.read("reading.full_refresh_pages").as_i64(), Some(6));
    assert!(!f.settings.has("reading.font_size"));
    assert!(!f.settings.has("reading.highlight_color"));
    assert!(!f.settings.has("unknown"));
    assert_eq!(snapshot(&f.source), before);
}

#[cfg(unix)]
#[test]
fn lua_import_refuses_a_target_symlink_into_source() {
    let mut f = Fixture::new();
    std::os::unix::fs::symlink(f.source.join("kaia"), f.data.join("kaia")).unwrap();
    let before = snapshot(&f.source);
    assert!(f.plan().import(&f.data, &mut f.library, &mut f.settings).is_err());
    assert_eq!(snapshot(&f.source), before);
}

#[test]
fn lua_unsupported_annotation_shapes_are_counted_as_skipped() {
    let mut f = Fixture::new();
    let sidecar = f
        .source
        .join("docsettings")
        .join(f.book.strip_prefix("/").unwrap())
        .with_extension("sdr")
        .join("metadata.epub.lua");
    let text = fs::read_to_string(&sidecar)
        .unwrap()
        .replace("annotations = {", "annotations = { [3] = { page = 12 },");
    fs::write(sidecar, text).unwrap();
    let before = snapshot(&f.source);
    let plan = f.plan();
    assert_eq!(plan.counts.get("Other annotations"), 1);
    let report = plan.import(&f.data, &mut f.library, &mut f.settings).unwrap();
    assert_eq!(report.imported.get("Highlights"), 1);
    assert_eq!(report.imported.get("Bookmarks"), 1);
    assert_eq!(report.skipped.get("Other annotations"), 1);
    assert_eq!(snapshot(&f.source), before);
}

#[test]
fn lua_supported_plugin_files_and_scalar_settings_are_repeatable() {
    let mut f = Fixture::new();
    for name in [
        "gestures",
        "cloudstorage",
        "battery_stats",
        "kosync",
        "news_settings",
        "directory_defaults",
    ] {
        fs::write(
            f.source.join(format!("settings/{name}.lua")),
            "return { saved = true }",
        )
        .unwrap();
    }
    fs::write(f.source.join("settings.reader.lua"), "return { autoturn_distance = 2, language_japanese_text_scan_length = 30, profiles_autoexec = { Start = true } }").unwrap();
    let before = snapshot(&f.source);
    let plan = f.plan();
    assert_eq!(plan.counts.get("Plugin data"), 10);
    let first = plan.import(&f.data, &mut f.library, &mut f.settings).unwrap();
    assert_eq!(first.imported.get("Plugin data"), 10);
    let plugin =
        kaia_luadata::parse(&fs::read_to_string(f.data.join("settings.reader.lua")).unwrap()).unwrap();
    assert_eq!(plugin.get("autoturn_distance").as_i64(), Some(2));
    assert!(!f.settings.has("autoturn_distance"));
    assert_eq!(
        f.plan()
            .import(&f.data, &mut f.library, &mut f.settings)
            .unwrap()
            .imported
            .total(),
        0
    );
    assert_eq!(snapshot(&f.source), before);
}

#[test]
fn lua_sidecars_for_books_in_an_exports_folder_are_imported() {
    let mut f = Fixture::new();
    let old_sidecar = f
        .source
        .join("docsettings")
        .join(f.book.strip_prefix("/").unwrap())
        .with_extension("sdr")
        .join("metadata.epub.lua");
    let book = f.book.parent().unwrap().join("exports/juliet.epub");
    fs::create_dir_all(book.parent().unwrap()).unwrap();
    fs::copy(&f.book, &book).unwrap();
    f.book = book;
    let new_sidecar = f
        .source
        .join("docsettings")
        .join(f.book.strip_prefix("/").unwrap())
        .with_extension("sdr")
        .join("metadata.epub.lua");
    fs::create_dir_all(new_sidecar.parent().unwrap()).unwrap();
    fs::rename(old_sidecar, new_sidecar).unwrap();
    let before = snapshot(&f.source);
    let plan = f.plan();
    assert_eq!(plan.counts.get("Book states"), 1);
    assert_eq!(
        plan.import(&f.data, &mut f.library, &mut f.settings)
            .unwrap()
            .imported
            .get("Book states"),
        1
    );
    assert_eq!(snapshot(&f.source), before);
}
