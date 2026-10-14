//! Headless tests for the Dictionary settings page, which Settings opens from the
//! Reading section. They cover the row that opens the page, the on or off switches
//! and the move buttons, and the lookup order the reader uses afterwards. The
//! settings file is read back, so the tests check what reaches the disk.

use std::fs;
use std::path::Path;
use std::time::Duration;

use kaia_app::{App, HeadlessBackend, Route};
use kaia_events::{Settings, Value};
use kaia_gfx::Point;
use kaia_input::Key;
use kaia_ui::components::Paragraph;
use kaia_ui::composite::ListRow;

mod common;

use common::*;

const ALPHA: &str = "Alpha Dictionary";
const BETA: &str = "Beta Dictionary";

/// Writes one StarDict dictionary called `bookname` into `<data>/dict`, with one
/// entry for `word`.
fn write_dictionary(data: &Path, stem: &str, bookname: &str, word: &str, definition: &str) {
    let dir = data.join("dict");
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

/// Two dictionaries that both define `reader`. Without a saved order, Alpha sorts
/// first by name.
fn write_two_dictionaries(data: &Path) {
    write_dictionary(data, "alpha", ALPHA, "reader", "alpha definition");
    write_dictionary(data, "beta", BETA, "reader", "beta definition");
}

/// An app with the test books in its library, scanned and ready.
fn app_with_books(dir: &Path) -> App<HeadlessBackend> {
    let lib = library_with_books(dir);
    let mut app = make_app(dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    app
}

/// The names saved under `key` in the settings file at `path`, in order.
fn saved_names(path: &Path, key: &str) -> Vec<String> {
    let settings = Settings::open(path);
    settings
        .read(key)
        .as_table()
        .into_iter()
        .flat_map(|table| table.values())
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// Whether the switch with `key` on the top window is on.
fn switch_is_on(app: &mut App<HeadlessBackend>, key: &str) -> bool {
    let mut on = None;
    app.update_top::<ListRow, _>(key, |row, _| on = Some(row.is_on()));
    on.unwrap_or_else(|| panic!("no switch {key:?} on the top window"))
}

/// The text of the paragraph `key` on the top window.
fn text_of(app: &mut App<HeadlessBackend>, key: &str) -> String {
    let mut text = None;
    app.update_top::<Paragraph, _>(key, |p, _| text = Some(p.text().to_string()));
    text.unwrap_or_else(|| panic!("no text {key:?} on the top window"))
}

/// Turns the pager of the list `list` to its next page. The Next button is drawn by
/// the pager and is not a widget, so this taps where the pager draws it.
fn next_page(app: &mut App<HeadlessBackend>, list: &str) {
    wait_reader(app);
    let rect = app
        .find_rect(list)
        .unwrap_or_else(|| panic!("no list {list:?} on the top window"));
    let touch = app.services().theme.touch_target as i32;
    let side = app.services().theme.space.sm as i32;
    let button_w = (3 * touch).min(rect.w as i32 / 3);
    let strip_top = rect.y + rect.h as i32 - touch - side;
    let at = Point::new(rect.x + rect.w as i32 - button_w / 2, strip_top + touch / 2);
    app.tap(at).unwrap();
    wait_reader(app);
}

/// Opens the Dictionary settings page: Settings, Reading, the second page of
/// Reading (where the row is), then the row.
fn open_dictionary_settings(app: &mut App<HeadlessBackend>) {
    tap_key(app, "nav-settings");
    tap_key(app, "settings-reading");
    next_page(app, "settings-entries");
    tap_key(app, "settings:dictionaries");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "dictionaries".into()
        })
    );
}

/// Goes back from the Dictionary settings page to the Settings root, then to the
/// Library, where a book can be opened.
fn leave_settings(app: &mut App<HeadlessBackend>) {
    app.key(Key::Back).unwrap();
    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Settings));
    tap_key(app, "nav-library");
}

/// Opens `book` from the Library and asks the reader for `word`. Returns the text
/// of the dictionary sheet.
fn lookup(app: &mut App<HeadlessBackend>, book: &str, word: &str) -> String {
    open_from_library(app, book);
    app.command("reader:dictionary-word", &[Value::from(word)]);
    wait_reader(app);
    text_of(app, "dict-body")
}

#[test]
fn the_page_saves_the_order_and_the_switches_to_settings() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-dictionaries-save");
    write_two_dictionaries(&dir.join("data"));
    let mut app = app_with_books(&dir);
    let path = app.services().settings_path();
    open_dictionary_settings(&mut app);

    // The first dictionary cannot move up, so nothing is saved.
    tap_key(&mut app, "dict-up-0");
    assert!(
        saved_names(&path, "dictionary.order").is_empty(),
        "no move, no save"
    );

    // Alpha moves down, so Beta is first.
    tap_key(&mut app, "dict-down-0");
    assert_eq!(saved_names(&path, "dictionary.order"), [BETA, ALPHA]);

    // Alpha is second now. Switching it off saves it as disabled.
    tap_key(&mut app, "dict-toggle-1");
    assert_eq!(saved_names(&path, "dictionary.disabled"), [ALPHA]);
    assert!(switch_is_on(&mut app, "dict-toggle-0"), "Beta stays on");
    assert!(!switch_is_on(&mut app, "dict-toggle-1"), "Alpha shows off");

    // Switching Alpha back on clears the off list.
    tap_key(&mut app, "dict-toggle-1");
    assert!(saved_names(&path, "dictionary.disabled").is_empty());
    assert!(switch_is_on(&mut app, "dict-toggle-1"), "Alpha shows on");
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn the_reader_looks_words_up_in_the_order_set_in_settings() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-dictionaries-order");
    write_two_dictionaries(&dir.join("data"));
    let mut app = app_with_books(&dir);
    open_dictionary_settings(&mut app);
    tap_key(&mut app, "dict-down-0");
    leave_settings(&mut app);

    let body = lookup(&mut app, "juliet.epub", "reader");
    let beta = body.find(BETA).expect("Beta is in the sheet");
    let alpha = body.find(ALPHA).expect("Alpha is in the sheet");
    assert!(beta < alpha, "Beta comes first in the sheet, got {body:?}");
}

#[test]
fn the_reader_skips_a_dictionary_that_is_switched_off_in_settings() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-dictionaries-off");
    write_two_dictionaries(&dir.join("data"));
    let mut app = app_with_books(&dir);
    open_dictionary_settings(&mut app);
    // Without a saved order Beta is second, so this switches Beta off.
    tap_key(&mut app, "dict-toggle-1");
    leave_settings(&mut app);

    let body = lookup(&mut app, "juliet.epub", "reader");
    assert!(body.contains("alpha definition"), "got {body:?}");
    assert!(
        !body.contains(BETA),
        "the off dictionary is skipped, got {body:?}"
    );
}

#[test]
fn without_a_dictionary_the_reading_page_has_no_dictionary_settings_row() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-dictionaries-none");
    let mut app = app_with_books(&dir);
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-reading");
    next_page(&mut app, "settings-entries");

    assert!(
        app.find_rect("Dictionary folder").is_some(),
        "the folder row stays"
    );
    assert!(
        app.find_rect("settings:dictionaries").is_none(),
        "no dictionary, no settings row"
    );
}

fn write_kobo(root: &Path, protected: bool) {
    use std::io::Write;
    fs::create_dir_all(root).unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(root.join("dicthtml-en.zip")).unwrap());
    zip.start_file("re.html", zip::write::SimpleFileOptions::default())
        .unwrap();
    if protected {
        zip.write_all(&[0xa5; 32]).unwrap();
    } else {
        zip.write_all(b"<html><w><a name=\"reader\" /><p>Kobo definition</p></w></html>")
            .unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn kobo_and_stardict_share_order_switches_and_book_preference() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-kobo-dictionaries");
    write_two_dictionaries(&dir.join("data"));
    let mut app = app_with_books(&dir);
    let kobo = app.services().paths.books_dir.join(".kobo/dict");
    write_kobo(&kobo, false);
    // A second StarDict source must also load, even when Kaia's folder has entries.
    write_dictionary(
        &app.services().paths.books_dir.join(".adds/koreader/data"),
        "extra",
        "Other source",
        "reader",
        "Other definition",
    );
    app.services_mut().dictionary = None;
    open_dictionary_settings(&mut app);
    assert_eq!(app.services().dictionary.as_ref().unwrap().names().len(), 4);
    app.command("settings:dicts:up:2", &[]);
    app.command("settings:dicts:up:1", &[]);
    assert_eq!(
        saved_names(&app.services().settings_path(), "dictionary.order")[0],
        "Kobo en"
    );
    app.command("settings:dicts:toggle:0", &[]);
    assert_eq!(
        saved_names(&app.services().settings_path(), "dictionary.disabled"),
        ["Kobo en"]
    );
    app.command("settings:dicts:toggle:0", &[]);
    app.iterate(Some(Duration::ZERO)).unwrap();
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden("settings_kobo_dictionaries.png"), 14);
    leave_settings(&mut app);
    let body = lookup(&mut app, "juliet.epub", "reader");
    assert!(body.find("Kobo definition").unwrap() < body.find("alpha definition").unwrap());
    app.command("reader:dict:first", &[]);
    app.services_mut()
        .settings
        .save("dictionary.disabled", Value::array([Value::from("Kobo en")]));
    app.key(Key::Back).unwrap();
    app.command("reader:dictionary-word", &[Value::from("reader")]);
    wait_reader(&mut app);
    assert!(text_of(&mut app, "dict-body").contains("Kobo definition"));
    // Removing the book preference restores the global off switch.
    app.command("reader:dict:first", &[]);
    wait_reader(&mut app);
    assert!(!text_of(&mut app, "dict-body").contains("Kobo definition"));
}

#[test]
fn encrypted_kobo_note_is_visible_when_no_dictionary_opens() {
    use kaia_app::surfaces::settings::{entries::Entry, sections};
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-kobo-encrypted");
    let mut app = app_with_books(&dir);
    write_kobo(&app.services().paths.books_dir.join(".kobo/dict"), true);
    app.services_mut().dictionary = None;
    open_dictionary_settings(&mut app);
    assert!(sections::entries("dictionaries", app.services(), 0)
        .iter()
        .any(|entry| matches!(entry, Entry::Text(text) if text.contains("Encrypted or unsupported"))));
}

#[test]
fn dictionary_download_page_requires_opt_in_and_airplane_blocks_requests() {
    use kaia_app::dictionary_downloads::ALLOW_KEY;
    use kaia_app::surfaces::settings::{entries::Entry, keys, sections};
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-dictionary-downloads");
    let mut app = app_with_books(&dir);
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-reading");
    assert!(!kaia_app::dictionary_downloads::allowed(&app.services().settings));
    assert!(sections::entries("dictionary-downloads", app.services(), 0).is_empty());
    let route = app.top_route().cloned();
    app.command("settings:dictionary-downloads", &[]);
    assert_eq!(app.top_route(), route.as_ref());
    app.command(&format!("settings:set:{ALLOW_KEY}"), &[Value::Bool(true)]);
    assert!(Settings::open(&app.services().settings_path())
        .read(ALLOW_KEY)
        .as_bool()
        .unwrap());
    assert!(sections::entries("reading", app.services(), 0).iter().any(|entry| matches!(entry, Entry::Row { action: Some(action), .. } if action == "settings:dictionary-downloads")));
    app.command("settings:dictionary-downloads", &[]);
    app.iterate(Some(Duration::ZERO)).unwrap();
    kaia_gfx::golden::assert_matches_golden(app.frame(), &golden("settings_dictionary_downloads.png"), 14);
    app.services_mut()
        .settings
        .save(keys::NETWORK_AIRPLANE_MODE, Value::Bool(true));
    let id = &kaia_dict::download::catalog()[0].id;
    app.command(&format!("settings:dictionary-install:{id}"), &[]);
    assert!(!app.services().dictionary_downloads.busy());
    assert!(app
        .notices()
        .iter()
        .any(|s| s == "Airplane mode is on. Turn it off to use Wi-Fi."));
    assert!(!dir.join("data/dict/.partial").exists());
    app.command(&format!("settings:set:{ALLOW_KEY}"), &[Value::Bool(false)]);
    app.services_mut()
        .settings
        .save(keys::NETWORK_AIRPLANE_MODE, Value::Bool(false));
    app.command(&format!("settings:dictionary-install:{id}"), &[]);
    assert!(!app.services().dictionary_downloads.busy());
}
