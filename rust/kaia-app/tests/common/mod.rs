//! Helpers for the headless test files. Each test file includes this module
//! with `mod common;`. A file does not use every helper, so dead code is allowed.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kaia_app::{App, AppOptions, HeadlessBackend, Route, ScriptInput, Services};
use kaia_display::PanelSpec;
use kaia_events::{RefreshMode, Settings, SystemClock, Value};
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

/// Pins the screens to English in the data folder `data`, so a test reads the same
/// text on any machine. `Services::open` reads the setting before the system
/// locale. A settings file that already has a language is left alone, and so is
/// one that cannot be read: the app keeps those bytes and reports them.
pub fn pin_english(data: &Path) {
    fs::create_dir_all(data).unwrap();
    let path = data.join("settings.json");
    if path.exists() && Settings::read_exact(&path).is_err() {
        return;
    }
    let mut settings = Settings::open(&path);
    if settings.read(kaia_app::i18n::SETTING).as_str().is_none() {
        settings.save(kaia_app::i18n::SETTING, Value::from("en"));
        settings.flush(&path).unwrap();
    }
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
    // MuPDF loads its fallback faces from FONTDIR, which defaults to ./fonts.
    // The device launcher starts in the install folder that has it; tests do not.
    if std::env::var_os("FONTDIR").is_none() {
        std::env::set_var("FONTDIR", repo_root().join("resources/fonts"));
    }
    pin_english(&data.join("data"));
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
    if let Some(slot) = ["nav-home", "nav-library", "nav-more"]
        .iter()
        .position(|k| *k == key)
    {
        let r = app.find_rect("nav-bar").expect("navigation bar");
        return Point::new(r.x + r.w as i32 * (2 * slot as i32 + 1) / 6, r.y + r.h as i32 / 2);
    }
    let r: Rect = app
        .find_rect(key)
        .unwrap_or_else(|| panic!("no widget {key:?} on the top window"));
    Point::new(r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
}

pub fn wait_reader(app: &mut App<HeadlessBackend>) {
    wait_for(app, "reader worker", |a| !a.reader_busy());
    app.process(Vec::new()).unwrap();
}

pub fn tap_key(app: &mut App<HeadlessBackend>, key: &str) {
    // Notes and Settings now open through More. Keep callers exercising taps.
    if matches!(key, "nav-notes" | "nav-settings") {
        tap_key(app, "nav-more");
        tap_key(
            app,
            if key == "nav-notes" {
                "more-notes"
            } else {
                "more-settings"
            },
        );
        return;
    }
    wait_reader(app);
    let p = centre_of(app, key);
    app.tap(p).unwrap();
    wait_reader(app);
    wait_for(app, "visible covers", |_| !kaia_app::covers::busy());
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
