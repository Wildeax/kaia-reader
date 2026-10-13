mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use kaia_library::{Filter, ImportReport, LibraryError, Sort, SortKey};

/// `history.lua` as KOReader's `dump.lua` writes a list of tables.
fn history_lua(dir: &Path) -> String {
    format!(
        r#"return {{
    [1] = {{
        ["time"] = 1700000300,
        ["file"] = "{d}/leaves.epub",
    }},
    [2] = {{
        ["time"] = 1700000100,
        ["file"] = "{d}/two-column.pdf",
    }},
    [3] = {{
        ["time"] = 1699999999,
        ["file"] = "{d}/gone.pdf",
    }},
    [4] = {{
        ["time"] = 1700000200,
        ["file"] = "{d}/notes.txt",
    }},
    [5] = {{
        ["file"] = "{d}/leaves.epub",
    }},
}}
"#,
        d = dir.display()
    )
}

/// `collection.lua`: a collection table with a `settings` table and numbered entries.
fn collection_lua(dir: &Path) -> String {
    format!(
        r#"return {{
    ["favorites"] = {{
        ["settings"] = {{
            ["order"] = 1,
            ["default"] = true,
        }},
        [1] = {{
            ["file"] = "{d}/leaves.epub",
            ["order"] = 2,
        }},
        [2] = {{
            ["file"] = "{d}/two-column.pdf",
            ["order"] = 1,
        }},
        [3] = {{
            ["file"] = "{d}/gone.pdf",
            ["order"] = 3,
        }},
    }},
    ["reading list"] = {{
        ["settings"] = {{
            ["order"] = 1,
        }},
    }},
}}
"#,
        d = dir.display()
    )
}

fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    fs::create_dir_all(&root).unwrap();
    (tmp, root)
}

#[test]
fn history_import_adds_entries_and_skips_bad_ones() {
    let (tmp, root) = setup();
    fixture_set(&root);
    fs::write(root.join("notes.txt"), b"plain").unwrap();
    let lua = tmp.path().join("history.lua");
    fs::write(&lua, history_lua(&root)).unwrap();

    let mut lib = open_library(tmp.path());
    let report = lib.import_koreader_history(&lua).unwrap();
    assert_eq!(
        report,
        ImportReport {
            added: 2,
            already_present: 0,
            skipped_missing: 1,
            skipped_unsupported: 1,
            skipped_invalid: 1,
            lossy_values: 0,
        }
    );

    // The imported books have no metadata yet. Their rows and history are kept
    // when a scan reaches them.
    let leaves = lib.book_by_path(&root.join("leaves.epub")).unwrap().unwrap();
    assert_eq!(leaves.last_opened_at, Some(1700000300), "latest entry wins");
    let recent = lib.recent(10).unwrap();
    assert_eq!(recent[0].id, leaves.id);
    assert_eq!(recent.len(), 2);
    assert!(!leaves.extracted);

    // Two books were not in the history, so the scan adds only those. The two
    // imported rows are reused, and the scan extracts all four books.
    let rescan = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(rescan.added, 2, "only the books missing from the history are new");
    assert_eq!(rescan.extracted, 4, "imported books have no metadata yet");
    assert!(lib.book(leaves.id).unwrap().unwrap().extracted);
    assert_eq!(lib.recent(10).unwrap().len(), 2, "history survives the scan");
}

#[test]
fn history_import_twice_adds_nothing_new() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let lua = tmp.path().join("history.lua");
    fs::write(&lua, history_lua(&root)).unwrap();
    let mut lib = open_library(tmp.path());
    lib.import_koreader_history(&lua).unwrap();
    let again = lib.import_koreader_history(&lua).unwrap();
    assert_eq!(again.added, 0);
    assert_eq!(again.already_present, 2);
    let raw = raw_connection(tmp.path());
    let n: i64 = raw
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 2);
}

#[test]
fn collection_import_keeps_manual_order_and_skips_missing_files() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let lua = tmp.path().join("collection.lua");
    fs::write(&lua, collection_lua(&root)).unwrap();

    let mut lib = open_library(tmp.path());
    let report = lib.import_koreader_collections(&lua).unwrap();
    assert_eq!(report.added, 2);
    assert_eq!(report.skipped_missing, 1);
    assert_eq!(report.skipped_invalid, 0);

    let colls = lib.collections().unwrap();
    let names: Vec<(&str, u64)> = colls.iter().map(|c| (c.name.as_str(), c.books)).collect();
    assert_eq!(names, vec![("favorites", 2), ("reading list", 0)]);

    let fav = colls.iter().find(|c| c.name == "favorites").unwrap().id;
    let ordered = lib
        .books(
            Sort::new(SortKey::Manual, false),
            &Filter {
                collection: Some(fav),
                ..Filter::default()
            },
            0,
            10,
        )
        .unwrap();
    let names: Vec<String> = ordered
        .iter()
        .map(|b| b.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec!["two-column.pdf", "leaves.epub"],
        "order 1 then order 2"
    );

    let again = lib.import_koreader_collections(&lua).unwrap();
    assert_eq!(again.added, 0);
    assert_eq!(again.already_present, 2);
}

#[test]
fn import_rejects_files_that_are_not_lua_tables() {
    let (tmp, _root) = setup();
    let lua = tmp.path().join("bad.lua");
    fs::write(&lua, "return os.exit()").unwrap();
    let mut lib = open_library(tmp.path());
    assert!(matches!(
        lib.import_koreader_history(&lua),
        Err(LibraryError::KoreaderData { .. })
    ));
    let lua2 = tmp.path().join("number.lua");
    fs::write(&lua2, "return 42").unwrap();
    assert!(matches!(
        lib.import_koreader_collections(&lua2),
        Err(LibraryError::KoreaderData { .. })
    ));
    assert!(matches!(
        lib.import_koreader_history(&tmp.path().join("absent.lua")),
        Err(LibraryError::KoreaderData { .. })
    ));
}

#[test]
fn history_import_reads_a_file_written_by_kaia_luadata() {
    // The writer and reader of kaia-luadata agree on the layout KOReader uses.
    use kaia_luadata::{Key, Table, Value};
    let (tmp, root) = setup();
    let leaves = copy_fixture("leaves.epub", &root, "leaves.epub");
    let mut entry = Table::new();
    entry.insert(Key::Str("time".into()), Value::Number(1_700_001_000.0));
    entry.insert(
        Key::Str("file".into()),
        Value::Str(leaves.to_string_lossy().into_owned()),
    );
    let list = Value::Table(Table::from([(Key::Int(1), Value::Table(entry))]));
    let lua = tmp.path().join("history.lua");
    kaia_luadata::write_file(&lua, &list).unwrap();

    let mut lib = open_library(tmp.path());
    let report = lib.import_koreader_history(&lua).unwrap();
    assert_eq!(report.added, 1);
    let book = lib.book_by_path(&leaves).unwrap().unwrap();
    assert_eq!(book.last_opened_at, Some(1_700_001_000));
}

#[test]
fn collection_import_accepts_invalid_utf8_and_counts_affected_values() {
    let (tmp, root) = setup();
    let file = copy_fixture("leaves.epub", &root, "leaves.epub");
    let lua = tmp.path().join("collection.lua");
    let mut bytes = b"return { [\"fav".to_vec();
    bytes.push(0xff);
    bytes.extend_from_slice(
        format!("rites\"] = {{ [1] = {{ file = \"{}\" }} }} }}", file.display()).as_bytes(),
    );
    fs::write(&lua, bytes).unwrap();
    let mut lib = open_library(tmp.path());
    let report = lib.import_koreader_collections(&lua).unwrap();
    assert_eq!(report.added, 1);
    assert_eq!(report.lossy_values, 1);
}

#[test]
fn invalid_utf8_in_comment_does_not_count_an_existing_replacement_character() {
    let (tmp, root) = setup();
    let file = copy_fixture("leaves.epub", &root, "leaves.epub");
    let lua = tmp.path().join("collection.lua");
    let mut bytes = b"-- invalid byte: ".to_vec();
    bytes.push(0xff);
    bytes.extend_from_slice(
        format!(
            "\nreturn {{ [\"fav\u{fffd}\"] = {{ [1] = {{ file = \"{}\" }} }} }}",
            file.display()
        )
        .as_bytes(),
    );
    fs::write(&lua, bytes).unwrap();
    let mut lib = open_library(tmp.path());
    let report = lib.import_koreader_collections(&lua).unwrap();
    assert_eq!(report.added, 1);
    assert_eq!(report.lossy_values, 0);
}
