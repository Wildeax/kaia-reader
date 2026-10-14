//! Review 3, R3-H5. A sync queue write that fails on exit keeps the app open, and
//! the pending update stays in memory. The next request to stop retries the write.
//! A second refusal offers to exit without the update. The sync server is never
//! set, so these tests make no network request.

mod common;

use std::cell::Cell;
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::thread;
use std::time::{Duration, Instant};

use common::{library_with_books, panel, pin_english, scratch, SERIAL};
use kaia_app::{App, AppOptions, Flow, Services};
use kaia_display::{Display, DisplayError, HeadlessDisplay, InputEvent, InputSource, Refresh, Rotation};
use kaia_events::{Event, ManualClock, Value};
use kaia_gfx::{PixelFormat, Pixmap, Point, Size};
use kaia_library::{Sort, SortKey};
use kaia_platform::{Env, Platform};
use kaia_sync::SyncQueue;

/// The toast from the first refused exit.
const REFUSED: &str = "Could not save the reading position for sync. Try again.";

/// The longest an idle poll sleeps.
const IDLE: Duration = Duration::from_millis(10);

/// A headless display. Its input comes from `next`, which runs once per poll.
struct Scripted {
    display: HeadlessDisplay,
    next: Box<dyn FnMut() -> Vec<InputEvent>>,
}

impl Display for Scripted {
    fn size(&self) -> Size {
        Display::size(&self.display)
    }
    fn format(&self) -> PixelFormat {
        Display::format(&self.display)
    }
    fn dpi(&self) -> u32 {
        Display::dpi(&self.display)
    }
    fn present(&mut self, frame: &Pixmap, refresh: &Refresh) -> Result<(), DisplayError> {
        Display::present(&mut self.display, frame, refresh)
    }
    fn set_rotation(&mut self, rotation: Rotation) -> Result<(), DisplayError> {
        Display::set_rotation(&mut self.display, rotation)
    }
}

impl InputSource for Scripted {
    fn poll(&mut self, timeout: Option<Duration>) -> Vec<InputEvent> {
        let events = (self.next)();
        if events.is_empty() {
            thread::sleep(timeout.map_or(IDLE, |t| t.min(IDLE)));
        }
        events
    }
}

/// An app on a manual clock, so a pending update keeps its deadline. The clock
/// does not move in these tests. The sync queue lives at `<dir>/data/sync-queue.json`.
fn app_in(dir: &Path) -> App<Scripted> {
    let books = library_with_books(dir);
    let root = dir.join("platform");
    fs::create_dir_all(&root).unwrap();
    let env = Env {
        home: Some(root.clone()),
        ..Env::default()
    };
    let platform = Platform::detect_in(&root, &env).unwrap();
    pin_english(&dir.join("data"));
    let mut services = Services::open(platform, panel(), Some(dir.join("data"))).unwrap();
    services.settings.save("device.sleep_minutes", Value::Number(0.0));
    let backend = Scripted {
        display: HeadlessDisplay::new(panel()),
        next: Box::new(Vec::<InputEvent>::new),
    };
    App::new(
        backend,
        services,
        Box::new(ManualClock::new()),
        AppOptions {
            library_roots: vec![books],
        },
    )
}

/// Runs the loop until `done` holds. It fails after two minutes.
fn wait_until(app: &mut App<Scripted>, done: impl Fn(&App<Scripted>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(120);
    while !done(app) {
        assert!(Instant::now() < deadline, "timed out waiting for the app");
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
}

/// Waits until the reader has no page work, then runs one more iteration.
fn settle(app: &mut App<Scripted>) {
    wait_until(app, |a| !a.reader_busy());
    app.process(Vec::new()).unwrap();
}

/// The id of juliet.epub in the library index.
fn juliet_id(app: &App<Scripted>) -> i64 {
    app.services()
        .library
        .books(Sort::new(SortKey::Title, false), &Default::default(), 0, 100)
        .unwrap()
        .into_iter()
        .find(|book| book.path.file_name().is_some_and(|name| name == "juliet.epub"))
        .expect("juliet.epub is in the library")
        .id
}

/// Opens the test book, turns sync on and records one pending update. Returns the
/// book's partial hash, which keys the update.
fn open_with_pending_update(app: &mut App<Scripted>) -> String {
    app.start().unwrap();
    wait_until(app, |a| !a.scan_running());
    let id = juliet_id(app);
    app.command(&format!("open_book:{id}"), &[]);
    settle(app);
    app.services_mut()
        .settings
        .save("sync.enabled", Value::Bool(true));
    // Pushes follow the automatic sync switch, as KOReader's do.
    app.services_mut().settings.save("sync.auto", Value::Bool(true));
    app.broadcast_app(Event::new("SaveState"));
    let md5 = app.services().library.book(id).unwrap().unwrap().partial_md5;
    assert!(
        app.services().sync.pending_progress(&md5).is_some(),
        "no update is pending"
    );
    md5
}

/// Replaces the input source. Polls made during setup do not reach the new source.
fn set_input(app: &mut App<Scripted>, next: Box<dyn FnMut() -> Vec<InputEvent>>) {
    app.backend_mut().next = next;
}

/// The centre of the widget `key` on the top window.
fn widget_centre(app: &App<Scripted>, key: &str) -> Point {
    let r = app
        .find_rect(key)
        .unwrap_or_else(|| panic!("no widget {key} on the top window"));
    Point::new(r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
}

/// The documents in the sync queue at `path`. A missing file holds none.
fn queued(path: &Path) -> Vec<String> {
    SyncQueue::new(path)
        .load()
        .unwrap()
        .into_iter()
        .map(|item| item.progress.document)
        .collect()
}

/// The probe from the review. The queue path is a directory, so the first quit
/// cannot save. `run` must not return after that quit. Once the path works, the
/// second quit saves the update and `run` returns.
#[test]
fn failed_queue_write_keeps_app_open_until_the_next_quit() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("r3-shutdown-probe");
    let mut app = app_in(&dir);
    let md5 = open_with_pending_update(&mut app);
    let queue = app.services().paths.data_dir.join("sync-queue.json");
    fs::create_dir_all(&queue).unwrap();

    // Poll 1 is the first quit, made while the queue is blocked. Poll 2 fixes the
    // path and quits again. A third poll means the app kept running after it saved.
    let polls = Rc::new(Cell::new(0_u32));
    let count = polls.clone();
    let blocked = queue.clone();
    set_input(
        &mut app,
        Box::new(move || {
            count.set(count.get() + 1);
            match count.get() {
                1 => vec![InputEvent::Quit],
                2 => {
                    assert!(blocked.is_dir(), "the queue path should still be blocked");
                    fs::remove_dir(&blocked).unwrap();
                    vec![InputEvent::Quit]
                }
                _ => panic!("the app kept running after its update was saved"),
            }
        }),
    );

    app.run().unwrap();

    assert!(polls.get() >= 2, "run returned after the first refused save");
    assert_eq!(queued(&queue), vec![md5.clone()]);
    assert!(app.services().sync.pending_progress(&md5).is_none());
    assert!(app.notices().iter().any(|note| note == REFUSED));
}

/// The way out. After the second refused save, the prompt offers to exit without
/// the update. Taking it exits, and the update is dropped. The blocked path is
/// not replaced.
#[test]
fn second_refusal_offers_exit_anyway_and_exit_drops_the_update() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("r3-shutdown-exit");
    let mut app = app_in(&dir);
    let md5 = open_with_pending_update(&mut app);
    let queue = app.services().paths.data_dir.join("sync-queue.json");
    fs::create_dir_all(&queue).unwrap();

    app.quit();
    assert_eq!(
        app.process(Vec::new()).unwrap(),
        Flow::Continue,
        "the first refusal keeps the app open"
    );
    assert!(app.notices().iter().any(|note| note == REFUSED));
    assert!(app.find_rect("dialog-button:sync:exit-anyway").is_none());

    app.quit();
    assert_eq!(
        app.process(Vec::new()).unwrap(),
        Flow::Continue,
        "the second refusal keeps the app open"
    );
    assert!(
        app.find_rect("dialog-button:sync:exit-anyway").is_some(),
        "no prompt offers to exit"
    );

    let exit = widget_centre(&app, "dialog-button:sync:exit-anyway");
    assert_eq!(app.tap(exit).unwrap(), Flow::Exit);
    app.shutdown();
    assert!(
        app.services().sync.pending_progress(&md5).is_none(),
        "the update should be dropped"
    );
    assert!(queue.is_dir(), "the blocked path must not be replaced");
}

/// The other choice at the prompt. Keep reading leaves the update pending, and the
/// next quit saves it once the path works.
#[test]
fn keep_reading_keeps_the_update_for_the_next_quit() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("r3-shutdown-keep");
    let mut app = app_in(&dir);
    let md5 = open_with_pending_update(&mut app);
    let queue = app.services().paths.data_dir.join("sync-queue.json");
    fs::create_dir_all(&queue).unwrap();

    app.quit();
    assert_eq!(app.process(Vec::new()).unwrap(), Flow::Continue);
    app.quit();
    assert_eq!(app.process(Vec::new()).unwrap(), Flow::Continue);

    let keep = widget_centre(&app, "dialog-button:dialog:dismiss");
    assert_eq!(
        app.tap(keep).unwrap(),
        Flow::Continue,
        "Keep reading keeps the app open"
    );
    assert!(
        app.find_rect("dialog-button:sync:exit-anyway").is_none(),
        "the prompt should close"
    );
    assert!(
        app.services().sync.pending_progress(&md5).is_some(),
        "the update should stay pending"
    );

    fs::remove_dir(&queue).unwrap();
    app.quit();
    assert_eq!(app.process(Vec::new()).unwrap(), Flow::Exit);
    app.shutdown();
    assert_eq!(queued(&queue), vec![md5]);
}
