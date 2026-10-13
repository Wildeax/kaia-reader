//! Store API behavior: sticker CRUD, validation, asset intake, GC and document changes.

mod common;

use std::fs;

use kaia_core::limits::ASSET_MAX_BYTES;
use kaia_core::StickerState;
use kaia_storage::{Migration, Store, StoreError, MIGRATIONS, PENDING_ASSET_GRACE_SECS};

use common::*;

fn open() -> (tempfile::TempDir, std::path::PathBuf, Store) {
    let (tmp, dir) = temp_store_dir();
    let store = Store::open(&dir).expect("open");
    (tmp, dir, store)
}

#[test]
fn put_assigns_id_and_timestamps_and_update_keeps_created_at() {
    let (_tmp, _dir, mut store) = open();
    let first = store.put_sticker(&book_text(MD5_A, "one")).expect("insert");
    let id = first.id.clone().expect("id assigned");
    assert_eq!(id.len(), 36);
    assert_eq!(id, id.to_lowercase());
    assert!(first.created_at > 0);
    assert_eq!(first.created_at, first.updated_at);
    assert_eq!(first.doc.partial_md5, MD5_A);

    let mut edit = first.clone();
    edit.text = Some("two".into());
    edit.rotation = -45;
    edit.state = StickerState::NeedsReview;
    let updated = store.put_sticker(&edit).expect("update");
    assert_eq!(updated.id.as_deref(), Some(id.as_str()));
    assert_eq!(updated.created_at, first.created_at);
    assert_eq!(updated.text.as_deref(), Some("two"));
    assert_eq!(updated.rotation, -45);
    assert_eq!(store.get_sticker(&id).expect("get"), updated);
    assert_eq!(store.list_all().expect("list").len(), 1);
}

#[test]
fn update_of_unknown_id_is_not_found() {
    let (_tmp, _dir, mut store) = open();
    let mut s = book_text(MD5_A, "x");
    s.id = Some("00000000-0000-4000-8000-000000000000".into());
    assert!(matches!(store.put_sticker(&s), Err(StoreError::NotFound(_))));
    s.id = Some("not-a-uuid".into());
    assert!(matches!(store.put_sticker(&s), Err(StoreError::InvalidArg(_))));
}

#[test]
fn sticker_cannot_move_to_another_document() {
    let (_tmp, _dir, mut store) = open();
    let s = store.put_sticker(&book_text(MD5_A, "x")).expect("insert");
    let mut moved = s.clone();
    moved.doc.partial_md5 = MD5_B.into();
    let err = store.put_sticker(&moved).unwrap_err();
    assert!(
        matches!(&err, StoreError::Validation(v) if v.field == "doc.partial_md5"),
        "{err:?}"
    );
}

#[test]
fn validation_failure_writes_nothing_and_names_the_field() {
    let (_tmp, _dir, mut store) = open();
    let mut bad = book_text(MD5_A, "x");
    bad.text = Some("x".repeat(2001));
    let err = store.put_sticker(&bad).unwrap_err();
    assert_eq!(err.code(), -7);
    assert_eq!(err.to_string(), "invalid text: must be at most 2000 chars");
    assert!(store.list_all().expect("list").is_empty());
}

#[test]
fn unknown_asset_is_a_validation_error() {
    let (_tmp, _dir, mut store) = open();
    let err = store
        .put_sticker(&fixed_with_asset(MD5_A, &"c".repeat(64)))
        .unwrap_err();
    assert!(
        matches!(&err, StoreError::Validation(v) if v.field == "asset"),
        "{err:?}"
    );
}

#[test]
fn list_stickers_filters_by_document() {
    let (_tmp, _dir, mut store) = open();
    store.put_sticker(&book_text(MD5_A, "a")).expect("a");
    store.put_sticker(&book_text(MD5_B, "b")).expect("b");
    let a = store.list_stickers(MD5_A).expect("list a");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].text.as_deref(), Some("a"));
    assert!(store
        .list_stickers("00000000000000000000000000000000")
        .expect("unknown")
        .is_empty());
    assert!(matches!(
        store.list_stickers("NOT-HEX"),
        Err(StoreError::InvalidArg(_))
    ));
}

#[test]
fn delete_and_set_state() {
    let (_tmp, _dir, mut store) = open();
    let s = store.put_sticker(&book_text(MD5_A, "a")).expect("put");
    let id = s.id.clone().expect("id");
    store.set_state(&id, StickerState::Orphaned).expect("set state");
    assert_eq!(store.get_sticker(&id).expect("get").state, StickerState::Orphaned);
    store.delete_sticker(&id).expect("delete");
    assert!(matches!(store.get_sticker(&id), Err(StoreError::NotFound(_))));
    assert!(matches!(store.delete_sticker(&id), Err(StoreError::NotFound(_))));
    assert!(matches!(
        store.set_state(&id, StickerState::Attached),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn png_and_jpeg_are_accepted_with_header_dimensions() {
    let (tmp, _dir, mut store) = open();
    let png = store
        .add_asset_file(&write_file(tmp.path(), "a.png", PNG_RED), "CC0")
        .expect("png");
    assert_eq!(png.mime, "image/png");
    assert_eq!((png.width, png.height), (1, 1));
    assert_eq!(png.bytes, PNG_RED.len() as u64);
    assert_eq!(png.sha256, sha256_of(PNG_RED));
    assert_eq!(png.license, "CC0");
    assert_eq!(png.source_name.as_deref(), Some("a.png"));

    let jpg = store
        .add_asset_file(&write_file(tmp.path(), "b.jpg", JPEG_3X2), "")
        .expect("jpeg");
    assert_eq!(jpg.mime, "image/jpeg");
    assert_eq!((jpg.width, jpg.height), (3, 2));
    assert!(store
        .asset_path(&jpg.sha256)
        .expect("path")
        .ends_with(format!("{}.jpg", jpg.sha256)));
}

#[test]
fn detection_uses_magic_bytes_not_the_file_name() {
    let (tmp, _dir, mut store) = open();
    let disguised = write_file(tmp.path(), "photo.png", JPEG_3X2);
    let asset = store.add_asset_file(&disguised, "").expect("jpeg named .png");
    assert_eq!(asset.mime, "image/jpeg");
    let text = write_file(tmp.path(), "notes.png", b"not an image at all");
    assert!(matches!(
        store.add_asset_file(&text, ""),
        Err(StoreError::AssetRejected(_))
    ));
}

#[test]
fn oversize_dimensions_are_rejected_from_the_header() {
    let (tmp, _dir, mut store) = open();
    let mut png = PNG_RED.to_vec();
    png[16..20].copy_from_slice(&2049u32.to_be_bytes()); // IHDR width
    let path = write_file(tmp.path(), "wide.png", &png);
    let err = store.add_asset_file(&path, "").unwrap_err();
    assert!(matches!(err, StoreError::AssetRejected(_)), "{err:?}");
    assert_eq!(err.code(), -8);
    assert!(fs::read_dir(store.dir().join("assets"))
        .expect("assets")
        .next()
        .is_none());
}

#[test]
fn size_limit_is_four_mib_inclusive() {
    let (tmp, _dir, mut store) = open();
    let mut exact = PNG_RED.to_vec();
    exact.resize(ASSET_MAX_BYTES as usize, 0);
    assert!(store
        .add_asset_file(&write_file(tmp.path(), "exact.png", &exact), "")
        .is_ok());

    let mut over = PNG_RED.to_vec();
    over.resize(ASSET_MAX_BYTES as usize + 1, 0);
    let err = store
        .add_asset_file(&write_file(tmp.path(), "over.png", &over), "")
        .unwrap_err();
    assert!(matches!(err, StoreError::AssetRejected(_)), "{err:?}");
}

#[test]
fn same_bytes_return_the_same_asset_and_keep_the_first_license() {
    let (tmp, _dir, mut store) = open();
    let path = write_file(tmp.path(), "red.png", PNG_RED);
    let first = store.add_asset_file(&path, "first").expect("first");
    let second = store.add_asset_file(&path, "second").expect("second");
    assert_eq!(first, second);
    assert_eq!(second.license, "first");
}

#[test]
fn asset_path_errors() {
    let (tmp, _dir, mut store) = open();
    assert!(matches!(store.asset_path("nope"), Err(StoreError::InvalidArg(_))));
    assert!(matches!(
        store.asset_path(&"a".repeat(64)),
        Err(StoreError::NotFound(_))
    ));
    let asset = store
        .add_asset_file(&write_file(tmp.path(), "red.png", PNG_RED), "")
        .expect("add");
    fs::remove_file(store.asset_path(&asset.sha256).expect("path")).expect("remove file");
    assert!(matches!(
        store.asset_path(&asset.sha256),
        Err(StoreError::NotFound(_))
    ));
}

#[test]
fn gc_removes_only_unreferenced_assets() {
    let (tmp, dir, mut store) = open();
    let kept = store
        .add_asset_file(&write_file(tmp.path(), "red.png", PNG_RED), "")
        .expect("red");
    let dropped = store
        .add_asset_file(&write_file(tmp.path(), "blue.png", PNG_BLUE), "")
        .expect("blue");
    store
        .put_sticker(&fixed_with_asset(MD5_A, &kept.sha256))
        .expect("sticker");
    // `dropped` was never saved. Age it past the grace period so gc treats it as an orphan.
    age_pending_asset(&dir, &dropped.sha256, PENDING_ASSET_GRACE_SECS + 1);

    // An orphan file with no row, as left by an interrupted write.
    let orphan = store.dir().join("assets").join(format!("{}.png", "e".repeat(64)));
    fs::write(&orphan, b"leftover").expect("orphan");

    assert_eq!(store.gc().expect("gc"), 2);
    assert!(store.asset_path(&kept.sha256).is_ok());
    assert!(matches!(
        store.asset_path(&dropped.sha256),
        Err(StoreError::NotFound(_))
    ));
    assert!(!orphan.exists());
    assert_eq!(store.gc().expect("second gc"), 0);
}

#[test]
fn gc_keeps_an_asset_picked_for_a_sticker_that_is_not_saved_yet() {
    let (tmp, _dir, mut store) = open();
    let asset = store
        .add_asset_file(&write_file(tmp.path(), "red.png", PNG_RED), "")
        .expect("add");
    // The sticker that references the image is saved a moment later.
    assert_eq!(store.gc().expect("gc between add and save"), 0);
    store
        .put_sticker(&fixed_with_asset(MD5_A, &asset.sha256))
        .expect("save the sticker");
    assert!(store.asset_path(&asset.sha256).is_ok());
    assert_eq!(store.gc().expect("gc after save"), 0);
    assert!(store.asset_path(&asset.sha256).is_ok());
}

#[test]
fn gc_collects_an_unsaved_asset_once_the_grace_period_has_passed() {
    let (tmp, dir, mut store) = open();
    let asset = store
        .add_asset_file(&write_file(tmp.path(), "red.png", PNG_RED), "")
        .expect("add");
    assert_eq!(store.gc().expect("gc inside the grace period"), 0);

    age_pending_asset(&dir, &asset.sha256, PENDING_ASSET_GRACE_SECS + 1);
    assert_eq!(store.gc().expect("gc after the grace period"), 1);
    assert!(matches!(
        store.asset_path(&asset.sha256),
        Err(StoreError::NotFound(_))
    ));
    assert!(!dir.join("assets").join(format!("{}.png", asset.sha256)).exists());
}

#[test]
fn asset_added_before_a_crash_is_collected_after_the_grace_period() {
    let (tmp, dir) = temp_store_dir();
    let png = write_file(tmp.path(), "red.png", PNG_RED);
    let sha = {
        let mut store = Store::open(&dir).expect("open");
        let asset = store.add_asset_file(&png, "").expect("add");
        // The store closes here with no sticker saved, as after a crash.
        asset.sha256
    };

    let mut store = Store::open(&dir).expect("reopen");
    assert_eq!(store.gc().expect("gc after the crash"), 0);
    assert!(store.asset_path(&sha).is_ok());

    age_pending_asset(&dir, &sha, PENDING_ASSET_GRACE_SECS + 1);
    assert_eq!(store.gc().expect("gc after the grace period"), 1);
    assert!(matches!(store.asset_path(&sha), Err(StoreError::NotFound(_))));
}

#[test]
fn asset_is_collected_at_once_when_its_last_sticker_is_deleted() {
    let (tmp, _dir, mut store) = open();
    let asset = store
        .add_asset_file(&write_file(tmp.path(), "red.png", PNG_RED), "")
        .expect("add");
    let sticker = store
        .put_sticker(&fixed_with_asset(MD5_A, &asset.sha256))
        .expect("save");
    store
        .delete_sticker(sticker.id.as_deref().expect("id"))
        .expect("delete");
    assert_eq!(store.gc().expect("gc"), 1);
}

#[test]
fn picking_an_image_again_makes_it_wait_for_its_sticker_again() {
    let (tmp, _dir, mut store) = open();
    let path = write_file(tmp.path(), "red.png", PNG_RED);
    let asset = store.add_asset_file(&path, "").expect("add");
    let sticker = store
        .put_sticker(&fixed_with_asset(MD5_A, &asset.sha256))
        .expect("save");
    store
        .delete_sticker(sticker.id.as_deref().expect("id"))
        .expect("delete");

    // The same image is picked again for a new sticker.
    store.add_asset_file(&path, "").expect("add again");
    assert_eq!(store.gc().expect("gc between add and save"), 0);
    store
        .put_sticker(&fixed_with_asset(MD5_B, &asset.sha256))
        .expect("save again");
    assert!(store.asset_path(&asset.sha256).is_ok());
}

#[test]
fn document_seen_reports_changes_and_flags_attached_stickers() {
    let (_tmp, _dir, mut store) = open();
    assert!(!store
        .document_seen(MD5_A, 1000, Some("Book"))
        .expect("first sighting"));
    let attached = store
        .put_sticker(&book_text(MD5_A, "attached"))
        .expect("attached");
    let orphaned = store
        .put_sticker(&book_text(MD5_A, "orphaned"))
        .expect("orphaned");
    store
        .set_state(orphaned.id.as_deref().expect("id"), StickerState::Orphaned)
        .expect("orphan");

    assert!(!store.document_seen(MD5_A, 1000, None).expect("same size"));
    assert!(store.document_seen(MD5_A, 1001, None).expect("changed size"));

    let id = attached.id.expect("id");
    assert_eq!(
        store.get_sticker(&id).expect("get").state,
        StickerState::NeedsReview
    );
    let orphan_id = orphaned.id.expect("id");
    assert_eq!(
        store.get_sticker(&orphan_id).expect("get").state,
        StickerState::Orphaned
    );

    // The new size is stored, so the same size again is not a change.
    assert!(!store
        .document_seen(MD5_A, 1001, None)
        .expect("stored size now matches"));
}

#[test]
fn document_seen_rejects_bad_hash() {
    let (_tmp, _dir, mut store) = open();
    let err = store.document_seen("XYZ", 1, None).unwrap_err();
    assert!(
        matches!(&err, StoreError::Validation(v) if v.field == "doc.partial_md5"),
        "{err:?}"
    );
}

#[test]
fn migration_list_must_be_strictly_increasing() {
    let (_tmp, dir) = common::temp_store_dir();
    let bad = [
        MIGRATIONS[0],
        Migration {
            version: 1,
            sql: "SELECT 1;",
        },
    ];
    assert!(matches!(
        Store::open_with_migrations(&dir, &bad),
        Err(StoreError::InvalidArg(_))
    ));
}

#[test]
fn error_codes_match_the_abi() {
    let codes = [
        (StoreError::InvalidArg(String::new()), -1),
        (StoreError::NotFound(String::new()), -3),
        (StoreError::Io(std::io::Error::other("x")), -4),
        (StoreError::Db(rusqlite::Error::InvalidQuery), -5),
        (
            StoreError::SchemaTooNew {
                found: 2,
                supported: 1,
            },
            -6,
        ),
        (
            StoreError::Validation(kaia_core::ValidationError::new("f", "r")),
            -7,
        ),
        (StoreError::AssetRejected(String::new()), -8),
    ];
    for (err, code) in codes {
        assert_eq!(err.code(), code, "{err}");
    }
}
