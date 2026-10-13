//! Headless tests for the Notes and Settings surfaces: navigation, the notes
//! list and its filters, export, the settings controls and their persistence,
//! the sync test button, the statistics section and the quick settings and
//! sleep screen builders. Goldens live in `tests/golden/`.

mod common;

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;

use common::*;
use kaia_app::surfaces::notes;
use kaia_app::surfaces::settings::entries::Entry;
use kaia_app::surfaces::settings::{keys, quick, section, sections, sleep};
use kaia_app::{App, HeadlessBackend, Route};
use kaia_book::state::{STATE_FORMAT, STATE_VERSION};
use kaia_events::value::{Key as TableKey, Table, Value};
use kaia_events::Settings;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Pixmap;
use kaia_id::partial_md5;
use kaia_input::Key;
use kaia_stats::Stats;
use kaia_ui::{render, Widget};

/// The clock the statistics goldens use. Fixture times are relative to it.
const FIXED_NOW: i64 = 1_700_000_000;

fn table(pairs: Vec<(&str, Value)>) -> Table {
    pairs.into_iter().map(|(k, v)| (TableKey::from(k), v)).collect()
}

/// One annotation table, as `kaia-book` writes it. `page` is an xpointer
/// string or a 1-based page number. A bookmark has no drawer.
fn annotation(
    id: i64,
    datetime: &str,
    page: Value,
    pageno: i64,
    drawer: Option<&str>,
    text: Option<&str>,
    note: Option<&str>,
) -> Value {
    let mut pairs = vec![
        ("kaia_id", Value::from(id)),
        ("datetime", Value::from(datetime)),
        ("page", page),
        ("pageno", Value::from(pageno)),
    ];
    if let Some(drawer) = drawer {
        pairs.push(("drawer", Value::from(drawer)));
    }
    if let Some(text) = text {
        pairs.push(("text", Value::from(text)));
    }
    if let Some(note) = note {
        pairs.push(("note", Value::from(note)));
    }
    Value::Table(table(pairs))
}

/// Writes a per-book state file in the `kaia-book` format, under `<data>/kaia/books/`.
fn write_state(data: &Path, md5: &str, title: &str, annotations: Vec<Value>) {
    let dir = data.join("kaia").join("books");
    fs::create_dir_all(&dir).unwrap();
    let doc = Value::Table(table(vec![
        ("partial_md5", Value::from(md5)),
        ("file_size", Value::from(1i64)),
        ("title", Value::from(title)),
    ]));
    let root = table(vec![
        ("format", Value::from(STATE_FORMAT)),
        ("version", Value::from(STATE_VERSION)),
        ("doc", doc),
        ("annotations", Value::array(annotations)),
    ]);
    Settings::from_table(root)
        .flush(&dir.join(format!("{md5}.json")))
        .unwrap();
}

/// Two books with notes: Juliet (three entries, newest) and Leaves (one highlight).
/// Returns the app with the library scanned, and the two partial MD5s.
fn notes_app(dir: &Path) -> (App<HeadlessBackend>, String, String) {
    let lib = library_with_books(dir);
    let juliet = partial_md5(&lib.join("juliet.epub")).unwrap();
    let leaves = partial_md5(&lib.join("leaves.epub")).unwrap();
    let data = dir.join("data");
    write_state(
        &data,
        &juliet,
        "Juliet",
        vec![
            annotation(
                1,
                "2026-10-09 10:00:00",
                Value::from("/body/DocFragment[10]/body/p[2].0"),
                9,
                Some("lighten"),
                Some("Two households, both alike in dignity"),
                None,
            ),
            annotation(
                2,
                "2026-10-11 08:30:00",
                Value::from("/body/DocFragment[2]/body/p[1].0"),
                2,
                Some("lighten"),
                Some("Fair Verona"),
                Some("check the setting"),
            ),
            annotation(3, "2026-10-11 09:00:00", Value::from(5i64), 5, None, None, None),
        ],
    );
    write_state(
        &data,
        &leaves,
        "Leaves",
        vec![annotation(
            4,
            "2026-10-01 07:00:00",
            Value::from("/body/DocFragment[2]/p.0"),
            3,
            Some("lighten"),
            Some("Leaves quote"),
            None,
        )],
    );
    let mut app = make_app(dir, vec![lib.clone()]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| {
        !a.scan_running()
            && a.services()
                .library
                .book_by_path(&lib.join("leaves.epub"))
                .ok()
                .flatten()
                .is_some()
    });
    (app, juliet, leaves)
}

/// A TCP server on the loopback address that answers every request with 200 and
/// counts the connections. Returns its address and the counter.
fn counting_server() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&count);
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            seen.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        }
    });
    (url, count)
}

/// Renders any widget at the screen size of the app, with the app's theme and assets.
fn paint(app: &App<HeadlessBackend>, root: impl Widget + 'static) -> Pixmap {
    let theme = app.services().theme.clone();
    let assets = app.services().assets.clone();
    render(root, app.screen(), &theme, &assets)
}

#[test]
fn notes_root_is_a_root_and_the_nav_bar_returns_home() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("notes");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    tap_key(&mut app, "nav-notes");
    assert_eq!(app.top_route(), Some(&Route::Notes));
    assert_eq!(app.router().routes().len(), 1, "Notes replaces the root");

    tap_key(&mut app, "nav-home");
    assert_eq!(app.top_route(), Some(&Route::Home));
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn notes_list_orders_books_and_entries_and_filters() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("notes-list");
    let (mut app, juliet, leaves) = notes_app(&dir);

    let loaded = notes::load_books(app.services());
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].md5, juliet, "newest annotation first");
    assert_eq!(loaded[0].library_id, Some(book_id(&app, "juliet.epub")));
    let order: Vec<i64> = loaded[0].notes.iter().map(|n| n.id as i64).collect();
    assert_eq!(
        order,
        vec![2, 3, 1],
        "reading order: DocFragment[2], page 5, DocFragment[10]"
    );

    tap_key(&mut app, "nav-notes");
    assert!(app.find_rect(&format!("notes-book-{juliet}")).is_some());
    assert!(app.find_rect(&format!("notes-book-{leaves}")).is_some());

    tap_key(&mut app, "notes-filter-bookmarks");
    assert!(
        app.find_rect(&format!("notes-book-{juliet}")).is_some(),
        "Juliet has a bookmark"
    );
    assert!(
        app.find_rect(&format!("notes-book-{leaves}")).is_none(),
        "Leaves has none"
    );

    tap_key(&mut app, "notes-filter-all");
    assert!(app.find_rect(&format!("notes-book-{leaves}")).is_some());
    assert_eq!(app.top_route(), Some(&Route::Notes));
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn notes_entry_opens_the_book_at_its_place() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("notes-open");
    let (mut app, juliet, _leaves) = notes_app(&dir);
    let id = book_id(&app, "juliet.epub");

    tap_key(&mut app, "nav-notes");
    tap_key(&mut app, &format!("notes-book-{juliet}"));
    assert!(
        app.find_rect("notes-entry-0").is_some(),
        "the book view lists its entries"
    );
    tap_key(&mut app, "notes-entry-0");
    assert_eq!(app.top_route(), Some(&Route::Reader { book: id }));
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn notes_export_writes_markdown_and_json() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("notes-export");
    let (mut app, _juliet, _leaves) = notes_app(&dir);

    tap_key(&mut app, "nav-notes");
    tap_key(&mut app, "notes-export");

    let exports = dir.join("data").join("exports");
    let names: Vec<String> = fs::read_dir(&exports)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let markdown: Vec<&String> = names.iter().filter(|n| n.ends_with(".md")).collect();
    let json: Vec<&String> = names.iter().filter(|n| n.ends_with(".json")).collect();
    assert_eq!(markdown.len(), 1, "{names:?}");
    assert_eq!(json.len(), 1, "{names:?}");

    let md = fs::read_to_string(exports.join(markdown[0])).unwrap();
    assert!(md.contains("## Romeo and Juliet"), "{md}");
    assert!(md.contains("Leaves quote"), "{md}");
    assert!(md.contains("note: check the setting"), "{md}");

    let js = fs::read_to_string(exports.join(json[0])).unwrap();
    assert!(js.contains("\"title\": \"Romeo and Juliet\""), "{js}");
    assert!(js.contains("\"highlight\": true"), "{js}");
    assert!(js.contains("\"bookmark\": true"), "{js}");
}

#[test]
fn notes_list_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("notes-golden");
    let (mut app, _juliet, _leaves) = notes_app(&dir);
    tap_key(&mut app, "nav-notes");
    assert_matches_golden(app.frame(), &golden("notes_list.png"), 0);
}

#[test]
fn notes_book_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("notes-book-golden");
    let (mut app, juliet, _leaves) = notes_app(&dir);
    tap_key(&mut app, "nav-notes");
    tap_key(&mut app, &format!("notes-book-{juliet}"));
    assert_matches_golden(app.frame(), &golden("notes_book.png"), 0);
}

/// Opens Settings, types `query` in the search and opens the first hit.
fn search_and_open(app: &mut App<HeadlessBackend>, query: &str) {
    tap_key(app, "nav-settings");
    tap_key(app, "settings-search");
    tap_key(app, "settings-search-field");
    for c in query.chars() {
        app.key(Key::Char(c)).unwrap();
    }
    app.key(Key::Press).unwrap();
    tap_key(app, "settings-hit-0");
}

#[test]
fn settings_search_finds_items_across_sections() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-search");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-search");
    tap_key(&mut app, "settings-search-field");
    for c in "font".chars() {
        app.key(Key::Char(c)).unwrap();
    }
    app.key(Key::Press).unwrap();

    assert!(app.find_rect("settings-hit-0").is_some());
    assert!(
        app.find_rect("settings-hit-1").is_some(),
        "Font family and Font size"
    );
    assert!(
        app.find_rect("settings-reading").is_none(),
        "the section list is replaced"
    );
    assert_eq!(app.top_route(), Some(&Route::Settings));
}

#[test]
fn settings_sections_open_and_toggles_persist() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-toggle");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let path = app.services().settings_path();

    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-display");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "display".into()
        })
    );

    tap_key(&mut app, "set-display.night_mode");
    assert!(
        Settings::open(&path).is_true(keys::DISPLAY_NIGHT_MODE),
        "night mode is saved"
    );
    tap_key(&mut app, "set-display.night_mode");
    assert!(
        Settings::open(&path).is_false(keys::DISPLAY_NIGHT_MODE),
        "and saved off"
    );
    assert!(app.unknown_commands().is_empty(), "{:?}", app.unknown_commands());
}

#[test]
fn network_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-network-golden");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-network");
    assert_matches_golden(app.frame(), &golden("settings_network.png"), 0);
}

#[test]
fn stepper_saves_the_new_value() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-stepper");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let path = app.services().settings_path();

    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-reading");
    let rect = app.find_rect("set-reading.font_size").expect("font size stepper");
    let touch = app.services().theme.touch_target as i32;
    // The plus button is the right-hand touch target of the stepper.
    let plus = kaia_gfx::Point::new(rect.x + rect.w as i32 - touch / 2, rect.y + rect.h as i32 / 2);
    app.tap(plus).unwrap();

    let saved = Settings::open(&path);
    assert_eq!(saved.read(keys::READING_FONT_SIZE).as_f64(), Some(23.0));
}

#[test]
fn sync_test_calls_the_server_only_when_tapped() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-sync");
    let (url, hits) = counting_server();
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    {
        let settings = &mut app.services_mut().settings;
        settings.save(keys::SYNC_ENABLED, Value::Bool(true));
        settings.save(keys::SYNC_SERVER, Value::from(url.as_str()));
        settings.save(keys::SYNC_USERNAME, Value::from("reader"));
        settings.save(keys::SYNC_PASSWORD, Value::from("secret"));
    }

    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-network");
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "opening the section sends nothing"
    );
    tap_key(&mut app, "settings-fields");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "editing the fields sends nothing");

    for page in 0..20 {
        app.update_top::<kaia_ui::composite::PagedList<Entry>, _>("settings-entries", |list, cx| {
            list.set_page(page, cx)
        });
        app.process(Vec::new()).unwrap();
        if app.find_rect("settings-sync-test").is_some() {
            break;
        }
    }
    tap_key(&mut app, "settings-sync-test");
    wait_for(&mut app, "the health check", |_| hits.load(Ordering::SeqCst) == 1);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "the tap sends one health check");
}

#[test]
fn statistics_section_reads_the_database() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-stats");
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    write_statistics(&data, FIXED_NOW);
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    let rows = sections::entries("statistics", app.services(), FIXED_NOW);
    let bars = rows.iter().filter(|e| matches!(e, Entry::Bar { .. })).count();
    assert_eq!(bars, 7, "one bar per day");
    assert!(
        rows.iter()
            .any(|e| matches!(e, Entry::Row { primary, .. } if primary == "Juliet")),
        "the book list names the book"
    );

    tap_key(&mut app, "nav-settings");
    tap_key(&mut app, "settings-statistics");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection {
            name: "statistics".into()
        })
    );
}

/// Fixture reading: Juliet, 40 pages, read in the last hour.
fn write_statistics(data: &Path, now: i64) {
    let mut stats = Stats::open(data.join("statistics.sqlite3")).unwrap();
    stats
        .start_book(
            "378df3409d69b6994febc92492dbb19e",
            "Juliet",
            "Shakespeare",
            40,
            1,
            now - 900,
        )
        .unwrap();
    stats.page_turn(2, now - 600).unwrap();
    stats.page_turn(3, now - 300).unwrap();
    stats.end_book(now).unwrap();
}

#[test]
fn statistics_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-stats-golden");
    let data = dir.join("data");
    fs::create_dir_all(&data).unwrap();
    write_statistics(&data, FIXED_NOW);
    let app = make_app(&dir, Vec::new());
    let page = section::section_page(app.services(), "statistics", false, FIXED_NOW);
    assert_matches_golden(&paint(&app, page), &golden("settings_statistics.png"), 0);
}

#[test]
fn settings_root_and_reading_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-golden");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    tap_key(&mut app, "nav-settings");
    assert_matches_golden(app.frame(), &golden("settings_root.png"), 0);

    tap_key(&mut app, "settings-reading");
    assert_matches_golden(app.frame(), &golden("settings_reading.png"), 0);
}

#[test]
fn about_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("settings-about");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();

    search_and_open(&mut app, "license");
    assert_eq!(
        app.top_route(),
        Some(&Route::SettingsSection { name: "about".into() })
    );
    assert_matches_golden(app.frame(), &golden("settings_about.png"), 0);
}

#[test]
fn quick_settings_sheet_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("quick-golden");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let sheet = quick::quick_settings(app.services());
    assert_matches_golden(&paint(&app, sheet), &golden("quick_settings.png"), 0);
}

#[test]
fn sleep_screen_golden() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("sleep-golden");
    let (app, _juliet, _leaves) = notes_app(&dir);
    let id = book_id(&app, "juliet.epub");
    let screen = sleep::sleep_screen(app.services(), app.screen(), Some(id));
    assert_matches_golden(&paint(&app, screen), &golden("sleep_screen.png"), 0);
}
