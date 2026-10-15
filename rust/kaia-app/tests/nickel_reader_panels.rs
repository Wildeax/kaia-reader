//! Reader panel geometry, draft settings, local search and fixed-page navigation.
mod common;
use common::*;
use kaia_app::surfaces::reader::{book_dialogs::ContentsList, page::PageView};
use kaia_app::{App, HeadlessBackend};
use kaia_events::{Event, Settings, Value};
use kaia_gfx::{golden::assert_matches_golden, Point};
use kaia_input::Key;
use kaia_ui::components::Label;
use kaia_ui::controls::TextField;

fn start(name: &str, tag: &str) -> App<HeadlessBackend> {
    let dir = scratch(tag);
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    struct Battery;
    impl kaia_platform::Battery for Battery {
        fn capacity(&self) -> Option<u8> {
            Some(86)
        }
        fn is_charging(&self) -> Option<bool> {
            Some(false)
        }
        fn is_charged(&self) -> Option<bool> {
            Some(false)
        }
    }
    app.services_mut().platform.battery = Box::new(Battery);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let id = book_id(&app, name);
    app.command(&format!("open_book:{id}"), &[]);
    wait_reader(&mut app);
    app
}
fn saved(app: &mut App<HeadlessBackend>, name: &str) -> Value {
    app.broadcast_app(Event::new("SaveState"));
    let book = app.services().library.book(book_id(app, name)).unwrap().unwrap();
    let path = app
        .services()
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{}.json", book.partial_md5));
    Settings::read_exact(&path)
        .unwrap()
        .unwrap()
        .read("layout")
        .clone()
}
fn clock(app: &mut App<HeadlessBackend>) {
    app.update_top::<Label, _>("overlay-clock", |label, cx| {
        label.set_text("09:41");
        cx.request_layout();
        cx.request_paint();
    });
    app.process(vec![]).unwrap();
}
fn capture(app: &mut App<HeadlessBackend>, name: &str) {
    clock(app);
    assert_matches_golden(app.frame(), &golden(name), 14);
}
fn overlay(app: &mut App<HeadlessBackend>) {
    app.command("reader:overlay", &[]);
    wait_reader(app);
    clock(app);
}
fn set_query(app: &mut App<HeadlessBackend>, query: &str) {
    assert!(app.update_top::<TextField, _>("search-field", |field, cx| {
        field.set_text(query);
        cx.request_paint();
    }));
}

#[test]
fn typography_drafts_cancel_restore_and_apply_without_changing_defaults() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = start("juliet.epub", "nickel-panels-font");
    let before = saved(&mut app, "juliet.epub");
    overlay(&mut app);
    assert!(app.find_rect("overlay-back").is_none());
    tap_key(&mut app, "overlay-layout");
    assert_eq!(app.find_rect("layout-font").unwrap().y, 239);
    assert!(app.find_rect("overlay-slider").is_none());
    capture(&mut app, "reader-nickel-font-panel.png");
    assert_eq!(saved(&mut app, "juliet.epub"), before);
    tap_key(&mut app, "layout-font");
    assert_eq!(app.find_rect("reader-layout-font-0").unwrap().y, 258);
    capture(&mut app, "reader-nickel-font-list.png");
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    tap_key(&mut app, "layout-advanced");
    tap_key(&mut app, "advanced-size-plus");
    tap_key(&mut app, "advanced-weight-plus");
    assert_eq!(saved(&mut app, "juliet.epub"), before, "preview does not persist");
    capture(&mut app, "reader-nickel-font-advanced.png");
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    assert_eq!(saved(&mut app, "juliet.epub"), before);
    tap_key(&mut app, "layout-advanced");
    tap_key(&mut app, "advanced-size-plus");
    tap_key(&mut app, "advanced-restore");
    tap_key(&mut app, "advanced-apply");
    assert_eq!(saved(&mut app, "juliet.epub"), before);
    tap_key(&mut app, "layout-advanced");
    tap_key(&mut app, "advanced-font");
    tap_key(&mut app, "reader-layout-font-2");
    assert_eq!(
        saved(&mut app, "juliet.epub"),
        before,
        "face also remains a draft"
    );
    tap_key(&mut app, "advanced-size-plus");
    tap_key(&mut app, "advanced-weight-plus");
    tap_key(&mut app, "advanced-apply");
    wait_reader(&mut app);
    let after = saved(&mut app, "juliet.epub");
    assert_eq!(after.get("font_family").as_str(), Some("Noto Sans"));
    assert_eq!(
        after.get("font_size").as_f64(),
        Some(before.get("font_size").as_f64().unwrap() + 1.0)
    );
    assert_eq!(after.get("font_weight").as_f64(), Some(0.5));
    tap_key(&mut app, "layout-margins-plus");
    let after = saved(&mut app, "juliet.epub");
    assert_eq!(after.get("margins").get("top"), before.get("margins").get("top"));
    tap_key(&mut app, "layout-alignment-1");
    assert_eq!(
        saved(&mut app, "juliet.epub").get("align_left").as_bool(),
        Some(true)
    );
    tap_key(&mut app, "layout-alignment-2");
    let l = saved(&mut app, "juliet.epub");
    assert_eq!(l.get("justify").as_bool(), Some(true));
    assert_eq!(l.get("align_left").as_bool(), Some(false));
    tap_key(&mut app, "layout-alignment-0");
    let l = saved(&mut app, "juliet.epub");
    assert_eq!(l.get("justify").as_bool(), Some(false));
    assert_eq!(l.get("align_left").as_bool(), Some(false));
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}
#[test]
fn contents_has_sixteen_nested_rows_and_combines_book_annotations() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = start("juliet.epub", "nickel-panels-contents");
    overlay(&mut app);
    tap_key(&mut app, "overlay-contents");
    let mut entries = vec![];
    assert!(app.update_top::<ContentsList, _>("contents-list", |list, _| entries = list.items().to_vec()));
    assert!(entries.len() > 16);
    for row in entries.iter().take(16) {
        assert!(app
            .find_rect(&kaia_app::surfaces::reader::panels::key_of(&row.action))
            .is_some());
    }
    assert!(app
        .find_rect(&kaia_app::surfaces::reader::panels::key_of(&entries[16].action))
        .is_none());
    capture(&mut app, "reader-nickel-contents.png");
    app.tap(Point::new(1167, 1510)).unwrap();
    wait_reader(&mut app);
    assert!(app
        .find_rect(&kaia_app::surfaces::reader::panels::key_of(&entries[16].action))
        .is_some());
    capture(&mut app, "reader-nickel-contents-page2.png");
    app.key(Key::RPgBack).unwrap();
    let mut page = usize::MAX;
    app.update_top::<ContentsList, _>("contents-list", |list, _| page = list.page());
    assert_eq!(page, 0);
    app.key(Key::RPgFwd).unwrap();
    app.update_top::<ContentsList, _>("contents-list", |list, _| page = list.page());
    assert_eq!(page, 1);
    tap_key(&mut app, "contents-tab-1");
    capture(&mut app, "reader-nickel-annotations-empty.png");
    app.command("reader:bookmark", &[]);
    wait_reader(&mut app);
    app.command("reader:contents:tab:1", &[]);
    wait_reader(&mut app);
    let mut count = 0;
    app.update_top::<ContentsList, _>("contents-list", |list, _| count = list.items().len());
    assert_eq!(count, 1);
    tap_key(&mut app, "annotations-filter");
    tap_key(&mut app, "annotations-filter-3");
    assert!(app.find_rect("contents-empty").is_some());
    tap_key(&mut app, "annotations-filter");
    tap_key(&mut app, "annotations-filter-0");
    capture(&mut app, "reader-nickel-annotations.png");
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}
#[test]
fn scoped_search_keyboard_and_more_routes() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = start("juliet.epub", "nickel-panels-search");
    overlay(&mut app);
    tap_key(&mut app, "overlay-search");
    capture(&mut app, "reader-nickel-search.png");
    assert_eq!(app.find_rect("search-keyboard").unwrap().y, 1040);
    // Tap a digit, move the caret, type an accented letter, then erase it.
    app.tap(Point::new(114, 1134)).unwrap();
    app.tap(Point::new(805, 1600)).unwrap();
    app.tap(Point::new(315, 1600)).unwrap();
    app.tap(Point::new(114, 1250)).unwrap();
    let mut query = String::new();
    app.update_top::<TextField, _>("search-field", |f, _| query = f.text().into());
    assert_eq!(query, "á1");
    app.tap(Point::new(1149, 1488)).unwrap();
    app.update_top::<TextField, _>("search-field", |f, _| query = f.text().into());
    assert_eq!(query, "1");
    tap_key(&mut app, "search-scope");
    capture(&mut app, "reader-nickel-search-scopes.png");
    tap_key(&mut app, "search-scope-1");
    set_query(&mut app, "missing annotation");
    tap_key(&mut app, "search-go");
    let mut status = String::new();
    app.update_top::<Label, _>("search-status", |l, _| status = l.text().into());
    assert_eq!(status, "No matches.");
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    tap_key(&mut app, "overlay-more");
    capture(&mut app, "reader-nickel-more.png");
    tap_key(&mut app, "more-details");
    assert!(matches!(
        app.top_route(),
        Some(kaia_app::Route::BookDetails { .. })
    ));
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    tap_key(&mut app, "overlay-more");
    tap_key(&mut app, "more-dictionary");
    set_query(&mut app, "reader");
    tap_key(&mut app, "search-go");
    assert!(app.find_rect("search-results").is_some());
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}
#[test]
fn pdf_zoom_menu_and_turns_keep_view() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = start("2col.pdf", "nickel-panels-pdf");
    app.command(
        "reader:zoom:pinch",
        &[
            Value::Number(3.0),
            Value::Number(350.0),
            Value::Number(500.0),
            Value::Number(350.0),
            Value::Number(500.0),
        ],
    );
    wait_reader(&mut app);
    app.command(
        "reader:zoom:drag",
        &[
            Value::Number(600.0),
            Value::Number(800.0),
            Value::Number(500.0),
            Value::Number(700.0),
            Value::Bool(false),
        ],
    );
    wait_reader(&mut app);
    let mut before = 0;
    app.update_top::<PageView, _>("reader-page", |p, _| before = p.position.unwrap().0);
    app.tap(Point::new(1200, 900)).unwrap();
    wait_reader(&mut app);
    let mut after = 0;
    app.update_top::<PageView, _>("reader-page", |p, _| after = p.position.unwrap().0);
    assert_eq!(after, before + 1);
    app.key(Key::RPgBack).unwrap();
    wait_reader(&mut app);
    app.update_top::<PageView, _>("reader-page", |p, _| after = p.position.unwrap().0);
    assert_eq!(after, before);
    overlay(&mut app);
    capture(&mut app, "reader-nickel-pdf-menu.png");
    assert!(app.find_rect("zoom-map").is_some());
    tap_key(&mut app, "overlay-zoom");
    capture(&mut app, "reader-nickel-pdf-zoom.png");
    assert!(app.find_rect("overlay-slider").is_none());
    tap_key(&mut app, "zoom-plus");
    let mut percent = 0.0;
    app.update_top::<kaia_app::slider::Slider, _>("zoom-slider", |s, _| percent = s.value());
    assert_eq!(percent, 310.0);
    tap_key(&mut app, "overlay-zoom");
    overlay(&mut app);
    tap_key(&mut app, "overlay-more");
    tap_key(&mut app, "more-tools");
    tap_key(&mut app, "more-fit-modes");
    assert!(app.find_rect("layout-zoom").is_some());
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}
