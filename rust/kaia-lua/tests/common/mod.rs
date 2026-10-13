//! Helpers for the integration tests: the KOReader tree, scratch folders and a
//! serial lock. Each test file uses only some of these.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard};

use kaia_lua::{LuaHost, LuaPaths};

/// LuaJIT states and KOReader's native modules are loaded once per process, so
/// the tests of one binary run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

/// The repository root, `rust/kaia-lua/../..`.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The Kaia shim directory, `rust/kaia-lua/lua`.
pub fn shim_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lua")
}

/// The KOReader tree that the desktop build creates: `koreader-emulator-*/koreader`.
/// `KAIA_KOREADER_DIR` overrides the search.
pub fn koreader_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("KAIA_KOREADER_DIR") {
        return PathBuf::from(dir);
    }
    let tag = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else {
        "linux"
    };
    let mut found: Vec<PathBuf> = fs::read_dir(repo_root())
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("koreader-emulator-") && n.contains(tag))
                })
                .map(|p| p.join("koreader"))
                .filter(|p| p.join("frontend/util.lua").is_file())
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found.into_iter().next().unwrap_or_else(|| {
        panic!("no KOReader emulator tree found. Run ./kodev build or set KAIA_KOREADER_DIR.")
    })
}

/// The KOReader translations in the repository.
pub fn repo_l10n() -> PathBuf {
    repo_root().join("l10n")
}

/// A scratch directory that is removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(label: &str) -> TempDir {
        let mut path = std::env::temp_dir();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        path.push(format!("kaia-lua-{label}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create temp dir");
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Paths for a host with the given data folder.
pub fn paths(data_dir: &Path) -> LuaPaths {
    LuaPaths::new(shim_dir(), koreader_dir(), data_dir)
}

/// A host with its own data folder, which the caller keeps alive.
pub fn host(data: &TempDir) -> LuaHost {
    LuaHost::new(paths(data.path())).expect("LuaHost::new")
}

/// The bundled Noto fonts and mdlight icons that the UI tests need.
pub fn ui_assets() -> kaia_ui::assets::SharedAssets {
    let root = repo_root().join("resources");
    kaia_ui::assets::Assets::load(&root.join("fonts/noto"), &root.join("icons/mdlight"))
        .unwrap_or_else(|e| panic!("cannot load the bundled fonts and icons: {e}"))
        .shared()
}

/// The Libra Colour screen in pixels, with its 300 DPI override.
pub const UI_SCREEN: kaia_gfx::Size = kaia_gfx::Size::new(1264, 1680);

/// A Lua UI on a manual clock. The clock handle moves time for scheduled tasks.
pub fn lua_ui(data: &TempDir, color: bool) -> (kaia_lua::LuaUi, kaia_events::ManualClock) {
    let clock = kaia_events::ManualClock::new();
    let config = kaia_lua::UiConfig {
        size: UI_SCREEN,
        dpi: Some(300),
        color,
    };
    let ui =
        kaia_lua::LuaUi::new(host(data), config, ui_assets(), Box::new(clock.clone())).expect("LuaUi::new");
    (ui, clock)
}

/// Path of a golden PNG in `tests/golden`.
pub fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}
