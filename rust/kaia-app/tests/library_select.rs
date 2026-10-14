//! The Library's search history, select mode, and removing a book from the
//! collection being viewed.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kaia_app::{App, HeadlessBackend, Route};
use kaia_display::InputEvent;
use kaia_gfx::Point;
use kaia_input::{Key, Phase, TouchEvent};
use kaia_library::{Filter, Sort, SortKey, Status};

mod common;

use common::*;

/// The key a button or row with `action` has in the tree.
fn key(action: &str) -> String {
    action.replace(':', "-")
}

/// Types `text` into the focused field, one key at a time.
fn type_text(app: &mut App<HeadlessBackend>, text: &str) {
    for c in text.chars() {
        app.key(Key::Char(c)).unwrap();
    }
}

/// A library app with the three test books indexed.
fn indexed(name: &str) -> (App<HeadlessBackend>, PathBuf) {
    let dir = scratch(name);
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    (app, dir)
}

/// Reopens the app on the same data folder with no scan.
fn reopen(dir: &Path) -> App<HeadlessBackend> {
    let mut app = make_app(dir, Vec::new());
    app.start().unwrap();
    app
}

fn open_library(app: &mut App<HeadlessBackend>) {
    tap_key(app, "library");
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
}

/// Taps the search action at the trailing edge of the header.
fn tap_search_icon(app: &mut App<HeadlessBackend>) {
    let r = app.find_rect("library-header").expect("the header");
    let touch = app.services().theme.touch_target as i32;
    let p = Point::new(r.x + r.w as i32 - touch / 2, r.y + r.h as i32 / 2);
    app.tap(p).unwrap();
}

/// Taps the select action, the icon left of the search icon.
fn tap_select_icon(app: &mut App<HeadlessBackend>) {
    let r = app.find_rect("library-header").expect("the header");
    let touch = app.services().theme.touch_target as i32;
    let p = Point::new(r.x + r.w as i32 - touch * 3 / 2, r.y + r.h as i32 / 2);
    app.tap(p).unwrap();
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

/// The text of a label on the top window.
fn label_text(app: &mut App<HeadlessBackend>, key: &str) -> String {
    let mut text = String::new();
    app.update_top::<kaia_ui::components::Label, _>(key, |label, _| text = label.text().to_owned());
    text
}

/// Runs the loop until the label `key` shows `text`. Fails after one minute.
fn wait_for_label(app: &mut App<HeadlessBackend>, key: &str, text: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let now = label_text(app, key);
        if now.contains(text) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "label {key:?} never showed {text:?}; it shows {now:?}"
        );
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
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

/// True when the collection `cid` holds the book `book`.
fn in_collection(app: &App<HeadlessBackend>, cid: i64, book: i64) -> bool {
    let filter = Filter {
        collection: Some(cid),
        ..Filter::default()
    };
    app.services()
        .library
        .books(Sort::new(SortKey::Title, false), &filter, 0, 100)
        .unwrap()
        .iter()
        .any(|b| b.id == book)
}

#[test]
fn recent_searches_run_from_the_empty_search_page_and_clear() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("recent-search");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    open_library(&mut app);

    tap_search_icon(&mut app);
    tap_key(&mut app, "search-field");
    type_text(&mut app, "leaves");
    wait_for(&mut app, "the results", |a| {
        a.find_rect(&format!("book-{leaves}")).is_some()
    });
    tap_key(&mut app, "library-search-close");

    tap_search_icon(&mut app);
    assert!(
        app.find_rect("recent-heading").is_some(),
        "the empty search page lists the recent searches"
    );
    assert!(app.find_rect("recent-0").is_some(), "the search is listed");
    tap_key(&mut app, "recent-0");
    assert!(
        app.find_rect(&format!("book-{leaves}")).is_some(),
        "the recent search runs again"
    );
    assert!(
        app.find_rect(&format!("book-{juliet}")).is_none(),
        "its results do not include other books"
    );

    tap_key(&mut app, "library-search-close");
    tap_search_icon(&mut app);
    tap_key(&mut app, "library-search-recent-clear");
    assert!(
        app.find_rect("recent-0").is_none(),
        "clearing removes the recent searches from the page"
    );
    assert!(library_db(&app).recent_searches(10).unwrap().is_empty());
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
}

#[test]
fn select_mode_marks_the_checked_books_finished_and_unread() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, _dir) = indexed("select-mode");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    let pdf = book_id(&app, "2col.pdf");
    let md5 = library_db(&app).book(juliet).unwrap().unwrap().partial_md5;
    open_library(&mut app);

    tap_select_icon(&mut app);
    assert!(
        app.find_rect("select-count").is_some(),
        "select mode shows its bar"
    );
    assert_eq!(label_text(&mut app, "select-count"), "No books selected");
    tap_key(&mut app, &format!("book-{juliet}"));
    tap_key(&mut app, &format!("book-{leaves}"));
    assert_eq!(label_text(&mut app, "select-count"), "2 books selected");

    tap_key(&mut app, "library-select-finished");
    wait_for_label(&mut app, "select-count", "2 books marked as finished.");
    for id in [juliet, leaves] {
        assert_eq!(
            library_db(&app).book(id).unwrap().unwrap().status,
            Status::Complete
        );
    }
    assert_eq!(
        library_db(&app).book(pdf).unwrap().unwrap().status,
        Status::New,
        "a book that is not checked is not changed"
    );
    assert_eq!(
        app.services()
            .settings
            .read(format!("book.status.{md5}"))
            .as_str(),
        Some("complete"),
        "a book with no state file keeps its status for the first open"
    );

    tap_key(&mut app, "library-select-unread");
    wait_for_label(&mut app, "select-count", "2 books marked as unread.");
    for id in [juliet, leaves] {
        assert_eq!(library_db(&app).book(id).unwrap().unwrap().status, Status::New);
    }
    assert_eq!(
        app.services()
            .settings
            .read(format!("book.status.{md5}"))
            .as_str(),
        Some("new")
    );

    tap_key(&mut app, "library-select-off");
    assert!(app.find_rect("select-count").is_none(), "Done leaves select mode");
    assert!(app.find_rect("library-view-list").is_some(), "the tabs are back");
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
}

#[test]
fn details_take_a_book_out_of_the_collection_being_viewed() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let (mut app, dir) = indexed("remove-collection");
    let juliet = book_id(&app, "juliet.epub");
    let leaves = book_id(&app, "leaves.epub");
    app.services_mut()
        .library
        .add_to_collection(leaves, "Favourites")
        .unwrap();
    app.services_mut()
        .library
        .add_to_collection(juliet, "Favourites")
        .unwrap();
    let cid = collection_id(&app, "Favourites");
    drop(app);
    let mut app = reopen(&dir);
    open_library(&mut app);

    tap_key(&mut app, "library-filter");
    tap_key(&mut app, "library-filter-collection");
    tap_key(&mut app, &format!("collection-{cid}"));
    hold_key(&mut app, &format!("book-{leaves}"), |a| {
        a.find_rect("library-panel-close").is_some()
    });
    let remove = key(&format!("library:remove:{leaves}:{cid}"));
    assert!(
        app.find_rect(&remove).is_some(),
        "inside the collection, the details offer to remove the book"
    );
    tap_key(&mut app, &remove);

    assert!(
        !in_collection(&app, cid, leaves),
        "the book leaves the collection"
    );
    assert!(in_collection(&app, cid, juliet), "the other books stay");
    assert!(
        app.find_rect(&remove).is_none(),
        "the details no longer offer the removal"
    );
    assert!(app.notices().is_empty(), "no notices: {:?}", app.notices());
    let _ = fs::remove_dir_all(dir);
}
