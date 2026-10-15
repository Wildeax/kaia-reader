//! The two resets in Settings > Advanced, on a headless app. A temp folder stands
//! in for the Kobo volume: `books`, `.kobo`, `.adds/koreader` and `.adds/kaia` are
//! never changed by a reset, and `data` is Kaia's data folder. The confirm button
//! is disabled until the typed word matches. Startup finishes an erase that
//! stopped after its first step.

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use kaia_events::{Settings, Value};
use kaia_gfx::golden::assert_matches_golden;
use kaia_platform::reset;

/// Every entry under `root`: folders as `None`, files with their bytes. Keys are
/// relative to `root`.
type Snapshot = BTreeMap<PathBuf, Option<Vec<u8>>>;

fn snapshot(root: &Path) -> Snapshot {
    fn walk(root: &Path, rel: &Path, out: &mut Snapshot) {
        for entry in fs::read_dir(root.join(rel)).unwrap() {
            let entry = entry.unwrap();
            let child = rel.join(entry.file_name());
            let kind = fs::symlink_metadata(entry.path()).unwrap().file_type();
            if kind.is_dir() {
                out.insert(child.clone(), None);
                walk(root, &child, out);
            } else {
                out.insert(child, Some(fs::read(entry.path()).unwrap()));
            }
        }
    }
    let mut out = Snapshot::new();
    walk(root, Path::new(""), &mut out);
    out
}

/// The device folders: the books, the Kobo's Nickel folder, and the KOReader and
/// Lua app folders. A reset or erase must leave all of them byte for byte.
fn device(map: &Snapshot) -> Snapshot {
    const DEVICE: [&str; 3] = ["books", ".kobo", ".adds"];
    map.iter()
        .filter(|(path, _)| {
            path.components()
                .next()
                .and_then(|c| c.as_os_str().to_str())
                .is_some_and(|first| DEVICE.contains(&first))
        })
        .map(|(path, bytes)| (path.clone(), bytes.clone()))
        .collect()
}

/// The entries inside the data folder, with the data folder's name removed.
fn inside_data(map: &Snapshot) -> Snapshot {
    map.iter()
        .filter_map(|(path, bytes)| {
            path.strip_prefix("data")
                .ok()
                .filter(|rest| !rest.as_os_str().is_empty())
                .map(|rest| (rest.to_path_buf(), bytes.clone()))
        })
        .collect()
}

/// A temp folder with the device folders next to the data folder, and the data
/// folder with settings, reading state, fonts, dictionaries and cache.
fn volume(name: &str) -> PathBuf {
    let dir = scratch(name);
    let device = [
        ("books/juliet.epub", "book one"),
        ("books/sub/leaves.epub", "book two"),
        (".kobo/KoboReader.sqlite", "nickel"),
        (".adds/koreader/settings.reader.lua", "koreader"),
        (".adds/kaia/bin/kaia", "app"),
    ];
    for (path, bytes) in device {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let data = dir.join("data");
    // The language is pinned first, so the app does not rewrite the settings file.
    pin_english(&data);
    let mut settings = Settings::open(&data.join("settings.json"));
    settings.save("display.warmth", Value::Number(40.0));
    settings.flush(&data.join("settings.json")).unwrap();
    let kaia_files = [
        ("kaia/books/abc.json", "progress"),
        ("statistics.sqlite3", "stats"),
        ("fonts/Mine.ttf", "font"),
        ("dict/x.ifo", "dictionary"),
        ("cache/cover.jpg", "cover"),
    ];
    for (path, bytes) in kaia_files {
        let path = data.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    dir
}

/// Opens Settings > Advanced on a started app. Its reset rows are below the first
/// screen, so the tests send their actions directly.
fn open_advanced(app: &mut kaia_app::App<kaia_app::HeadlessBackend>) {
    tap_key(app, "nav-settings");
    command(app, "settings:advanced", &[]);
}

fn command(app: &mut kaia_app::App<kaia_app::HeadlessBackend>, id: &str, args: &[Value]) {
    app.command(id, args);
    app.process(Vec::new()).unwrap();
}

#[test]
fn startup_finishes_an_erase_that_stopped_after_the_rename() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = volume("reset-startup");
    fs::write(dir.join("data/library.sqlite3"), "old index").unwrap();
    // Step one only: the data folder is aside and the process stops.
    let aside = reset::move_aside(&dir.join("data")).unwrap().unwrap();
    assert!(aside.exists());
    let outside_before = device(&snapshot(&dir));

    let app = make_app(&dir, Vec::new());

    assert!(!aside.exists(), "the leftover erase folder is still there");
    assert!(
        !fs::read_dir(&dir)
            .unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().contains(".erasing-")),
        "an erase folder is left beside the data folder"
    );
    assert_eq!(device(&snapshot(&dir)), outside_before);
    assert_ne!(
        fs::read(dir.join("data/library.sqlite3")).ok().as_deref(),
        Some(b"old index".as_slice()),
        "the old index came back"
    );
    assert!(app.services().paths.data_dir.is_dir());
}

#[test]
fn reset_settings_needs_the_word_and_keeps_one_copy() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = volume("reset-settings-app");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let settings_path = dir.join("data/settings.json");
    let settings_before = fs::read(&settings_path).unwrap();
    let data_before = inside_data(&snapshot(&dir));
    let outside_before = device(&snapshot(&dir));

    open_advanced(&mut app);
    command(&mut app, "settings:reset:settings", &[]);
    assert!(app.find_rect("reset-confirm-run").is_some());

    // A tap on the disabled button does nothing.
    tap_key(&mut app, "reset-confirm-run");
    assert_eq!(fs::read(&settings_path).unwrap(), settings_before);
    assert!(!dir.join("data/settings.json.before-reset").exists());

    // A partial word does nothing, even when the button is sent its action directly.
    command(&mut app, "settings:reset:type", &[Value::from("RES")]);
    command(&mut app, "settings:reset:run", &[]);
    assert_eq!(fs::read(&settings_path).unwrap(), settings_before);

    // The word in lower case and with spaces counts. Then the enabled button runs.
    command(&mut app, "settings:reset:type", &[Value::from(" reset ")]);
    tap_key(&mut app, "reset-confirm-run");

    assert_eq!(
        fs::read(dir.join("data/settings.json.before-reset")).unwrap(),
        settings_before
    );
    assert!(app.services().settings.read("display.warmth").as_i64().is_none());

    // The app writes its now empty settings when the Settings page closes. The file
    // holds no value from before the reset.
    assert!(Settings::open(&settings_path).table().is_empty());
    let mut expected = data_before;
    expected.remove(Path::new("settings.json"));
    expected.insert(PathBuf::from("settings.json.before-reset"), Some(settings_before));
    let mut actual = inside_data(&snapshot(&dir));
    actual.remove(Path::new("settings.json"));
    assert_eq!(actual, expected);
    assert_eq!(device(&snapshot(&dir)), outside_before);
}

#[test]
fn erase_needs_the_word_and_touches_only_the_data_folder() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = volume("erase-app");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    let outside_before = device(&snapshot(&dir));
    let fonts = dir.join("data/fonts/Mine.ttf");
    let books_state = dir.join("data/kaia/books/abc.json");

    open_advanced(&mut app);
    command(&mut app, "settings:reset:erase", &[]);
    assert!(app.find_rect("reset-confirm-run").is_some());

    tap_key(&mut app, "reset-confirm-run");
    assert!(fonts.exists() && books_state.exists());

    command(&mut app, "settings:reset:type", &[Value::from("ERAS")]);
    tap_key(&mut app, "reset-confirm-run");
    assert!(fonts.exists() && books_state.exists());

    command(&mut app, "settings:reset:type", &[Value::from("ERASE")]);
    tap_key(&mut app, "reset-confirm-run");

    assert!(!fonts.exists());
    assert!(!books_state.exists());
    assert!(!dir.join("data/statistics.sqlite3").exists());
    assert!(!dir.join("data/dict/x.ifo").exists());
    assert!(!dir.join("data/cache/cover.jpg").exists());
    assert_eq!(device(&snapshot(&dir)), outside_before);
    assert!(!snapshot(&dir)
        .keys()
        .any(|path| path.to_string_lossy().contains(".erasing-")));
}

#[test]
fn the_reset_pages_match_their_goldens() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = volume("reset-goldens");
    let mut app = make_app(&dir, Vec::new());
    app.start().unwrap();
    open_advanced(&mut app);

    command(&mut app, "settings:reset:settings", &[]);
    assert_matches_golden(app.frame(), &golden("settings_reset_disabled.png"), 0);
    command(&mut app, "settings:reset:cancel", &[]);

    command(&mut app, "settings:reset:erase", &[]);
    command(&mut app, "settings:reset:type", &[Value::from("ERASE")]);
    assert_matches_golden(app.frame(), &golden("settings_erase_enabled.png"), 0);
}
