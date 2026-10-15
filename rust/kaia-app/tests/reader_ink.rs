//! Pen input through the headless app, storage, redraw, and toolbar actions.
mod common;
use common::*;
use kaia_app::surfaces::reader::page::{PageView, PAGE_KEY};
use kaia_app::{App, HeadlessBackend, Route};
use kaia_display::InputEvent;
use kaia_events::RefreshMode;
use kaia_gfx::Point;
use kaia_input::{Key, PenEvent, Phase};
use kaia_storage::Store;
use std::path::Path;

fn open(dir: &Path, name: &str) -> App<HeadlessBackend> {
    let lib = library_with_books(dir);
    let mut app = make_app(dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    open_from_library(&mut app, name);
    wait_reader(&mut app);
    app
}
fn enable(app: &mut App<HeadlessBackend>) {
    app.tap(Point::new(600, 800)).unwrap();
    tap_key(app, "overlay-pen");
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));
    assert!(app.find_rect("ink-width").is_some());
}
fn pen(app: &mut App<HeadlessBackend>, phase: Phase, x: i32, y: i32, eraser: bool) {
    app.process(vec![InputEvent::Pen(PenEvent {
        phase,
        pos: Point::new(x, y),
        pressure: 700,
        eraser,
        button: false,
        time_us: 1,
    })])
    .unwrap();
}
fn stroke(app: &mut App<HeadlessBackend>, y: i32) {
    pen(app, Phase::Down, 320, y, false);
    pen(app, Phase::Move, 900, y + 60, false);
    pen(app, Phase::Up, 900, y + 60, false);
}
fn page(app: &mut App<HeadlessBackend>) -> u32 {
    let mut page = 0;
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| page = view.position.unwrap().0);
    page
}
fn count(dir: &Path) -> u64 {
    Store::open(&dir.join("data/kaia"))
        .unwrap()
        .ink_counts()
        .unwrap()
        .iter()
        .map(|(_, _, n)| n)
        .sum()
}

#[test]
fn ink_stores_redraws_erases_undoes_and_never_turns_the_page() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("ink-reader");
    let mut app = open(&dir, "2col.pdf");
    enable(&mut app);
    let before = page(&mut app);
    let skip = app.refresh_modes().len();
    let refresh_skip = app.backend().display().refreshes().len();
    let old_frame = app.frame().clone();
    pen(&mut app, Phase::Down, 320, 600, false);
    pen(&mut app, Phase::Move, 900, 660, false);
    assert!(app.refresh_modes()[skip..].iter().all(|m| *m == RefreshMode::A2));
    let refreshes = &app.backend().display().refreshes()[refresh_skip..];
    assert_eq!(refreshes.len(), 2);
    assert!(refreshes
        .iter()
        .all(|r| r.region.w < app.screen().w && r.region.h < 100 && !r.dither));
    assert_eq!(app.frame().get(50, 300), old_frame.get(50, 300));
    assert_eq!(count(&dir), 0, "disk write waits for lift");
    pen(&mut app, Phase::Up, 900, 660, false);
    assert_eq!(count(&dir), 1);
    assert_eq!(page(&mut app), before);
    let pixel = app.frame().get(600, 629);
    assert!(pixel.unwrap().luma() < 30);
    tap_key(&mut app, "ink-done");
    drop(app);
    let mut app = open(&dir, "2col.pdf");
    assert_eq!(app.frame().get(600, 629), pixel, "ink returns on reopen");
    enable(&mut app);
    // Crossing the middle of a long segment must erase it, even without a point there.
    pen(&mut app, Phase::Down, 600, 500, true);
    pen(&mut app, Phase::Move, 600, 800, true);
    pen(&mut app, Phase::Up, 600, 800, true);
    assert_eq!(count(&dir), 0);
    stroke(&mut app, 600);
    tap_key(&mut app, "ink-eraser");
    pen(&mut app, Phase::Down, 600, 500, false);
    pen(&mut app, Phase::Move, 600, 800, false);
    pen(&mut app, Phase::Up, 600, 800, false);
    assert_eq!(count(&dir), 0, "the toolbar eraser also removes whole strokes");
    tap_key(&mut app, "ink-eraser");
    stroke(&mut app, 600);
    stroke(&mut app, 800);
    assert_eq!(count(&dir), 2);
    tap_key(&mut app, "ink-undo");
    assert_eq!(count(&dir), 1);
    tap_key(&mut app, "ink-delete");
    assert_eq!(count(&dir), 1, "delete needs confirmation");
    app.key(Key::Back).unwrap();
    assert_eq!(count(&dir), 1);
    tap_key(&mut app, "ink-delete");
    // Dialog buttons use their action as their key.
    tap_key(&mut app, "dialog-button:reader:ink:delete-confirm");
    assert_eq!(count(&dir), 0);
    app.tap(Point::new(1100, 1400)).unwrap();
    wait_reader(&mut app);
    assert_eq!(page(&mut app), before + 1, "finger taps keep page turning");
}

#[test]
fn ink_text_anchor_export_and_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("ink-golden");
    let mut app = open(&dir, "juliet.epub");
    for _ in 0..8 {
        app.key(Key::RPgFwd).unwrap();
        wait_reader(&mut app);
    }
    enable(&mut app);
    tap_key(&mut app, "ink-width");
    stroke(&mut app, 400);
    tap_key(&mut app, "ink-color");
    pen(&mut app, Phase::Down, 250, 700, false);
    assert_eq!(
        app.frame().get(250, 700).unwrap().luma(),
        0,
        "colored ink has a black A2 preview"
    );
    for (x, y) in [
        (280, 640),
        (350, 620),
        (420, 660),
        (430, 720),
        (370, 760),
        (290, 750),
        (250, 700),
    ] {
        pen(&mut app, Phase::Move, x, y, false);
    }
    pen(&mut app, Phase::Up, 250, 700, false);
    assert!(
        app.backend().display().refreshes().last().unwrap().dither,
        "color ink settles with a color refresh"
    );
    let store = Store::open(&dir.join("data/kaia")).unwrap();
    let doc = &store.ink_counts().unwrap()[0].0;
    let ink = store.ink(&doc.partial_md5).unwrap();
    assert!(ink.iter().all(|s| s.locator.is_some()));
    assert_eq!(ink[1].color, "blue");
    let notes = kaia_app::surfaces::notes::data::load(&dir.join("data"), &Default::default());
    let exported = kaia_app::surfaces::notes::export::export(&dir.join("data"), &notes, 1).unwrap();
    assert!(std::fs::read_to_string(exported.markdown)
        .unwrap()
        .contains("Page 9: 2 ink strokes"));
    assert!(std::fs::read_to_string(exported.json)
        .unwrap()
        .contains("\"ink_counts\": [{\"page\": 9, \"count\": 2}]"));
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden("reader-ink.png"), 14);
    app.command("reader:layout:size", &[kaia_events::Value::Number(32.0)]);
    wait_reader(&mut app);
    // Resolve the stored xpointer after the layout, then visit its new page.
    app.command(
        "reader:location",
        &[kaia_events::Value::from(
            ink[0].locator.as_ref().unwrap().as_str(),
        )],
    );
    wait_reader(&mut app);
    let mut visible = 0;
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| {
        visible = view.ink.as_ref().unwrap().strokes.len()
    });
    assert_eq!(visible, 2, "text anchors follow the relayout");
    assert_eq!(
        store.ink(&doc.partial_md5).unwrap(),
        ink,
        "relayout does not rewrite ink"
    );
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    stroke(&mut app, 900);
    assert_eq!(count(&dir), 3);
    assert!(
        app.backend().display().refreshes().last().unwrap().dither,
        "the chosen color still settles after turning a page"
    );
}

#[test]
fn ink_maps_to_the_page_image_below_the_header() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let mut settings = kaia_events::Settings::open(&data.join("settings.json"));
    settings.save("reading.header_title", kaia_events::Value::Bool(true));
    settings.flush(&data.join("settings.json")).unwrap();
    let mut app = open(tmp.path(), "juliet.epub");
    let screen = app.screen();
    let mut areas = None;
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| {
        assert!(view.header_height > 0, "the header is on");
        let page = view.page_area(kaia_gfx::Rect::from_size(screen));
        areas = view
            .ink
            .as_ref()
            .map(|ink| (ink.bounds(), page, view.header_height));
    });
    let (ink, page, header) = areas.expect("the page has an ink layer");
    assert_eq!(ink, page, "ink maps to where the page image is drawn");
    assert!(ink.y >= header as i32, "ink starts below the header strip");
}
