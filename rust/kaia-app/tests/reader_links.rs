mod common;
#[path = "../../kaia-doc/tests/support/links_fixture.rs"]
mod fixture;
use common::*;
use kaia_app::{surfaces::reader::page::PageView, App, HeadlessBackend, Route};
use kaia_book::{BookSession, Link, LinkTarget, Margins, OpenOptions, TextLayout};
use kaia_events::{Settings, Value};
use kaia_gfx::Point;
use kaia_ui::{composite::PagedList, controls::TextField};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn open(dir: &Path, pdf: bool) -> (App<HeadlessBackend>, BookSession, i64) {
    let books = dir.join("books");
    std::fs::create_dir_all(&books).unwrap();
    let name = if pdf { "links.pdf" } else { "links.epub" };
    let path = books.join(name);
    if pdf {
        fixture::pdf(&path);
    } else {
        fixture::epub(&path);
    }
    let mut app = make_app(dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let id = book_id(&app, name);
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    let mut opts = OpenOptions::new(dir.join("probe"));
    opts.viewport = app.screen();
    let px = |n: f64| (n * app.screen().w.min(app.screen().h) as f64 / 600.0).ceil() as f32;
    opts.default_layout = TextLayout {
        font_size: px(22.0),
        margins: Margins {
            top: px(15.0),
            right: px(10.0),
            bottom: px(15.0),
            left: px(10.0),
        },
        ..TextLayout::default()
    };
    opts.sidecars = vec![];
    let probe = BookSession::open(path, opts).unwrap();
    (app, probe, id)
}
fn position(app: &mut App<HeadlessBackend>) -> u32 {
    wait_reader(app);
    let mut page = None;
    app.update_top::<PageView, _>("reader-page", |v, _| page = v.position.map(|p| p.0));
    page.expect("reader page")
}
fn tap_link(app: &mut App<HeadlessBackend>, probe: &mut BookSession, link: &Link) {
    let viewport = app.screen();
    let t = probe.page_transform(viewport).unwrap();
    let r = t.to_screen(link.bbox);
    let p = Point::new(
        r.x + r.w as i32 / 2 + (viewport.w as i32 - t.image.w as i32) / 2,
        r.y + r.h as i32 / 2 + (viewport.h as i32 - t.image.h as i32) / 2,
    );
    let start = Instant::now();
    app.command("reader:tap", &[Value::from(p.x as i64), Value::from(p.y as i64)]);
    assert!(
        start.elapsed() < Duration::from_millis(100),
        "link dispatch blocked: {:?}",
        start.elapsed()
    );
    wait_reader(app);
}
fn sheet_text(app: &mut App<HeadlessBackend>) -> String {
    let mut text = String::new();
    app.update_top::<PagedList<String>, _>("link-text", |list, _| text = list.items().join(""));
    text
}
fn back(app: &mut App<HeadlessBackend>) {
    app.command("reader:overlay", &[]);
    tap_key(app, "overlay-back");
}
fn enter(app: &mut App<HeadlessBackend>, text: &str, percent: bool) {
    app.command("reader:goto:open", &[]);
    app.process(vec![]).unwrap();
    assert!(app.update_top::<TextField, _>("goto-field", |f, cx| {
        f.set_text(text);
        cx.request_paint();
    }));
    app.command(
        if percent {
            "reader:goto:percent"
        } else {
            "reader:goto:page"
        },
        &[],
    );
    wait_reader(app);
}
fn saved_status(app: &App<HeadlessBackend>, id: i64) -> String {
    let row = app.services().library.book(id).unwrap().unwrap();
    let path = app
        .services()
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{}.json", row.partial_md5));
    Settings::read_exact(&path)
        .unwrap()
        .unwrap()
        .read("summary")
        .get("status")
        .as_str()
        .unwrap_or("")
        .to_string()
}

#[test]
fn epub_taps_show_complete_notes_and_offline_addresses_then_jump_and_return() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("reader-links-epub");
    let (mut app, mut probe, _) = open(&dir, false);
    let links = probe.links().unwrap();
    let note = links
        .iter()
        .find(|l| {
            l.note
                .as_deref()
                .is_some_and(|n| n.contains("First note paragraph."))
        })
        .unwrap();
    tap_link(&mut app, &mut probe, note);
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel));
    kaia_gfx::png::write_pixmap(app.frame(), &dir.join("footnote.png")).unwrap();
    let text = sheet_text(&mut app);
    assert!(
        text.contains("First note paragraph.") && text.contains("Second note paragraph."),
        "{text}"
    );
    app.command("reader:panel:close", &[]);
    assert_eq!(position(&mut app), 0);
    app.command("reader:overlay", &[]);
    app.process(vec![]).unwrap();
    assert!(app.find_rect("overlay-back").is_none_or(|r| r.is_empty()));
    app.command("back", &[]);
    let external = links
        .iter()
        .find(|l| matches!(&l.target, LinkTarget::Uri(u) if u.starts_with("https:")))
        .unwrap();
    tap_link(&mut app, &mut probe, external);
    assert!(sheet_text(&mut app).contains("https://example.org/offline"));
    app.command("reader:panel:close", &[]);
    assert_eq!(position(&mut app), 0);
    let chapter = links
        .iter()
        .find(|l| matches!(l.target, LinkTarget::Page(_)) && l.note.is_none())
        .unwrap();
    tap_link(&mut app, &mut probe, chapter);
    assert!(position(&mut app) > 0);
    back(&mut app);
    assert_eq!(position(&mut app), 0);
}

#[test]
fn pdf_link_taps_account_for_centering_and_page_entry_shares_back() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("reader-links-pdf");
    let (mut app, mut probe, _) = open(&dir, true);
    let links = probe.links().unwrap();
    let internal = links.iter().find(|l| l.target == LinkTarget::Page(1)).unwrap();
    tap_link(&mut app, &mut probe, internal);
    assert_eq!(position(&mut app), 1);
    back(&mut app);
    assert_eq!(position(&mut app), 0);
    enter(&mut app, "2", false);
    assert_eq!(position(&mut app), 1);
    back(&mut app);
    assert_eq!(position(&mut app), 0);
    enter(&mut app, "99", false);
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel));
    app.command("reader:panel:close", &[]);
    assert_eq!(position(&mut app), 0);
}

#[test]
fn percent_entry_and_nested_jumps_return_in_order_and_back_updates_overlay() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("reader-links-percent");
    let (mut app, probe, _) = open(&dir, false);
    let count = probe.page_count();
    enter(&mut app, "50", true);
    let middle = position(&mut app);
    assert_eq!(middle, ((count as f64 / 2.0).round() as u32).saturating_sub(1));
    enter(&mut app, "100%", false);
    assert_eq!(position(&mut app), count as u32 - 1);
    back(&mut app);
    assert_eq!(position(&mut app), middle);
    back(&mut app);
    assert_eq!(position(&mut app), 0);
    app.command("reader:overlay", &[]);
    app.process(vec![]).unwrap();
    assert!(app.find_rect("overlay-back").is_none_or(|r| r.is_empty()));
    app.command("reader:panel:more", &[]);
    tap_key(&mut app, "more-end");
    assert!(
        app.find_rect("overlay-back").is_some_and(|r| !r.is_empty()),
        "Back appears on the existing overlay after a jump"
    );
    kaia_gfx::png::write_pixmap(app.frame(), &dir.join("back-overlay.png")).unwrap();
}

#[test]
fn finishing_requires_an_answer_and_survives_close_and_reopen() {
    let _lock = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("reader-end-answers");
    let (mut app, _probe, id) = open(&dir, true);
    let before = saved_status(&app, id);
    let status = app.services().library.book(id).unwrap().unwrap().status;
    app.command("reader:next", &[]);
    assert_eq!(position(&mut app), 1);
    assert_eq!(saved_status(&app, id), before);
    app.command("reader:next", &[]);
    app.process(vec![]).unwrap();
    kaia_gfx::png::write_pixmap(app.frame(), &dir.join("end.png")).unwrap();
    tap_key(&mut app, "end-keep");
    assert_eq!(position(&mut app), 1);
    assert_eq!(saved_status(&app, id), before);
    assert_eq!(app.services().library.book(id).unwrap().unwrap().status, status);
    app.command("reader:next", &[]);
    app.process(vec![]).unwrap();
    tap_key(&mut app, "end-library");
    assert!(matches!(app.top_route(), Some(Route::Library { .. })));
    assert_eq!(saved_status(&app, id), before);
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    app.command("reader:next", &[]);
    app.process(vec![]).unwrap();
    tap_key(&mut app, "end-finish");
    assert_eq!(saved_status(&app, id), "complete");
    assert_eq!(
        app.services().library.book(id).unwrap().unwrap().status,
        kaia_library::Status::Complete
    );
    app.command("nav:library", &[]);
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    app.command("reader:next", &[]);
    app.process(vec![]).unwrap();
    tap_key(&mut app, "end-keep");
    assert_eq!(
        saved_status(&app, id),
        "complete",
        "still reading does not undo a finished status"
    );
}
