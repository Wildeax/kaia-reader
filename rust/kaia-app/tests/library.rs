//! Headless tests for the Library surface: its views, sort, filter, search,
//! the book details panel, collections, delete, and the goldens.

use std::fs;
use std::time::{Duration, Instant};

use kaia_app::surfaces::settings::keys;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_display::InputEvent;
use kaia_events::RefreshMode;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Rect;
use kaia_input::{Key, Phase, TouchEvent};
use kaia_library::Status;
use kaia_ui::theme::LARGE_PRINT_SCALE;

mod common;

use common::*;

/// The key a button with `action` has in the tree: `library:sort` is `library-sort`.
fn key(action: &str) -> String {
    action.replace(':', "-")
}

/// Types `text` into the focused field, one key at a time.
fn type_text(app: &mut App<HeadlessBackend>, text: &str) {
    for c in text.chars() {
        app.key(Key::Char(c)).unwrap();
    }
}

/// The top-left corner of a widget as (y, x), for ordering rows and tiles.
fn position(app: &App<HeadlessBackend>, key: &str) -> (i32, i32) {
    let r: Rect = app
        .find_rect(key)
        .unwrap_or_else(|| panic!("no widget {key:?} on the top window"));
    (r.y, r.x)
}

/// Holds a finger on `key` until `done` holds, then lifts it.
fn hold_key(app: &mut App<HeadlessBackend>, key: &str, done: impl Fn(&App<HeadlessBackend>) -> bool) {
    let p = centre_of(app, key);
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Down,
        pos: p,
        time_us: 0,
    })])
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(app) {
        assert!(
            Instant::now() < deadline,
            "the hold on {key:?} never showed its result"
        );
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: p,
        time_us: 0,
    })])
    .unwrap();
}

/// Taps the search action at the trailing edge of the header.
fn tap_search_icon(app: &mut App<HeadlessBackend>) {
    let r = app.find_rect("library-header").expect("the header");
    let touch = app.services().theme.touch_target as i32;
    let p = kaia_gfx::Point::new(r.x + r.w as i32 - touch / 2, r.y + r.h as i32 / 2);
    app.tap(p).unwrap();
}

/// Opens the Library from Home, after the scan.
fn open_library(app: &mut App<HeadlessBackend>) {
    tap_key(app, "library");
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
}

/// Books of the test library by file name.
fn ids(app: &App<HeadlessBackend>) -> (i64, i64, i64) {
    (
        book_id(app, "juliet.epub"),
        book_id(app, "leaves.epub"),
        book_id(app, "2col.pdf"),
    )
}

/// A library app with the three test books indexed and Home on screen.
fn indexed(name: &str) -> (App<HeadlessBackend>, std::path::PathBuf) {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    (app, dir)
}

/// Reopens the app on the same data folder with no scan, so Home and Library
/// are built from the index as it is now.
fn reopen(dir: &std::path::Path) -> App<HeadlessBackend> {
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app
}

#[test]
fn tapping_library_shows_the_covers() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("tap-library");
    let (juliet, leaves, pdf) = ids(&app);
    let before = app.refresh_modes().len();

    open_library(&mut app);

    assert!(
        app.find_rect("library-view-list").is_some(),
        "Library shows the view tabs"
    );
    for id in [juliet, leaves, pdf] {
        assert!(
            app.find_rect(&format!("book-{id}")).is_some(),
            "book {id} is a tile"
        );
    }
    assert!(app.find_rect("library").is_none(), "Home is gone");
    // The root and each decoded cover use Ui.
    let modes = modes_after(&app, before);
    assert!(modes.len() >= 2);
    assert!(modes.iter().all(|mode| *mode == RefreshMode::Ui));
    assert_eq!(app.router().routes().len(), 1, "the root is replaced, not pushed");
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
}

#[test]
fn views_switch_between_covers_list_folders_and_collections() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("views");
    let (juliet, leaves, _) = ids(&app);
    app.services_mut()
        .library
        .add_to_collection(leaves, "Favourites")
        .unwrap();
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);

    tap_key(&mut app, &key("library:view:list"));
    assert!(app.find_rect("list").is_some(), "the list view is a paged list");
    assert!(app.find_rect(&format!("book-{juliet}")).is_some());

    tap_key(&mut app, &key("library:view:folders"));
    assert!(
        app.find_rect("folder-list").is_some(),
        "the folder view lists the folder"
    );
    assert!(
        app.find_rect(&format!("book-{leaves}")).is_some(),
        "books sit in their folder"
    );

    tap_key(&mut app, &key("library:view:collections"));
    assert!(
        app.find_rect(&format!("collection-{}", collection_id(&app, "Favourites")))
            .is_some(),
        "the collections tab lists the collection"
    );
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
}

/// The id of the collection named `name`.
fn collection_id(app: &App<HeadlessBackend>, name: &str) -> i64 {
    app.services()
        .library
        .collections()
        .unwrap()
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or_else(|| panic!("no collection {name}"))
        .id
}

#[test]
fn sort_by_progress_puts_the_most_read_book_first() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("sort");
    let (_, leaves, _) = ids(&app);
    app.services_mut().library.record_open(leaves, 0.9).unwrap();
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);
    let leaves_key = format!("book-{leaves}");
    let (juliet, _, pdf) = ids(&app);
    let others: Vec<String> = [juliet, pdf].iter().map(|id| format!("book-{id}")).collect();

    tap_key(&mut app, "library-sort");
    assert!(
        app.find_rect("sort-progress").is_some(),
        "the sort panel lists the keys"
    );
    tap_key(&mut app, "sort-progress");

    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert!(
        app.find_rect("sort-progress").is_none(),
        "the panel closes after a choice"
    );
    for other in &others {
        assert!(
            position(&app, &leaves_key) < position(&app, other),
            "{leaves_key} comes before {other} when sorted by progress"
        );
    }
}

#[test]
fn the_reading_filter_keeps_only_books_in_progress() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("chip");
    let (juliet, leaves, pdf) = ids(&app);
    app.services_mut().library.record_open(juliet, 0.5).unwrap();
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);

    tap_key(&mut app, "library-filter");
    assert!(
        app.find_rect("library-filter-reading").is_some(),
        "the filter panel lists the statuses"
    );
    tap_key(&mut app, "library-filter-reading");

    assert!(app.find_rect(&format!("book-{juliet}")).is_some());
    assert!(app.find_rect(&format!("book-{leaves}")).is_none());
    assert!(app.find_rect(&format!("book-{pdf}")).is_none());

    // The filter panel's All books shows every book again.
    tap_key(&mut app, "library-filter");
    tap_key(&mut app, "library-filter-all");
    assert!(app.find_rect(&format!("book-{leaves}")).is_some());
}

#[test]
fn search_narrows_the_results_after_each_key() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("search");
    let (juliet, leaves, _) = ids(&app);
    open_library(&mut app);

    tap_search_icon(&mut app);
    assert!(
        app.find_rect("search-field").is_some(),
        "the search page has a field"
    );
    tap_key(&mut app, "search-field");
    type_text(&mut app, "leaves");

    let leaves_key = format!("book-{leaves}");
    let juliet_key = format!("book-{juliet}");
    wait_for(&mut app, "the search results", |a| {
        a.find_rect(&leaves_key).is_some()
    });
    assert!(
        app.find_rect(&juliet_key).is_none(),
        "Romeo and Juliet does not match"
    );
    assert!(
        app.find_rect("search-field").is_some(),
        "the field stays on screen"
    );
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
}

#[test]
fn book_details_show_the_book_and_mark_it_finished() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("details");
    let (_, leaves, _) = ids(&app);
    open_library(&mut app);

    hold_key(&mut app, &format!("book-{leaves}"), |a| {
        a.find_rect(&key("library:panel:close")).is_some()
    });
    assert!(
        app.find_rect(&key(&format!("library:open:{leaves}"))).is_some(),
        "the details offer Open"
    );
    assert!(app
        .find_rect(&key(&format!("library:book:collection:{leaves}")))
        .is_some());
    assert!(app
        .find_rect(&key(&format!("library:book:delete:{leaves}")))
        .is_some());

    tap_key(&mut app, &key(&format!("library:book:finished:{leaves}")));
    assert_eq!(
        library_db(&app).book(leaves).unwrap().unwrap().status,
        Status::Complete,
        "Mark as finished sets the status"
    );

    tap_key(&mut app, &key("library:panel:close"));
    assert!(
        app.find_rect(&format!("book-{leaves}")).is_some(),
        "the list is back"
    );
}

#[test]
fn open_from_details_opens_the_reader() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("details-open");
    let (juliet, _, _) = ids(&app);
    open_library(&mut app);

    hold_key(&mut app, &format!("book-{juliet}"), |a| {
        a.find_rect(&key("library:panel:close")).is_some()
    });
    tap_key(&mut app, &key(&format!("library:open:{juliet}")));

    assert_eq!(app.top_route(), Some(&Route::Reader { book: juliet }));
}

#[test]
fn add_to_collection_creates_it_and_the_filter_shows_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("collections");
    let (juliet, leaves, _) = ids(&app);
    open_library(&mut app);

    hold_key(&mut app, &format!("book-{leaves}"), |a| {
        a.find_rect(&key("library:panel:close")).is_some()
    });
    tap_key(&mut app, &key(&format!("library:book:collection:{leaves}")));
    tap_key(&mut app, &key(&format!("library:add:{leaves}:new")));
    tap_key(&mut app, "name-field");
    type_text(&mut app, "Favourites");
    tap_key(&mut app, &key("library:name:save"));

    let id = collection_id(&app, "Favourites");
    let members: Vec<i64> = library_db(&app)
        .books(
            kaia_library::Sort::new(kaia_library::SortKey::Title, false),
            &kaia_library::Filter {
                collection: Some(id),
                ..Default::default()
            },
            0,
            10,
        )
        .unwrap()
        .into_iter()
        .map(|b| b.id)
        .collect();
    assert_eq!(members, vec![leaves], "the book joins the new collection");

    tap_key(&mut app, &key("library:panel:close"));
    tap_key(&mut app, "library-filter");
    tap_key(&mut app, "library-filter-collection");
    tap_key(&mut app, &format!("collection-{id}"));

    assert!(app.find_rect(&format!("book-{leaves}")).is_some());
    assert!(
        app.find_rect(&format!("book-{juliet}")).is_none(),
        "the filter hides other books"
    );
}

#[test]
fn delete_removes_the_file_after_confirmation() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("delete");
    let (juliet, _, _) = ids(&app);
    let path = library_db(&app).book(juliet).unwrap().unwrap().path;
    assert!(path.exists());
    open_library(&mut app);

    hold_key(&mut app, &format!("book-{juliet}"), |a| {
        a.find_rect(&key("library:panel:close")).is_some()
    });
    tap_key(&mut app, &key(&format!("library:book:delete:{juliet}")));
    assert!(path.exists(), "nothing is deleted before the confirm");
    assert!(app
        .find_rect(&key(&format!("library:book:delete-confirm:{juliet}")))
        .is_some());
    tap_key(&mut app, &key(&format!("library:book:delete-confirm:{juliet}")));

    assert!(!path.exists(), "the file is removed");
    assert!(
        library_db(&app).book(juliet).unwrap().is_none(),
        "the index row is removed immediately"
    );
    assert!(
        app.find_rect(&format!("book-{juliet}")).is_none(),
        "the book leaves the list"
    );
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn library_covers_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("golden-covers");
    open_library(&mut app);
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    assert_matches_golden(app.frame(), &golden("library.png"), 14);
}

#[test]
fn library_list_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("golden-list");
    open_library(&mut app);
    tap_key(&mut app, &key("library:view:list"));
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    assert_matches_golden(app.frame(), &golden("library_list.png"), 14);
}

#[test]
fn library_search_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("golden-search");
    open_library(&mut app);
    tap_search_icon(&mut app);
    tap_key(&mut app, "search-field");
    type_text(&mut app, "leaves");
    let (_, leaves, _) = ids(&app);
    wait_for(&mut app, "the search results", |a| {
        a.find_rect(&format!("book-{leaves}")).is_some()
    });
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    assert_matches_golden(app.frame(), &golden("library_search.png"), 14);
}

#[test]
fn book_details_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("golden-details");
    let (_, leaves, _) = ids(&app);
    open_library(&mut app);
    hold_key(&mut app, &format!("book-{leaves}"), |a| {
        a.find_rect(&key("library:panel:close")).is_some()
    });
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    assert_matches_golden(app.frame(), &golden("library_details.png"), 14);
}

#[test]
fn library_views_keep_the_route_and_the_frame() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("routes");
    open_library(&mut app);
    tap_key(&mut app, &key("library:view:folders"));
    tap_key(&mut app, "library-view-books");
    tap_key(&mut app, &key("library:view:covers"));
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert!(
        app.unknown_commands().is_empty(),
        "every id has an owner: {:?}",
        app.unknown_commands()
    );
}

#[test]
fn removing_history_keeps_reading_progress() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("history-progress");
    let (juliet, _, _) = ids(&app);
    app.services_mut().library.record_open(juliet, 0.35).unwrap();
    open_library(&mut app);
    app.command(&format!("library:book:history:{juliet}"), &[]);
    app.process(Vec::new()).unwrap();
    let row = library_db(&app).book(juliet).unwrap().unwrap();
    assert_eq!(row.percent, 0.35);
    assert!(row.last_opened_at.is_none());
}

#[test]
fn the_tabs_share_one_row_and_the_books_controls_sit_below_with_a_gap() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("header-layout");
    let (juliet, _, _) = ids(&app);
    open_library(&mut app);

    let touch = app.services().theme.touch_target;
    let gap = app.services().theme.space.md as i32;
    let tab_keys = [
        "library-view-books",
        "library-view-folders",
        "library-view-collections",
        "library-view-series",
        "library-view-authors",
    ];
    let tabs = tab_keys.map(|k| app.find_rect(k).unwrap_or_else(|| panic!("no {k}")));
    let row_y = tabs[0].y;
    for (i, r) in tabs.iter().enumerate() {
        assert_eq!(r.y, row_y, "the five tabs share one row");
        assert!(r.h >= touch, "a tab is at least the touch target high");
        if let Some(next) = tabs.get(i + 1) {
            assert!(r.x + r.w as i32 <= next.x, "tab {i} does not overlap the next");
        }
    }

    let filter = app.find_rect("library-filter").expect("the filter button");
    let sort = app.find_rect("library-sort").expect("the sort button");
    let choice = ["library-view-covers", "library-view-list"]
        .map(|k| app.find_rect(k).unwrap_or_else(|| panic!("no {k}")));
    for r in [&sort, &choice[0], &choice[1]] {
        assert_eq!(r.y, filter.y, "the controls share one row");
        assert!(r.h >= touch, "a control is a touch target");
    }
    assert!(
        filter.y >= tabs[0].y + tabs[0].h as i32,
        "the controls sit below the tabs"
    );

    let cover = app.find_rect(&format!("book-{juliet}")).expect("a cover");
    assert!(
        cover.y >= filter.y + filter.h as i32 + gap,
        "a standard gap of {gap} px sits between the controls and the covers"
    );
}

#[test]
fn the_controls_are_for_the_books_tab_only() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("controls-tabs");
    open_library(&mut app);
    assert!(app.find_rect("library-filter").is_some());

    tap_key(&mut app, &key("library:view:folders"));
    assert!(
        app.find_rect("library-view-covers").is_none(),
        "the Covers and List choice belongs to the Books tab"
    );
    assert!(app.find_rect("library-filter").is_none(), "Folders has no filter");
    assert!(app.find_rect("library-sort").is_none(), "Folders has no sort");
    assert!(
        app.find_rect("library-view-folders").is_some(),
        "the tab row stays"
    );

    tap_key(&mut app, "library-view-books");
    assert!(
        app.find_rect("library-filter").is_some(),
        "Books has the filter again"
    );
}

#[test]
fn the_books_tab_returns_to_the_layout_it_left() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("books-tab");
    let (juliet, _, _) = ids(&app);
    open_library(&mut app);
    assert!(app.find_rect("list").is_none(), "Books opens as covers");

    tap_key(&mut app, &key("library:view:list"));
    tap_key(&mut app, &key("library:view:folders"));
    tap_key(&mut app, "library-view-books");
    assert!(app.find_rect("list").is_some(), "Books comes back as the list");
    assert!(app.find_rect(&format!("book-{juliet}")).is_some());

    tap_key(&mut app, &key("library:view:covers"));
    tap_key(&mut app, &key("library:view:series"));
    tap_key(&mut app, "library-view-books");
    assert!(app.find_rect("list").is_none(), "Books comes back as covers");
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
}

#[test]
fn library_filter_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("golden-filter");
    open_library(&mut app);
    tap_key(&mut app, "library-filter");
    assert_matches_golden(app.frame(), &golden("library_filter.png"), 14);
}

#[test]
fn library_large_print_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-large-print");
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    let path = data.join("settings.json");
    let mut settings = kaia_events::Settings::open(&path);
    settings.save(keys::DISPLAY_LARGE_PRINT, kaia_events::Value::Bool(true));
    settings.flush(&path).unwrap();

    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    assert_eq!(app.services().theme.text_scale, LARGE_PRINT_SCALE);
    open_library(&mut app);
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    // The tabs wrap to two rows: three on the first, then Series and Authors.
    let tab = |k: &str| app.find_rect(k).unwrap_or_else(|| panic!("no {k}"));
    let first = tab("library-view-books").y;
    for k in ["library-view-folders", "library-view-collections"] {
        assert_eq!(tab(k).y, first, "{k} is on the first row");
    }
    let second = tab("library-view-series").y;
    assert!(second > first, "Series is on the second row");
    assert_eq!(
        tab("library-view-authors").y,
        second,
        "Authors is on the second row"
    );
    assert_matches_golden(app.frame(), &golden("library_large_print.png"), 14);
    let _ = fs::remove_dir_all(dir);
}
