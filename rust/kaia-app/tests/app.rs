//! Headless tests for the app loop, navigation, the scan thread and the surfaces.
//!
//! Each test runs the app on a `HeadlessBackend`, so frames and refreshes are
//! recorded without a window. Goldens are checked with `kaia_gfx::golden`; run
//! with `KAIA_UPDATE_GOLDEN=1` to write them.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use kaia_app::surfaces::reader::ReaderPage;
use kaia_app::{App, AppOptions, HeadlessBackend, Route, ScriptInput, Services};
use kaia_display::PanelSpec;
use kaia_events::{RefreshMode, SystemClock};
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::{Point, Rect};
use kaia_input::Key;
use kaia_library::{Library, Sort, SortKey};
use kaia_platform::{Env, Platform};

/// The engines share state within a process, so the tests run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

const TEST_BOOKS: &[&str] = &["juliet.epub", "leaves.epub", "2col.pdf"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

/// A fresh scratch directory under the system temp folder.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaia-app-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A library folder with the three test books copied in.
fn library_with_books(dir: &Path) -> PathBuf {
    let lib = dir.join("books");
    fs::create_dir_all(&lib).unwrap();
    for name in TEST_BOOKS {
        fs::copy(repo_root().join("test").join(name), lib.join(name)).unwrap();
    }
    lib
}

fn panel() -> PanelSpec {
    PanelSpec::LIBRA_COLOUR
}

/// An app on a headless backend that reads `input`. `roots` is the library scan.
fn make_app_with(data: &Path, roots: Vec<PathBuf>, input: ScriptInput) -> App<HeadlessBackend> {
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

fn make_app(data: &Path, roots: Vec<PathBuf>) -> App<HeadlessBackend> {
    make_app_with(data, roots, ScriptInput::idle())
}

/// Runs the loop until `done` holds. Fails after two minutes.
fn wait_for(app: &mut App<HeadlessBackend>, what: &str, done: impl Fn(&App<HeadlessBackend>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
}

/// The centre of the widget with `key` on the top window.
fn centre_of(app: &App<HeadlessBackend>, key: &str) -> Point {
    let r: Rect = app
        .find_rect(key)
        .unwrap_or_else(|| panic!("no widget {key:?} on the top window"));
    Point::new(r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
}

fn tap_key(app: &mut App<HeadlessBackend>, key: &str) {
    let p = centre_of(app, key);
    app.tap(p).unwrap();
}

/// The refresh modes sent since the loop started, after the first `skip`.
fn modes_after(app: &App<HeadlessBackend>, skip: usize) -> Vec<RefreshMode> {
    app.refresh_modes()[skip..].to_vec()
}

fn library_db(app: &App<HeadlessBackend>) -> &Library {
    &app.services().library
}

/// The id of the book whose file is `name`, from the library index.
fn book_id(app: &App<HeadlessBackend>, name: &str) -> i64 {
    library_db(app)
        .books(Sort::new(SortKey::Title, false), &Default::default(), 0, 100)
        .unwrap()
        .into_iter()
        .find(|b| b.path.file_name().is_some_and(|f| f == name))
        .unwrap_or_else(|| panic!("{name} is not in the library"))
        .id
}

/// Opens `name` from the Library list: Home, Library, then the book's row.
fn open_from_library(app: &mut App<HeadlessBackend>, name: &str) {
    if app.top_route() != Some(&Route::Library { page: 0 }) {
        tap_key(app, "library");
    }
    let id = book_id(app, name);
    tap_key(app, &format!("book-{id}"));
}

#[test]
fn startup_renders_home() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("startup");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    assert_eq!(app.top_route(), Some(&Route::Home));
    assert!(
        app.find_rect("library").is_some(),
        "Home shows the Library button"
    );
    assert!(
        app.find_rect("continue").is_none(),
        "an empty library has nothing to continue"
    );
    assert_eq!(
        app.refresh_modes(),
        &[RefreshMode::Full],
        "the first frame is a full refresh"
    );
    assert_eq!(app.stats().frames, 1);
}

#[test]
fn tapping_library_shows_library() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("tap-library");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let before = app.refresh_modes().len();

    tap_key(&mut app, "library");

    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert!(app.find_rect("next").is_some(), "Library shows the page buttons");
    assert!(app.find_rect("library").is_none(), "Home is gone");
    // The press shows on Home first (Fast), then the root changes (Ui).
    assert_eq!(
        modes_after(&app, before),
        vec![RefreshMode::Fast, RefreshMode::Ui]
    );
    assert_eq!(app.router().routes().len(), 1, "the root is replaced, not pushed");
}

#[test]
fn opening_a_book_shows_the_reader_with_a_full_refresh() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("open-book");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();

    let before = app.refresh_modes().len();
    open_from_library(&mut app, "juliet.epub");

    let id = book_id(&app, "juliet.epub");
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));
    let mut pages = 0;
    app.update_top::<ReaderPage, _>("reader", |r, _| pages = r.page_count());
    assert!(
        pages >= 3,
        "the test book needs at least three pages, has {pages}"
    );
    assert_eq!(
        app.refresh_modes().last(),
        Some(&RefreshMode::Full),
        "opening the reader is a full refresh"
    );
    assert!(modes_after(&app, before).contains(&RefreshMode::Full));
    assert!(library_db(&app)
        .book(id)
        .unwrap()
        .unwrap()
        .last_opened_at
        .is_some());
}

#[test]
fn page_turns_are_partial_refreshes_and_back_returns() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("page-turns");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    open_from_library(&mut app, "juliet.epub");
    let id = book_id(&app, "juliet.epub");

    let before = app.refresh_modes().len();
    app.key(Key::RPgFwd).unwrap();
    assert_eq!(
        modes_after(&app, before),
        vec![RefreshMode::Partial],
        "one page forward"
    );

    // A tap on the right side turns the page too.
    let (w, h) = (app.screen().w as i32, app.screen().h as i32);
    let before = app.refresh_modes().len();
    app.tap(Point::new(w * 9 / 10, h / 2)).unwrap();
    assert_eq!(
        modes_after(&app, before),
        vec![RefreshMode::Partial],
        "a right tap is a page forward"
    );

    let mut page = 0;
    app.update_top::<ReaderPage, _>("reader", |r, _| page = r.page());
    assert_eq!(page, 2);

    // The left third goes back a page.
    let before = app.refresh_modes().len();
    app.tap(Point::new(w / 10, h / 2)).unwrap();
    assert_eq!(modes_after(&app, before), vec![RefreshMode::Partial]);

    // Back leaves the reader, returns to the Library, and saves the progress.
    let before = app.refresh_modes().len();
    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert_eq!(modes_after(&app, before), vec![RefreshMode::Ui]);
    let book = library_db(&app).book(id).unwrap().unwrap();
    assert!(
        book.percent > 0.0,
        "progress is saved on Back, got {}",
        book.percent
    );
}

#[test]
fn centre_tap_opens_the_overlay_and_back_closes_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("overlay");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    open_from_library(&mut app, "juliet.epub");
    let id = book_id(&app, "juliet.epub");

    let (w, h) = (app.screen().w as i32, app.screen().h as i32);
    app.tap(Point::new(w / 2, h / 2)).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Overlay));
    assert!(app.find_rect("back").is_some());

    // The overlay's Back button closes the overlay, not the reader.
    tap_key(&mut app, "back");
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));

    // A tap the overlay does not use also closes it.
    app.tap(Point::new(w / 2, h / 2)).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Overlay));
    app.tap(Point::new(w / 2, h - 20)).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));
}

#[test]
fn a_book_that_cannot_open_shows_an_error_and_back_still_works() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("broken");
    let lib = dir.join("books");
    fs::create_dir_all(&lib).unwrap();
    fs::write(lib.join("broken.epub"), b"not a zip file at all").unwrap();
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    open_from_library(&mut app, "broken.epub");

    let id = book_id(&app, "broken.epub");
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));
    let mut pages = None;
    app.update_top::<ReaderPage, _>("reader", |r, _| pages = Some(r.page_count()));
    assert_eq!(pages, Some(0), "a book that does not open has no pages");

    app.key(Key::Back).unwrap();
    assert!(matches!(app.top_route(), Some(Route::Library { .. })));
}

#[test]
fn the_scan_thread_updates_home() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("scan-home");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    assert!(app.find_rect("continue").is_none(), "Home starts with no book");
    assert!(app.scan_running());

    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();

    assert_eq!(app.top_route(), Some(&Route::Home));
    assert!(
        app.find_rect("continue").is_some(),
        "Home features the newest book after the scan"
    );
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
}

#[test]
fn a_scripted_session_produces_the_expected_refreshes() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("scripted");

    // Find where the Library button sits on the empty Home, so the script can tap it.
    let mut probe = make_app(&dir, Vec::new());
    probe.start().unwrap();
    let p = centre_of(&probe, "library");
    drop(probe);

    let script = format!("tap {} {}\nkey Back\n", p.x, p.y);
    let input = ScriptInput::from_script(&script, true).unwrap();
    let mut app = make_app_with(&dir, Vec::new(), input);
    app.start().unwrap();
    app.run().unwrap();

    // Start: Home is drawn in full. Tap: the pressed button (Fast), then the Library
    // root (Ui). Back: Home again (Ui). The loop then quits with the script.
    assert_eq!(
        app.refresh_modes(),
        &[
            RefreshMode::Full,
            RefreshMode::Fast,
            RefreshMode::Ui,
            RefreshMode::Ui
        ]
    );
    assert_eq!(app.top_route(), Some(&Route::Home));
    assert_eq!(app.stats().refreshes, 4);
}

#[test]
fn home_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-home");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    assert_matches_golden(app.frame(), &golden("home.png"), 2);
}

#[test]
fn library_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-library");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    tap_key(&mut app, "library");
    assert_matches_golden(app.frame(), &golden("library.png"), 2);
}

#[test]
fn reader_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-reader");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    open_from_library(&mut app, "2col.pdf");
    assert_matches_golden(app.frame(), &golden("reader.png"), 2);
}
