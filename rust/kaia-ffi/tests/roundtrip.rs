//! The store through the C ABI, one call per export: stickers, assets, documents,
//! export and import. The Lua spec repeats the common calls through LuaJIT.

mod common;

use std::ffi::CString;
use std::fs;

use common::{
    close, cstr, get, last_error, list, open, put, sticker_id, sticker_json, take_string, temp_store, zeroed,
    MD5_A, MD5_B, PNG_RED,
};
use kaia::{
    kaia_asset_add_file, kaia_asset_path, kaia_buf_free, kaia_document_seen, kaia_export, kaia_import,
    kaia_sticker_delete, kaia_sticker_list_all, kaia_sticker_set_state, kaia_store_gc, KAIA_E_NOT_FOUND,
    KAIA_E_VALIDATION, KAIA_OK,
};

fn json_value(text: &str) -> serde_json::Value {
    serde_json::from_str(text).expect("libkaia writes valid JSON")
}

fn list_all(handle: u64) -> String {
    let mut out = zeroed();
    // SAFETY: out is a valid buffer.
    assert_eq!(unsafe { kaia_sticker_list_all(handle, &mut out) }, KAIA_OK);
    take_string(&mut out)
}

fn set_state(handle: u64, id: &str, state: &str) {
    let id = cstr(id);
    let state = cstr(state);
    // SAFETY: both strings are NUL-terminated.
    assert_eq!(
        unsafe { kaia_sticker_set_state(handle, id.as_ptr(), state.as_ptr()) },
        KAIA_OK
    );
}

fn delete(handle: u64, id: &str) -> i32 {
    let id = cstr(id);
    // SAFETY: id is NUL-terminated.
    unsafe { kaia_sticker_delete(handle, id.as_ptr()) }
}

fn add_png(handle: u64, path: &std::path::Path) -> String {
    let path_c = cstr(path.to_str().expect("UTF-8 path"));
    let license = cstr("CC0");
    let mut out = zeroed();
    // SAFETY: strings are NUL-terminated and out is a valid buffer.
    let rc = unsafe { kaia_asset_add_file(handle, path_c.as_ptr(), license.as_ptr(), &mut out) };
    assert_eq!(rc, KAIA_OK, "add_file failed: {}", last_error());
    let asset = json_value(&take_string(&mut out));
    assert_eq!(asset["mime"], "image/png");
    assert_eq!(asset["width"], 1);
    asset["sha256"].as_str().expect("sha256 string").to_string()
}

#[test]
fn stickers_documents_assets_export_and_import() {
    let (tmp, dir) = temp_store();
    let handle = open(&dir);
    let png = tmp.path().join("red.png");
    fs::write(&png, PNG_RED).expect("write png");

    // A text sticker and an image sticker on the same document.
    let (rc, text_stored) = put(handle, &sticker_json(MD5_A, "quoted text"));
    assert_eq!(rc, KAIA_OK);
    let text_id = sticker_id(&text_stored);
    let stored = json_value(&text_stored);
    assert_eq!(stored["state"], "attached");
    assert_eq!(stored["doc"]["partial_md5"], MD5_A);
    assert!(stored["created_at"].as_i64().expect("created_at") > 0);
    assert_eq!(stored["created_at"], stored["updated_at"]);

    let sha = add_png(handle, &png);
    let image_json = sticker_json(MD5_A, "second").replace("\"asset\":null", &format!("\"asset\":\"{sha}\""));
    let (rc, image_stored) = put(handle, &image_json);
    assert_eq!(rc, KAIA_OK, "message: {}", last_error());
    let image_id = sticker_id(&image_stored);

    // Get and list return what put stored.
    let (rc, got) = get(handle, &text_id);
    assert_eq!(rc, KAIA_OK);
    assert_eq!(json_value(&got), stored);
    let (rc, listed) = list(handle, MD5_A);
    assert_eq!(rc, KAIA_OK);
    assert_eq!(json_value(&listed).as_array().map(Vec::len), Some(2));
    assert_eq!(json_value(&list_all(handle)).as_array().map(Vec::len), Some(2));
    assert_eq!(
        list(handle, MD5_B).1,
        "[]",
        "unknown document gives an empty array"
    );

    // Put with an id updates the stored sticker in place.
    let updated = format!(r#"{{"id":"{text_id}",{}"#, &sticker_json(MD5_A, "changed")[1..]);
    let (rc, updated_stored) = put(handle, &updated);
    assert_eq!(rc, KAIA_OK, "message: {}", last_error());
    assert_eq!(json_value(&updated_stored)["anchor"]["quote"]["exact"], "changed");
    assert_eq!(json_value(&updated_stored)["created_at"], stored["created_at"]);

    // An unknown asset is a validation error that names the field.
    let bad_asset = sticker_json(MD5_B, "x").replace("\"asset\":null", "\"asset\":\"00\"");
    let (rc, _) = put(handle, &bad_asset);
    assert_eq!(rc, KAIA_E_VALIDATION);

    // Documents: the same size changes nothing. A new size sends attached stickers to review.
    let md5 = cstr(MD5_A);
    let mut changed = 0i32;
    // SAFETY: md5 and title are NUL-terminated, changed is a valid i32 pointer.
    unsafe {
        assert_eq!(
            kaia_document_seen(handle, md5.as_ptr(), 1000, cstr("").as_ptr(), &mut changed),
            KAIA_OK
        );
    }
    assert_eq!(changed, 0);
    set_state(handle, &text_id, "orphaned");
    // SAFETY: as above.
    unsafe {
        assert_eq!(
            kaia_document_seen(handle, md5.as_ptr(), 999, cstr("").as_ptr(), &mut changed),
            KAIA_OK
        );
    }
    assert_eq!(changed, 1);
    assert_eq!(json_value(&get(handle, &image_id).1)["state"], "needs_review");
    assert_eq!(
        json_value(&get(handle, &text_id).1)["state"],
        "orphaned",
        "only attached stickers move"
    );

    // The asset file is on disk and its path comes back as text.
    let sha_c = cstr(&sha);
    let mut asset_path = zeroed();
    // SAFETY: sha_c is NUL-terminated and asset_path is a valid buffer.
    assert_eq!(
        unsafe { kaia_asset_path(handle, sha_c.as_ptr(), &mut asset_path) },
        KAIA_OK
    );
    let asset_path = take_string(&mut asset_path);
    assert!(asset_path.ends_with(".png") && std::path::Path::new(&asset_path).is_file());

    // Export, then import into an empty store. The second store gets both stickers and the asset.
    let mut exported = zeroed();
    // SAFETY: exported is a valid buffer.
    assert_eq!(unsafe { kaia_export(handle, &mut exported) }, KAIA_OK);
    let export_dir = take_string(&mut exported);
    let (_tmp2, dir2) = temp_store();
    let second = open(&dir2);
    let export_c = cstr(&export_dir);
    let mut report = zeroed();
    // SAFETY: export_c is NUL-terminated and report is a valid buffer.
    assert_eq!(
        unsafe { kaia_import(second, export_c.as_ptr(), &mut report) },
        KAIA_OK,
        "{}",
        last_error()
    );
    let report = json_value(&take_string(&mut report));
    assert_eq!(report["stickers_added"], 2);
    assert_eq!(report["stickers_skipped"], 0);
    assert_eq!(report["assets_added"], 1);
    assert_eq!(
        json_value(&list(second, MD5_A).1).as_array().map(Vec::len),
        Some(2)
    );
    assert_eq!(close(second), KAIA_OK);

    // Delete, then collect. The asset is still referenced by nothing once its sticker is gone.
    assert_eq!(delete(handle, &image_id), KAIA_OK);
    assert_eq!(get(handle, &image_id).0, KAIA_E_NOT_FOUND);
    let mut removed = 1u32;
    // SAFETY: removed is a valid u32 pointer.
    assert_eq!(unsafe { kaia_store_gc(handle, &mut removed) }, KAIA_OK);
    assert_eq!(removed, 1, "the asset had no sticker left");

    assert_eq!(close(handle), KAIA_OK);
}

#[test]
fn gc_keeps_assets_that_stickers_reference() {
    let (tmp, dir) = temp_store();
    let handle = open(&dir);
    let png = tmp.path().join("red.png");
    fs::write(&png, PNG_RED).expect("write png");
    let sha = add_png(handle, &png);
    let json = sticker_json(MD5_A, "q").replace("\"asset\":null", &format!("\"asset\":\"{sha}\""));
    assert_eq!(put(handle, &json).0, KAIA_OK);
    let mut removed = 9u32;
    // SAFETY: removed is a valid u32 pointer.
    assert_eq!(unsafe { kaia_store_gc(handle, &mut removed) }, KAIA_OK);
    assert_eq!(removed, 0);
    assert_eq!(close(handle), KAIA_OK);
}

#[test]
fn quote_locate_needs_no_store() {
    let mut index = 0i64;
    let haystack = CString::new("Alpha beta. The needle sits here.").expect("no NUL");
    // SAFETY: all strings are NUL-terminated and index is a valid i64 pointer.
    let rc = unsafe {
        kaia::kaia_quote_locate(
            haystack.as_ptr(),
            cstr("needle").as_ptr(),
            cstr("The ").as_ptr(),
            cstr(" sits").as_ptr(),
            &mut index,
        )
    };
    assert_eq!(rc, KAIA_OK);
    assert_eq!(index, 16);
    // SAFETY: as above, with a quote that is not in the text.
    let rc = unsafe {
        kaia::kaia_quote_locate(
            haystack.as_ptr(),
            cstr("pin").as_ptr(),
            cstr("").as_ptr(),
            cstr("").as_ptr(),
            &mut index,
        )
    };
    assert_eq!(rc, KAIA_OK);
    assert_eq!(index, -1);
}

#[test]
fn failed_import_leaves_the_output_untouched() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    let mut out = zeroed();
    // SAFETY: the import dir does not exist, so the call fails and out must stay zeroed.
    let rc = unsafe { kaia_import(handle, cstr("/nonexistent/kaia-import").as_ptr(), &mut out) };
    assert_ne!(rc, KAIA_OK);
    assert!(out.ptr.is_null() && out.len == 0);
    // SAFETY: out is zeroed, so free is a no-op.
    unsafe { kaia_buf_free(&mut out) };
    assert_eq!(close(handle), KAIA_OK);
}
