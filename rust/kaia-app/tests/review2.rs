mod common;
use common::*;
use kaia_app::surfaces::reader::page::PageView;
use kaia_app::{App, HeadlessBackend, ScriptInput, Services};
use kaia_events::{Event, Settings, Value};
use kaia_platform::{Env, Platform};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

fn opened(name: &str) -> (App<HeadlessBackend>, PathBuf, i64) {
    let dir = scratch(name);
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_page(&mut app, 0);
    (app, dir, id)
}
fn state_path(app: &App<HeadlessBackend>, id: i64) -> PathBuf {
    let b = app.services().library.book(id).unwrap().unwrap();
    app.services()
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{}.json", b.partial_md5))
}
fn wait_page(app: &mut App<HeadlessBackend>, expected: u32) {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        app.iterate(Some(Duration::from_millis(10))).unwrap();
        let mut page = None;
        app.update_top::<PageView, _>("reader-page", |v, _| page = v.position);
        if page.is_some_and(|p| p.0 == expected) {
            app.process(vec![]).unwrap();
            return;
        }
        assert!(
            Instant::now() < end,
            "page {expected} did not arrive, got {page:?}"
        );
    }
}
fn position(app: &mut App<HeadlessBackend>) -> u32 {
    let mut page = None;
    app.update_top::<PageView, _>("reader-page", |v, _| page = v.position);
    page.unwrap().0
}
#[test]
fn unreadable_settings_keep_original_bytes() {
    let dir = scratch("r2-corrupt");
    fs::create_dir_all(dir.join("data")).unwrap();
    let bad = b"{\"recoverable\":42,";
    fs::write(dir.join("data/settings.json.old"), bad).unwrap();
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    assert!(app.notices().iter().any(|message| message.contains("unreadable")));
    assert!(fs::read_dir(dir.join("data"))
        .unwrap()
        .filter_map(Result::ok)
        .any(|f| fs::read(f.path()).is_ok_and(|b| b == bad)));
}
#[test]
fn second_app_is_refused_until_first_drops() {
    let dir = scratch("r2-lock");
    let a = make_app(&dir, vec![]);
    let open = || {
        let root = dir.join("platform");
        let platform = Platform::detect_in(
            &root,
            &Env {
                home: Some(root.clone()),
                ..Env::default()
            },
        )
        .unwrap();
        pin_english(&dir.join("data"));
        Services::open(platform, panel(), Some(dir.join("data")))
    };
    assert!(open().err().is_some_and(|e| e.to_string().contains("already")));
    drop(a);
    assert!(open().is_ok());
}
#[test]
fn idle_jump_saves_and_updates_index() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, id) = opened("r2-idle");
    app.broadcast_app(Event::new("SaveState"));
    let path = state_path(&app, id);
    let before = fs::read(&path).unwrap();
    app.command("reader:location", &[Value::Number(20.0)]);
    wait_page(&mut app, 20);
    assert_eq!(position(&mut app), 20);
    let end = Instant::now() + Duration::from_secs(6);
    while Instant::now() < end {
        app.iterate(Some(Duration::from_millis(50))).unwrap();
    }
    assert_ne!(before, fs::read(path).unwrap());
    assert!(app.services().library.book(id).unwrap().unwrap().percent > 0.05);
}
#[test]
fn failed_close_keeps_reader_until_retry() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, id) = opened("r2-close");
    let path = state_path(&app, id);
    app.broadcast_app(Event::new("SaveState"));
    app.command("reader:location", &[Value::Number(20.0)]);
    wait_page(&mut app, 20);
    let parent = path.parent().unwrap();
    let held = parent.with_extension("held");
    fs::rename(parent, &held).unwrap();
    fs::write(parent, b"storage unavailable").unwrap();
    app.command("nav:home", &[]);
    assert_eq!(app.top_route(), Some(&kaia_app::Route::Reader { book: id }));
    assert!(app.notices().iter().any(|n| n.contains("save")));
    fs::remove_file(parent).unwrap();
    fs::rename(held, parent).unwrap();
    app.command("nav:home", &[]);
    assert_eq!(app.top_route(), Some(&kaia_app::Route::Home));
}
#[test]
fn reader_finished_updates_library() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, id) = opened("r2-finish");
    app.command("reader:finish", &[]);
    app.command("nav:home", &[]);
    assert_eq!(
        app.services().library.book(id).unwrap().unwrap().status,
        kaia_library::Status::Complete
    );
}
#[test]
fn suspend_updates_library_progress() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, id) = opened("r2-suspend");
    let before = app.services().library.book(id).unwrap().unwrap().percent;
    app.command("reader:location", &[Value::Number(20.0)]);
    wait_page(&mut app, 20);
    app.broadcast_app(Event::new("Suspend"));
    assert!(app.services().library.book(id).unwrap().unwrap().percent > before);
}
#[test]
fn global_font_uses_koreader_units_and_keeps_saved_layout() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("r2-font");
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    app.services_mut()
        .settings
        .save("reading.font_size", Value::Number(30.0));
    app.services_mut()
        .settings
        .save("reading.font_family", Value::from("Noto Sans"));
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_page(&mut app, 0);
    app.broadcast_app(Event::new("SaveState"));
    let path = state_path(&app, id);
    let saved = Settings::open(&path);
    assert_eq!(saved.read("layout").get("font_size").as_f64(), Some(64.0));
    assert_eq!(
        saved.read("layout").get("font_family").as_str(),
        Some("Noto Sans")
    );
    app.command("nav:home", &[]);
    app.services_mut()
        .settings
        .save("reading.font_size", Value::Number(40.0));
    app.command(&format!("open_book:{id}"), &[]);
    wait_page(&mut app, 0);
    app.broadcast_app(Event::new("SaveState"));
    assert_eq!(
        Settings::open(&path).read("layout").get("font_size").as_f64(),
        Some(64.0)
    );
}
#[test]
fn export_names_are_fat_safe() {
    let dir = scratch("r2-export");
    let out = kaia_app::surfaces::notes::export::export(&dir, &[], 1_700_000_000).unwrap();
    for path in [out.json, out.markdown] {
        assert!(!path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains([':', '<', '>', '"', '/', '\\', '|', '?', '*']));
    }
}
#[test]
fn malformed_script_returns_error_without_panic() {
    assert!(ScriptInput::from_script("down 0 1 1 18446744073709551\ntap 2 2\n", false).is_err());
}
#[test]
fn reading_records_statistics() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, _) = opened("r2-stats");
    let end = Instant::now() + Duration::from_secs(6);
    while Instant::now() < end {
        app.iterate(Some(Duration::from_millis(50))).unwrap();
    }
    app.command("reader:next", &[]);
    wait_page(&mut app, 1);
    app.broadcast_app(Event::new("Suspend"));
    let mut stats =
        kaia_stats::Stats::open(app.services().paths.data_dir.join("statistics.sqlite3")).unwrap();
    let rows = stats.books().unwrap();
    assert!(!rows.is_empty());
    assert!(stats.book_totals(rows[0].id).unwrap().unwrap().read_seconds >= 5);
}
#[test]
fn jump_replaces_pending_sync_position() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, id) = opened("r2-jump-sync");
    app.services_mut()
        .settings
        .save("sync.enabled", Value::Bool(true));
    for page in 1..=10 {
        app.command("reader:next", &[]);
        wait_page(&mut app, page);
    }
    let md5 = app.services().library.book(id).unwrap().unwrap().partial_md5;
    let before = app
        .services()
        .sync
        .pending_progress(&md5)
        .map(|p| p.progress.clone());
    app.command("reader:location", &[Value::Number(50.0)]);
    wait_page(&mut app, 50);
    let after = app
        .services()
        .sync
        .pending_progress(&md5)
        .map(|p| p.progress.clone());
    assert!(after.is_some());
    assert_ne!(before, after);
}
#[test]
fn long_dictionary_can_reach_last_definition() {
    let dir = scratch("r2-dictionary");
    let app = make_app(&dir, vec![]);
    let text = (0..100).map(|i| format!("Definition {i}.\n")).collect::<String>();
    let window = kaia_app::surfaces::reader::panels::dictionary(app.services(), app.screen(), "word", &text);
    let mut ctx = kaia_events::Ctx::new(
        Box::new(kaia_events::ManualClock::new()),
        kaia_events::Rect::new(0, 0, 1264, 1680),
    );
    window.show(&mut ctx, Default::default());
    let mut frame = kaia_gfx::Pixmap::new(1264, 1680, app.services().theme.pixel_format());
    window.paint(&mut frame, &mut ctx.dirty);
    assert!(
        window.update::<kaia_ui::composite::PagedList<String>, _>(&mut ctx, "dict-pages", |list, cx| {
            assert_eq!(list.items().join(""), text);
            assert!(list.page_count() > 1);
            list.set_page(list.page_count() - 1, cx);
            assert_eq!(list.page(), list.page_count() - 1);
        })
    );
    window.paint(&mut frame, &mut ctx.dirty);
    kaia_app::app::save_frame(&frame, &std::env::temp_dir().join("kaia-r2-dictionary-last.png")).unwrap();
}
#[test]
fn library_finished_updates_saved_book() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, id) = opened("r2-library-status");
    let path = state_path(&app, id);
    app.command("nav:library", &[]);
    app.command(&format!("library:book:finished:{id}"), &[]);
    assert_eq!(
        Settings::open(&path).read("summary").get("status").as_str(),
        Some("complete")
    );
}
#[test]
fn startup_reconciles_saved_progress() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir, id) = opened("r2-reconcile");
    app.command("reader:location", &[Value::Number(20.0)]);
    wait_page(&mut app, 20);
    app.command("nav:home", &[]);
    let expected = app.services().library.book(id).unwrap().unwrap().percent;
    app.services_mut().library.record_open(id, 0.01).unwrap();
    drop(app);
    let app = make_app(&dir, vec![]);
    assert_eq!(
        app.services().library.book(id).unwrap().unwrap().percent,
        expected
    );
}
#[test]
fn left_handed_taps_and_hidden_finished_books_apply() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, id) = opened("r2-preferences");
    app.services_mut()
        .settings
        .save("reading.tap_zones", Value::from("left_handed"));
    app.command("reader:tap", &[Value::Number(100.0), Value::Number(1300.0)]);
    wait_page(&mut app, 1);
    app.command("reader:finish", &[]);
    app.command("nav:home", &[]);
    app.services_mut()
        .settings
        .save("library.show_finished", Value::Bool(false));
    let prefs = kaia_app::surfaces::library::data::Prefs::load(&app.services().settings);
    assert!(
        !kaia_app::surfaces::library::data::list(&app.services().library, &prefs)
            .iter()
            .any(|b| b.id == id)
    );
}
#[test]
fn lock_child() {
    let Some(dir) = std::env::var_os("KAIA_LOCK_TEST_DIR") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let _app = make_app(&dir, vec![]);
    fs::write(dir.join("ready"), b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(30));
}
#[test]
fn instance_lock_is_released_after_process_death() {
    let dir = scratch("r2-crash-lock");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "lock_child", "--nocapture"])
        .env("KAIA_LOCK_TEST_DIR", &dir)
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while !dir.join("ready").exists() {
        assert!(Instant::now() < end, "child did not acquire lock");
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let _app = make_app(&dir, vec![]);
}

struct NightBackend {
    inner: HeadlessBackend,
    night: bool,
}
impl kaia_display::Display for NightBackend {
    fn size(&self) -> kaia_gfx::Size {
        kaia_display::Display::size(&self.inner)
    }
    fn format(&self) -> kaia_gfx::PixelFormat {
        kaia_display::Display::format(&self.inner)
    }
    fn dpi(&self) -> u32 {
        kaia_display::Display::dpi(&self.inner)
    }
    fn present(
        &mut self,
        frame: &kaia_gfx::Pixmap,
        refresh: &kaia_display::Refresh,
    ) -> Result<(), kaia_display::DisplayError> {
        kaia_display::Display::present(&mut self.inner, frame, refresh)
    }
    fn set_rotation(&mut self, rotation: kaia_display::Rotation) -> Result<(), kaia_display::DisplayError> {
        kaia_display::Display::set_rotation(&mut self.inner, rotation)
    }
    fn set_night_mode(&mut self, on: bool) {
        self.night = on;
    }
}
impl kaia_display::InputSource for NightBackend {
    fn poll(&mut self, timeout: Option<Duration>) -> Vec<kaia_display::InputEvent> {
        kaia_display::InputSource::poll(&mut self.inner, timeout)
    }
}
#[test]
fn night_mode_reaches_display_at_startup_and_when_changed() {
    let dir = scratch("r2-night");
    let platform = Platform::detect_in(
        &dir,
        &Env {
            home: Some(dir.clone()),
            ..Env::default()
        },
    )
    .unwrap();
    pin_english(&dir.join("data"));
    let mut services = Services::open(platform, panel(), Some(dir.join("data"))).unwrap();
    services.settings.save("display.night_mode", Value::Bool(true));
    let backend = NightBackend {
        inner: HeadlessBackend::new(panel(), ScriptInput::idle()),
        night: false,
    };
    let mut app = App::new(
        backend,
        services,
        Box::new(kaia_events::ManualClock::new()),
        Default::default(),
    );
    app.start().unwrap();
    assert!(app.backend().night);
    app.command("nav:settings", &[]);
    app.command("settings:display", &[]);
    app.command("settings:set:display.night_mode", &[Value::Bool(false)]);
    app.process(vec![]).unwrap();
    assert!(!app.backend().night);
}
#[test]
fn statistics_exclude_time_asleep() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("r2-stats-suspend");
    let books = library_with_books(&dir);
    let platform = Platform::detect_in(
        &dir,
        &Env {
            home: Some(dir.clone()),
            ..Env::default()
        },
    )
    .unwrap();
    pin_english(&dir.join("data"));
    let mut services = Services::open(platform, panel(), Some(dir.join("data"))).unwrap();
    services.settings.save("device.sleep_minutes", Value::Number(0.0));
    let clock = kaia_events::ManualClock::new();
    let mut app = App::new(
        HeadlessBackend::new(panel(), ScriptInput::idle()),
        services,
        Box::new(clock.clone()),
        kaia_app::AppOptions {
            library_roots: vec![books],
        },
    );
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    wait_page(&mut app, 0);
    clock.advance(Duration::from_secs(6));
    app.broadcast_app(Event::new("Suspend"));
    clock.advance(Duration::from_secs(3600));
    app.broadcast_app(Event::new("Resume"));
    clock.advance(Duration::from_secs(6));
    app.command("reader:next", &[]);
    wait_page(&mut app, 1);
    app.command("nav:home", &[]);
    let mut stats =
        kaia_stats::Stats::open(app.services().paths.data_dir.join("statistics.sqlite3")).unwrap();
    let rows = stats.books().unwrap();
    assert_eq!(stats.book_totals(rows[0].id).unwrap().unwrap().read_seconds, 12);
}
#[test]
fn quit_waits_for_pending_book_job() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _, _) = opened("r2-quit-job");
    app.command("reader:location", &[Value::Number(40.0)]);
    app.quit();
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        if app.iterate(Some(Duration::from_millis(10))).unwrap() == kaia_app::Flow::Exit {
            break;
        }
        assert!(
            Instant::now() < end,
            "quit request was lost while a page job was pending"
        );
    }
}
