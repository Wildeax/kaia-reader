//! Helpers for the headless test files. Each test file includes this module
//! with `mod common;`. A file does not use every helper, so dead code is allowed.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kaia_app::{App, AppOptions, HeadlessBackend, Route, ScriptInput, Services};
use kaia_display::PanelSpec;
use kaia_events::{RefreshMode, SystemClock};
use kaia_gfx::{Point, Rect};
use kaia_library::{Library, Sort, SortKey};
use kaia_platform::{Env, Platform};

/// The engines share state within a process, so the tests run one at a time.
pub static SERIAL: Mutex<()> = Mutex::new(());

pub const TEST_BOOKS: &[&str] = &["juliet.epub", "leaves.epub", "2col.pdf"];

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

/// A fresh scratch directory under the system temp folder.
pub fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaia-app-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A library folder with the three test books copied in.
pub fn library_with_books(dir: &Path) -> PathBuf {
    let lib = dir.join("books");
    fs::create_dir_all(&lib).unwrap();
    for name in TEST_BOOKS {
        fs::copy(repo_root().join("test").join(name), lib.join(name)).unwrap();
    }
    lib
}

pub fn panel() -> PanelSpec {
    PanelSpec::LIBRA_COLOUR
}

/// An app on a headless backend that reads `input`. `roots` is the library scan.
pub fn make_app_with(data: &Path, roots: Vec<PathBuf>, input: ScriptInput) -> App<HeadlessBackend> {
    let root = data.join("platform");
    fs::create_dir_all(&root).unwrap();
    let env = Env {
        home: Some(root.clone()),
        ..Env::default()
    };
    let platform = Platform::detect_in(&root, &env).unwrap();
    let services = Services::open(platform, panel(), Some(data.join("data"))).unwrap();
    let backend = HeadlessBackend::new(panel(), input);
    App::new(
        backend,
        services,
        Box::new(SystemClock::new()),
        AppOptions { library_roots: roots },
    )
}

pub fn make_app(data: &Path, roots: Vec<PathBuf>) -> App<HeadlessBackend> {
    make_app_with(data, roots, ScriptInput::idle())
}

/// Runs the loop until `done` holds. Fails after two minutes.
pub fn wait_for(app: &mut App<HeadlessBackend>, what: &str, done: impl Fn(&App<HeadlessBackend>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
}

/// The centre of the widget with `key` on the top window.
pub fn centre_of(app: &App<HeadlessBackend>, key: &str) -> Point {
    let r: Rect = app
        .find_rect(key)
        .unwrap_or_else(|| panic!("no widget {key:?} on the top window"));
    Point::new(r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
}

pub fn tap_key(app: &mut App<HeadlessBackend>, key: &str) {
    let p = centre_of(app, key);
    app.tap(p).unwrap();
}

/// The refresh modes sent since the loop started, after the first `skip`.
pub fn modes_after(app: &App<HeadlessBackend>, skip: usize) -> Vec<RefreshMode> {
    app.refresh_modes()[skip..].to_vec()
}

pub fn library_db(app: &App<HeadlessBackend>) -> &Library {
    &app.services().library
}

/// The id of the book whose file is `name`, from the library index.
pub fn book_id(app: &App<HeadlessBackend>, name: &str) -> i64 {
    library_db(app)
        .books(Sort::new(SortKey::Title, false), &Default::default(), 0, 100)
        .unwrap()
        .into_iter()
        .find(|b| b.path.file_name().is_some_and(|f| f == name))
        .unwrap_or_else(|| panic!("{name} is not in the library"))
        .id
}

/// Opens `name` from the Library list: Home, Library, then the book's row.
pub fn open_from_library(app: &mut App<HeadlessBackend>, name: &str) {
    if app.top_route() != Some(&Route::Library { page: 0 }) {
        tap_key(app, "library");
    }
    let id = book_id(app, name);
    tap_key(app, &format!("book-{id}"));
}
