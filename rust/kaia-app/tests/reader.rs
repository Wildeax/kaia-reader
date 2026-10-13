//! Headless tests for the reader surface: opening, page turns and their refresh
//! modes, the overlay, the contents panel, the text and layout sheet, selection
//! and highlights, the dictionary, search, the end of book sheet, and goldens.

use std::fs;
use std::path::Path;
use std::time::Duration;

use kaia_app::surfaces::reader::page::PageView;
use kaia_app::surfaces::reader::panels::{ContentItem, SearchRow};
use kaia_app::App;
use kaia_app::HeadlessBackend;
use kaia_app::Route;
use kaia_display::InputEvent;
use kaia_events::RefreshMode;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Point;
use kaia_input::{Key, Phase, TouchEvent};
use kaia_ui::components::Label;
use kaia_ui::components::Paragraph;
use kaia_ui::composite::PagedList;
use kaia_ui::controls::{Slider, TextField};

mod common;

use common::*;

/// The book shown in the reader and its page, from the reader window on top.
fn position(app: &mut App<HeadlessBackend>) -> (u32, usize) {
    let mut position = None;
    app.update_top::<PageView, _>("reader-page", |view, _| position = view.position);
    position.expect("the reader window is on top and has a position")
}

/// The page the overlay slider shows (1-based), from the overlay on top.
fn slider_value(app: &mut App<HeadlessBackend>) -> f64 {
    let mut value = None;
    app.update_top::<Slider, _>("overlay-slider", |slider, _| value = Some(slider.value()));
    value.expect("the overlay is on top")
}

fn text_of(app: &mut App<HeadlessBackend>, key: &str) -> String {
    let mut text = None;
    app.update_top::<Paragraph, _>(key, |p, _| text = Some(p.text().to_string()));
    if text.is_none() {
        app.update_top::<Label, _>(key, |l, _| text = Some(l.text().to_string()));
    }
    text.unwrap_or_else(|| panic!("no text widget {key:?} on the top window"))
}

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
    }
}

/// A point on a word of the text page that `turn_to_text` shows.
fn word_point() -> Point {
    Point::new(200, 120)
}

/// Centre of the screen, for a tap that opens the overlay.
fn centre(app: &App<HeadlessBackend>) -> Point {
    Point::new(app.screen().w as i32 / 2, app.screen().h as i32 / 2)
}

/// Opens the overlay with a centre tap and checks it is up.
fn open_overlay(app: &mut App<HeadlessBackend>) {
    let p = centre(app);
    app.tap(p).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Overlay));
}

/// A hold at `p`: the touch goes down at a small timestamp, the hold timer
/// (500 ms on the gesture clock) runs out in real time, and the touch lifts.
fn hold_at(app: &mut App<HeadlessBackend>, p: Point) {
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Down,
        pos: p,
        time_us: 1,
    })])
    .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    app.iterate(Some(Duration::from_millis(1))).unwrap();
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: p,
        time_us: 2_000_000,
    })])
    .unwrap();
}

/// Writes a one-word StarDict dictionary into `<data>/dict`.
fn write_stardict(data_dir: &Path, word: &str, definition: &str) {
    let dir = data_dir.join("dict");
    fs::create_dir_all(&dir).unwrap();
    let body = definition.as_bytes();
    let mut idx = Vec::new();
    idx.extend_from_slice(word.as_bytes());
    idx.push(0);
    idx.extend_from_slice(&0u32.to_be_bytes());
    idx.extend_from_slice(&(body.len() as u32).to_be_bytes());
    fs::write(dir.join("test.dict"), body).unwrap();
    fs::write(dir.join("test.idx"), &idx).unwrap();
    let ifo = format!(
        "StarDict's dict ifo file\nversion=2.4.2\nbookname=Test Dictionary\nwordcount=1\nidxfilesize={}\nsametypesequence=m\n",
        idx.len()
    );
    fs::write(dir.join("test.ifo"), ifo).unwrap();
}

/// The first word of a label, without the punctuation around it.
fn word_of(label: &str) -> String {
    label
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_string()
}

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
    let (page, count) = position(&mut app);
    assert_eq!(page, 0);
    assert!(
        count >= 3,
        "the test book needs at least three pages, has {count}"
    );
    assert_eq!(app.refresh_modes().last(), Some(&RefreshMode::Full));
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
    let mut app = open_reader("juliet.epub", &dir);
    let id = book_id(&app, "juliet.epub");

    // A turn is partial unless it lands on a chapter start. Walk forward, on pages
    // past the front matter, to a page whose next two pages are not chapter starts.
    let chapters = chapter_pages(&mut app);
    let is_start = |page: u32| chapters.contains(&(page - 1));
    let free = |page: u32| !is_start(page + 1) && !is_start(page + 2);
    while position(&mut app).0 < 4 || !free(position(&mut app).0) {
        app.key(Key::RPgFwd).unwrap();
    }
    let page = position(&mut app).0;

    let before = app.refresh_modes().len();
    app.key(Key::RPgFwd).unwrap();
    assert_eq!(
        modes_after(&app, before),
        vec![RefreshMode::Partial],
        "one page forward"
    );
    assert_eq!(position(&mut app).0, page + 1);

    // A tap on the right side turns the page too.
    let (w, h) = (app.screen().w as i32, app.screen().h as i32);
    let before = app.refresh_modes().len();
    app.tap(Point::new(w * 9 / 10, h / 2)).unwrap();
    assert_eq!(modes_after(&app, before), vec![RefreshMode::Partial]);
    assert_eq!(position(&mut app).0, page + 2);

    // The left third goes back a page.
    let before = app.refresh_modes().len();
    app.tap(Point::new(w / 10, h / 2)).unwrap();
    assert_eq!(modes_after(&app, before), vec![RefreshMode::Partial]);
    assert_eq!(position(&mut app).0, page + 1);

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

/// The pages where a table of contents entry starts, 0-based. Read from the
/// Contents panel, which lists each entry as "Page N".
fn chapter_pages(app: &mut App<HeadlessBackend>) -> std::collections::HashSet<u32> {
    open_overlay(app);
    tap_key(app, "overlay-contents");
    let mut pages = std::collections::HashSet::new();
    app.update_top::<PagedList<ContentItem>, _>("contents-list", |list, _| {
        for item in list.items() {
            if let Some(n) = item
                .detail
                .strip_prefix("Page ")
                .and_then(|n| n.parse::<u32>().ok())
            {
                pages.insert(n - 1);
            }
        }
    });
    // Back closes the panel, and a tap in the middle closes the overlay.
    app.key(Key::Back).unwrap();
    let (w, h) = (app.screen().w as i32, app.screen().h as i32);
    app.tap(Point::new(w / 2, h / 2)).unwrap();
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));
    pages
}

#[test]
fn full_refresh_at_chapter_starts_and_every_sixth_turn() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sixth-turn");
    let mut app = open_reader("juliet.epub", &dir);
    let chapters = chapter_pages(&mut app);
    assert!(
        chapters.len() > 4,
        "the test book has many chapters, got {}",
        chapters.len()
    );

    // Each turn is one refresh. A turn to a chapter start is full, and so is
    // every sixth turn since the last full refresh. The others are partial.
    let mut since_full = 0;
    for turn in 0..14 {
        let before = app.refresh_modes().len();
        app.key(Key::RPgFwd).unwrap();
        let modes = modes_after(&app, before);
        assert_eq!(modes.len(), 1, "turn {turn} sends one refresh, got {modes:?}");
        let page = position(&mut app).0;
        let expect_full = chapters.contains(&page) || since_full + 1 >= 6;
        if expect_full {
            since_full = 0;
            assert!(
                modes[0] == RefreshMode::Full,
                "turn {turn} to page {page} is a full refresh, got {modes:?}"
            );
        } else {
            since_full += 1;
            assert!(
                modes[0] == RefreshMode::Partial,
                "turn {turn} to page {page} is partial, got {modes:?}"
            );
        }
    }
}

#[test]
fn centre_tap_opens_the_overlay_and_an_unused_tap_closes_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("overlay");
    let mut app = open_reader("juliet.epub", &dir);
    let id = book_id(&app, "juliet.epub");

    let before = app.refresh_modes().len();
    open_overlay(&mut app);
    assert!(app.find_rect("overlay-home").is_some());
    assert!(app.find_rect("overlay-slider").is_some());
    assert!(app.find_rect("overlay-contents").is_some());
    assert!(app.find_rect("overlay-notes").is_some());
    assert!(app.find_rect("overlay-layout").is_some());
    assert_eq!(slider_value(&mut app), 1.0, "the slider shows page 1");
    assert_eq!(
        modes_after(&app, before),
        vec![RefreshMode::Ui],
        "the bars are a ui refresh"
    );

    // A tap in the middle of the page, which the bars do not use, closes the overlay.
    let (w, h) = (app.screen().w as i32, app.screen().h as i32);
    app.tap(Point::new(w / 2, h / 2)).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));
}

#[test]
fn bookmark_button_adds_a_bookmark_listed_in_the_contents() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("bookmark");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);

    tap_key(&mut app, "overlay-bookmark");
    tap_key(&mut app, "overlay-contents");
    tap_key(&mut app, "contents-tab-1");
    let mut bookmarks = 0;
    app.update_top::<PagedList<ContentItem>, _>("contents-list", |list, _| bookmarks = list.items().len());
    assert_eq!(bookmarks, 1, "one bookmark on the page");
}

#[test]
fn contents_entry_jumps_and_the_overlay_follows() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("contents");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-contents");

    let mut entries = 0;
    app.update_top::<PagedList<ContentItem>, _>("contents-list", |list, _| entries = list.items().len());
    assert!(entries >= 2, "the test book has chapters, got {entries}");

    // The second entry of the first page jumps to its chapter.
    tap_key(&mut app, "reader-contents-goto-1");
    assert_eq!(
        app.top_route(),
        Some(&Route::Overlay),
        "the contents panel closes"
    );
    assert!(
        slider_value(&mut app) > 1.0,
        "the second chapter starts after the first page, slider at {}",
        slider_value(&mut app)
    );
}

#[test]
fn font_size_change_relayouts_and_keeps_the_place() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("relayout");
    let mut app = open_reader("juliet.epub", &dir);
    app.key(Key::RPgFwd).unwrap();
    app.key(Key::RPgFwd).unwrap();
    let (page_before, count_before) = position(&mut app);
    let percent_before = page_before as f64 / count_before as f64;

    open_overlay(&mut app);
    tap_key(&mut app, "overlay-layout");
    // The plus button sits at the right edge of the stepper.
    let rect = app.find_rect("layout-size").expect("the size stepper");
    app.tap(Point::new(rect.x + rect.w as i32 - 6, rect.y + rect.h as i32 / 2))
        .unwrap();

    // The layout runs on the engine thread. Give it time, then leave the sheet.
    let deadline = std::time::Instant::now() + Duration::from_millis(1500);
    while std::time::Instant::now() < deadline {
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
    app.key(Key::Back).unwrap();
    app.tap(centre(&app)).unwrap();
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));

    let (page_after, count_after) = position(&mut app);
    assert_ne!(count_before, count_after, "a bigger font makes more pages");
    let percent_after = page_after as f64 / count_after as f64;
    assert!(
        (percent_after - percent_before).abs() < 0.05,
        "the place is kept: {percent_before:.3} became {percent_after:.3}"
    );
}

#[test]
fn hold_selects_a_word_and_the_highlight_survives_reopening() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("highlight");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel), "the popover opens");
    assert!(app.find_rect("sel-highlight").is_some());
    tap_key(&mut app, "sel-highlight");
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));

    // Leave the book and open it again. The highlight is in the Highlights tab.
    app.key(Key::Back).unwrap();
    open_from_library(&mut app, "juliet.epub");
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-contents");
    tap_key(&mut app, "contents-tab-2");
    let mut highlights = 0;
    app.update_top::<PagedList<ContentItem>, _>("contents-list", |list, _| highlights = list.items().len());
    assert_eq!(highlights, 1, "the highlight survives reopening");
}

#[test]
fn note_on_a_selection_is_saved_with_the_highlight() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("note");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-note");
    assert!(app.find_rect("note-save").is_some(), "the note editor is up");

    app.update_top::<TextField, _>("note-field", |field, cx| {
        field.set_text("check the wording");
        cx.request_paint();
    });
    tap_key(&mut app, "note-save");
    assert!(
        matches!(app.top_route(), Some(Route::Reader { .. })),
        "the editor closes"
    );

    open_overlay(&mut app);
    tap_key(&mut app, "overlay-contents");
    tap_key(&mut app, "contents-tab-2");
    let mut details = Vec::new();
    app.update_top::<PagedList<ContentItem>, _>("contents-list", |list, _| {
        details = list.items().iter().map(|i| i.detail.clone()).collect();
    });
    assert_eq!(
        details,
        vec!["check the wording".to_string()],
        "the note is listed with the highlight"
    );
}

#[test]
fn copy_closes_the_popover() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("copy");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel));
    let selection = text_of(&mut app, "sel-word");
    tap_key(&mut app, "sel-copy");
    assert_eq!(app.services().clipboard.as_deref(), Some(selection.as_str()));
    assert!(
        matches!(app.top_route(), Some(Route::Reader { .. })),
        "the popover closes after Copy"
    );
}

#[test]
fn fixed_layout_books_get_a_zoom_sheet_instead_of_text_settings() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("zoom");
    let mut app = open_reader("2col.pdf", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-layout");
    assert!(
        app.find_rect("layout-zoom").is_some(),
        "a PDF shows the zoom choice"
    );
    assert!(app.find_rect("layout-size").is_none(), "a PDF has no font size");
}

#[test]
fn dictionary_sheet_shows_the_definition_of_the_selected_word() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("dictionary");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    let word = word_of(&text_of(&mut app, "sel-word"));
    assert!(!word.is_empty(), "a word is selected");

    // The dictionary is read when the word is looked up, so write it now.
    write_stardict(&dir.join("data"), &word, "first line\nsecond line");
    let dictionaries = dir.join("data/dict");
    fs::copy(dictionaries.join("test.idx"), dictionaries.join("second.idx")).unwrap();
    fs::copy(dictionaries.join("test.dict"), dictionaries.join("second.dict")).unwrap();
    let info = fs::read_to_string(dictionaries.join("test.ifo"))
        .unwrap()
        .replace("Test Dictionary", "Second Dictionary");
    fs::write(dictionaries.join("second.ifo"), info).unwrap();
    app.services_mut().settings.save(
        "dictionary.order",
        kaia_events::Value::array([
            kaia_events::Value::from("Second Dictionary"),
            kaia_events::Value::from("Test Dictionary"),
        ]),
    );
    tap_key(&mut app, "sel-dict");
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel));
    let body = text_of(&mut app, "dict-body");
    assert!(body.find("Second Dictionary").unwrap() < body.find("Test Dictionary").unwrap());
    assert!(
        body.contains("first line\nsecond line"),
        "the sheet shows the definition, got {body:?}"
    );
    app.services_mut().settings.save(
        "dictionary.disabled",
        kaia_events::Value::array([kaia_events::Value::from("Second Dictionary")]),
    );
    fs::remove_file(dir.join("data/dict/test.ifo")).unwrap();
    app.key(Key::Back).unwrap();
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-dict");
    assert!(!text_of(&mut app, "dict-body").contains("Second Dictionary"));
    assert!(
        text_of(&mut app, "dict-body").contains("first line\nsecond line"),
        "the session keeps the loaded dictionary"
    );
}

#[test]
fn dictionary_sheet_says_when_none_is_installed() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("no-dictionary");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-dict");
    let body = text_of(&mut app, "dict-body");
    assert!(body.starts_with("No dictionary installed."), "got {body:?}");
    assert!(
        body.contains("StarDict"),
        "the sheet says what to copy, got {body:?}"
    );
}

#[test]
fn search_lists_hits_and_a_hit_jumps_there() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("search");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-search");

    app.update_top::<TextField, _>("search-field", |field, cx| {
        field.set_text("Romeo");
        cx.request_paint();
    });
    tap_key(&mut app, "search-go");
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let mut hits = 0;
    loop {
        app.update_top::<PagedList<SearchRow>, _>("search-results", |list, _| hits = list.items().len());
        if hits > 0 || std::time::Instant::now() > deadline {
            break;
        }
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
    assert!(hits > 0, "the search finds Romeo");

    let mut first = None;
    app.update_top::<PagedList<SearchRow>, _>("search-results", |list, _| {
        first = list.items().first().map(|r| r.label.clone());
    });
    let page: f64 = first
        .as_deref()
        .and_then(|l| l.strip_prefix("Page "))
        .and_then(|n| n.parse().ok())
        .expect("a hit names its page");

    tap_key(&mut app, "reader-search-hit-0");
    assert_eq!(app.top_route(), Some(&Route::Overlay), "the search panel closes");
    assert_eq!(slider_value(&mut app), page, "the reader jumps to the hit's page");
}

#[test]
fn end_of_book_sheet_appears_after_the_last_page() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("end-of-book");
    let mut app = open_reader("juliet.epub", &dir);
    let (_, count) = position(&mut app);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-more");
    tap_key(&mut app, "more-end");
    assert_eq!(app.top_route(), Some(&Route::Overlay));
    assert_eq!(slider_value(&mut app), count as f64, "the last page is shown");

    app.send(kaia_events::Event::new("PageForward"));
    app.process(Vec::new()).unwrap();
    assert!(
        app.find_rect("end-finish").is_some(),
        "the end of book sheet is up"
    );
    tap_key(&mut app, "end-finish");
    assert_eq!(app.top_route(), Some(&Route::Overlay), "the sheet closes");
}

#[test]
fn reopening_restores_the_page() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("reopen");
    let mut app = open_reader("juliet.epub", &dir);
    app.key(Key::RPgFwd).unwrap();
    app.key(Key::RPgFwd).unwrap();
    assert_eq!(position(&mut app).0, 2);
    app.key(Key::Back).unwrap();

    open_from_library(&mut app, "juliet.epub");
    assert_eq!(position(&mut app).0, 2, "the reader comes back to page 3");
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
    let mut message = None;
    app.update_top::<PageView, _>("reader-page", |view, _| message = view.message.clone());
    assert!(
        message
            .as_deref()
            .is_some_and(|m| m.starts_with("Cannot open this book.")),
        "the page says why it is blank, got {message:?}"
    );

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

#[test]
fn overlay_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-overlay");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    assert_matches_golden(app.frame(), &golden("reader-overlay.png"), 2);
}

#[test]
fn contents_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-contents");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-contents");
    assert_matches_golden(app.frame(), &golden("reader-contents.png"), 2);
}

#[test]
fn text_and_layout_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-layout");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-layout");
    assert_matches_golden(app.frame(), &golden("reader-layout.png"), 2);
}

#[test]
fn selection_popover_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-selection");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    assert_matches_golden(app.frame(), &golden("reader-selection.png"), 2);
}

#[test]
fn dictionary_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-dictionary");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    let word = word_of(&text_of(&mut app, "sel-word"));
    write_stardict(&dir.join("data"), &word, "a test definition for the word");
    tap_key(&mut app, "sel-dict");
    assert_matches_golden(app.frame(), &golden("reader-dictionary.png"), 2);
}

#[test]
fn search_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-search");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-search");
    app.update_top::<TextField, _>("search-field", |field, cx| {
        field.set_text("Romeo");
        cx.request_paint();
    });
    tap_key(&mut app, "search-go");
    // Wait for the search to finish: the status line stops saying "Searching".
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        let status = text_of(&mut app, "search-status");
        if !status.starts_with("Searching") || std::time::Instant::now() > deadline {
            break;
        }
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
    assert!(
        text_of(&mut app, "search-status").ends_with("matches."),
        "the search finished"
    );
    assert_matches_golden(app.frame(), &golden("reader-search.png"), 2);
}

#[test]
fn named_page_events_cross_the_overlay_but_page_keys_move_focus_first() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("overlay-page-routing");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    let page = slider_value(&mut app);
    app.key(Key::RPgFwd).unwrap();
    assert_eq!(
        slider_value(&mut app),
        page,
        "a focusable overlay gets the key first"
    );
    assert!(app.send(kaia_events::Event::new("PageForward")));
    app.process(Vec::new()).unwrap();
    assert_eq!(slider_value(&mut app), page + 1.0);
}

#[test]
fn opening_the_open_book_reuses_it_and_applies_the_requested_location() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("reuse-reader");
    let mut app = open_reader("juliet.epub", &dir);
    let book = book_id(&app, "juliet.epub");
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-layout");
    app.command(&format!("open_book:{book}"), &[kaia_events::Value::Number(8.0)]);
    app.process(Vec::new()).unwrap();
    assert_eq!(app.router().depth(), 2);
    assert_eq!(position(&mut app).0, 8);
    assert!(app.notices().is_empty(), "{:?}", app.notices());
    assert!(!app.services().sync.busy(), "sync stays off by default");
}

#[test]
fn autosave_suspend_and_shutdown_preserve_position_without_closing_first() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("save-lifecycle");
    let mut app = open_reader("juliet.epub", &dir);
    let row = app
        .services()
        .library
        .book(book_id(&app, "juliet.epub"))
        .unwrap()
        .unwrap();
    let state = dir
        .join("data/kaia/books")
        .join(format!("{}.json", row.partial_md5));
    let initial = fs::read(&state).unwrap_or_default();
    std::thread::sleep(Duration::from_millis(5100));
    app.key(Key::RPgFwd).unwrap();
    let saved = fs::read(&state).expect("a page turn saves after five seconds");
    assert_ne!(initial, saved);
    app.key(Key::RPgFwd).unwrap();
    app.command("sleep", &[]);
    app.process(Vec::new()).unwrap();
    let sleeping = fs::read(&state).unwrap();
    assert_ne!(saved, sleeping, "suspend saves immediately");
    app.key(Key::Power).unwrap();
    app.key(Key::RPgFwd).unwrap();
    app.shutdown();
    assert_ne!(sleeping, fs::read(&state).unwrap(), "shutdown saves immediately");
    let mut opts = kaia_book::OpenOptions::new(dir.join("data"));
    opts.viewport = app.screen();
    let session = kaia_book::BookSession::open(&row.path, opts).unwrap();
    assert_eq!(session.current().page, 3);
}

#[test]
fn remote_progress_uses_the_selected_strategy_and_waits_for_confirmation() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sync-strategies");
    let mut app = open_reader("2col.pdf", &dir);
    let book = book_id(&app, "2col.pdf");
    use kaia_events::{Event, Value};
    app.services_mut()
        .settings
        .save("sync.enabled", Value::Bool(true));
    let remote = Event::with_args(
        "SyncProgress",
        vec![
            Value::Number(book as f64),
            Value::from("2"),
            Value::Number(1.0),
            Value::from("Other reader"),
            Value::from("other-installation"),
            Value::Number(9999999999.0),
        ],
    );
    app.services_mut()
        .settings
        .save("sync.forward", Value::from("never"));
    app.broadcast_app(remote.clone());
    app.process(Vec::new()).unwrap();
    assert_eq!(position(&mut app).0, 0);
    assert!(!app.services().sync.busy(), "Never does not push during pull");
    app.services_mut()
        .settings
        .save("sync.forward", Value::from("prompt"));
    app.broadcast_app(remote.clone());
    app.process(Vec::new()).unwrap();
    assert!(app.find_rect("dialog-button:reader:sync:accept").is_some());
    tap_key(&mut app, "dialog-button:reader:sync:accept");
    assert_eq!(position(&mut app).0, 1);
    app.command("reader:location", &[Value::Number(0.0)]);
    app.services_mut()
        .settings
        .save("sync.forward", Value::from("silent"));
    app.broadcast_app(remote);
    app.process(Vec::new()).unwrap();
    assert_eq!(position(&mut app).0, 1);
    app.services_mut()
        .settings
        .save("sync.enabled", Value::Bool(false));
}
