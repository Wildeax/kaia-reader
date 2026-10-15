//! Headless tests for the reader surface: opening, page turns and their refresh
//! modes, the overlay, the contents panel, the text and layout sheet, selection
//! and highlights, the dictionary, search, the end of book sheet, and goldens.

use std::fs;
use std::path::Path;
use std::time::Duration;

use kaia_app::slider::Slider;
use kaia_app::surfaces::reader::page::PageView;
use kaia_app::surfaces::reader::panels::{ContentItem, SearchRow};
use kaia_app::App;
use kaia_app::HeadlessBackend;
use kaia_app::Route;
use kaia_display::InputEvent;
use kaia_events::{Event, RefreshMode, Settings};
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Point;
use kaia_input::{Key, Phase, TouchEvent};
use kaia_ui::components::Label;
use kaia_ui::components::Paragraph;
use kaia_ui::composite::PagedList;
use kaia_ui::controls::TextField;

mod common;

use common::*;

/// The book shown in the reader and its page, from the reader window on top.
fn position(app: &mut App<HeadlessBackend>) -> (u32, usize) {
    wait_reader(app);
    let mut position = None;
    app.update_top::<PageView, _>("reader-page", |view, _| position = view.position);
    position.expect("the reader window is on top and has a position")
}

/// The page the overlay slider shows (1-based), from the overlay on top.
fn slider_value(app: &mut App<HeadlessBackend>) -> f64 {
    wait_reader(app);
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
        wait_reader(app);
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
    wait_reader(app);
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
    wait_reader(app);
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
        wait_reader(&mut app);
    }
    let page = position(&mut app).0;

    let before = app.refresh_modes().len();
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
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
    wait_reader(&mut app);
    assert_eq!(modes_after(&app, before), vec![RefreshMode::Partial]);
    assert_eq!(position(&mut app).0, page + 2);

    // The left third goes back a page.
    let before = app.refresh_modes().len();
    app.tap(Point::new(w / 10, h / 2)).unwrap();
    wait_reader(&mut app);
    assert_eq!(modes_after(&app, before), vec![RefreshMode::Partial]);
    assert_eq!(position(&mut app).0, page + 1);

    // Back leaves the reader, returns to the Library, and saves the progress.
    let before = app.refresh_modes().len();
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    assert_eq!(app.top_route(), Some(&Route::Library { page: 0 }));
    assert!(modes_after(&app, before).iter().all(|m| *m == RefreshMode::Ui));
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
    wait_reader(app);
    let (w, h) = (app.screen().w as i32, app.screen().h as i32);
    app.tap(Point::new(w / 2, h / 2)).unwrap();
    wait_reader(app);
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));
    pages
}

#[test]
fn full_refresh_at_chapter_starts_and_every_sixth_turn() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sixth-turn");
    // The default is now a full refresh per chapter. This test keeps the page count.
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    let mut settings = Settings::open(&data.join("settings.json"));
    settings.save("reading.full_refresh_pages", kaia_events::Value::from(6.0));
    settings.flush(&data.join("settings.json")).unwrap();
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
        wait_reader(&mut app);
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
    wait_reader(&mut app);
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
    wait_reader(&mut app);
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    let (page_before, count_before) = position(&mut app);
    let percent_before = page_before as f64 / count_before as f64;

    open_overlay(&mut app);
    tap_key(&mut app, "overlay-layout");
    // The plus button sits at the right edge of the stepper.
    let rect = app.find_rect("layout-size").expect("the size stepper");
    app.tap(Point::new(rect.x + rect.w as i32 - 6, rect.y + rect.h as i32 / 2))
        .unwrap();
    wait_reader(&mut app);

    // The layout runs on the engine thread. Give it time, then leave the sheet.
    let deadline = std::time::Instant::now() + Duration::from_millis(1500);
    while std::time::Instant::now() < deadline {
        app.iterate(Some(Duration::from_millis(20))).unwrap();
    }
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    app.tap(centre(&app)).unwrap();
    wait_reader(&mut app);
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));

    let (page_after, count_after) = position(&mut app);
    assert_ne!(count_before, count_after, "a bigger font makes more pages");
    let percent_after = page_after as f64 / count_after as f64;
    assert!(
        (percent_after - percent_before).abs() < 0.05,
        "the place is kept: {percent_before:.3} became {percent_after:.3}"
    );
}

/// The font family saved in the book's state file, after a save.
fn saved_font_family(app: &mut App<HeadlessBackend>, name: &str) -> Option<String> {
    app.broadcast_app(Event::new("SaveState"));
    wait_reader(app);
    let id = book_id(app, name);
    let md5 = app.services().library.book(id).unwrap().unwrap().partial_md5;
    let path = app
        .services()
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{md5}.json"));
    Settings::open(&path)
        .read("layout")
        .get("font_family")
        .as_str()
        .map(str::to_owned)
}

#[test]
fn font_row_opens_the_font_list_and_picking_a_face_returns_to_the_sheet() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("font-pick");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-layout");
    assert!(
        app.find_rect("layout-font").is_some(),
        "the Font row is on the sheet"
    );

    tap_key(&mut app, "layout-font");
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel));
    assert!(
        app.find_rect("reader-layout-font-4").is_some(),
        "the font list shows OpenDyslexic"
    );
    assert!(
        app.find_rect("layout-font").is_none(),
        "the list is on top of the sheet"
    );

    tap_key(&mut app, "reader-layout-font-4");
    assert!(
        app.find_rect("reader-layout-font-4").is_none(),
        "picking a face closes the font list"
    );
    assert!(
        app.find_rect("layout-font").is_some(),
        "the Text and layout sheet is back"
    );
    assert_eq!(
        saved_font_family(&mut app, "juliet.epub").as_deref(),
        Some("OpenDyslexic")
    );
    assert_matches_golden(app.frame(), &golden("reader-layout-opendyslexic.png"), 14);
}

#[test]
fn font_list_back_and_close_return_to_the_text_sheet() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("font-back");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-layout");

    tap_key(&mut app, "layout-font");
    assert!(
        app.find_rect("reader-layout-font-0").is_some(),
        "the font list is open"
    );
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    assert!(
        app.find_rect("reader-layout-font-0").is_none(),
        "Back closes the font list"
    );
    assert!(
        app.find_rect("layout-font").is_some(),
        "Back shows the Text and layout sheet"
    );

    tap_key(&mut app, "layout-font");
    assert!(
        app.find_rect("reader-layout-font-0").is_some(),
        "the font list is open"
    );
    tap_key(&mut app, "panel-close");
    assert!(
        app.find_rect("reader-layout-font-0").is_none(),
        "the close button closes the list"
    );
    assert!(
        app.find_rect("layout-font").is_some(),
        "the close button shows the sheet"
    );
    assert_eq!(
        saved_font_family(&mut app, "juliet.epub"),
        None,
        "no face was picked"
    );
}

#[test]
fn font_list_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-font-list");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-layout");
    tap_key(&mut app, "layout-font");
    assert_matches_golden(app.frame(), &golden("reader-font-list.png"), 14);
}

#[test]
fn hold_selects_a_word_and_the_highlight_survives_reopening() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("highlight");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel), "the popover opens");
    assert!(app.find_rect("color-yellow").is_some());
    tap_key(&mut app, "color-yellow");
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));

    // Leave the book and open it again. The highlight is in the Highlights tab.
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
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
    wait_reader(&mut app);
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
    wait_reader(&mut app);
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
    wait_reader(&mut app);
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    assert_eq!(position(&mut app).0, 2);
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);

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
    wait_reader(&mut app);
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
    assert_matches_golden(app.frame(), &golden("reader.png"), 14);
}

#[test]
fn overlay_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-overlay");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    assert_matches_golden(app.frame(), &golden("reader-overlay.png"), 14);
}

#[test]
fn contents_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-contents");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-contents");
    assert_matches_golden(app.frame(), &golden("reader-contents.png"), 14);
}

#[test]
fn text_and_layout_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-layout");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-layout");
    assert_matches_golden(app.frame(), &golden("reader-layout.png"), 14);
}

#[test]
fn selection_popover_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("golden-selection");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    assert_matches_golden(app.frame(), &golden("reader-selection.png"), 14);
}

#[test]
fn selection_panel_takes_a_quarter_of_the_screen_and_moves_up_for_a_low_word() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("selection-edge");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    let screen = app.screen();

    // A word in the upper half: the panel sits at the bottom.
    hold_at(&mut app, word_point());
    let more = app.find_rect("sel-more").expect("the panel shows More");
    assert!(
        more.y >= screen.h as i32 * 3 / 4,
        "the bottom panel is in the lower quarter, got y {}",
        more.y
    );
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);

    // A word in the lower half: the panel moves to the top and stays above the word.
    let low = Point::new(200, screen.h as i32 * 3 / 5);
    hold_at(&mut app, low);
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel));
    let more = app.find_rect("sel-more").expect("the panel shows More");
    assert!(
        more.y + more.h as i32 <= screen.h as i32 / 4,
        "the top panel ends in the top quarter, got bottom {}",
        more.y + more.h as i32
    );
    assert!(
        more.y + more.h as i32 <= low.y,
        "the panel does not cover the word"
    );
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
    assert_matches_golden(app.frame(), &golden("reader-dictionary.png"), 14);
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
    assert_matches_golden(app.frame(), &golden("reader-search.png"), 14);
}

#[test]
fn named_page_events_cross_the_overlay_but_page_keys_move_focus_first() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("overlay-page-routing");
    let mut app = open_reader("juliet.epub", &dir);
    open_overlay(&mut app);
    let page = slider_value(&mut app);
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    assert_eq!(
        slider_value(&mut app),
        page,
        "a focusable overlay gets the key first"
    );
    assert!(app.send(kaia_events::Event::new("PageForward")));
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
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
    wait_reader(&mut app);
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
    wait_reader(&mut app);
    let saved = fs::read(&state).expect("a page turn saves after five seconds");
    assert_ne!(initial, saved);
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    app.command("sleep", &[]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    let sleeping = fs::read(&state).unwrap();
    assert_ne!(saved, sleeping, "suspend saves immediately");
    app.key(Key::Power).unwrap();
    wait_reader(&mut app);
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    app.shutdown();
    assert_ne!(sleeping, fs::read(&state).unwrap(), "shutdown saves immediately");
    let mut opts = kaia_book::OpenOptions::new(dir.join("data"));
    opts.viewport = app.screen();
    app.update_top::<PageView, _>("reader-page", |view, _| {
        opts.viewport.h -= view.footer_height;
    });
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
    app.services_mut().settings.save("sync.auto", Value::Bool(true));
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
    wait_reader(&mut app);
    assert_eq!(position(&mut app).0, 0);
    assert!(!app.services().sync.busy(), "Never does not push during pull");
    app.services_mut()
        .settings
        .save("sync.forward", Value::from("prompt"));
    app.broadcast_app(remote.clone());
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    assert!(app.find_rect("dialog-button:reader:sync:accept").is_some());
    tap_key(&mut app, "dialog-button:reader:sync:accept");
    assert_eq!(position(&mut app).0, 1);
    app.command("reader:location", &[Value::Number(0.0)]);
    app.services_mut()
        .settings
        .save("sync.forward", Value::from("silent"));
    app.broadcast_app(remote);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    assert_eq!(position(&mut app).0, 1);
    app.services_mut()
        .settings
        .save("sync.enabled", Value::Bool(false));
}

fn stored_stickers(dir: &Path) -> Vec<kaia_storage::Sticker> {
    kaia_storage::Store::open(&dir.join("data/kaia"))
        .unwrap()
        .list_all()
        .unwrap()
}

fn page_stickers(app: &mut App<HeadlessBackend>) -> Vec<kaia_app::surfaces::reader::stickers::Drawing> {
    wait_reader(app);
    let mut stickers = Vec::new();
    assert!(app.update_top::<PageView, _>("reader-page", |v, _| stickers = v.stickers.clone()));
    stickers
}

#[test]
fn sticker_picker_page_and_sheet_goldens_and_reopen() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sticker-goldens");
    let mut app = open_reader("juliet.epub", &dir);
    app.command("reader:layout:margins", &[kaia_events::Value::Number(160.0)]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    turn_to_text(&mut app);
    hold_at(&mut app, Point::new(240, 190));
    tap_key(&mut app, "sel-more");
    tap_key(&mut app, "sel-sticker");
    assert_matches_golden(app.frame(), &golden("reader-sticker-picker.png"), 14);
    tap_key(&mut app, "sticker-pick-star");
    let records = stored_stickers(&dir);
    assert_eq!(
        records.len(),
        1,
        "the picker saved a sticker: {:?}",
        app.notices()
    );
    let sticker = records[0].clone();
    let kaia_storage::Anchor::Reflowable { locator, quote, .. } = &sticker.anchor else {
        panic!("EPUB anchor");
    };
    assert!(locator.starts_with('/'));
    assert!(!quote.exact.is_empty());
    assert!(!quote.prefix.is_empty() || !quote.suffix.is_empty());
    assert_eq!(sticker.state, kaia_storage::StickerState::Attached);
    let drawings = page_stickers(&mut app);
    assert_eq!(drawings.len(), 1);
    assert!(!drawings[0].faded, "a wide margin holds the image");
    assert_matches_golden(app.frame(), &golden("reader-margin-stickers.png"), 14);
    let rect = drawings[0].rect;
    app.tap(Point::new(rect.x + rect.w as i32 / 2, rect.y + rect.h as i32 / 2))
        .unwrap();
    wait_reader(&mut app);
    assert!(app.find_rect("sticker-view").is_some());
    assert_matches_golden(app.frame(), &golden("reader-sticker-sheet.png"), 14);
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    open_from_library(&mut app, "juliet.epub");
    assert_eq!(page_stickers(&mut app)[0].id, sticker.id.clone().unwrap());
    let before = page_stickers(&mut app)[0].anchor_line;
    app.command("reader:layout:size", &[kaia_events::Value::Number(60.0)]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    app.command("reader:location", &[kaia_events::Value::from(locator.clone())]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    let after = page_stickers(&mut app);
    assert_eq!(after.len(), 1, "reflow keeps the sticker with its text");
    assert_ne!(before, after[0].anchor_line);
    assert_eq!(
        stored_stickers(&dir)[0],
        sticker,
        "layout changes do not change stored anchors or timestamps"
    );
}

#[test]
fn sticker_needs_repair_and_notes_can_place_it_again() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sticker-repair");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-more");
    tap_key(&mut app, "sel-sticker");
    tap_key(&mut app, "sticker-pick-heart");
    let mut sticker = stored_stickers(&dir).remove(0);
    let id = sticker.id.clone().unwrap();
    if let kaia_storage::Anchor::Reflowable { locator, quote, .. } = &mut sticker.anchor {
        *locator = "/body/removed/p.0".into();
        quote.prefix = quote.prefix.trim().to_string();
        quote.suffix = quote.suffix.trim().to_string();
    }
    let mut store = kaia_storage::Store::open(&dir.join("data/kaia")).unwrap();
    store.put_sticker(&sticker).unwrap();
    app.command("reader:slider:release", &[]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    assert_eq!(
        page_stickers(&mut app).len(),
        1,
        "quote lookup repairs a stale Lua locator for drawing"
    );
    assert_eq!(store.get_sticker(&id).unwrap().anchor, sticker.anchor);
    if let kaia_storage::Anchor::Reflowable { quote, .. } = &mut sticker.anchor {
        quote.exact = "This quote was removed from the book entirely 987654321".into();
    }
    let mut store = kaia_storage::Store::open(&dir.join("data/kaia")).unwrap();
    store.put_sticker(&sticker).unwrap();
    app.command("reader:slider:release", &[]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    assert!(page_stickers(&mut app).is_empty());
    assert_eq!(
        store.get_sticker(&id).unwrap().state,
        kaia_storage::StickerState::NeedsReview
    );
    app.command("nav:notes", &[]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    let books = kaia_app::surfaces::notes::load_books(app.services());
    assert!(books[0].notes[0].orphaned);
    assert_matches_golden(app.frame(), &golden("notes-sticker-repair.png"), 14);
    assert_eq!(books[0].notes[0].sticker.as_deref(), Some(id.as_str()));
    tap_key(&mut app, "notes-entry-1");
    assert!(matches!(app.top_route(), Some(Route::Reader { .. })));
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-more");
    tap_key(&mut app, "sel-sticker");
    let repaired = store.get_sticker(&id).unwrap();
    assert_eq!(repaired.state, kaia_storage::StickerState::Attached);
    assert_ne!(repaired.anchor, sticker.anchor);
    assert_eq!(page_stickers(&mut app)[0].id, id);
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    assert_eq!(app.top_route(), Some(&Route::Notes));
    assert!(
        app.find_rect("notes-entry-1").is_none(),
        "the repair group refreshes after leaving the reader"
    );
}

#[test]
fn sticker_text_note_and_shared_selection_are_saved_and_editable() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sticker-note-share");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    let quote = text_of(&mut app, "sel-word");
    tap_key(&mut app, "sel-more");
    tap_key(&mut app, "sel-share");
    let books = kaia_app::surfaces::notes::load_books(app.services());
    assert_eq!(books[0].notes[0].note.as_deref(), Some("Shared to Notes"));
    assert_eq!(books[0].notes[0].text.as_deref(), Some(quote.as_str()));
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-more");
    tap_key(&mut app, "sel-sticker");
    tap_key(&mut app, "sticker-text");
    app.update_top::<TextField, _>("sticker-note-field", |f, _| f.set_text("Read this again"));
    tap_key(&mut app, "sticker-note-save");
    let sticker = stored_stickers(&dir).remove(0);
    assert_eq!(sticker.text.as_deref(), Some("Read this again"));
    assert!(
        page_stickers(&mut app)[0].marker,
        "a narrow margin uses a note marker"
    );
    app.command(
        &format!("reader:sticker:open:{}", sticker.id.as_deref().unwrap()),
        &[],
    );
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "sticker-edit");
    app.update_top::<TextField, _>("sticker-note-field", |f, _| f.set_text("Read chapter two"));
    tap_key(&mut app, "sticker-note-save");
    assert_eq!(stored_stickers(&dir)[0].text.as_deref(), Some("Read chapter two"));
    app.command(
        &format!("reader:sticker:open:{}", sticker.id.as_deref().unwrap()),
        &[],
    );
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "sticker-move");
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-more");
    tap_key(&mut app, "sel-sticker");
    let moved = stored_stickers(&dir).remove(0);
    assert_eq!(moved.id, sticker.id);
    assert_ne!(moved.anchor, sticker.anchor);
    assert_eq!(moved.text.as_deref(), Some("Read chapter two"));

    app.command(
        &format!("reader:sticker:open:{}", sticker.id.as_deref().unwrap()),
        &[],
    );
    app.process(Vec::new()).unwrap();
    tap_key(&mut app, "sticker-delete");
    assert!(stored_stickers(&dir).is_empty());
    assert!(page_stickers(&mut app).is_empty());
}

#[test]
fn pdf_sticker_uses_page_fractions_and_shared_store_images() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sticker-pdf");
    let mut app = open_reader("2col.pdf", &dir);
    let row = app
        .services()
        .library
        .book(book_id(&app, "2col.pdf"))
        .unwrap()
        .unwrap();
    let mut store = kaia_storage::Store::open(&dir.join("data/kaia")).unwrap();
    let asset = store
        .add_asset_file(&repo_root().join("resources/kaia/stickers/check.png"), "")
        .unwrap();
    // This is the same record shape the Lua FFI passes to the store.
    let sticker = store
        .put_sticker(&kaia_storage::Sticker {
            id: None,
            doc: kaia_storage::DocumentId {
                partial_md5: row.partial_md5.clone(),
                file_size: fs::metadata(&row.path).unwrap().len(),
                title: row.title,
            },
            anchor: kaia_storage::Anchor::Fixed {
                page: 1,
                x: 0.25,
                y: 0.4,
            },
            asset: Some(asset.sha256),
            text: None,
            size: kaia_storage::StickerSize { w: 0.07, h: 0.05 },
            rotation: 0,
            state: kaia_storage::StickerState::Attached,
            created_at: 0,
            updated_at: 0,
        })
        .unwrap();
    app.command("reader:slider:release", &[]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    let before = page_stickers(&mut app);
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].id, sticker.id.unwrap());
    app.command("reader:layout:zoom", &[kaia_events::Value::Number(1.0)]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    let zoomed = page_stickers(&mut app);
    assert_eq!(zoomed.len(), 1);
    assert!(zoomed[0].rect.h > before[0].rect.h);
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    let panned = page_stickers(&mut app);
    assert_eq!(panned.len(), 1, "panning stays on the sticker's page");
    assert!(panned[0].rect.y < zoomed[0].rect.y);
    assert_eq!(panned[0].rect.h, zoomed[0].rect.h);
    app.key(Key::RPgFwd).unwrap();
    wait_reader(&mut app);
    assert!(
        page_stickers(&mut app).is_empty(),
        "a sticker belongs only to its page"
    );
}

#[test]
fn pdf_selection_creates_a_sticker_and_reopens_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sticker-pdf-selection");
    let mut app = open_reader("2col.pdf", &dir);
    let path = dir.join("books/2col.pdf");
    let doc = kaia_doc::open(&path).unwrap();
    let size = doc.page_size(0).unwrap();
    let mut screen = app.screen();
    app.update_top::<PageView, _>("reader-page", |view, _| {
        screen.h -= view.footer_height;
    });
    let scale = (screen.w as f32 / size.width).min(screen.h as f32 / size.height);
    // This scanned PDF has no text layer. A hold can still place a page sticker.
    assert!(doc.page_text(0).unwrap().chars.is_empty());
    let point = Point::new(350, 500);
    let x_expected = (point.x as f32 - (screen.w as f32 - size.width * scale) / 2.0) / scale / size.width;
    let y_expected = (point.y as f32 - (screen.h as f32 - size.height * scale) / 2.0) / scale / size.height;
    hold_at(&mut app, point);
    // A scanned page has no words, so the sheet offers Sticker at once.
    tap_key(&mut app, "sel-sticker");
    tap_key(&mut app, "sticker-pick-question");
    let sticker = stored_stickers(&dir).remove(0);
    let kaia_storage::Anchor::Fixed { page, x, y } = sticker.anchor else {
        panic!("PDF anchor");
    };
    assert_eq!(page, 1);
    assert!((x - x_expected).abs() < 0.01);
    assert!((y - y_expected).abs() < 0.01);
    assert_eq!(page_stickers(&mut app).len(), 1);
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    open_from_library(&mut app, "2col.pdf");
    assert_eq!(page_stickers(&mut app)[0].id, sticker.id.unwrap());
}

#[test]
fn sticker_fades_over_the_line_end_when_the_margin_is_narrow() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sticker-faded");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-more");
    tap_key(&mut app, "sel-sticker");
    tap_key(&mut app, "sticker-pick-star");
    let sticker = stored_stickers(&dir).remove(0);
    let kaia_storage::Anchor::Reflowable { locator, .. } = &sticker.anchor else {
        panic!("EPUB");
    };
    app.command("reader:layout:styles", &[kaia_events::Value::Bool(false)]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    app.command("reader:layout:margins", &[kaia_events::Value::Number(0.0)]);
    app.process(Vec::new()).unwrap();
    wait_reader(&mut app);
    let mut faded = None;
    for size in [32.0, 40.0, 48.0, 56.0, 64.0] {
        app.command("reader:layout:size", &[kaia_events::Value::Number(size)]);
        app.process(Vec::new()).unwrap();
        wait_reader(&mut app);
        app.command("reader:location", &[kaia_events::Value::from(locator.clone())]);
        app.process(Vec::new()).unwrap();
        wait_reader(&mut app);
        if let Some(drawing) = page_stickers(&mut app).into_iter().find(|d| d.faded) {
            faded = Some(drawing);
            break;
        }
    }
    let drawing = faded.expect("a full text column uses the faded fallback");
    assert!(drawing.rect.x >= drawing.anchor_line.x);
    assert!(
        drawing.rect.x + drawing.rect.w as i32 <= drawing.anchor_line.x + drawing.anchor_line.w as i32 + 1
    );
    assert!(drawing
        .image
        .as_ref()
        .unwrap()
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .all(|pixel| pixel[3] <= 102));
    assert_eq!(stored_stickers(&dir)[0], sticker);
}
