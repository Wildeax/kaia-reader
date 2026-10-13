mod common;

use common::{book::BookReader, lua_ui, serial, workflow::prepare, TempDir};
use kaia_lua::{LuaUi, PluginOptions};
use std::{cell::RefCell, path::PathBuf, rc::Rc};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/community")
}

fn copy_fixture(source: &std::path::Path, target: &std::path::Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let dest = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_fixture(&entry.path(), &dest);
        } else {
            std::fs::copy(entry.path(), dest).unwrap();
        }
    }
}

fn load(data: &TempDir, book: &str, names: &[&str]) -> LuaUi {
    let plugins = data.path().join("plugins");
    for name in names {
        let folder = format!("{name}.koplugin");
        copy_fixture(&fixtures().join(&folder), &plugins.join(&folder));
    }
    let reader = BookReader::open(book, data);
    let (mut ui, _) = lua_ui(data, false);
    ui.set_reader_host(Rc::new(RefCell::new(reader))).unwrap();
    prepare(&mut ui);
    ui.load_plugins(PluginOptions {
        names: names.iter().map(|name| (*name).into()).collect(),
        extra_paths: vec![plugins],
        ..Default::default()
    })
    .unwrap();
    ui
}

use common::workflow::{close_windows, healthy, tap, type_text, value};
use kaia_events::Value;

#[test]
fn pagenotes_adds_and_paints_a_pdf_note() {
    let _guard = serial();
    let data = TempDir::new("pagenotes");
    let mut reader = BookReader::open("sample.pdf", &data);
    let original = reader.render();
    drop(reader);
    let mut ui = load(&data, "sample.pdf", &["pagenotes"]);
    ui.send_reader_event("ReaderReady", &[]);
    ui.open_plugin_menu("pagenotes").unwrap();
    tap(&mut ui, "Add note or sticker");
    tap(&mut ui, "Text note");
    type_text(&mut ui, "A page note");
    tap(&mut ui, "Place");
    tap(&mut ui, "Done");
    close_windows(&mut ui);
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.pagenotes.notes[1].text"),
        Value::from("A page note")
    );
    let mut page = original.clone();
    ui.paint_reader_overlays(&mut page).unwrap();
    healthy(&ui);
    assert_ne!(page.data(), original.data());
    ui.exec("turn-page", "require('apps/reader/readerui').paging:gotoPage(2)")
        .unwrap();
    let mut other_page = original.clone();
    ui.paint_reader_overlays(&mut other_page).unwrap();
    healthy(&ui);
    assert_eq!(other_page.data(), original.data());
    ui.exec("flush", "require('apps/reader/readerui').doc_settings:flush()")
        .unwrap();
    assert_eq!(
        value(&ui, "KAIA_READER.get_setting('pagenotes')[1].text"),
        Value::from("A page note")
    );
    ui.send_reader_event("CloseDocument", &[]);
    healthy(&ui);
}

#[test]
fn readingstreak_tracks_real_page_turns() {
    let _guard = serial();
    let data = TempDir::new("readingstreak");
    let mut ui = load(&data, "juliet.epub", &["readingstreak"]);
    ui.send_reader_event("ReaderReady", &[]);
    ui.send_reader_event("PageUpdate", &[Value::from(1i64)]);
    ui.exec("turn-page", "require('apps/reader/readerui').rolling:gotoPage(2)")
        .unwrap();
    ui.send_reader_event("PageUpdate", &[Value::from(2i64)]);
    assert_eq!(
        value(
            &ui,
            "KAIA_PLUGIN.instances.readingstreak.settings.daily_progress.pages"
        ),
        Value::from(1i64)
    );
    ui.open_plugin_menu("reading_streak").unwrap();
    tap(&mut ui, "View Streak");
    healthy(&ui);
    assert!(ui.window_count() > 0);
    close_windows(&mut ui);
    ui.open_plugin_menu("reading_streak").unwrap();
    tap(&mut ui, "Calendar View");
    healthy(&ui);
    close_windows(&mut ui);
    drop(ui);
    let ui = load(&data, "juliet.epub", &["readingstreak"]);
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.readingstreak.settings.current_streak"),
        Value::from(1i64)
    );
    healthy(&ui);
}

#[test]
fn bookends_paints_an_overlay() {
    let _guard = serial();
    let data = TempDir::new("bookends");
    let mut reader = BookReader::open("sample.pdf", &data);
    let original = reader.render();
    drop(reader);
    let mut ui = load(&data, "sample.pdf", &["bookends"]);
    ui.send_reader_event("ReaderReady", &[]);
    ui.open_plugin_menu("bookends").unwrap();
    tap(&mut ui, "Bookends settings");
    tap(&mut ui, "Enable bookends");
    ui.frame();
    healthy(&ui);

    close_windows(&mut ui);
    let mut page = original.clone();
    ui.paint_reader_overlays(&mut page).unwrap();
    healthy(&ui);
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.bookends._paint_error_count or 0"),
        Value::from(0i64)
    );
    assert!(page.data() != original.data(), "overlay did not change the page");
    ui.send_reader_event("FlushSettings", &[]);
    healthy(&ui);
    drop(ui);
    let ui = load(&data, "sample.pdf", &["bookends"]);
    assert!(value(&ui, "KAIA_PLUGIN.instances.bookends.enabled").truthy());
    let mut page = original.clone();
    ui.paint_reader_overlays(&mut page).unwrap();
    healthy(&ui);
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.bookends._paint_error_count or 0"),
        Value::from(0i64)
    );
    assert!(page.data() != original.data());
}

fn blocked(ui: &LuaUi, name: &str, reason: &str) {
    let statuses = ui.plugin_status();
    let status = statuses.iter().find(|status| status.name == name).unwrap();
    assert!(!status.loaded, "{statuses:?}");
    assert!(status.error.as_deref().unwrap().contains(reason), "{statuses:?}");
    assert!(ui.take_errors().is_empty());
}

#[test]
fn pencil_requires_stylus_event_delivery() {
    let _guard = serial();
    let data = TempDir::new("pencil");
    let ui = load(&data, "juliet.epub", &["pencil"]);
    blocked(&ui, "pencil", "ReaderUI.active_widgets is unavailable");
    assert!(value(&ui, "next(KAIA_PLUGIN.view_modules) == nil").truthy());
}

#[test]
fn anki_requires_dictionary_buttons() {
    let _guard = serial();
    let data = TempDir::new("anki");
    let ui = load(&data, "juliet.epub", &["anki"]);
    blocked(&ui, "anki", "attempt to index field 'dictionary'");
}

#[test]
fn foldermemory_requires_library_display_hooks() {
    let _guard = serial();
    let data = TempDir::new("foldermemory");
    let plugins = data.path().join("plugins");
    copy_fixture(
        &fixtures().join("foldermemory.koplugin"),
        &plugins.join("foldermemory.koplugin"),
    );
    std::fs::copy(
        common::repo_root().join("test/juliet.epub"),
        data.path().join("juliet.epub"),
    )
    .unwrap();
    let (mut ui, _) = lua_ui(&data, false);
    ui.set_library_host(Rc::new(RefCell::new(common::workflow::Library {
        folder: data.path().to_string_lossy().into(),
        opened: None,
    })))
    .unwrap();
    prepare(&mut ui);
    ui.load_plugins(PluginOptions {
        names: vec!["foldermemory".into()],
        extra_paths: vec![plugins],
        ..Default::default()
    })
    .unwrap();
    blocked(&ui, "foldermemory", "FileManager.file_chooser is unavailable");
}

#[test]
fn footcream_requires_document_text_search() {
    let _guard = serial();
    let data = TempDir::new("footcream");
    let mut ui = load(&data, "juliet.epub", &["footcream"]);
    healthy(&ui);
    // ReaderReady starts the plugin's document setup before scanning units.
    ui.send_reader_event("ReaderReady", &[]);
    blocked(&ui, "footcream", "document.findAllText is unavailable");
    assert!(value(&ui, "next(KAIA_PLUGIN.menus) == nil").truthy());
}

#[test]
fn highlightsync_opens_settings_then_rejects_sidecar_sync() {
    let _guard = serial();
    let data = TempDir::new("highlightsync");
    let mut ui = load(&data, "juliet.epub", &["highlightsync"]);
    ui.send_reader_event("ReaderReady", &[]);
    ui.open_plugin_menu("highlight_sync").unwrap();
    tap(&mut ui, "Sync Cloud");
    healthy(&ui);
    assert!(ui.window_count() > 0);
    close_windows(&mut ui);
    ui.exec("sync-fixture", r#"
        local R = require('apps/reader/readerui')
        local xp = R.document:getXPointer()
        R.annotation:addItem{page=xp, pos0=xp, pos1=xp, text='A highlight', note='A note', drawer='lighten'}
        KAIA_PLUGIN.instances.highlightsync.settings.sync_server = {type='webdav', url='https://example.invalid'}
    "#).unwrap();
    ui.open_plugin_menu("highlight_sync").unwrap();
    // The main action must stop before creating a KOReader sidecar or contacting a server.
    ui.send_reader_event("SyncBookHighlights", &[]);
    blocked(&ui, "highlightsync", "doc_settings.getSidecarDir is unavailable");
    assert_eq!(
        value(&ui, "#require('apps/reader/readerui').annotation.annotations"),
        Value::from(1i64)
    );
}

#[test]
fn readeck_opens_settings_and_requires_a_server() {
    let _guard = serial();
    let data = TempDir::new("readeck");
    let mut ui = load(&data, "juliet.epub", &["readeck"]);
    ui.send_reader_event("ReaderReady", &[]);
    ui.open_plugin_menu("readeck").unwrap();
    tap(&mut ui, "Settings");
    healthy(&ui);
    close_windows(&mut ui);
    ui.open_plugin_menu("readeck").unwrap();
    tap(&mut ui, "Synchronize articles with server");
    healthy(&ui);
    ui.run_due();
    healthy(&ui);
    assert_eq!(
        value(&ui, "require('ui/uimanager'):getTopmostVisibleWidget().text"),
        Value::from("Please configure the server settings and set a download folder.")
    );
    assert!(!value(&ui, "KAIA_PLUGIN.instances.readeck.sync_in_progress").truthy());
}

#[test]
fn safe_mode_excludes_the_pinned_community_list() {
    let _guard = serial();
    let data = TempDir::new("community-safe");
    let (mut ui, _) = lua_ui(&data, false);
    ui.set_reader_host(Rc::new(RefCell::new(BookReader::open("juliet.epub", &data))))
        .unwrap();
    ui.load_plugins(PluginOptions {
        names: [
            "pencil",
            "pagenotes",
            "footcream",
            "anki",
            "highlightsync",
            "readingstreak",
            "bookends",
            "readeck",
            "foldermemory",
        ]
        .map(String::from)
        .into(),
        extra_paths: vec![fixtures()],
        safe_mode: true,
        ..Default::default()
    })
    .unwrap();
    assert!(ui.plugin_status().is_empty());
    assert!(value(&ui, "next(KAIA_PLUGIN.instances) == nil").truthy());
    healthy(&ui);
}
