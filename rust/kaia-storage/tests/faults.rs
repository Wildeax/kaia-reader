//! The seven fault tests required by `docs/kaia/specs/STORAGE.md`.

mod common;

use std::fs;
use std::path::Path;

use kaia_core::{quote, Anchor, StickerState, TextQuote};
use kaia_storage::{ImportReport, Migration, Store, StoreError, DB_FILE, MIGRATIONS};
use rusqlite::{params, Connection};

use common::*;

fn user_version(path: &Path) -> u32 {
    let conn = Connection::open(path).expect("open db");
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .expect("user_version")
}

fn backup_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir.join("backups"))
        .expect("backups dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

// 1. Reopen after a clean write returns identical stickers.
#[test]
fn reopen_after_clean_write_returns_identical_stickers() {
    let (tmp, dir) = temp_store_dir();
    let png = write_file(tmp.path(), "red.png", PNG_RED);
    let stored = {
        let mut store = Store::open(&dir).expect("open");
        let asset = store.add_asset_file(&png, "CC0").expect("asset");
        store.document_seen(MD5_A, 1000, Some("Test book")).expect("seen");
        store
            .put_sticker(&reflowable(MD5_A, "needle", "The ", " sits"))
            .expect("put 1");
        store
            .put_sticker(&fixed_with_asset(MD5_A, &asset.sha256))
            .expect("put 2");
        store.put_sticker(&book_text(MD5_B, "book note")).expect("put 3");
        store.list_all().expect("list")
    };
    assert_eq!(stored.len(), 3);

    let reopened = Store::open(&dir).expect("reopen");
    assert_eq!(reopened.list_all().expect("list after reopen"), stored);
}

// 2. A process killed between BEGIN and COMMIT leaves the previous committed state.
#[test]
fn interrupted_transaction_leaves_previous_state() {
    let (tmp, dir) = temp_store_dir();
    let mut store = Store::open(&dir).expect("open");
    store
        .put_sticker(&reflowable(MD5_A, "committed", "", ""))
        .expect("committed sticker");
    let before = store.list_all().expect("list");
    drop(store);

    // Start a write, copy the database and its hot journal while the transaction is
    // open (what a killed process leaves behind), then drop the connection.
    let db = dir.join(DB_FILE);
    let journal = dir.join(format!("{DB_FILE}-journal"));
    let conn = Connection::open(&db).expect("raw connection");
    conn.execute_batch("BEGIN IMMEDIATE").expect("begin");
    conn.execute(
        "INSERT INTO documents (partial_md5, file_size, title) VALUES (?1, 5, NULL)",
        [MD5_B],
    )
    .expect("insert doc");
    conn.execute_batch("UPDATE stickers SET state = 'orphaned'")
        .expect("update");
    assert!(journal.exists(), "the open transaction must have a hot journal");

    let crash_dir = tmp.path().join("crashed");
    fs::create_dir_all(&crash_dir).expect("crash dir");
    fs::copy(&db, crash_dir.join(DB_FILE)).expect("copy db");
    fs::copy(&journal, crash_dir.join(format!("{DB_FILE}-journal"))).expect("copy journal");

    // Dropping without COMMIT must roll back. This is the case the spec names.
    drop(conn);
    {
        let store = Store::open(&dir).expect("reopen after dropped transaction");
        assert_eq!(store.list_all().expect("list"), before);
    }

    // The copied crash state has a hot journal. Opening it must recover the previous state.
    let recovered = Store::open(&crash_dir).expect("open crashed copy");
    assert_eq!(recovered.list_all().expect("list crashed copy"), before);
    assert!(recovered.list_stickers(MD5_B).expect("list").is_empty());
}

// 3. A disk-full error during an asset write leaves no partial file and no row.
#[test]
fn disk_full_during_asset_write_leaves_no_file_and_no_row() {
    let (tmp, dir) = temp_store_dir();
    let png = write_file(tmp.path(), "red.png", PNG_RED);
    let sha = sha256_of(PNG_RED);
    let mut store = Store::open(&dir).expect("open");

    // Fail after 10 of the 69 bytes have been written.
    let err = store
        .add_asset_file_simulating_disk_full(&png, "", 10)
        .unwrap_err();
    assert!(matches!(err, StoreError::Io(_)), "got {err:?}");
    assert_eq!(err.code(), -4);

    let leftovers: Vec<_> = fs::read_dir(dir.join("assets")).expect("assets dir").collect();
    assert!(
        leftovers.is_empty(),
        "assets/ must be empty after a failed write, found {leftovers:?}"
    );
    assert!(
        matches!(store.asset_path(&sha), Err(StoreError::NotFound(_))),
        "no asset row may exist"
    );

    // A sticker that points at the failed asset is refused, so no sticker row can exist either.
    let err = store.put_sticker(&fixed_with_asset(MD5_A, &sha)).unwrap_err();
    assert!(matches!(err, StoreError::Validation(_)), "got {err:?}");
    assert!(store.list_all().expect("list").is_empty());

    // The store still works after the failure.
    let ok = store.add_asset_file(&png, "").expect("retry succeeds");
    assert_eq!(ok.sha256, sha);
    assert_eq!(
        fs::read(store.asset_path(&sha).expect("path")).expect("read"),
        PNG_RED
    );
}

// 4. A failed migration rolls back and leaves the original file and user_version unchanged,
//    and the backup exists.
#[test]
fn failed_migration_rolls_back_and_keeps_backup() {
    let (_tmp, dir) = temp_store_dir();
    {
        let mut store = Store::open_with_migrations(&dir, &MIGRATIONS[..1]).expect("open v1");
        store.put_sticker(&book_text(MD5_A, "keep me")).expect("put");
    }
    let db = dir.join(DB_FILE);
    let before_bytes = fs::read(&db).expect("read db");
    assert_eq!(user_version(&db), 1);

    let failing = [
        MIGRATIONS[0],
        Migration {
            version: 2,
            sql: "CREATE TABLE extra (x INTEGER); INSERT INTO no_such_table VALUES (1);",
        },
    ];
    let err = Store::open_with_migrations(&dir, &failing).unwrap_err();
    assert!(matches!(err, StoreError::Db(_)), "got {err:?}");

    assert_eq!(user_version(&db), 1, "user_version must not move");
    assert_eq!(
        fs::read(&db).expect("read db"),
        before_bytes,
        "database file must be unchanged"
    );

    let backups = backup_names(&dir);
    assert_eq!(backups.len(), 1, "one backup expected, got {backups:?}");
    assert!(
        backups[0].starts_with("kaia-v1-") && backups[0].ends_with(".sqlite3"),
        "{backups:?}"
    );
    let backup_path = dir.join("backups").join(&backups[0]);
    assert_eq!(user_version(&backup_path), 1);

    // The store still opens with the real migrations and keeps its data.
    let store = Store::open(&dir).expect("reopen with real migrations");
    assert_eq!(store.list_all().expect("list").len(), 1);
}

// 5. Opening a database with a newer user_version fails without writing.
#[test]
fn newer_schema_is_refused_without_writing() {
    let (_tmp, dir) = temp_store_dir();
    drop(Store::open(&dir).expect("create"));
    let db = dir.join(DB_FILE);
    {
        let conn = Connection::open(&db).expect("raw");
        conn.pragma_update(None, "user_version", 99)
            .expect("set newer version");
    }
    let before_bytes = fs::read(&db).expect("read");
    let backups_before = backup_names(&dir);

    let err = Store::open(&dir).unwrap_err();
    assert!(
        matches!(
            err,
            StoreError::SchemaTooNew {
                found: 99,
                supported
            } if supported == MIGRATIONS.len() as u32
        ),
        "got {err:?}"
    );
    assert_eq!(err.code(), -6);

    assert_eq!(
        fs::read(&db).expect("read"),
        before_bytes,
        "database file must be unchanged"
    );
    assert!(
        !dir.join(format!("{DB_FILE}-journal")).exists(),
        "no journal may be left behind"
    );
    assert_eq!(
        backup_names(&dir),
        backups_before,
        "no backup may be taken for a refused open"
    );
    assert_eq!(user_version(&db), 99);
}

// 6. Export followed by import into an empty store reproduces the same stickers and assets.
#[test]
fn export_then_import_into_empty_store_reproduces_data() {
    let (tmp, src_dir) = temp_store_dir();
    let png = write_file(tmp.path(), "red.png", PNG_RED);
    let jpg = write_file(tmp.path(), "scan.jpg", JPEG_3X2);
    let mut src = Store::open(&src_dir).expect("open source");
    let png_asset = src.add_asset_file(&png, "CC0 by the author").expect("png");
    let jpg_asset = src.add_asset_file(&jpg, "").expect("jpeg");
    src.document_seen(MD5_A, 1000, Some("Book A")).expect("seen a");
    src.document_seen(MD5_B, 2000, None).expect("seen b");
    src.put_sticker(&reflowable(MD5_A, "needle", "The ", " sits"))
        .expect("s1");
    src.put_sticker(&fixed_with_asset(MD5_A, &png_asset.sha256))
        .expect("s2");
    src.put_sticker(&fixed_with_asset(MD5_B, &jpg_asset.sha256))
        .expect("s3");
    src.put_sticker(&book_text(MD5_B, "note")).expect("s4");
    let source_stickers = src.list_all().expect("list source");

    let export_dir = src.export().expect("export");
    assert!(export_dir.join("stickers.json").is_file());
    assert!(export_dir
        .join("assets")
        .join(format!("{}.png", png_asset.sha256))
        .is_file());
    assert!(export_dir
        .join("assets")
        .join(format!("{}.jpg", jpg_asset.sha256))
        .is_file());

    let (_dst_tmp, dst_dir) = temp_store_dir();
    let mut dst = Store::open(&dst_dir).expect("open destination");
    let report = dst.import(&export_dir).expect("import");
    assert_eq!(
        report,
        ImportReport {
            stickers_added: 4,
            stickers_skipped: 0,
            assets_added: 2
        }
    );

    assert_eq!(dst.list_all().expect("list destination"), source_stickers);
    for sha in [&png_asset.sha256, &jpg_asset.sha256] {
        let original = fs::read(src.asset_path(sha).expect("src path")).expect("read src");
        let copy = fs::read(dst.asset_path(sha).expect("dst path")).expect("read dst");
        assert_eq!(original, copy, "asset {sha} must match byte for byte");
    }
    // Documents without stickers come across too: the stored size is kept.
    assert!(!dst.document_seen(MD5_B, 2000, None).expect("seen"));

    // A second import changes nothing.
    let again = dst.import(&export_dir).expect("second import");
    assert_eq!(
        again,
        ImportReport {
            stickers_added: 0,
            stickers_skipped: 4,
            assets_added: 0
        }
    );
    assert_eq!(dst.list_all().expect("list"), source_stickers);
}

// 7. Re-anchoring finds the quote after text is inserted before it, and orphans the sticker
//    when the quote is removed.
#[test]
fn reanchoring_follows_insertions_and_orphans_removed_quotes() {
    let (_tmp, dir) = temp_store_dir();
    let mut store = Store::open(&dir).expect("open");
    let stored = store
        .put_sticker(&reflowable(MD5_A, "needle", "The ", " sits"))
        .expect("put");
    let id = stored.id.clone().expect("id assigned");
    let Anchor::Reflowable { quote: q, .. } = &stored.anchor else {
        panic!("expected a reflowable anchor");
    };
    let TextQuote {
        exact,
        prefix,
        suffix,
    } = q.clone();

    // Text inserted before the quote: the quote is found at its new position.
    let original = "Chapter one. The needle sits here. Chapter one ends.";
    let reflowed = "A new paragraph was added. Chapter one. The needle sits here. Chapter one ends.";
    let before = quote::locate(original, &exact, &prefix, &suffix).expect("found in original");
    let after = quote::locate(reflowed, &exact, &prefix, &suffix).expect("found after insertion");
    assert_eq!(after - before, "A new paragraph was added. ".chars().count());
    assert_eq!(
        reflowed
            .chars()
            .skip(after)
            .take(exact.chars().count())
            .collect::<String>(),
        exact
    );

    // Quote removed from the text: no match, so the sticker is orphaned and kept.
    let edited = "Chapter one. The pin sits here. Chapter one ends.";
    assert_eq!(quote::locate(edited, &exact, &prefix, &suffix), None);
    store.set_state(&id, StickerState::Orphaned).expect("orphan");
    let kept = store.get_sticker(&id).expect("still stored");
    assert_eq!(kept.state, StickerState::Orphaned);
    assert_eq!(store.list_stickers(MD5_A).expect("list").len(), 1);
}

// Schema version 2 adds `assets.pending_since`. A version 1 store keeps its assets. Its rows
// are not pending, so gc removes an unreferenced one at once, as it did before version 2.
#[test]
fn version_1_store_upgrades_and_keeps_its_assets() {
    let (_tmp, dir) = temp_store_dir();
    let sha = sha256_of(PNG_RED);
    drop(Store::open_with_migrations(&dir, &MIGRATIONS[..1]).expect("create v1"));
    {
        // A version 1 row and its file, as written before schema version 2.
        let conn = Connection::open(dir.join(DB_FILE)).expect("raw");
        conn.execute(
            "INSERT INTO assets (sha256, mime, width, height, bytes, source_name, license)
             VALUES (?1, 'image/png', 1, 1, ?2, 'red.png', '')",
            params![sha, PNG_RED.len() as i64],
        )
        .expect("v1 row");
    }
    write_file(&dir.join("assets"), &format!("{sha}.png"), PNG_RED);

    let mut store = Store::open(&dir).expect("open with the latest version");
    assert_eq!(user_version(&dir.join(DB_FILE)), MIGRATIONS.len() as u32);
    assert_eq!(backup_names(&dir).len(), 1, "the upgrade takes a backup first");
    assert!(
        store.asset_path(&sha).is_ok(),
        "the version 1 asset stays readable"
    );
    assert_eq!(store.gc().expect("gc"), 1);
    assert!(matches!(store.asset_path(&sha), Err(StoreError::NotFound(_))));
}
