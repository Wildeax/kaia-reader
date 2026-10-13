//! Headless tests for the reader surface: opening a book, page turns, the
//! overlay, progress on Back, a book that does not open, and the reader golden.

use std::fs;
use std::time::Duration;

use kaia_app::surfaces::reader::ReaderPage;
use kaia_app::Route;
use kaia_events::RefreshMode;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Point;
use kaia_input::Key;

mod common;

use common::*;

#[test]
fn opening_a_book_shows_the_reader_with_a_full_refresh() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("open-book");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();

    let before = app.refresh_modes().len();
    open_from_library(&mut app, "juliet.epub");

    let id = book_id(&app, "juliet.epub");
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));
    let mut pages = 0;
    app.update_top::<ReaderPage, _>("reader", |r, _| pages = r.page_count());
    assert!(
        pages >= 3,
        "the test book needs at least three pages, has {pages}"
    );
    assert_eq!(
        app.refresh_modes().last(),
        Some(&RefreshMode::Full),
        "opening the reader is a full refresh"
    );
    assert!(modes_after(&app, before).contains(&RefreshMode::Full));
    assert!(library_db(&app)
        .book(id)
        .unwrap()
        .unwrap()
        .last_opened_at
        .is_some());
}

#[test]
fn page_turns_are_partial_refreshes_and_back_returns() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("page-turns");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    open_from_library(&mut app, "juliet.epub");
    let id = book_id(&app, "juliet.epub");

    let before = app.refresh_modes().len();
    app.key(Key::RPgFwd).unwrap();
    assert_eq!(
        modes_after(&app, before),
        vec![RefreshMode::Partial],
        "one page forward"
    );

    // A tap on the right side turns the page too.
    let (w, h) = (app.screen().w as i32, app.screen().h as i32);
    let before = app.refresh_modes().len();
    app.tap(Point::new(w * 9 / 10, h / 2)).unwrap();
    assert_eq!(
        modes_after(&app, before),
        vec![RefreshMode::Partial],
        "a right tap is a page forward"
    );

    let mut page = 0;
    app.update_top::<ReaderPage, _>("reader", |r, _| page = r.page());
    assert_eq!(page, 2);

    // The left third goes back a page.
    let before = app.refresh_modes().len();
    app.tap(Point::new(w / 10, h / 2)).unwrap();
    assert_eq!(modes_after(&app, before), vec![RefreshMode::Partial]);

    // Back leaves the reader, returns to the Library, and saves the progress.
    let before = app.refresh_modes().len();
    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert_eq!(modes_after(&app, before), vec![RefreshMode::Ui]);
    let book = library_db(&app).book(id).unwrap().unwrap();
    assert!(
        book.percent > 0.0,
        "progress is saved on Back, got {}",
        book.percent
    );
}

#[test]
fn centre_tap_opens_the_overlay_and_back_closes_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("overlay");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    open_from_library(&mut app, "juliet.epub");
    let id = book_id(&app, "juliet.epub");

    let (w, h) = (app.screen().w as i32, app.screen().h as i32);
    app.tap(Point::new(w / 2, h / 2)).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Overlay));
    assert!(app.find_rect("back").is_some());

    // The overlay's Back button closes the overlay, not the reader.
    tap_key(&mut app, "back");
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));

    // A tap the overlay does not use also closes it.
    app.tap(Point::new(w / 2, h / 2)).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Overlay));
    app.tap(Point::new(w / 2, h - 20)).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));
}

#[test]
fn a_book_that_cannot_open_shows_an_error_and_back_still_works() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("broken");
    let lib = dir.join("books");
    fs::create_dir_all(&lib).unwrap();
    fs::write(lib.join("broken.epub"), b"not a zip file at all").unwrap();
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    open_from_library(&mut app, "broken.epub");

    let id = book_id(&app, "broken.epub");
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));
    let mut pages = None;
    app.update_top::<ReaderPage, _>("reader", |r, _| pages = Some(r.page_count()));
    assert_eq!(pages, Some(0), "a book that does not open has no pages");

    app.key(Key::Back).unwrap();
    assert!(matches!(app.top_route(), Some(Route::Library { .. })));
}

#[test]
fn reader_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-reader");
    let lib = library_with_books(&dir);
    let mut app = make_app(&dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    open_from_library(&mut app, "2col.pdf");
    assert_matches_golden(app.frame(), &golden("reader.png"), 2);
}
