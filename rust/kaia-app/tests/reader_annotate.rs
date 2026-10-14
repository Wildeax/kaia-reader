//! Highlight colors and drawer styles, the dictionary order and per-book dictionary,
//! and the word list, through the reader's sheets. The settings files and the book's
//! state file are read back, so the tests check what reaches the disk.

use std::fs;
use std::path::Path;
use std::time::Duration;

use kaia_app::surfaces::reader::panels::WordRow;
use kaia_app::App;
use kaia_app::HeadlessBackend;
use kaia_app::Route;
use kaia_display::InputEvent;
use kaia_events::{Settings, Value};
use kaia_gfx::Point;
use kaia_input::{Key, Phase, TouchEvent};
use kaia_storage::Store;
use kaia_ui::components::{Label, Paragraph};
use kaia_ui::composite::PagedList;

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

/// A point on a word of the text page that `turn_to_text` shows.
fn word_point() -> Point {
    Point::new(200, 120)
}

/// Centre of the screen, for a tap that opens the overlay.
fn centre(app: &App<HeadlessBackend>) -> Point {
    Point::new(app.screen().w as i32 / 2, app.screen().h as i32 / 2)
}

/// A hold at `p`, which selects the word there and opens the selection popover.
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

fn text_of(app: &mut App<HeadlessBackend>, key: &str) -> String {
    let mut text = None;
    app.update_top::<Paragraph, _>(key, |p, _| text = Some(p.text().to_string()));
    if text.is_none() {
        app.update_top::<Label, _>(key, |l, _| text = Some(l.text().to_string()));
    }
    text.unwrap_or_else(|| panic!("no text widget {key:?} on the top window"))
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

/// Writes one StarDict dictionary called `stem` into `<data>/dict`, with `bookname`
/// as its name and one entry for `word`.
fn write_dictionary(data_dir: &Path, stem: &str, bookname: &str, word: &str, definition: &str) {
    let dir = data_dir.join("dict");
    fs::create_dir_all(&dir).unwrap();
    let body = definition.as_bytes();
    let mut idx = Vec::new();
    idx.extend_from_slice(word.as_bytes());
    idx.push(0);
    idx.extend_from_slice(&0u32.to_be_bytes());
    idx.extend_from_slice(&(body.len() as u32).to_be_bytes());
    fs::write(dir.join(format!("{stem}.dict")), body).unwrap();
    fs::write(dir.join(format!("{stem}.idx")), &idx).unwrap();
    let ifo = format!(
        "StarDict's dict ifo file\nversion=2.4.2\nbookname={bookname}\nwordcount=1\nidxfilesize={}\nsametypesequence=m\n",
        idx.len()
    );
    fs::write(dir.join(format!("{stem}.ifo")), ifo).unwrap();
}

/// The state file of the one book in the test data folder.
fn book_state(dir: &Path) -> Settings {
    let books = dir.join("data/kaia/books");
    let file = fs::read_dir(&books)
        .expect("the books folder")
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .expect("the book has a state file");
    Settings::read_exact(&file)
        .unwrap()
        .expect("a readable state file")
}

/// The highlights in the book's state file, as (drawer, color) pairs in order.
fn highlights(dir: &Path) -> Vec<(String, String)> {
    let state = book_state(dir);
    let mut out = Vec::new();
    if let Some(table) = state.read("annotations").as_table() {
        for item in table.values() {
            if let Some(drawer) = item.get("drawer").as_str() {
                let color = item.get("color").as_str().unwrap_or("").to_string();
                out.push((drawer.to_string(), color));
            }
        }
    }
    out
}

/// The names in a settings list, in order.
fn names_in(app: &App<HeadlessBackend>, key: &str) -> Vec<String> {
    app.services()
        .settings
        .read(key)
        .as_table()
        .into_iter()
        .flat_map(|t| t.values())
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// The preferred dictionaries that the book's state file holds.
fn preferred_in_state(dir: &Path) -> Vec<String> {
    let state = book_state(dir);
    state
        .read("preferred_dictionaries")
        .as_table()
        .into_iter()
        .flat_map(|t| t.values())
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn string_list(names: &[&str]) -> Value {
    Value::array(names.iter().map(|n| Value::from(*n)))
}

#[test]
fn a_highlight_takes_the_chosen_color_and_style_and_remembers_them() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("annotate-choice");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());

    tap_key(&mut app, "color-green");
    assert_eq!(
        app.top_route(),
        Some(&Route::ReaderPanel),
        "the popover stays open"
    );
    tap_key(&mut app, "style-underscore");
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel));
    assert_eq!(
        app.services().settings.read("reading.highlight_color").as_str(),
        Some("green"),
        "the color is the default for the next highlight"
    );
    assert_eq!(
        app.services().settings.read("reading.highlight_drawer").as_str(),
        Some("underscore")
    );
    let saved = Settings::read_exact(&dir.join("data/settings.json"))
        .unwrap()
        .expect("settings.json");
    assert_eq!(saved.read("reading.highlight_color").as_str(), Some("green"));
    assert_eq!(
        saved.read("reading.highlight_drawer").as_str(),
        Some("underscore")
    );

    tap_key(&mut app, "sel-highlight");
    wait_reader(&mut app);
    assert_eq!(
        highlights(&dir),
        [("underscore".to_string(), "green".to_string())]
    );
}

#[test]
fn a_new_highlight_uses_the_remembered_choice_and_koreader_defaults_without_one() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("annotate-default");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-highlight");
    wait_reader(&mut app);
    let default_color = if app.services().theme.color_panel {
        "yellow"
    } else {
        "gray"
    };
    assert_eq!(
        highlights(&dir),
        [("lighten".to_string(), default_color.to_string())],
        "KOReader's defaults: lighten, and yellow on a color panel or gray on a grayscale one"
    );

    app.services_mut()
        .settings
        .save("reading.highlight_color", Value::from("blue"));
    app.services_mut()
        .settings
        .save("reading.highlight_drawer", Value::from("strikeout"));
    hold_at(&mut app, Point::new(200, 260));
    tap_key(&mut app, "sel-highlight");
    wait_reader(&mut app);
    let all = highlights(&dir);
    assert_eq!(all.len(), 2);
    assert_eq!(all[1], ("strikeout".to_string(), "blue".to_string()));
}

#[test]
fn tapping_a_highlight_changes_its_style_and_keeps_its_color() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("annotate-edit");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    tap_key(&mut app, "color-red");
    tap_key(&mut app, "sel-highlight");
    wait_reader(&mut app);
    assert_eq!(highlights(&dir), [("lighten".to_string(), "red".to_string())]);

    app.tap(word_point()).unwrap();
    wait_reader(&mut app);
    assert_eq!(
        app.top_route(),
        Some(&Route::ReaderPanel),
        "the highlight sheet opens"
    );
    tap_key(&mut app, "style-strikeout");
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel));
    assert_eq!(
        highlights(&dir),
        [("strikeout".to_string(), "red".to_string())],
        "the style changed and the color stayed"
    );
    assert_ne!(
        app.services().settings.read("reading.highlight_drawer").as_str(),
        Some("strikeout"),
        "editing a highlight leaves the default for new ones alone"
    );

    // Invert draws no color, so the swatches have no action while it is chosen.
    tap_key(&mut app, "style-invert");
    tap_key(&mut app, "color-blue");
    assert_eq!(highlights(&dir), [("invert".to_string(), "red".to_string())]);
}

#[test]
fn dictionary_settings_reorder_and_switch_dictionaries_off() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("dict-settings");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    let word = word_of(&text_of(&mut app, "sel-word"));
    write_dictionary(
        &dir.join("data"),
        "test",
        "Test Dictionary",
        &word,
        "first definition",
    );
    write_dictionary(
        &dir.join("data"),
        "second",
        "Second Dictionary",
        &word,
        "second definition",
    );
    tap_key(&mut app, "sel-dict");
    tap_key(&mut app, "dict-settings");
    assert_eq!(app.top_route(), Some(&Route::ReaderPanel));
    assert!(
        app.find_rect("dict-toggle-1").is_some(),
        "both dictionaries are listed"
    );

    tap_key(&mut app, "dict-down-0");
    assert_eq!(
        names_in(&app, "dictionary.order"),
        ["Test Dictionary", "Second Dictionary"],
        "the order is saved in the settings"
    );
    tap_key(&mut app, "dict-toggle-1");
    assert_eq!(
        names_in(&app, "dictionary.disabled"),
        ["Second Dictionary"],
        "a dictionary that is off is saved as disabled"
    );
    let saved = Settings::read_exact(&dir.join("data/settings.json"))
        .unwrap()
        .expect("settings.json");
    assert_eq!(
        saved.read("dictionary.disabled").array_len(),
        1,
        "the settings file holds the disabled list"
    );

    // With Second off, the sheet for the same word shows only Test.
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-dict");
    let body = text_of(&mut app, "dict-body");
    assert!(body.contains("Test Dictionary"), "got {body:?}");
    assert!(!body.contains("Second Dictionary"), "got {body:?}");
}

#[test]
fn a_book_can_use_one_dictionary_first_and_keeps_that_after_a_reopen() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("dict-book");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    let word = word_of(&text_of(&mut app, "sel-word"));
    write_dictionary(
        &dir.join("data"),
        "test",
        "Test Dictionary",
        &word,
        "first definition",
    );
    write_dictionary(
        &dir.join("data"),
        "second",
        "Second Dictionary",
        &word,
        "second definition",
    );
    app.services_mut().settings.save(
        "dictionary.order",
        string_list(&["Test Dictionary", "Second Dictionary"]),
    );
    tap_key(&mut app, "sel-dict");
    assert!(
        text_of(&mut app, "dict-body").starts_with(&format!("{word} (Test Dictionary)")),
        "the global order puts Test first"
    );

    tap_key(&mut app, "dict-first");
    assert_eq!(preferred_in_state(&dir), ["Test Dictionary"]);

    // The settings now put Second first. The book's choice still puts Test first.
    app.services_mut().settings.save(
        "dictionary.order",
        string_list(&["Second Dictionary", "Test Dictionary"]),
    );
    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-dict");
    let body = text_of(&mut app, "dict-body");
    let test_at = body.find("Test Dictionary").expect("Test is shown");
    let second_at = body.find("Second Dictionary").expect("Second is shown");
    assert!(
        test_at < second_at,
        "the book's dictionary comes first, got {body:?}"
    );

    // Using it again stops the preference.
    tap_key(&mut app, "dict-first");
    assert!(preferred_in_state(&dir).is_empty());
}

#[test]
fn a_word_is_saved_with_its_book_and_sentence_and_can_be_deleted_from_the_word_list() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("word-list");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    let word = word_of(&text_of(&mut app, "sel-word"));
    write_dictionary(
        &dir.join("data"),
        "test",
        "Test Dictionary",
        &word,
        "a definition",
    );
    tap_key(&mut app, "sel-dict");
    tap_key(&mut app, "dict-add-word");

    let store = Store::open(&dir.join("data/kaia")).unwrap();
    let words = store.list_words().unwrap();
    assert_eq!(words.len(), 1, "one word is saved");
    assert!(words[0].word.eq_ignore_ascii_case(&word));
    assert!(
        words[0].book_title.as_deref().is_some_and(|t| !t.is_empty()),
        "the book title is saved"
    );
    let context = words[0].context.clone().expect("the sentence is saved");
    assert!(
        context.to_lowercase().contains(&word.to_lowercase()),
        "the sentence holds the word, got {context:?}"
    );
    assert!(words[0].created_at > 0);
    let id = words[0].id;

    app.key(Key::Back).unwrap();
    wait_reader(&mut app);
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-more");
    tap_key(&mut app, "more-words");
    assert!(
        app.find_rect(&format!("word-row-{id}")).is_some(),
        "the word is listed"
    );
    tap_key(&mut app, &format!("word-delete-{id}"));
    assert!(
        app.find_rect(&format!("word-row-{id}")).is_none(),
        "the row is gone"
    );
    let store = Store::open(&dir.join("data/kaia")).unwrap();
    assert!(store.list_words().unwrap().is_empty(), "the word is deleted");
}

#[test]
fn the_word_list_shows_the_newest_word_first() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("word-order");
    let mut app = open_reader("juliet.epub", &dir);
    {
        let mut store = Store::open(&dir.join("data/kaia")).unwrap();
        store.add_word("older", Some("Book"), None, 1_000).unwrap();
        store
            .add_word("newest", None, Some("A sentence."), 3_000)
            .unwrap();
        store.add_word("middle", Some("Book"), None, 2_000).unwrap();
    }
    open_overlay(&mut app);
    tap_key(&mut app, "overlay-more");
    tap_key(&mut app, "more-words");
    let mut words = Vec::new();
    app.update_top::<PagedList<WordRow>, _>("words-list", |list, _| {
        words = list.items().iter().map(|row| row.word.clone()).collect();
    });
    assert_eq!(words, ["newest", "middle", "older"]);
}

/// Opens the overlay with a centre tap and checks it is up.
fn open_overlay(app: &mut App<HeadlessBackend>) {
    let p = centre(app);
    app.tap(p).unwrap();
    wait_reader(app);
    assert_eq!(app.top_route(), Some(&Route::Overlay));
}
