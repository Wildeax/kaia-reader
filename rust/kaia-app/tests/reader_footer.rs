//! The footer: the Footer sheet, opened from More, turns items on and off. The
//! choices reach settings.json, and the footer line on the bars follows them.

use std::path::Path;
use std::time::Duration;

use kaia_app::surfaces::reader::page::{PageView, PAGE_KEY};
use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::{Settings, Value};
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Point;
use kaia_input::Key;
use kaia_ui::components::Label;

mod common;

use common::*;

/// Opens the reader on `name` and waits for the library scan.
fn open_reader(name: &str, dir: &Path) -> App<HeadlessBackend> {
    let lib = library_with_books(dir);
    let mut app = make_app(dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    open_from_library(&mut app, name);
    app
}

/// Turns to a page of juliet with text. Page 0 is the cover.
fn turn_to_text(app: &mut App<HeadlessBackend>) {
    for _ in 0..8 {
        app.key(Key::RPgFwd).unwrap();
        wait_reader(app);
    }
}

fn open_overlay(app: &mut App<HeadlessBackend>) {
    let p = Point::new(app.screen().w as i32 / 2, app.screen().h as i32 / 2);
    app.tap(p).unwrap();
    wait_reader(app);
    assert_eq!(app.top_route(), Some(&Route::Overlay));
    app.update_top::<kaia_ui::components::Label, _>("overlay-clock", |text, cx| {
        text.set_text("09:41");
        cx.request_layout();
        cx.request_paint();
    });
    app.iterate(None).unwrap();
}

/// The footer line on the bars. The overlay must be the top window.
fn footer_line(app: &mut App<HeadlessBackend>) -> String {
    let mut text = None;
    app.update_top::<Label, _>("overlay-footer", |label, _| text = Some(label.text().to_string()));
    text.expect("the overlay shows the footer line")
}

/// Closes panels until the overlay is the top window again.
fn close_panels(app: &mut App<HeadlessBackend>) {
    while app.top_route() == Some(&Route::ReaderPanel) {
        tap_key(app, "panel-close");
    }
}

/// The settings file on disk, as the reader saved it.
fn saved_settings(dir: &Path) -> Settings {
    Settings::read_exact(&dir.join("data/settings.json"))
        .unwrap()
        .expect("settings.json")
}

#[test]
fn the_footer_sheet_turns_items_on_and_the_line_follows() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("footer-sheet");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    open_overlay(&mut app);
    let default_line = footer_line(&mut app);
    assert!(
        default_line.starts_with("Page "),
        "the default line: {default_line:?}"
    );
    assert!(
        !default_line.contains(" · "),
        "only the page count is on by default: {default_line:?}"
    );

    // More, then Footer. The sheet stays open while items change.
    tap_key(&mut app, "overlay-more");
    tap_key(&mut app, "more-tools");
    tap_key(&mut app, "more-footer");
    assert_eq!(
        app.top_route(),
        Some(&Route::ReaderPanel),
        "the Footer sheet is up"
    );
    tap_key(&mut app, "footer-time_chapter");
    tap_key(&mut app, "footer-percent");
    assert_eq!(
        app.services()
            .settings
            .read("reading.footer_time_chapter")
            .as_bool(),
        Some(true),
        "the choice is kept in memory"
    );
    let saved = saved_settings(&dir);
    assert_eq!(
        saved.read("reading.footer_time_chapter").as_bool(),
        Some(true),
        "and saved"
    );
    assert_eq!(saved.read("reading.footer_percent").as_bool(), Some(true));

    // Turning an item off again removes it from the line.
    tap_key(&mut app, "footer-percent");
    assert_eq!(
        saved_settings(&dir).read("reading.footer_percent").as_bool(),
        Some(false)
    );

    close_panels(&mut app);
    assert_eq!(app.top_route(), Some(&Route::Overlay), "back on the bars");
    assert_eq!(
        footer_line(&mut app),
        default_line,
        "the menu keeps one position label"
    );
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    let line = page_state(&mut app).0;
    assert!(
        line.starts_with("Romeo and Juliet · Page "),
        "the page number follows the title: {line:?}"
    );
    assert!(line.contains(" of "), "page of total: {line:?}");
    assert!(line.contains(" | "), "two items are joined: {line:?}");
    assert!(line.contains("left in"), "the time is still on: {line:?}");
    assert!(!line.contains('%'), "the percent is off: {line:?}");
}

#[test]
fn the_clock_shows_the_local_time_when_the_bars_open() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("footer-clock");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);

    open_overlay(&mut app);
    tap_key(&mut app, "overlay-more");
    tap_key(&mut app, "more-tools");
    tap_key(&mut app, "more-footer");
    tap_key(&mut app, "footer-clock");
    close_panels(&mut app);

    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    let line = page_state(&mut app).0;
    let clock = line
        .split(" | ")
        .find(|part| part.len() == 5 && part.as_bytes()[2] == b':')
        .unwrap_or_else(|| panic!("a HH:MM part in {line:?}"));
    assert!(clock[..2].parse::<u32>().is_ok_and(|h| h < 24), "hours: {clock}");
    assert!(
        clock[3..].parse::<u32>().is_ok_and(|m| m < 60),
        "minutes: {clock}"
    );
}

fn set_item(app: &mut App<HeadlessBackend>, item: &str, on: bool) {
    app.command(&format!("reader:footer:{item}"), &[Value::Bool(on)]);
    wait_reader(app);
}

fn page_state(app: &mut App<HeadlessBackend>) -> (String, u32, std::sync::Arc<kaia_gfx::Pixmap>) {
    let mut state = None;
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| {
        state = Some((
            view.footer.clone(),
            view.footer_height,
            view.page.clone().unwrap(),
        ));
    });
    state.expect("the reading page")
}

#[test]
fn the_persistent_footer_reserves_space_and_updates_after_a_page_turn() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("persistent-footer");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    let (before, strip, page) = page_state(&mut app);
    assert_eq!(strip, 120, "the capture centres the footer in a 120 px strip");
    assert_eq!(page.height(), app.screen().h - strip - header_state(&mut app).1);
    assert_eq!(before, "Romeo and Juliet · Page 3 of 96");
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    let (after, next_strip, _) = page_state(&mut app);
    assert_eq!(
        after, before,
        "a screen turn within a logical block keeps its label"
    );
    app.command("reader:location", &[Value::Number(11.0)]);
    wait_reader(&mut app);
    assert_eq!(page_state(&mut app).0, "Romeo and Juliet · Page 4 of 96");
    assert_eq!(strip, next_strip);
}

#[test]
fn changing_footer_items_keeps_the_layout_and_all_off_keeps_the_header() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("footer-layout-toggle");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    let (_, strip, page) = page_state(&mut app);
    set_item(&mut app, "percent", true);
    let (text, same_strip, same_page) = page_state(&mut app);
    assert!(text.contains(" | "), "{text}");
    assert_eq!(same_strip, strip);
    assert!(
        std::sync::Arc::ptr_eq(&page, &same_page),
        "an item change reuses the rendered page"
    );
    set_item(&mut app, "page", false);
    set_item(&mut app, "percent", false);
    let (text, no_strip, full_page) = page_state(&mut app);
    assert_eq!(no_strip, 0);
    assert!(text.is_empty());
    assert_eq!(full_page.height(), app.screen().h - header_state(&mut app).1);
    set_item(&mut app, "page", true);
    let (_, restored_strip, reduced_page) = page_state(&mut app);
    assert_eq!(restored_strip, strip);
    assert_eq!(
        reduced_page.height(),
        app.screen().h - strip - header_state(&mut app).1
    );
}

#[test]
fn fixed_pages_fit_above_the_footer() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("footer-pdf");
    let mut app = open_reader("2col.pdf", &dir);
    let (_, strip, page) = page_state(&mut app);
    assert!(strip > 0);
    assert!(page.height() <= app.screen().h - strip);
    assert!(page.width() <= app.screen().w);
    set_item(&mut app, "page", false);
    let (text, no_strip, full_page) = page_state(&mut app);
    assert_eq!(no_strip, 0);
    assert!(text.is_empty());
    assert!(full_page.height() > page.height());
}

#[test]
fn persistent_footer_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-persistent-footer");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    set_item(&mut app, "time_chapter", true);
    let path = golden("reader-footer.png");
    // Keep the first render available for inspection before accepting a new golden.
    if !path.exists() {
        kaia_gfx::png::write_pixmap(app.frame(), &golden("reader-footer.actual.png")).unwrap();
    }
    assert_matches_golden(app.frame(), &path, 14);
}

#[test]
fn header_and_footer_clock_and_battery_follow_a_page_redraw_without_a_page_turn() {
    struct Battery(std::rc::Rc<std::cell::Cell<u8>>);
    impl kaia_platform::Battery for Battery {
        fn capacity(&self) -> Option<u8> {
            Some(self.0.get())
        }
        fn is_charging(&self) -> Option<bool> {
            Some(false)
        }
        fn is_charged(&self) -> Option<bool> {
            Some(false)
        }
    }
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("footer-status-redraw");
    let mut app = open_reader("juliet.epub", &dir);
    let level = std::rc::Rc::new(std::cell::Cell::new(64));
    app.services_mut().platform.battery = Box::new(Battery(level.clone()));
    set_item(&mut app, "clock", true);
    set_item(&mut app, "battery", true);
    set_header(&mut app, "clock", true);
    set_header(&mut app, "battery", true);
    let (before, _, page) = page_state(&mut app);
    assert!(before.contains("Battery 64%"), "{before}");
    assert!(header_state(&mut app).0 .1.contains("Battery 64%"));
    level.set(63);
    app.update_top::<PageView, _>(PAGE_KEY, |view, cx| {
        view.footer.clear();
        view.header.1.clear();
        cx.request_paint();
    });
    let clock_before = chrono::Local::now().format("%H:%M").to_string();
    app.process(Vec::new()).unwrap();
    let clock_after = chrono::Local::now().format("%H:%M").to_string();
    let (after, _, same_page) = page_state(&mut app);
    assert!(after.contains("Battery 63%"), "{after}");
    assert!(
        after.contains(&clock_before) || after.contains(&clock_after),
        "{after}"
    );
    let header = header_state(&mut app).0 .1;
    assert!(header.contains("Battery 63%"), "{header}");
    assert!(
        header.contains(&clock_before) || header.contains(&clock_after),
        "{header}"
    );
    assert!(std::sync::Arc::ptr_eq(&page, &same_page));
    assert!(!app.reader_busy());
}

fn set_header(app: &mut App<HeadlessBackend>, item: &str, on: bool) {
    app.command(&format!("reader:header:{item}"), &[Value::Bool(on)]);
    wait_reader(app);
}

fn header_state(app: &mut App<HeadlessBackend>) -> ((String, String), u32) {
    let mut state = None;
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| {
        state = Some((view.header.clone(), view.header_height));
    });
    state.expect("the reading page")
}

#[test]
fn header_and_footer_reserve_both_strips_and_restore_height() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("header-height");
    let mut app = open_reader("juliet.epub", &dir);
    assert!(header_state(&mut app).1 > 0, "the chapter header defaults on");
    set_header(&mut app, "chapter", false);
    assert_eq!(header_state(&mut app), ((String::new(), String::new()), 0));
    let (_, footer, _) = page_state(&mut app);
    set_header(&mut app, "title", true);
    let ((title, _), header) = header_state(&mut app);
    assert!(!title.is_empty());
    assert_eq!(header, footer);
    let (_, _, page) = page_state(&mut app);
    assert_eq!(page.height(), app.screen().h - header - footer);
    set_header(&mut app, "chapter", true);
    let (_, _, same_page) = page_state(&mut app);
    assert!(std::sync::Arc::ptr_eq(&page, &same_page));
    set_header(&mut app, "title", false);
    assert_eq!(header_state(&mut app).1, header);
    set_header(&mut app, "chapter", false);
    assert_eq!(header_state(&mut app).1, 0);
    assert_eq!(page_state(&mut app).2.height(), app.screen().h - footer);
    set_item(&mut app, "page", false);
    assert_eq!(page_state(&mut app).2.height(), app.screen().h);
    set_header(&mut app, "title", true);
    assert_eq!(page_state(&mut app).2.height(), app.screen().h - header);
}

#[test]
fn header_chapter_changes_at_the_toc_boundary() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("header-chapters");
    let mut app = open_reader("juliet.epub", &dir);
    set_header(&mut app, "chapter", true);
    app.command("reader:panel:contents", &[]);
    wait_reader(&mut app);
    let mut chapters = Vec::new();
    app.update_top::<kaia_app::surfaces::reader::book_dialogs::ContentsList, _>(
        "contents-list",
        |list, _| {
            chapters = list
                .items()
                .iter()
                .map(|item| {
                    let page = item.detail.strip_prefix("Page ").unwrap().parse::<u32>().unwrap() - 1;
                    (item.label.trim().to_string(), page)
                })
                .collect();
        },
    );
    tap_key(&mut app, "panel-close");
    let boundary = chapters
        .windows(2)
        .find(|pair| pair[1].1 > pair[0].1 + 1 && pair[1].0 != pair[0].0)
        .unwrap();
    let (before, _) = &boundary[0];
    let (after, page) = &boundary[1];
    app.command("reader:location", &[Value::Number(f64::from(page - 1))]);
    wait_reader(&mut app);
    assert_eq!(before, "Title");
    assert_eq!(after, "About");
    assert_eq!(header_state(&mut app).0 .1, format!("{before} · 1 of 1"));
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    assert_eq!(header_state(&mut app).0 .1, format!("{after} · 1 of 2"));
}

#[test]
fn header_switches_save_and_reopen_with_both_groups_visible() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("header-sheet");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-more");
    tap_key(&mut app, "more-tools");
    tap_key(&mut app, "more-footer");
    for key in [
        "header-title",
        "header-chapter",
        "header-clock",
        "header-battery",
        "footer-battery",
    ] {
        let rect = app.find_rect(key).unwrap();
        assert!(
            rect.y >= 0 && rect.y + rect.h as i32 <= app.screen().h as i32,
            "{key}: {rect:?}"
        );
    }
    for item in ["title", "chapter", "clock", "battery"] {
        tap_key(&mut app, &format!("header-{item}"));
        if item == "chapter" {
            assert_eq!(
                saved_settings(&dir).read("reading.header_chapter").as_bool(),
                Some(false)
            );
            tap_key(&mut app, "header-chapter");
        }
        assert_eq!(
            saved_settings(&dir)
                .read(format!("reading.header_{item}"))
                .as_bool(),
            Some(true)
        );
    }
    close_panels(&mut app);
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    let ((title, status), height) = header_state(&mut app);
    assert!(!title.is_empty() && height > 0);
    assert!(status.contains(':'));
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    open_from_library(&mut app, "juliet.epub");
    assert_eq!(header_state(&mut app).1, height);
    assert_eq!(header_state(&mut app).0 .0, title);
}

#[test]
fn header_fixed_pages_fit_between_both_strips() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("header-pdf");
    let mut app = open_reader("2col.pdf", &dir);
    set_header(&mut app, "chapter", false);
    let (_, footer, before) = page_state(&mut app);
    set_header(&mut app, "title", true);
    let header = header_state(&mut app).1;
    let (_, _, page) = page_state(&mut app);
    assert!(page.height() <= app.screen().h - header - footer);
    assert!(page.width() <= app.screen().w);
    assert!(page.height() < before.height());
    set_header(&mut app, "title", false);
    assert_eq!(page_state(&mut app).2.size(), before.size());
}

#[test]
fn header_and_footer_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-header-footer");
    let mut app = open_reader("juliet.epub", &dir);
    set_header(&mut app, "title", true);
    set_header(&mut app, "chapter", true);
    set_item(&mut app, "time_chapter", true);
    turn_to_text(&mut app);
    let path = golden("reader-header-footer.png");
    if !path.exists() {
        kaia_gfx::png::write_pixmap(app.frame(), &golden("reader-header-footer.actual.png")).unwrap();
    }
    assert_matches_golden(app.frame(), &path, 14);
}

#[test]
fn header_and_footer_sheet_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-header-footer-sheet");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    app.command("reader:panel:footer", &[]);
    wait_reader(&mut app);
    let path = golden("reader-header-footer-sheet.png");
    if !path.exists() {
        kaia_gfx::png::write_pixmap(app.frame(), &golden("reader-header-footer-sheet.actual.png")).unwrap();
    }
    assert_matches_golden(app.frame(), &path, 14);
}

#[test]
fn header_keeps_selection_and_highlight_taps_on_the_text() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("header-selection");
    let mut app = open_reader("juliet.epub", &dir);
    set_header(&mut app, "title", true);
    turn_to_text(&mut app);
    let header = header_state(&mut app).1;
    let point = Point::new(200, 120 + header as i32);
    app.command(
        "reader:hold",
        &[
            Value::Number(f64::from(point.x)),
            Value::Number(f64::from(point.y)),
        ],
    );
    wait_reader(&mut app);
    let mut selected = Vec::new();
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| selected = view.selection.clone());
    assert!(selected.iter().any(|r| r.contains(point)), "{selected:?}");
    app.command("reader:release", &[]);
    wait_reader(&mut app);
    tap_key(&mut app, "color-yellow");
    let mut marks = Vec::new();
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| {
        marks = view.marks.iter().map(|m| m.rect).collect()
    });
    assert_eq!(marks, selected);
    app.tap(point).unwrap();
    wait_reader(&mut app);
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel));
    assert!(app.find_rect("style-underscore").is_some());
}
