mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::rc::Rc;

use common::{lua_ui, serial, TempDir};
use kaia_book::{BookSession, Location, OpenOptions, Pos};
use kaia_doc::Document;
use kaia_events::{Key, Value};
use kaia_gfx::golden::assert_matches_golden;
use kaia_lua::{LibraryHost, PluginOptions, ReaderHost};

struct TestReader {
    page: u32,
    settings: BTreeMap<String, Value>,
}

impl ReaderHost for TestReader {
    fn file(&self) -> String {
        "/tmp/test.epub".into()
    }
    fn page_count(&self) -> u32 {
        12
    }
    fn current_page(&self) -> u32 {
        self.page
    }
    fn go_to_page(&mut self, page: u32) -> Result<(), String> {
        self.page = page;
        Ok(())
    }
    fn page_text(&mut self, _page: u32) -> Result<String, String> {
        Ok("hello".into())
    }
    fn properties(&self) -> Value {
        Value::Table(
            [
                ("display_title".into(), Value::from("Test book")),
                ("authors".into(), Value::from("Kaia")),
            ]
            .into(),
        )
    }
    fn annotations(&mut self) -> Vec<Value> {
        Vec::new()
    }
    fn toc(&self) -> Vec<Value> {
        Vec::new()
    }
    fn get_setting(&self, key: &str) -> Value {
        self.settings.get(key).cloned().unwrap_or_default()
    }
    fn set_setting(&mut self, key: &str, value: Value) -> Result<(), String> {
        self.settings.insert(key.into(), value);
        Ok(())
    }
}

#[test]
fn hello_loads_through_koreader_and_opens_window() {
    let _guard = serial();
    let data = TempDir::new("hello-plugin");
    let (mut ui, _) = lua_ui(&data, false);
    ui.set_reader_host(Rc::new(RefCell::new(TestReader {
        page: 1,
        settings: BTreeMap::new(),
    })))
    .unwrap();
    ui.load_plugins(PluginOptions {
        names: vec!["hello".into()],
        force_hello: true,
        ..Default::default()
    })
    .unwrap();
    assert!(ui.plugin_status()[0].loaded, "{:?}", ui.plugin_status());
    assert!(ui.menu_registry().get("hello_world").is_some());
    assert!(ui.action_registry().get("helloworld_action").is_some());
    ui.open_plugin_menu("hello_world").unwrap();
    assert_eq!(ui.window_count(), 1);
    let message = ui
        .eval(
            "hello-message.lua",
            "return require('ui/uimanager'):getTopmostVisibleWidget().text",
        )
        .unwrap();
    assert_eq!(message.as_str(), Some("Hello, plugin world"));
}

#[test]
fn safe_mode_skips_external_plugins_and_handler_errors_disable_them() {
    let _guard = serial();
    let data = TempDir::new("plugin-safety");
    let external = data.path().join("external");
    let plugin = external.join("broken.koplugin");
    fs::create_dir_all(&plugin).unwrap();
    fs::write(
        plugin.join("_meta.lua"),
        "return { fullname = 'Broken', description = 'Test' }",
    )
    .unwrap();
    fs::write(plugin.join("main.lua"), "local W = require('ui/widget/widget'); local P = W:extend{ name = 'broken' }; function P:onPageUpdate() error('handler failed') end; return P").unwrap();
    let (mut ui, _) = lua_ui(&data, false);
    ui.set_reader_host(Rc::new(RefCell::new(TestReader {
        page: 1,
        settings: BTreeMap::new(),
    })))
    .unwrap();
    ui.load_plugins(PluginOptions {
        names: vec!["broken".into()],
        safe_mode: true,
        extra_paths: vec![external.clone()],
        ..Default::default()
    })
    .unwrap();
    assert!(ui.plugin_status().is_empty());
    ui.load_plugins(PluginOptions {
        names: vec!["broken".into()],
        extra_paths: vec![external],
        ..Default::default()
    })
    .unwrap();
    assert!(ui.plugin_status()[0].loaded);
    ui.send_reader_event("PageUpdate", &[Value::from(2i64)]);
    let status = &ui.plugin_status()[0];
    assert!(!status.loaded);
    assert!(status.error.as_deref().unwrap().contains("handler failed"));
}

#[test]
fn loader_reports_a_plugin_that_raises_during_load() {
    let _guard = serial();
    let data = TempDir::new("plugin-load-error");
    let external = data.path().join("external");
    let plugin = external.join("badload.koplugin");
    fs::create_dir_all(&plugin).unwrap();
    fs::write(
        plugin.join("_meta.lua"),
        "return { fullname = 'Bad load', description = 'Test' }",
    )
    .unwrap();
    fs::write(plugin.join("main.lua"), "error('load failed')").unwrap();
    let (mut ui, _) = lua_ui(&data, false);
    ui.set_reader_host(Rc::new(RefCell::new(TestReader {
        page: 1,
        settings: BTreeMap::new(),
    })))
    .unwrap();
    ui.load_plugins(PluginOptions {
        names: vec!["badload".into()],
        extra_paths: vec![external],
        ..Default::default()
    })
    .unwrap();
    let status = &ui.plugin_status()[0];
    assert_eq!(status.name, "badload");
    assert!(!status.loaded);
    assert!(status.error.as_deref().unwrap().contains("load failed"));
}

#[test]
fn rust_reader_events_reach_each_plugin_handler() {
    let _guard = serial();
    let data = TempDir::new("reader-events");
    let external = data.path().join("external");
    let plugin = external.join("eventprobe.koplugin");
    fs::create_dir_all(&plugin).unwrap();
    fs::write(
        plugin.join("_meta.lua"),
        "return { fullname = 'Event probe', description = 'Test' }",
    )
    .unwrap();
    fs::write(
        plugin.join("main.lua"),
        r#"
        local W = require('ui/widget/widget')
        local P = W:extend{ name = 'eventprobe' }
        KAIA_EVENT_COUNTS = {}
        for _, name in ipairs({ 'ReaderReady', 'PageUpdate', 'CloseDocument', 'Suspend', 'Resume' }) do
            P['on' .. name] = function()
                KAIA_EVENT_COUNTS[name] = (KAIA_EVENT_COUNTS[name] or 0) + 1
            end
        end
        return P
    "#,
    )
    .unwrap();
    let (mut ui, _) = lua_ui(&data, false);
    ui.set_reader_host(Rc::new(RefCell::new(TestReader {
        page: 1,
        settings: BTreeMap::new(),
    })))
    .unwrap();
    ui.load_plugins(PluginOptions {
        names: vec!["eventprobe".into()],
        extra_paths: vec![external],
        ..Default::default()
    })
    .unwrap();
    for name in ["ReaderReady", "PageUpdate", "CloseDocument", "Suspend", "Resume"] {
        ui.send_reader_event(name, &[]);
        let count = ui
            .eval("event-count.lua", &format!("return KAIA_EVENT_COUNTS.{name}"))
            .unwrap();
        assert_eq!(count.as_i64(), Some(1), "{name}");
    }
    assert!(ui.plugin_status()[0].loaded);
}

struct TestLibrary;

impl LibraryHost for TestLibrary {
    fn current_folder(&self) -> String {
        "/books".into()
    }
    fn files(&self) -> Vec<String> {
        vec!["/books/test.epub".into()]
    }
    fn open_book(&mut self, _path: &str) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn file_manager_proxy_registers_hello_menu() {
    let _guard = serial();
    let data = TempDir::new("filemanager-plugin");
    let (mut ui, _) = lua_ui(&data, false);
    ui.set_library_host(Rc::new(RefCell::new(TestLibrary))).unwrap();
    ui.load_plugins(PluginOptions {
        names: vec!["hello".into()],
        force_hello: true,
        ..Default::default()
    })
    .unwrap();
    assert!(ui.plugin_status()[0].loaded);
    assert!(ui.menu_registry().get("hello_world").is_some());
    ui.open_plugin_menu("hello_world").unwrap();
    assert_eq!(ui.window_count(), 1);
}

#[test]
fn reader_plugins_load_and_open_menus() {
    let _guard = serial();
    let data = TempDir::new("reader-plugins");
    let (mut ui, _) = lua_ui(&data, false);
    ui.set_reader_host(Rc::new(RefCell::new(TestReader {
        page: 1,
        settings: BTreeMap::new(),
    })))
    .unwrap();
    ui.load_plugins(PluginOptions {
        names: vec!["exporter".into(), "statistics".into()],
        ..Default::default()
    })
    .unwrap();
    assert!(
        ui.plugin_status().iter().all(|s| s.loaded),
        "{:?}",
        ui.plugin_status()
    );
    assert!(ui.menu_registry().get("statistics").is_some());
    assert!(ui.menu_registry().get("exporter").is_some());
    ui.open_plugin_menu("statistics").unwrap();
    assert_eq!(ui.window_count(), 1, "{:?}", ui.plugin_status());
    ui.send_reader_event("ReaderReady", &[]);
    assert!(
        ui.plugin_status().iter().all(|s| s.loaded),
        "{:?}",
        ui.plugin_status()
    );
    for (name, args) in [
        ("PageUpdate", vec![Value::from(2i64)]),
        ("Suspend", vec![]),
        ("Resume", vec![]),
        ("CloseDocument", vec![]),
    ] {
        ui.send_reader_event(name, &args);
        assert!(
            ui.plugin_status().iter().all(|s| s.loaded),
            "{name}: {:?}",
            ui.plugin_status()
        );
    }
}

struct BookReader {
    session: BookSession,
    document: Box<dyn Document>,
    settings: BTreeMap<String, Value>,
}

impl BookReader {
    fn open(file: &str, data: &TempDir) -> Self {
        let path = common::repo_root().join("test").join(file);
        let session = BookSession::open(&path, OpenOptions::new(data.path())).unwrap();
        let document = kaia_doc::open(&path).unwrap();
        let mut settings = BTreeMap::new();
        settings.insert(
            "partial_md5_checksum".into(),
            Value::from(session.identity().partial_md5.clone()),
        );
        Self {
            session,
            document,
            settings,
        }
    }
}

impl ReaderHost for BookReader {
    fn file(&self) -> String {
        self.session.path().to_string_lossy().to_string()
    }
    fn page_count(&self) -> u32 {
        self.session.page_count() as u32
    }
    fn current_page(&self) -> u32 {
        self.session.current().page + 1
    }
    fn go_to_page(&mut self, page: u32) -> Result<(), String> {
        self.session
            .go_to(Location::Page(page.saturating_sub(1)))
            .map_err(|e| e.to_string())
    }
    fn page_text(&mut self, page: u32) -> Result<String, String> {
        self.document
            .page_text(page.saturating_sub(1))
            .map(|p| p.text)
            .map_err(|e| e.to_string())
    }
    fn xpointer_for_page(&self, page: u32) -> Result<String, String> {
        if page != self.current_page() {
            return Err("BookReader xpointer page is not current".into());
        }
        match self.session.current().location {
            Location::Engine(xp) => Ok(xp),
            Location::Page(_) => Err("BookReader has no xpointer for a fixed page".into()),
        }
    }
    fn page_for_xpointer(&self, xpointer: &str) -> Result<u32, String> {
        self.document
            .resolve(&Location::Engine(xpointer.into()))
            .map(|page| page + 1)
            .map_err(|e| e.to_string())
    }
    fn native_page_dimensions(&self, page: u32) -> Result<Value, String> {
        let size = self
            .document
            .page_size(page.saturating_sub(1))
            .map_err(|e| e.to_string())?;
        Ok(Value::Table(
            [
                (Key::from("w"), Value::from(size.width as f64)),
                (Key::from("h"), Value::from(size.height as f64)),
            ]
            .into(),
        ))
    }
    fn properties(&self) -> Value {
        let m = self.session.metadata();
        Value::Table(
            [
                (
                    Key::from("display_title"),
                    Value::from(m.title.clone().unwrap_or_else(|| "Test book".into())),
                ),
                (
                    Key::from("authors"),
                    Value::from(m.author.clone().unwrap_or_default()),
                ),
            ]
            .into(),
        )
    }
    fn annotations(&mut self) -> Vec<Value> {
        self.session
            .annotations()
            .unwrap()
            .into_iter()
            .map(|a| {
                let mut fields = kaia_events::Table::new();
                fields.insert(Key::from("datetime"), Value::from(a.datetime));
                if let Some(text) = a.text {
                    fields.insert(Key::from("text"), Value::from(text));
                }
                if let Some(note) = a.note {
                    fields.insert(Key::from("note"), Value::from(note));
                }
                if let Some(chapter) = a.chapter {
                    fields.insert(Key::from("chapter"), Value::from(chapter));
                }
                if let Some(drawer) = a.drawer {
                    fields.insert(Key::from("drawer"), Value::from(drawer));
                }
                if let Some(pos) = a.pos0 {
                    fields.insert(Key::from("pos0"), position_value(pos));
                }
                if let Some(pos) = a.pos1 {
                    fields.insert(Key::from("pos1"), position_value(pos));
                }
                fields.insert(
                    Key::from("page"),
                    match a.page {
                        Location::Page(page) => Value::from(page as i64 + 1),
                        Location::Engine(xp) => Value::from(xp),
                    },
                );
                Value::Table(fields)
            })
            .collect()
    }
    fn toc(&self) -> Vec<Value> {
        self.session
            .toc()
            .iter()
            .map(|entry| {
                Value::Table(
                    [
                        (Key::from("title"), Value::from(entry.title.clone())),
                        (Key::from("level"), Value::from(entry.level as i64)),
                        (Key::from("page"), Value::from(entry.page.unwrap_or(0) as i64 + 1)),
                    ]
                    .into(),
                )
            })
            .collect()
    }
    fn get_setting(&self, key: &str) -> Value {
        self.settings.get(key).cloned().unwrap_or_default()
    }
    fn set_setting(&mut self, key: &str, value: Value) -> Result<(), String> {
        self.settings.insert(key.into(), value);
        Ok(())
    }
}

fn position_value(pos: Pos) -> Value {
    match pos {
        Pos::Xpointer(xp) => Value::from(xp),
        Pos::Point { page, x, y } => Value::Table(
            [
                (Key::from("page"), Value::from(page as i64 + 1)),
                (Key::from("x"), Value::from(x as f64)),
                (Key::from("y"), Value::from(y as f64)),
                (Key::from("zoom"), Value::from(1i64)),
                (Key::from("rotation"), Value::from(0i64)),
            ]
            .into(),
        ),
    }
}

#[test]
fn real_books_open_plugin_menus() {
    let _guard = serial();
    for (file, plugin, menu, golden) in [
        ("juliet.epub", "exporter", "exporter", "exporter_menu.png"),
        ("sample.pdf", "statistics", "statistics", "statistics_menu.png"),
    ] {
        let data = TempDir::new(plugin);
        let reader = BookReader::open(file, &data);
        assert!(reader.page_count() > 0);
        let (mut ui, _) = lua_ui(&data, false);
        ui.set_reader_host(Rc::new(RefCell::new(reader))).unwrap();
        let count = ui
            .eval(
                "book-facts.lua",
                "return require('apps/reader/readerui').document:getPageCount()",
            )
            .unwrap();
        assert!(count.as_f64().unwrap() > 0.0);
        let text = ui
            .eval(
                "book-text.lua",
                "return require('apps/reader/readerui').document:getPageText(1)",
            )
            .unwrap();
        assert!(text.as_str().is_some());
        ui.load_plugins(PluginOptions {
            names: vec![plugin.into()],
            ..Default::default()
        })
        .unwrap();
        assert!(ui.plugin_status()[0].loaded, "{:?}", ui.plugin_status());
        ui.send_reader_event("ReaderReady", &[]);
        assert!(ui.plugin_status()[0].loaded, "{:?}", ui.plugin_status());
        ui.open_plugin_menu(menu).unwrap();
        assert!(ui.plugin_status()[0].loaded, "{:?}", ui.plugin_status());
        assert_eq!(ui.window_count(), 1);
        ui.frame();
        assert!(ui.take_errors().is_empty());
        assert_matches_golden(ui.screen(), &common::golden_path(golden), 0);
    }
}
