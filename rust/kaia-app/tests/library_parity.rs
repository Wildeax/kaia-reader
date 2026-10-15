//! B-147 menus, dialog return paths, local state and scrolling.
mod common;
use common::*;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::Value;
use kaia_gfx::{golden::assert_matches_golden, Point, Size};
use kaia_input::Key;
use kaia_library::Status;
use kaia_ui::components::{Label, SizedBox};
use kaia_ui::{Tone, WidgetPod};
use std::cell::Cell;
use std::rc::Rc;

fn indexed(name: &str) -> App<HeadlessBackend> {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    tap_key(&mut app, "nav-library");
    app
}

fn details_golden(app: &App<HeadlessBackend>, id: i64, menu: bool, name: &str) {
    use kaia_app::surfaces::library::{data, views, widgets::Overlay};
    use kaia_gfx::Rect;
    let services = app.services();
    let mut book = library_db(app).book(id).unwrap().unwrap();
    // A portable path keeps the metadata capture independent of test scratch folders.
    book.path = "/books/reading/juliet.epub".into();
    let info = views::DetailsInfo {
        rating: data::rating(services, &book),
        metadata: true,
        statistics: false,
        favorite: false,
        tags: &[],
    };
    let details = views::details_panel(
        &services.theme,
        &book,
        data::cover(library_db(app), id, kaia_library::CoverSize::Large),
        "library:panel:close",
        None,
        info,
    );
    let base = WidgetPod::new(Overlay::new(
        None,
        details,
        Rect::new(12, 12, 608, 816),
        "library:popup:close",
    ));
    let root = if menu {
        WidgetPod::new(Overlay::new(
            Some(base),
            views::book_menu(&services.theme, id, true),
            Rect::new(228, 185, 230, 194),
            "library:popup:close",
        ))
    } else {
        base
    };
    let image = kaia_ui::harness::render(
        SizedBox::new(Some(root)),
        app.screen(),
        &services.theme,
        &services.assets,
    );
    assert_matches_golden(&image, &golden(name), 14);
}

#[test]
fn book_menu_and_details_keep_the_caller_and_metadata_tab() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = indexed("parity-dialog");
    let id = book_id(&app, "juliet.epub");
    tap_key(&mut app, &format!("book-menu-{id}"));
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_book_menu.png"), 14);
    tap_key(&mut app, &format!("library-details-{id}"));
    assert!(app.find_rect("book-details-dialog").is_some());
    let tab = app.find_rect("library-details-tab-metadata").unwrap();
    assert!(tab.w > 100 && tab.x + (tab.w as i32) < app.screen().w as i32);
    tap_key(&mut app, "library-details-tab-metadata");
    details_golden(&app, id, false, "library_metadata.png");
    tap_key(&mut app, &format!("details-menu-{id}"));
    details_golden(&app, id, true, "library_details_menu.png");
    tap_key(&mut app, &format!("library-book-finished-{id}"));
    assert_eq!(
        library_db(&app).book(id).unwrap().unwrap().status,
        Status::Complete
    );
    assert!(app.find_rect("book-details-dialog").is_some());
    tap_key(&mut app, &format!("details-menu-{id}"));
    tap_key(&mut app, &format!("library-book-unread-{id}"));
    assert_eq!(library_db(&app).book(id).unwrap().unwrap().status, Status::New);
    assert!(
        app.find_rect("library-rating-open-1").is_some()
            || app.find_rect(&format!("library-rating-open-{id}")).is_some()
    );
    app.key(Key::Back).unwrap();
    assert!(app.find_rect("book-details-dialog").is_none());
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert!(app.unknown_commands().is_empty());
}

#[test]
fn menu_dismissal_blocks_navigation_and_sort_uses_real_metadata() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = indexed("parity-menu");
    tap_key(&mut app, "library-sort");
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_sort.png"), 14);
    let home = centre_of(&app, "nav-home");
    app.tap(home).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert!(app.find_rect("sort-size").is_none());
    tap_key(&mut app, "library-sort");
    tap_key(&mut app, "sort-size");
    let prefs = kaia_app::surfaces::library::data::Prefs::load(&app.services().settings);
    let books = kaia_app::surfaces::library::data::list_in(app.services(), &prefs);
    assert!(books.windows(2).all(|pair| pair[0].size >= pair[1].size));
    tap_key(&mut app, "library-options");
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_options.png"), 14);
    tap_key(&mut app, "library-view-list");
    assert!(app.find_rect("list").is_some());
    tap_key(&mut app, "library-view-series");
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_series_empty.png"), 14);
    tap_key(&mut app, "library-view-collections");
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_collections_empty.png"), 14);
    tap_key(&mut app, "library-collections-new");
    assert!(app.find_rect("name-field").is_some());
}

#[test]
fn status_panels_read_live_light_state_and_reuse_quick_actions() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = make_app(&scratch("parity-light"), Vec::new());
    app.services_mut().platform.frontlight.set_intensity(26).unwrap();
    app.services_mut().platform.frontlight.set_warmth(80).unwrap();
    app.start().unwrap();
    freeze_status_clock(&mut app);
    tap_key(&mut app, "status-light");
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("status_light.png"), 14);
    tap_key(&mut app, "light-auto");
    assert_eq!(
        app.services()
            .settings
            .read("display.natural_light_enabled")
            .as_bool(),
        Some(true)
    );
    app.command("settings:quick:frontlight", &[Value::Number(43.0)]);
    assert_eq!(app.services().platform.frontlight.intensity(), 43);
    app.key(Key::Back).unwrap();
    tap_key(&mut app, "status-wifi");
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("status_wifi.png"), 14);
    tap_key(&mut app, "wifi-settings");
    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Home));
    assert!(app.find_rect("wifi-popup").is_none());
}

#[test]
fn scrolling_keeps_position_and_builds_only_visible_rows() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let app = make_app(&scratch("parity-scroll"), Vec::new());
    let services = app.services();
    let position = Rc::new(Cell::new(0));
    let builds = Rc::new(Cell::new(0));
    let calls = builds.clone();
    let list = kaia_app::surfaces::library::widgets::Scroll::new(
        (0..100).collect(),
        4,
        120,
        move |n: &i32, t: &kaia_ui::Theme| {
            calls.set(calls.get() + 1);
            WidgetPod::new(SizedBox::new(Some(WidgetPod::new(Label::new(
                n.to_string(),
                t.font.body.clone(),
                Tone::Ink,
            )))))
            .with_key(format!("row-{n}"))
        },
    )
    .position(position.clone());
    let mut harness =
        kaia_ui::harness::Harness::new(list, Size::new(1000, 800), &services.theme, &services.assets);
    harness.frame();
    assert!(builds.get() < 30);
    harness.swipe(Point::new(500, 650), Point::new(500, 100));
    harness.frame();
    assert!(position.get() > 0);
    assert!(harness.window().find_rect("row-0").is_none());
    let before = position.get();
    let list = kaia_app::surfaces::library::widgets::Scroll::new(
        (0..100).collect(),
        4,
        120,
        |n: &i32, t: &kaia_ui::Theme| {
            WidgetPod::new(Label::new(n.to_string(), t.font.body.clone(), Tone::Ink))
                .with_key(format!("row-{n}"))
        },
    )
    .position(position.clone());
    let mut restored =
        kaia_ui::harness::Harness::new(list, Size::new(1000, 800), &services.theme, &services.assets);
    restored.frame();
    assert_eq!(position.get(), before);
    assert!(restored.window().find_rect("row-0").is_none());
}

#[test]
fn twelve_valid_books_fill_four_columns_and_scroll_without_a_pager() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("parity-twelve");
    let lib = library_with_books(&dir);
    for copy in 1..4 {
        for name in TEST_BOOKS {
            std::fs::copy(lib.join(name), lib.join(format!("{copy}-{name}"))).unwrap();
        }
    }
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    tap_key(&mut app, "nav-library");
    let prefs = kaia_app::surfaces::library::data::Prefs::load(&app.services().settings);
    let books = kaia_app::surfaces::library::data::list_in(app.services(), &prefs);
    assert_eq!(books.len(), 12);
    let slots: Vec<_> = books[..4]
        .iter()
        .map(|book| app.find_rect(&format!("book-{}", book.id)).unwrap())
        .collect();
    assert!(slots
        .windows(2)
        .all(|pair| pair[0].y == pair[1].y && pair[0].x < pair[1].x));
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_four_columns.png"), 14);
    for (index, (phase, y)) in [
        (kaia_input::Phase::Down, 1250),
        (kaia_input::Phase::Move, 800),
        (kaia_input::Phase::Up, 450),
    ]
    .into_iter()
    .enumerate()
    {
        app.process(vec![kaia_display::InputEvent::Touch(kaia_input::TouchEvent {
            slot: 0,
            phase,
            pos: Point::new(1150, y),
            time_us: 1_000_000_000_000 + index as u64 * 50_000,
        })])
        .unwrap();
    }
    let mut offset = 0;
    assert!(
        app.update_top::<kaia_app::surfaces::library::widgets::Scroll<kaia_ui::composite::BookInfo>, _>(
            "book-scroll",
            |list, _| offset = list.offset()
        )
    );
    assert!(offset > 0);
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_scrolled.png"), 14);
    tap_key(&mut app, "library-options");
    tap_key(&mut app, "library-view-list");
    freeze_status_clock(&mut app);
    assert_matches_golden(app.frame(), &golden("library_five_rows.png"), 14);
}
