//! Screenshots. A two-finger swipe along the screen diagonal saves the frame on
//! screen as a PNG in a `screenshots` folder. The tests read the saved file and
//! compare its pixels with the frame that was on screen before the gesture.

mod common;

use chrono::{Local, NaiveDateTime, Timelike};
use common::*;
use kaia_app::{App, AppOptions, HeadlessBackend, Route, ScriptInput, Services};
use kaia_display::InputEvent;
use kaia_events::{Clock, ManualClock};
use kaia_gfx::image::{decode_rgb, DecodeLimits};
use kaia_gfx::{Pixmap, Point};
use kaia_input::{Phase, TouchEvent};
use kaia_platform::{Env, Platform};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Both fingers move from `DIAGONAL_FROM` to `DIAGONAL_TO`. The midpoint travels
/// 1040 px right and 1386 px down, which is 1732 px along the diagonal. The screen
/// diagonal is 2102 px and the gesture needs the diagonal minus 200 units (1680 px).
const DIAGONAL_FROM: [Point; 2] = [Point { x: 80, y: 130 }, Point { x: 120, y: 170 }];
const DIAGONAL_TO: [Point; 2] = [Point { x: 1120, y: 1516 }, Point { x: 1160, y: 1556 }];

fn app_in(name: &str) -> (App<HeadlessBackend>, ManualClock, PathBuf) {
    let dir = scratch(name);
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
    let services = Services::open(platform, panel(), Some(dir.join("data"))).unwrap();
    let clock = ManualClock::new();
    let app = App::new(
        HeadlessBackend::new(panel(), ScriptInput::idle()),
        services,
        Box::new(clock.clone()),
        AppOptions {
            library_roots: vec![books],
        },
    );
    (app, clock, dir)
}

/// The books folder the app uses for screenshots. The desktop paths put it at `<home>/Books`.
fn books_folder(dir: &Path) -> PathBuf {
    dir.join("Books").join("screenshots")
}

fn data_folder(dir: &Path) -> PathBuf {
    dir.join("data").join("screenshots")
}

fn settle(app: &mut App<HeadlessBackend>, clock: &ManualClock) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.reader_busy() || app.scan_running() {
        assert!(Instant::now() < deadline, "the app stayed busy");
        clock.advance(Duration::from_millis(20));
        app.process(vec![]).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    app.process(vec![]).unwrap();
}

fn home(name: &str) -> (App<HeadlessBackend>, ManualClock, PathBuf) {
    let (mut app, clock, dir) = app_in(name);
    app.start().unwrap();
    settle(&mut app, &clock);
    (app, clock, dir)
}

fn reader(name: &str) -> (App<HeadlessBackend>, ManualClock, PathBuf) {
    let (mut app, clock, dir) = app_in(name);
    app.start().unwrap();
    settle(&mut app, &clock);
    let id = book_id(&app, "juliet.epub");
    app.command(&format!("open_book:{id}"), &[]);
    settle(&mut app, &clock);
    (app, clock, dir)
}

fn frame(phase: Phase, points: [Point; 2], clock: &ManualClock) -> InputEvent {
    let time_us = clock.now().as_micros() as u64;
    InputEvent::TouchFrame(
        points
            .into_iter()
            .enumerate()
            .map(|(slot, pos)| TouchEvent {
                slot: slot as u8,
                phase,
                pos,
                time_us,
            })
            .collect(),
    )
}

/// Two fingers land and move in equal steps, as the device sends them. The lift is
/// left to the caller. The frame on screen after the last move is returned, so the
/// test has the frame that the gesture finds on screen.
fn swipe_until_lift(
    app: &mut App<HeadlessBackend>,
    clock: &ManualClock,
    from: [Point; 2],
    to: [Point; 2],
) -> Pixmap {
    const STEPS: i32 = 10;
    app.process(vec![frame(Phase::Down, from, clock)]).unwrap();
    for i in 1..=STEPS {
        clock.advance(Duration::from_millis(20));
        let at =
            |a: Point, b: Point| Point::new(a.x + (b.x - a.x) * i / STEPS, a.y + (b.y - a.y) * i / STEPS);
        app.process(vec![frame(
            Phase::Move,
            [at(from[0], to[0]), at(from[1], to[1])],
            clock,
        )])
        .unwrap();
    }
    app.frame().clone()
}

fn lift(app: &mut App<HeadlessBackend>, clock: &ManualClock, to: [Point; 2]) {
    clock.advance(Duration::from_millis(20));
    app.process(vec![frame(Phase::Up, to, clock)]).unwrap();
}

/// PNG files in `folder`, sorted by name. A missing folder has no files.
fn pngs(folder: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .collect();
    files.sort();
    files
}

/// Runs the loop until `folder` holds `count` PNG files that decode, and the frame
/// has changed from `before`. The change is the save toast, which the loop paints
/// after the worker has written the file.
fn wait_for_saves(
    app: &mut App<HeadlessBackend>,
    clock: &ManualClock,
    folder: &Path,
    count: usize,
    before: &Pixmap,
) -> Vec<PathBuf> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        clock.advance(Duration::from_millis(10));
        app.process(vec![]).unwrap();
        let files = pngs(folder);
        let decoded = files.len() >= count && files.iter().all(|p| read_png(p).is_ok());
        if decoded && app.frame().data() != before.data() {
            return files;
        }
        assert!(
            Instant::now() < deadline,
            "no {count} screenshot(s) in {}",
            folder.display()
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn read_png(path: &Path) -> Result<Pixmap, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    decode_rgb(&bytes, DecodeLimits::default()).map_err(|e| e.to_string())
}

/// The file name is `Kaia_<YYYY-MM-DD_HHMMSS>.png` with the local time of the gesture.
fn assert_name(path: &Path, from: NaiveDateTime, to: NaiveDateTime) {
    let name = path.file_name().unwrap().to_str().unwrap();
    let stem = name
        .strip_suffix(".png")
        .unwrap_or_else(|| panic!("{name} does not end in .png"));
    let stamp = stem
        .strip_prefix("Kaia_")
        .unwrap_or_else(|| panic!("{name} does not start with Kaia_"));
    // A second screenshot in the same second gets `_2`, `_3` and so on.
    let stamp = stamp.split('_').take(2).collect::<Vec<_>>().join("_");
    let at = NaiveDateTime::parse_from_str(&stamp, "%Y-%m-%d_%H%M%S")
        .unwrap_or_else(|e| panic!("{name} has no timestamp: {e}"));
    let floor = from.with_nanosecond(0).unwrap();
    assert!(at >= floor && at <= to, "{name} is not between {floor} and {to}");
}

/// The saved pixels equal `frame`, with the same size and format.
fn assert_saved_frame(saved: &Pixmap, frame: &Pixmap) {
    assert_eq!(saved.width(), frame.width(), "width");
    assert_eq!(saved.height(), frame.height(), "height");
    assert_eq!(saved.format(), frame.format(), "pixel format");
    let differing = saved
        .data()
        .iter()
        .zip(frame.data())
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        differing == 0 && saved.data().len() == frame.data().len(),
        "the saved PNG differs from the frame in {differing} samples"
    );
}

/// The swipe saves the frame and shows the toast. Runs on Home.
#[test]
fn diagonal_swipe_saves_the_frame_on_home() {
    let (mut app, clock, dir) = home("shot-home");
    let folder = books_folder(&dir);
    assert!(!folder.exists(), "the screenshots folder starts out missing");

    let before = swipe_until_lift(&mut app, &clock, DIAGONAL_FROM, DIAGONAL_TO);
    let from = Local::now().naive_local();
    lift(&mut app, &clock, DIAGONAL_TO);
    let to = Local::now().naive_local();

    let files = wait_for_saves(&mut app, &clock, &folder, 1, &before);
    assert_eq!(files.len(), 1);
    assert_name(&files[0], from, to);
    assert_saved_frame(&read_png(&files[0]).unwrap(), &before);
}

/// The same swipe in the reader saves the page on screen.
#[test]
fn diagonal_swipe_saves_the_frame_in_the_reader() {
    let (mut app, clock, dir) = reader("shot-reader");
    assert!(
        matches!(app.top_route(), Some(Route::Reader { .. })),
        "the book is open"
    );
    let folder = books_folder(&dir);

    let before = swipe_until_lift(&mut app, &clock, DIAGONAL_FROM, DIAGONAL_TO);
    lift(&mut app, &clock, DIAGONAL_TO);

    let files = wait_for_saves(&mut app, &clock, &folder, 1, &before);
    assert_saved_frame(&read_png(&files[0]).unwrap(), &before);
}

/// Swipes that are not diagonal, or that stop short of the screen diagonal, do not save.
#[test]
fn straight_and_short_swipes_do_not_save() {
    let (mut app, clock, dir) = home("shot-no-save");
    let folder = books_folder(&dir);

    // A straight swipe to the right, 1120 px long.
    let flat = [Point::new(80, 130), Point::new(120, 170)];
    let flat_to = [Point::new(1200, 130), Point::new(1240, 170)];
    swipe_until_lift(&mut app, &clock, flat, flat_to);
    lift(&mut app, &clock, flat_to);

    // A diagonal of about 300 px.
    let short_to = [Point::new(200, 260), Point::new(240, 300)];
    swipe_until_lift(&mut app, &clock, DIAGONAL_FROM, short_to);
    lift(&mut app, &clock, short_to);

    for _ in 0..20 {
        clock.advance(Duration::from_millis(10));
        app.process(vec![]).unwrap();
    }
    assert!(pngs(&folder).is_empty(), "no screenshot for these swipes");
}

/// Two saves in one second get different names, and neither overwrites the other.
#[test]
fn saves_in_the_same_second_do_not_overwrite() {
    let (mut app, clock, dir) = home("shot-twice");
    let folder = books_folder(&dir);

    let before = swipe_until_lift(&mut app, &clock, DIAGONAL_FROM, DIAGONAL_TO);
    lift(&mut app, &clock, DIAGONAL_TO);
    wait_for_saves(&mut app, &clock, &folder, 1, &before);

    let before = swipe_until_lift(&mut app, &clock, DIAGONAL_FROM, DIAGONAL_TO);
    lift(&mut app, &clock, DIAGONAL_TO);
    let files = wait_for_saves(&mut app, &clock, &folder, 2, &before);

    assert_eq!(files.len(), 2);
    assert_ne!(files[0], files[1]);
    for file in &files {
        read_png(file).unwrap();
    }
}

/// When the books folder cannot take the file, the save goes to the data folder.
#[test]
fn falls_back_to_the_data_folder_when_books_folder_is_blocked() {
    let (mut app, clock, dir) = home("shot-fallback");
    // A file where the folder should be makes the books folder unusable.
    fs::create_dir_all(dir.join("Books")).unwrap();
    fs::write(books_folder(&dir), b"not a folder").unwrap();
    let folder = data_folder(&dir);

    let before = swipe_until_lift(&mut app, &clock, DIAGONAL_FROM, DIAGONAL_TO);
    lift(&mut app, &clock, DIAGONAL_TO);

    let files = wait_for_saves(&mut app, &clock, &folder, 1, &before);
    assert_saved_frame(&read_png(&files[0]).unwrap(), &before);
    assert_eq!(fs::read(books_folder(&dir)).unwrap(), b"not a folder");
}
