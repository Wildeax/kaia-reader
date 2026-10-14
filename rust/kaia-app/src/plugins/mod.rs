//! One lazy Lua runtime, driven by the app thread.
/// File manager host for plugins. Public so the integration tests can drive it.
pub mod library;

use kaia_i18n::tr;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use kaia_events::{Event, MenuItem, Settings, Value};
use kaia_lua::{LuaPaths, LuaUi, ReaderHost};

pub const ENABLED: &str = "plugins.enabled";
pub const SAFE_MODE: &str = "plugins.safe_mode";

/// Bundled plugins with a passing desktop workflow in the inventory.
const DEFAULTS: &[&str] = &[
    "archiveviewer",
    "autoturn",
    "batterystat",
    "bookshortcuts",
    "calibre",
    "cloudstorage",
    "docsettingtweak",
    "exporter",
    "gestures",
    "httpinspector",
    "japanese",
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
    "texteditor",
    "vocabbuilder",
    "wallabag",
];

#[derive(Clone, Debug)]
pub struct Plugin {
    pub name: String,
    pub bundled: bool,
    pub enabled: bool,
    pub state: String,
}

pub(crate) enum Request {
    Reader(Rc<RefCell<dyn ReaderHost>>),
    Library(Box<dyn FnOnce()>),
    Event(Event),
    Menu(String),
    Reload,
    Dictionary(String, String),
    DictionaryActions,
}

#[derive(Default)]
pub(crate) struct DeviceState {
    pub values: Value,
    pub requests: Vec<(String, Value)>,
}

pub struct Plugins {
    pub paths: LuaPaths,
    pub startup_safe_mode: bool,
    pub(crate) ui: Option<Rc<RefCell<LuaUi>>>,
    pub(crate) requests: Vec<Request>,
    pub(crate) device: Rc<RefCell<DeviceState>>,
    pub(crate) library: Rc<RefCell<library::Host>>,
    pub(crate) reader: Option<Rc<RefCell<dyn ReaderHost>>>,
    pub(crate) active_book: Option<i64>,
    pub(crate) reader_book: Option<i64>,
    pub(crate) failed: BTreeMap<String, String>,
    pub(crate) initialized: bool,
    pub(crate) active: bool,
    pub(crate) reload: bool,
    pub(crate) error: Option<String>,
    pub(crate) paused: bool,
    pub entries: Vec<Plugin>,
    pub menus: Vec<MenuItem>,
    pub owners: BTreeMap<String, String>,
}

impl Plugins {
    pub fn new(paths: &kaia_platform::Paths) -> Self {
        let install = paths.install_dir.clone();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let koreader = if install.join("setupkoenv.lua").is_file() {
            install.clone()
        } else {
            std::fs::read_dir(&root)
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .find(|p| {
                    p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("koreader-emulator-"))
                })
                .map(|p| p.join("koreader"))
                .unwrap_or(install.clone())
        };
        let shim = if install.join("lua/datastorage.lua").is_file() {
            install.join("lua")
        } else {
            root.join("rust/kaia-lua/lua")
        };
        Self {
            paths: LuaPaths::new(shim, koreader, paths.data_dir.clone()),
            startup_safe_mode: false,
            ui: None,
            requests: vec![],
            device: Rc::default(),
            library: Rc::new(RefCell::new(library::Host::new(paths))),
            reader: None,
            active_book: None,
            reader_book: None,
            failed: BTreeMap::new(),
            initialized: false,
            active: false,
            reload: true,
            error: None,
            paused: false,
            entries: vec![],
            menus: vec![],
            owners: BTreeMap::new(),
        }
    }

    pub fn plugin_value(&self, plugin: &str, key: &str) -> Result<Value, String> {
        self.ui
            .as_ref()
            .ok_or("Plugins are disabled")?
            .borrow()
            .plugin_value(plugin, key)
            .map_err(|e| e.to_string())
    }

    pub fn running(&self) -> bool {
        self.active && self.ui.is_some()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn foreign_window_count(&self) -> usize {
        self.ui
            .as_ref()
            .map_or(0, |ui| ui.borrow().foreign_window_count())
    }
    pub fn idle_paused(&self) -> bool {
        self.paused
    }

    pub(crate) fn discover(&mut self, settings: &Settings) {
        let safe = self.startup_safe_mode || settings.is_true(SAFE_MODE);
        let mut entries = BTreeMap::new();
        for (root, bundled) in [
            (self.paths.plugins_dir(), true),
            (self.library.borrow().data.join("plugins"), false),
        ] {
            for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }
                let Some(name) = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_suffix(".koplugin"))
                else {
                    continue;
                };
                // A user folder cannot replace a bundled plugin with the same name.
                if entries.contains_key(name) {
                    continue;
                }
                let enabled = settings
                    .read(format!("plugins.on.{name}"))
                    .as_bool()
                    .unwrap_or(bundled && DEFAULTS.contains(&name));
                let state = if safe && !bundled {
                    tr("Disabled by safe mode")
                } else if enabled && settings.is_true(ENABLED) {
                    tr("Enabled")
                } else {
                    tr("Disabled")
                };
                entries.insert(
                    name.to_owned(),
                    Plugin {
                        name: name.into(),
                        bundled,
                        enabled: enabled && (bundled || !safe),
                        state,
                    },
                );
            }
        }
        self.entries = entries.into_values().collect();
    }

    pub(crate) fn event(&mut self, name: &str, args: Vec<Value>) {
        self.requests.push(Request::Event(Event::with_args(name, args)));
    }
}
