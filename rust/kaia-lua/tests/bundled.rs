mod common;
use common::{book::BookReader, lua_ui, serial, TempDir};
use kaia_lua::PluginOptions;
use std::{cell::RefCell, rc::Rc};
#[test]
fn bundled_inventory_load_outcomes() {
    let _guard = serial();
    for name in [
        "SSH",
        "archiveviewer",
        "autodim",
        "autostandby",
        "autosuspend",
        "autoturn",
        "autowarmth",
        "batterystat",
        "bookshortcuts",
        "calibre",
        "cloudstorage",
        "coverbrowser",
        "coverimage",
        "docsettingtweak",
        "exporter",
        "externalkeyboard",
        "gestures",
        "hello",
        "hotkeys",
        "httpinspector",
        "japanese",
        "keepalive",
        "kosync",
        "movetoarchive",
        "newsdownloader",
        "opds",
        "perceptionexpander",
        "profiles",
        "qrclipboard",
        "readtimer",
        "statistics",
        "systemstat",
        "terminal",
        "texteditor",
        "timesync",
        "vocabbuilder",
        "wallabag",
    ] {
        let data = TempDir::new(name);
        let reader = BookReader::open("juliet.epub", &data);
        let (mut ui, _) = lua_ui(&data, false);
        ui.set_reader_host(Rc::new(RefCell::new(reader))).unwrap();
        ui.load_plugins(PluginOptions {
            names: vec![name.into()],
            ..Default::default()
        })
        .unwrap();
        ui.send_reader_event("ReaderReady", &[]);
        let status = ui.plugin_status();
        // These gates need device services or binaries absent from the desktop.
        // autostandby, coverimage, hello and hotkeys are not Kobo features.
        if [
            "SSH",
            "autodim",
            "autostandby",
            "autosuspend",
            "coverimage",
            "externalkeyboard",
            "hello",
            "hotkeys",
            "keepalive",
            "timesync",
        ]
        .contains(&name)
        {
            assert!(status.is_empty(), "{name}: {status:?}");
        } else if ["batterystat", "systemstat", "coverbrowser"].contains(&name) {
            assert_eq!(status.len(), 1);
            assert!(!status[0].loaded, "{name}");
            let error = status[0].error.as_deref().unwrap();
            assert!(
                error.contains(if name == "coverbrowser" {
                    "FileManager.addFileDialogButtons"
                } else {
                    "nil value"
                }),
                "{error}"
            );
        } else {
            assert_eq!(status.len(), 1, "{name}");
            healthy(&ui);
        }
        assert!(ui.take_errors().is_empty(), "{name}");
    }
}

use common::workflow::{close_windows, healthy, setup, tap, tap_expr, type_text, value};
use kaia_events::Value;

#[test]
fn exporter_writes_four_formats_from_real_books() {
    let _guard = serial();
    for book in ["juliet.epub", "sample.pdf"] {
        let data = TempDir::new("export-workflow");
        let (mut ui, _) = setup(&data, book, &["exporter"]);
        ui.exec(
            "highlight",
            r#"
            local R = require('apps/reader/readerui')
            local pos = R.document.is_pdf and {page=1, x=20, y=20} or R.document:getXPointer()
            R.annotation:addItem{page=R.document.is_pdf and 1 or pos, pos0=pos, pos1=pos,
                text='A test highlight', note='A saved note', drawer='lighten', color='yellow'}
            KAIA_PLUGIN.instances.exporter.settings.clipping_dir = KAIA_PATHS.data
            KAIA_PLUGIN.instances.exporter.settings.clipping_filename_single = '%f-clippings'
        "#,
        )
        .unwrap();
        for (format, enable) in [
            ("Markdown", Some("Export to Markdown")),
            ("Html", None),
            ("Json", Some("Export to Json")),
            ("Text", None),
        ] {
            ui.open_plugin_menu("exporter").unwrap();
            tap(&mut ui, "Choose formats and services");
            tap(&mut ui, format);
            if let Some(enable) = enable {
                tap(&mut ui, enable);
            }
            close_windows(&mut ui);
        }
        ui.open_plugin_menu("exporter").unwrap();
        tap(&mut ui, "Export all notes in current book");
        ui.run_due();
        ui.frame();
        healthy(&ui);
        for extension in ["md", "html", "json", "txt"] {
            let path = data.path().join(format!("{book}-clippings.{extension}"));
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(text.contains("A test highlight"), "{extension}: {text}");
            assert!(text.contains("A saved note"), "{extension}: {text}");
        }
    }
}

#[test]
fn offline_network_settings_dialogs() {
    let _guard = serial();
    // Stop before login, connect, catalog fetch, sync or server start.
    for (plugin, menu, path) in [
        (
            "wallabag",
            "wallabag",
            vec!["Settings", "Configure Wallabag server"],
        ),
        ("calibre", "calibre", vec!["Wireless settings", "Set password"]),
        ("kosync", "progress_sync", vec!["Custom sync server"]),
        (
            "newsdownloader",
            "news_downloader",
            vec!["Settings", "Never download images"],
        ),
        ("httpinspector", "httpremote", vec!["Port: 8080"]),
    ] {
        let data = TempDir::new(plugin);
        let (mut ui, _) = setup(&data, "juliet.epub", &[plugin]);
        ui.open_plugin_menu(menu).unwrap();
        for label in path {
            tap(&mut ui, label);
        }
        ui.frame();
        healthy(&ui);
        assert!(ui.window_count() > 0, "{plugin}");
    }
}

#[test]
fn qrclipboard_renders_clipboard_text() {
    let _guard = serial();
    let data = TempDir::new("qr");
    let (mut ui, _) = setup(&data, "sample.pdf", &["qrclipboard"]);
    ui.exec(
        "clipboard",
        "require('device').input.setClipboardText('Kaia clipboard')",
    )
    .unwrap();
    ui.open_plugin_menu("qrclipboard").unwrap();
    ui.frame();
    healthy(&ui);
    assert_eq!(
        value(&ui, "require('ui/uimanager'):getTopmostVisibleWidget().text").as_str(),
        Some("Kaia clipboard")
    );
    assert!(ui.screen().data().contains(&0));
}

#[test]
fn readtimer_alarm_runs_on_the_kaia_scheduler() {
    let _guard = serial();
    let data = TempDir::new("timer");
    let (mut ui, clock) = setup(&data, "sample.pdf", &["readtimer"]);
    ui.exec(
        "timer-clock",
        "local time = require('ui/time'); TIMER_NOW = time.now(); time.now = function() return TIMER_NOW end",
    )
    .unwrap();
    ui.open_plugin_menu("read_timer").unwrap();
    tap(&mut ui, "Set interval");
    tap_expr(
        &mut ui,
        "FIND_CONTROL(require('ui/uimanager'):getTopmostVisibleWidget().min_widget, '▲')",
    );
    tap(&mut ui, "Show timer in status bar");
    tap(&mut ui, "Start timer");
    let mut page = ui.screen().clone();
    ui.paint_reader_overlays(&mut page).unwrap();
    healthy(&ui);
    assert_eq!(
        value(&ui, "#require('apps/reader/readerui').view.footer.content").as_i64(),
        Some(1)
    );
    assert!(value(&ui, "KAIA_PLUGIN.instances.readtimer:scheduled()").truthy());
    ui.send_reader_event("Suspend", &[]);
    clock.advance(std::time::Duration::from_secs(61));
    ui.exec(
        "timer-elapsed",
        "TIMER_NOW = TIMER_NOW + require('ui/time').s(61)",
    )
    .unwrap();
    ui.send_reader_event("Resume", &[]);
    ui.run_due();
    ui.frame();
    healthy(&ui);
    assert_eq!(
        value(&ui, "require('ui/uimanager'):getTopmostVisibleWidget().text").as_str(),
        Some("Time is up")
    );
    tap(&mut ui, "Repeat");
    assert!(value(&ui, "KAIA_PLUGIN.instances.readtimer:scheduled()").truthy());
    ui.send_reader_event("Suspend", &[]);
    clock.advance(std::time::Duration::from_secs(61));
    ui.exec(
        "repeat-elapsed",
        "TIMER_NOW = TIMER_NOW + require('ui/time').s(61)",
    )
    .unwrap();
    ui.send_reader_event("Resume", &[]);
    ui.run_due();
    ui.frame();
    tap(&mut ui, "Done");
    ui.exec("flush", "require('ui/uimanager'):flushSettings()")
        .unwrap();
    healthy(&ui);
    drop(ui);
    let (ui, _) = setup(&data, "sample.pdf", &["readtimer"]);
    assert!(!value(&ui, "KAIA_PLUGIN.instances.readtimer:scheduled()").truthy());
    assert!(value(
        &ui,
        "KAIA_PLUGIN.instances.readtimer.settings.show_value_in_footer"
    )
    .truthy());
}

#[test]
fn statistics_records_pages_and_opens_views() {
    let _guard = serial();
    let data = TempDir::new("statistics-workflow");
    let (mut ui, _) = setup(&data, "sample.pdf", &["statistics"]);
    // Advance the plugin's wall clock without spending five minutes in the test.
    ui.exec("clock", "REAL_TIME = os.time; TEST_TIME = REAL_TIME(); os.time = function(t) if t then return REAL_TIME(t) end return TEST_TIME end").unwrap();
    for page in 1..=3 {
        ui.exec("elapsed", "TEST_TIME = TEST_TIME + 100").unwrap();
        ui.send_reader_event("PageUpdate", &[Value::from(page as i64)]);
    }
    for label in [
        "Current book",
        "Reading progress",
        "Calendar view",
        "Today's timeline",
    ] {
        close_windows(&mut ui);
        ui.open_plugin_menu("statistics").unwrap();
        tap(&mut ui, label);
        assert!(ui.window_count() >= 2, "{label}");
    }
    ui.send_reader_event("Suspend", &[]);
    ui.send_reader_event("Resume", &[]);
    ui.send_reader_event("CloseDocument", &[]);
    healthy(&ui);
    assert!(data.path().join("settings/statistics.sqlite3").is_file());
    let totals = value(&ui, "(function() local P=KAIA_PLUGIN.instances.statistics; local pages, seconds=P:getPageTimeTotalStats(P.id_curr_book); return {pages=pages, seconds=seconds} end)()");
    assert!(totals.get("pages").as_f64().unwrap() >= 2.0, "{totals:?}");
    assert!(totals.get("seconds").as_f64().unwrap() >= 200.0, "{totals:?}");
    drop(ui);
    let (ui, _) = setup(&data, "sample.pdf", &["statistics"]);
    assert!(
        value(&ui, "KAIA_PLUGIN.instances.statistics.book_read_time")
            .as_f64()
            .unwrap()
            >= 200.0
    );
}

#[test]
fn vocabulary_records_words_and_reviews_a_card() {
    let _guard = serial();
    let data = TempDir::new("vocabulary");
    let (mut ui, _) = setup(&data, "juliet.epub", &["vocabbuilder"]);
    // Dictionary lookup itself needs the app's dictionary host and installed data.
    for word in ["reader", "paper", "book"] {
        ui.send_reader_event(
            "WordLookedUp",
            &[Value::from(word), Value::from("Juliet"), Value::Bool(true)],
        );
    }
    ui.exec("review-clock", "local real_time = os.time; local later = real_time() + 3600; os.time = function(t) if t then return real_time(t) end return later end").unwrap();
    ui.open_plugin_menu("vocabbuilder").unwrap();
    ui.frame();
    healthy(&ui);
    assert_eq!(
        value(&ui, "#KAIA_PLUGIN.instances.vocabbuilder.widget.item_table").as_i64(),
        Some(3)
    );
    tap(&mut ui, "Got it");
    ui.send_reader_event("FlushSettings", &[]);
    healthy(&ui);
    assert!(data.path().join("settings/vocabulary_builder.sqlite3").is_file());
}

#[test]
fn texteditor_creates_saves_and_reopens_a_file() {
    let _guard = serial();
    let data = TempDir::new("texteditor-workflow");
    let (mut ui, _) = setup(&data, "juliet.epub", &["texteditor"]);
    ui.open_plugin_menu("text_editor").unwrap();
    tap(&mut ui, "Settings");
    tap(&mut ui, "Text font size: 20");
    tap(&mut ui, "▲");
    tap(&mut ui, "Apply");
    tap(&mut ui, "Use monospace font");
    close_windows(&mut ui);
    ui.open_plugin_menu("text_editor").unwrap();
    tap(&mut ui, "New file");
    type_text(&mut ui, "note.txt");
    tap(&mut ui, "Edit");
    type_text(&mut ui, "a saved note");
    tap(&mut ui, "Save");
    ui.run_due();
    ui.run_due();
    ui.frame();
    healthy(&ui);
    assert_eq!(
        std::fs::read_to_string(data.path().join("note.txt")).unwrap(),
        "a saved note"
    );
    close_windows(&mut ui);
    ui.send_reader_event("FlushSettings", &[]);
    ui.open_plugin_menu("text_editor").unwrap();
    let history_label = value(
        &ui,
        "require('ui/uimanager'):getTopmostVisibleWidget().item_table[4].text",
    );
    tap(&mut ui, history_label.as_str().unwrap());
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.texteditor.input:getInputText()").as_str(),
        Some("a saved note")
    );
    close_windows(&mut ui);
    ui.send_reader_event("Suspend", &[]);
    ui.send_reader_event("Resume", &[]);
    ui.send_reader_event("FlushSettings", &[]);
    healthy(&ui);
    drop(ui);
    let (mut ui, _) = setup(&data, "juliet.epub", &["texteditor"]);
    ui.open_plugin_menu("text_editor").unwrap();
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.texteditor.font_size").as_i64(),
        Some(21)
    );
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.texteditor.font_face").as_str(),
        Some("infont")
    );
    let history_label = value(
        &ui,
        "require('ui/uimanager'):getTopmostVisibleWidget().item_table[4].text",
    );
    tap(&mut ui, history_label.as_str().unwrap());
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.texteditor.input:getInputText()").as_str(),
        Some("a saved note")
    );
}

#[test]
fn gestures_binds_a_plugin_action_and_fires_it() {
    let _guard = serial();
    let data = TempDir::new("gestures-workflow");
    let (mut ui, _) = setup(&data, "juliet.epub", &["gestures", "readtimer"]);
    ui.open_plugin_menu("gesture_manager").unwrap();
    tap(&mut ui, "Tap corner");
    let label = value(
        &ui,
        "KAIA_PLUGIN.instances.gestures:gestureTitleFunc('tap_top_left_corner')",
    );
    tap(&mut ui, label.as_str().unwrap());
    tap(&mut ui, "Pass through");
    tap(&mut ui, "Read timer: set interval");
    close_windows(&mut ui);
    ui.send_reader_event("FlushSettings", &[]);
    assert!(ui.action_registry().get("show_timer").is_some());
    ui.send_reader_event("Suspend", &[]);
    ui.send_reader_event("Resume", &[]);
    healthy(&ui);
    drop(ui);
    let (mut ui, _) = setup(&data, "juliet.epub", &["gestures", "readtimer"]);
    ui.gesture(&kaia_input::Gesture::Tap {
        pos: kaia_gfx::Point::new(5, 5),
        time_us: 0,
    });
    ui.frame();
    healthy(&ui);
    assert_eq!(
        value(
            &ui,
            "require('ui/uimanager'):getTopmostVisibleWidget().title_text"
        )
        .as_str(),
        Some("Interval")
    );
}

#[test]
fn perceptionexpander_paints_over_a_real_page_and_persists() {
    let _guard = serial();
    let data = TempDir::new("overlay");
    let mut reader = BookReader::open("sample.pdf", &data);
    let original = reader.render();
    drop(reader);
    let mut page = original.clone();
    let (mut ui, _) = setup(&data, "sample.pdf", &["perceptionexpander"]);
    ui.open_plugin_menu("speed_reading_module_perception_expander")
        .unwrap();
    tap(&mut ui, "Enable");
    close_windows(&mut ui);
    ui.open_plugin_menu("speed_reading_module_perception_expander")
        .unwrap();
    tap(&mut ui, "Settings");
    for (index, text) in [(1, "3"), (2, "0.15"), (3, "6")] {
        tap_expr(
            &mut ui,
            &format!("KAIA_PLUGIN.instances.perceptionexpander.settings_dialog.input_fields[{index}]"),
        );
        type_text(&mut ui, text);
    }
    tap(&mut ui, "Apply");
    close_windows(&mut ui);
    ui.paint_reader_overlays(&mut page).unwrap();
    healthy(&ui);
    assert_ne!(page.data(), original.data());
    ui.send_reader_event("PageUpdate", &[Value::from(2i64)]);
    ui.send_reader_event("Suspend", &[]);
    ui.send_reader_event("Resume", &[]);
    ui.send_reader_event("FlushSettings", &[]);
    drop(ui);
    let (mut ui, _) = setup(&data, "sample.pdf", &["perceptionexpander"]);
    assert!(value(&ui, "KAIA_PLUGIN.instances.perceptionexpander.is_enabled").truthy());
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.perceptionexpander.line_thickness").as_i64(),
        Some(3)
    );
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.perceptionexpander.margin").as_f64(),
        Some(0.15)
    );
    assert!(
        (value(
            &ui,
            "KAIA_PLUGIN.instances.perceptionexpander.line_color_intensity"
        )
        .as_f64()
        .unwrap()
            - 0.6)
            .abs()
            < 0.001
    );
    // No golden is added. Compare the overlay against the rendered page.
    let mut restored = original.clone();
    ui.paint_reader_overlays(&mut restored).unwrap();
    healthy(&ui);
    assert_eq!(page.data(), restored.data());
    ui.send_reader_event("PageUpdate", &[Value::from(2i64)]);
    healthy(&ui);
    assert_eq!(
        value(&ui, "KAIA_PLUGIN.instances.perceptionexpander.page_counter").as_i64(),
        Some(1)
    );
}

#[test]
fn opds_and_cloudstorage_open_local_lists_without_network() {
    let _guard = serial();
    let data = TempDir::new("catalogs");
    let (mut ui, _) = lua_ui(&data, false);
    ui.set_library_host(Rc::new(RefCell::new(common::workflow::Library {
        folder: data.path().to_string_lossy().into(),
        opened: None,
    })))
    .unwrap();
    common::workflow::prepare(&mut ui);
    ui.load_plugins(PluginOptions {
        names: vec!["opds".into(), "cloudstorage".into()],
        ..Default::default()
    })
    .unwrap();
    healthy(&ui);
    for menu in ["opds", "cloudstorage"] {
        ui.open_plugin_menu(menu).unwrap();
        ui.frame();
        healthy(&ui);
        assert!(ui.window_count() > 0);
        tap_expr(
            &mut ui,
            "require('ui/uimanager'):getTopmostVisibleWidget().title_bar.left_button",
        );
        if menu == "opds" {
            tap(&mut ui, "Add catalog");
        }
        ui.frame();
        healthy(&ui);
        close_windows(&mut ui);
    }
}

#[test]
fn movetoarchive_copies_and_moves_scratch_books() {
    let _guard = serial();
    for copy in [true, false] {
        let data = TempDir::new("archive-move");
        let source = data.path().join("sample.pdf");
        let folder = data.path().join("archive");
        std::fs::copy(common::repo_root().join("test/sample.pdf"), &source).unwrap();
        std::fs::create_dir(&folder).unwrap();
        let (mut ui, _) = lua_ui(&data, false);
        let library = Rc::new(RefCell::new(common::workflow::Library {
            folder: data.path().to_string_lossy().into(),
            opened: None,
        }));
        ui.set_library_host(library.clone()).unwrap();
        ui.set_reader_host(Rc::new(RefCell::new(BookReader::open_path(&source, &data))))
            .unwrap();
        // Use the same input helpers as the reader workflows.
        ui.exec("controls", "function FIND_CONTROL(w,text) if type(w) ~= 'table' then return end; if w.entry and w.entry.text == text or w.text == text and w.callback and w.dimen then return w end; for _,c in ipairs(w) do local f=FIND_CONTROL(c,text); if f then return f end end end; function FIND_SHOWN(text) return FIND_CONTROL(require('ui/uimanager'):getTopmostVisibleWidget(),text) end").unwrap();
        ui.load_plugins(PluginOptions {
            names: vec!["movetoarchive".into()],
            ..Default::default()
        })
        .unwrap();
        ui.exec("archive-folder", &format!("local P=KAIA_PLUGIN.instances.movetoarchive; P:loadSettings(); P.data.archive_dir_path={:?}; P.updated=true", format!("{}/", folder.display()))).unwrap();
        ui.open_plugin_menu("move_to_archive").unwrap();
        tap(
            &mut ui,
            if copy {
                "Copy current book to archive"
            } else {
                "Move current book to archive"
            },
        );
        assert_eq!(source.exists(), copy);
        assert!(folder.join("sample.pdf").is_file());
        tap(&mut ui, "Open");
        assert_eq!(
            library.borrow().opened.as_deref(),
            folder.join("sample.pdf").to_str()
        );
        ui.send_reader_event("FlushSettings", &[]);
        healthy(&ui);
    }
}

#[test]
fn japanese_deinflects_and_opens_scan_settings() {
    let _guard = serial();
    let data = TempDir::new("japanese");
    let (mut ui, _) = setup(&data, "juliet.epub", &["japanese"]);
    // No StarDict data is installed. Test the bundled deinflector separately.
    assert!(value(
        &ui,
        "#KAIA_PLUGIN.instances.japanese.deinflector:deinflect('食べました') > 0"
    )
    .truthy());
    ui.exec("language-menu", "local item = KAIA_PLUGIN.instances.japanese:genMenuItem(); require('ui/uimanager'):show(require('kaia/plugin_menu'):new{title=item.text, item_table=item.sub_item_table})").unwrap();
    tap(&mut ui, "Text scan length: 20 characters");
    tap(&mut ui, "▲");
    tap(&mut ui, "Set scan length");
    assert_eq!(
        value(
            &ui,
            "G_reader_settings:readSetting('language_japanese_text_scan_length')"
        )
        .as_i64(),
        Some(21)
    );
}

#[test]
fn profiles_restores_real_epub_font_size() {
    let _guard = serial();
    let data = TempDir::new("profiles-workflow");
    let (mut ui, _) = setup(&data, "juliet.epub", &["profiles"]);
    let initial = value(&ui, "KAIA_READER.get_setting('font_size')");
    ui.open_plugin_menu("profiles").unwrap();
    tap(&mut ui, "New with current book settings");
    type_text(&mut ui, "reading");
    tap(&mut ui, "Save");
    ui.exec("change-font", "KAIA_READER.set_setting('font_size', 30)")
        .unwrap();
    close_windows(&mut ui);
    ui.open_plugin_menu("profiles").unwrap();
    let label = value(
        &ui,
        "KAIA_PLUGIN.instances.profiles:getProfileMenuText('reading')",
    );
    tap(&mut ui, label.as_str().unwrap());
    tap(&mut ui, "Execute");
    assert_eq!(value(&ui, "KAIA_READER.get_setting('font_size')"), initial);
    ui.send_reader_event("FlushSettings", &[]);
    healthy(&ui);
    drop(ui);
    let (ui, _) = setup(&data, "juliet.epub", &["profiles"]);
    assert!(value(&ui, "KAIA_PLUGIN.instances.profiles.data.reading ~= nil").truthy());
}

#[test]
fn autoturn_uses_input_hook_and_advances_the_reader() {
    let _guard = serial();
    let data = TempDir::new("autoturn-workflow");
    let (mut ui, clock) = setup(&data, "sample.pdf", &["autoturn"]);
    ui.exec(
        "clock",
        "BOOT_TIME=0; require('ui/uimanager').getElapsedTimeSinceBoot=function() return BOOT_TIME end",
    )
    .unwrap();
    ui.open_plugin_menu("autoturn").unwrap();
    tap_expr(
        &mut ui,
        "FIND_CONTROL(require('ui/uimanager'):getTopmostVisibleWidget().sec_widget, '▲')",
    );
    tap(&mut ui, "Set timeout");
    close_windows(&mut ui);
    ui.exec("elapsed", "BOOT_TIME = require('ui/time').s(32)")
        .unwrap();
    clock.advance(std::time::Duration::from_secs(32));
    ui.run_due();
    healthy(&ui);
    assert_eq!(value(&ui, "KAIA_READER.current_page()").as_i64(), Some(2));
    ui.send_reader_event("CloseDocument", &[]);
    healthy(&ui);
}

#[test]
fn archiveviewer_lists_views_and_extracts_an_epub_member() {
    let _guard = serial();
    let data = TempDir::new("archiveviewer-workflow");
    let (mut ui, _) = setup(&data, "juliet.epub", &["archiveviewer"]);
    // The file manager's Open with entry calls this provider's openFile method.
    ui.exec("open-archive", "local P=KAIA_PLUGIN.instances.archiveviewer; P:openFile(KAIA_READER.file()); P.extract_dir=KAIA_PATHS.data .. '/'").unwrap();
    tap(&mut ui, "mimetype");
    tap(&mut ui, "View");
    assert_eq!(
        value(&ui, "require('ui/uimanager'):getTopmostVisibleWidget().text").as_str(),
        Some("application/epub+zip\r\n")
    );
    close_windows(&mut ui);
    ui.exec("open-archive", "local P=KAIA_PLUGIN.instances.archiveviewer; P:openFile(KAIA_READER.file()); P.extract_dir=KAIA_PATHS.data .. '/'").unwrap();
    tap(&mut ui, "mimetype");
    tap(&mut ui, "Extract");
    assert_eq!(
        std::fs::read_to_string(data.path().join("mimetype")).unwrap(),
        "application/epub+zip\r\n"
    );
}

#[test]
fn bookshortcuts_opens_a_saved_book_and_keeps_settings() {
    let _guard = serial();
    let data = TempDir::new("shortcuts-workflow");
    let (mut ui, _) = setup(&data, "juliet.epub", &["bookshortcuts"]);
    let library = Rc::new(RefCell::new(common::workflow::Library {
        folder: data.path().to_string_lossy().into(),
        opened: None,
    }));
    ui.set_library_host(library.clone()).unwrap();
    ui.exec(
        "shortcut",
        "KAIA_PLUGIN.instances.bookshortcuts:addShortcut(KAIA_READER.file())",
    )
    .unwrap();
    ui.open_plugin_menu("book_shortcuts").unwrap();
    let label = value(
        &ui,
        "require('ui/uimanager'):getTopmostVisibleWidget().item_table[3].text",
    );
    tap(&mut ui, label.as_str().unwrap());
    assert!(library.borrow().opened.as_ref().unwrap().ends_with("juliet.epub"));
    ui.send_reader_event("FlushSettings", &[]);
    healthy(&ui);
    assert!(data.path().join("settings/bookshortcuts.lua").is_file());
}

#[test]
fn terminal_settings_and_session_prerequisites() {
    let _guard = serial();
    let data = TempDir::new("terminal-workflow");
    let (mut ui, _) = setup(&data, "juliet.epub", &["terminal"]);
    ui.open_plugin_menu("terminal").unwrap();
    tap(&mut ui, "Font size: 14");
    close_windows(&mut ui);
    // Do not fork from the multithreaded Rust test process. The observed child
    // stalled before exec. The app needs to own terminal processes and PTYs.
    assert!(!value(&ui, "KAIA_PLUGIN.instances.terminal.is_shell_open").truthy());
}

#[test]
fn docsettingtweak_applies_directory_defaults() {
    let _guard = serial();
    let data = TempDir::new("defaults-workflow");
    let (mut ui, _) = setup(&data, "juliet.epub", &["docsettingtweak"]);
    ui.exec(
        "defaults-fixture",
        r#"
        local path = require('ffi/util').dirname(require('ffi/util').realpath(KAIA_READER.file()))
        G_reader_settings:saveSetting('home_dir', path)
        local defaults = require('luasettings'):open(KAIA_PATHS.data .. '/settings/directory_defaults.lua')
        defaults:flush()
    "#,
    )
    .unwrap();
    ui.open_plugin_menu("doc_setting_tweak").unwrap();
    ui.frame();
    healthy(&ui);
    let folder = value(&ui, "G_reader_settings:readSetting('home_dir')");
    // The editor opens at the start. Keep the existing table after this rule.
    type_text(
        &mut ui,
        &format!(
            "if true then return {{[{:?}]={{font_size=27}}}} end\n",
            folder.as_str().unwrap()
        ),
    );
    tap(&mut ui, "Save");
    ui.run_due();
    ui.run_due();
    ui.frame();
    healthy(&ui);
    // Opening an existing book does not apply directory defaults again.
    assert_eq!(value(&ui, "(require('luasettings'):open(KAIA_PATHS.data .. '/settings/directory_defaults.lua').data[G_reader_settings:readSetting('home_dir')] or {}).font_size").as_i64(), Some(27), "{}", std::fs::read_to_string(data.path().join("settings/directory_defaults.lua")).unwrap());
    ui.exec(
        "new-document",
        r#"
        local R = require('apps/reader/readerui')
        KAIA_PLUGIN.dispatch('DocSettingsLoad', R.doc_settings, {file=R.document.file, is_new=true})
        R.doc_settings:flush()
    "#,
    )
    .unwrap();
    assert_eq!(
        value(&ui, "KAIA_READER.get_setting('font_size')").as_i64(),
        Some(27)
    );
    healthy(&ui);
}

#[test]
fn annotation_writes_use_host_ids_and_survive_reopening() {
    let _guard = serial();
    for book in ["juliet.epub", "sample.pdf"] {
        let data = TempDir::new("annotation-writes");
        let (mut ui, _) = setup(&data, book, &["exporter"]);
        ui.exec(
            "add-highlight",
            r#"
            local R = require('apps/reader/readerui')
            local pos = R.document.is_pdf and {page=1, x=20, y=20} or R.document:getXPointer()
            INDEX = R.annotation:addItem{page=R.document.is_pdf and 1 or pos, pos0=pos, pos1=pos,
                text='Saved selection', drawer='lighten', color='yellow'}
            local dialog
            dialog = require('ui/widget/inputdialog'):new{
                title='Note', input='', buttons={{{text='Save', callback=function()
                    R.bookmark:setBookmarkNote(INDEX, true, dialog:getInputText())
                    R.doc_settings:flush()
                    require('ui/uimanager'):close(dialog)
                end}}},
            }
            require('ui/uimanager'):show(dialog)
        "#,
        )
        .unwrap();
        type_text(&mut ui, "a note");
        tap(&mut ui, "Save");
        assert_eq!(
            value(&ui, "KAIA_READER.annotations()[1].note").as_str(),
            Some("a note")
        );
        drop(ui);
        let (mut ui, _) = setup(&data, book, &["exporter"]);
        assert_eq!(
            value(&ui, "KAIA_READER.annotations()[1].note").as_str(),
            Some("a note")
        );
        ui.exec(
            "delete-highlight",
            "local R=require('apps/reader/readerui'); R.highlight:deleteHighlight(1); R.doc_settings:flush()",
        )
        .unwrap();
        assert_eq!(value(&ui, "#KAIA_READER.annotations()").as_i64(), Some(0));
        drop(ui);
        let (ui, _) = setup(&data, book, &["exporter"]);
        assert_eq!(value(&ui, "#KAIA_READER.annotations()").as_i64(), Some(0));
    }
}
