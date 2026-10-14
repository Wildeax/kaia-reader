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
    blocked(&ui, "pencil", "Pencil needs the native ink host");
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
fn footcream_scans_epub_and_paints_a_tappable_conversion() {
    let _guard = serial();
    let data = TempDir::new("footcream");
    let book = data.path().join("juliet.epub");
    std::fs::copy(common::repo_root().join("test/juliet.epub"), &book).unwrap();
    let original = std::fs::read(&book).unwrap();
    let sidecar = data.path().join("juliet.sdr");
    std::fs::create_dir(&sidecar).unwrap();
    std::fs::write(sidecar.join("untouched.txt"), "KOReader sidecar").unwrap();
    let mut ui = load(&data, book.to_str().unwrap(), &["footcream"]);
    healthy(&ui);
    ui.send_reader_event("ReaderReady", &[]);
    healthy(&ui);
    ui.exec(
        "scan",
        "KAIA_PLUGIN.instances.footcream:_startScan(require('apps/reader/readerui').document)",
    )
    .unwrap();
    healthy(&ui);
    assert!(value(&ui, "#KAIA_PLUGIN.instances.footcream._all_matches > 0").truthy());
    ui.exec(
        "show-match",
        r#"
        local R = require('apps/reader/readerui')
        local hit = KAIA_PLUGIN.instances.footcream._all_matches[1]
        local page = R.document:getPageFromXPointer(hit.start)
        assert(page > 1, 'scan must find a unit beyond the first page')
        R.rolling:gotoPage(page)
        local all, n = R.document:findAllText('feet|mile', true, 3, 100, true)
        assert(all and n > 1 and all[1].matched_text and all[1].prev_text)
    "#,
    )
    .unwrap();
    close_windows(&mut ui);
    let mut page = kaia_gfx::Pixmap::new(
        common::UI_SCREEN.w,
        common::UI_SCREEN.h,
        kaia_gfx::PixelFormat::Gray8,
    );
    page.fill_rect(
        kaia_gfx::Rect::from_size(common::UI_SCREEN),
        kaia_gfx::Color::WHITE,
    );
    let clean = page.clone();
    ui.paint_reader_overlays(&mut page).unwrap();
    assert_ne!(page.data(), clean.data(), "the conversion underline must paint");
    ui.exec(
        "conversion-box",
        r#"
        local b = KAIA_PLUGIN.instances.footcream._current_boxes[1].box
        HIT_X, HIT_Y = b.x + b.w / 2, b.y + b.h / 2
    "#,
    )
    .unwrap();
    let x = value(&ui, "HIT_X").as_f64().unwrap() as i32;
    let y = value(&ui, "HIT_Y").as_f64().unwrap() as i32;
    assert!(ui.reader_input(&kaia_input::Gesture::Tap {
        pos: kaia_gfx::Point::new(x, y),
        time_us: 0
    }));
    assert!(ui.window_count() > 0, "tap must show the conversion");
    healthy(&ui);
    ui.exec(
        "protected-book",
        r#"
        local p = KAIA_PLUGIN.instances.footcream
        p:_applyMetricEdition(p.ui.document, true)
        p:_doApplyMetricEdition(p.ui.document)
        p:_revertMetricEdition(p.ui.document)
    "#,
    )
    .unwrap();
    assert_eq!(std::fs::read(&book).unwrap(), original);
    assert_eq!(std::fs::read_dir(&sidecar).unwrap().count(), 1);
    assert_eq!(
        std::fs::read_to_string(sidecar.join("untouched.txt")).unwrap(),
        "KOReader sidecar"
    );
    assert!(std::fs::read_dir(data.path().join("footcream")).unwrap().count() > 0);
    drop(ui);
    let mut ui = load(&data, book.to_str().unwrap(), &["footcream"]);
    ui.send_reader_event("ReaderReady", &[]);
    assert!(value(&ui, "#KAIA_PLUGIN.instances.footcream._all_matches > 0").truthy());
    healthy(&ui);
}

#[test]
fn highlightsync_local_merge_keeps_host_ids_and_reopens() {
    let _guard = serial();
    let data = TempDir::new("highlightsync");
    let book = data.path().join("juliet.epub");
    std::fs::copy(common::repo_root().join("test/juliet.epub"), &book).unwrap();
    let sidecar = data.path().join("juliet.sdr");
    std::fs::create_dir(&sidecar).unwrap();
    let marker = sidecar.join("untouched.txt");
    std::fs::write(&marker, "KOReader owns this folder").unwrap();
    let original = std::fs::read(&book).unwrap();
    let mut ui = load(&data, book.to_str().unwrap(), &["highlightsync"]);
    ui.send_reader_event("ReaderReady", &[]);
    ui.open_plugin_menu("highlight_sync").unwrap();
    tap(&mut ui, "Sync Cloud");
    healthy(&ui);
    close_windows(&mut ui);
    ui.exec(
        "local-sync",
        r#"
        local R = require('apps/reader/readerui')
        local json = require('rapidjson')
        local xp = R.document:getXPointer()
        R.annotation:addItem{page=xp, pos0=xp, pos1=xp, text='A highlight', note='A note', drawer='lighten'}
        local original = R.annotation.annotations[1]
        SAVED_ID = original.kaia_id
        SidecarDir = R.doc_settings:getSidecarDir(R.document.file)
        local data = require("datastorage"):getDataDir()
        assert(SidecarDir:sub(1, #data) == data)
        FileName = 'local-merge'
        DataAnnotations = R.annotation.annotations
        local incoming = require('util').tableDeepCopy(original)
        incoming.kaia_id = 99999
        incoming.note = 'Incoming edit'
        incoming.text = 'Edited highlight'
        incoming.text_edited = true
        incoming.datetime_updated = '2099-01-01 12:00:00'
        local function write(name, items)
            local f = assert(io.open(SidecarDir .. '/' .. name, 'w'))
            f:write(json.encode(items)); f:close()
            return SidecarDir .. '/' .. name
        end
        local cached = write('cached.json', {original})
        local income = write('income.json', {incoming})
        local plugin = KAIA_PLUGIN.instances.highlightsync
        assert(plugin:onSync(nil, cached, income, true))
        assert(R.annotation.annotations[1].kaia_id == SAVED_ID)
        assert(R.annotation.annotations[1].note == 'Incoming edit')
        assert(R.annotation.annotations[1].datetime_updated == incoming.datetime_updated)
        R.annotation.annotations[1].note = 'New local edit'
        R.annotation.annotations[1].datetime_updated = '2100-01-01 12:00:00'
        R.annotation:replaceItems(R.annotation.annotations)
        DataAnnotations = R.annotation.annotations
        assert(plugin:onSync(nil, cached, income, false))
        assert(#R.annotation.annotations == 1)
        assert(R.annotation.annotations[1].note == 'New local edit')
        assert(R.annotation.annotations[1].kaia_id == SAVED_ID)
        local before = json.encode(KAIA_READER.annotations())
        local ok = pcall(KAIA_READER.replace_annotations, {R.annotation.annotations[1], {page=false}})
        assert(not ok and json.encode(KAIA_READER.annotations()) == before)
        -- The adapter rejects remote sync before any socket call.
        plugin.settings.sync_server = {type='webdav', url='https://example.invalid'}
        plugin:SyncBookHighlights(false, true)
    "#,
    )
    .unwrap();
    healthy(&ui);
    let id = value(&ui, "SAVED_ID");
    drop(ui);
    let ui = load(&data, book.to_str().unwrap(), &["highlightsync"]);
    assert_eq!(value(&ui, "KAIA_READER.annotations()[1].kaia_id"), id);
    assert_eq!(
        value(&ui, "KAIA_READER.annotations()[1].note"),
        Value::from("New local edit")
    );
    assert_eq!(
        value(&ui, "KAIA_READER.annotations()[1].text"),
        Value::from("Edited highlight")
    );
    assert_eq!(
        value(&ui, "KAIA_READER.annotations()[1].text_edited"),
        Value::from(true)
    );
    assert_eq!(std::fs::read(book).unwrap(), original);
    assert_eq!(
        std::fs::read_to_string(marker).unwrap(),
        "KOReader owns this folder"
    );
    assert_eq!(std::fs::read_dir(sidecar).unwrap().count(), 1);
    healthy(&ui);
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
