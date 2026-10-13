//! C ABI of libkaia, version 1.
//!
//! The declarations are in `include/kaia.h`. The contract is in
//! `docs/kaia/specs/ABI.md`. Every export runs its body under `error::guard`,
//! which turns panics into `KAIA_E_PANIC`, so no unwind crosses `extern "C"`.
//!
//! Out parameters are written only on success. A `kaia_buf` out parameter must be
//! zeroed or freed before the call.

mod args;
mod buf;
mod error;
#[cfg(all(feature = "glibc215", target_os = "linux"))]
mod glibc215;
mod registry;

use std::ffi::c_char;
use std::panic::catch_unwind;
use std::path::Path;

use kaia_core::{quote, Sticker, StickerState, ValidationError};
use kaia_storage::Store;
use serde::Serialize;

pub use buf::KaiaBuf;
pub use error::{
    KAIA_E_ASSET_REJECTED, KAIA_E_BAD_HANDLE, KAIA_E_DB, KAIA_E_INVALID_ARG, KAIA_E_IO, KAIA_E_NOT_FOUND,
    KAIA_E_PANIC, KAIA_E_SCHEMA_TOO_NEW, KAIA_E_VALIDATION, KAIA_OK,
};

use args::{cstr, json_text, non_null};
use error::{guard, Failure};

const ABI_VERSION: u32 = 1;

/// Serializes a value for Lua. Only fails if serde fails, which these types do not.
fn to_json<T: Serialize>(value: &T) -> Result<Vec<u8>, Failure> {
    serde_json::to_vec(value).map_err(|err| Failure::new(KAIA_E_DB, err.to_string()))
}

fn path_bytes(path: &Path) -> Result<Vec<u8>, Failure> {
    path.to_str()
        .map(|text| text.as_bytes().to_vec())
        .ok_or_else(|| Failure::new(KAIA_E_IO, "path is not valid UTF-8"))
}

/// Returns 1 for the ABI version this build implements.
#[no_mangle]
pub extern "C" fn kaia_abi_version() -> u32 {
    catch_unwind(|| ABI_VERSION).unwrap_or(0)
}

/// Writes the crate version, for example `0.1.0`.
///
/// # Safety
/// `out` must be a valid pointer to a `KaiaBuf`.
#[no_mangle]
pub unsafe extern "C" fn kaia_version(out: *mut KaiaBuf) -> i32 {
    guard(|| {
        let out = non_null(out, "out")?;
        buf::write(out, env!("CARGO_PKG_VERSION").as_bytes().to_vec());
        Ok(())
    })
}

/// Opens or creates the store in `dir` and writes its handle to `out_handle`.
///
/// # Safety
/// `dir` must be a NUL-terminated string. `out_handle` must be valid for a `u64` write.
#[no_mangle]
pub unsafe extern "C" fn kaia_store_open(dir: *const c_char, out_handle: *mut u64) -> i32 {
    guard(|| {
        let out = non_null(out_handle, "out_handle")?;
        let dir = cstr(dir, "dir")?;
        let store = Store::open(Path::new(dir))?;
        *out = registry::insert(store);
        Ok(())
    })
}

/// Closes a store. The handle is stale afterwards and returns `KAIA_E_BAD_HANDLE`.
#[no_mangle]
pub extern "C" fn kaia_store_close(handle: u64) -> i32 {
    guard(|| registry::close(handle))
}

/// Inserts a sticker when the JSON has no `id`, otherwise updates it. Writes the
/// stored sticker, with `id`, `created_at` and `updated_at` filled, to `out_json`.
///
/// # Safety
/// `json` must be valid for `len` bytes, or NULL when `len` is 0. `out_json` must
/// be a valid `KaiaBuf` pointer.
#[no_mangle]
pub unsafe extern "C" fn kaia_sticker_put(
    handle: u64,
    json: *const u8,
    len: usize,
    out_json: *mut KaiaBuf,
) -> i32 {
    guard(|| {
        let out = non_null(out_json, "out_json")?;
        let text = json_text(json, len)?;
        let sticker: Sticker =
            serde_json::from_str(text).map_err(|err| Failure::new(KAIA_E_VALIDATION, err.to_string()))?;
        let stored = registry::with_store(handle, |store| Ok(store.put_sticker(&sticker)?))?;
        buf::write(out, to_json(&stored)?);
        Ok(())
    })
}

/// Writes one sticker as JSON. Fails with `KAIA_E_NOT_FOUND` when the id is unknown.
///
/// # Safety
/// `id` must be a NUL-terminated string. `out_json` must be a valid `KaiaBuf` pointer.
#[no_mangle]
pub unsafe extern "C" fn kaia_sticker_get(handle: u64, id: *const c_char, out_json: *mut KaiaBuf) -> i32 {
    guard(|| {
        let out = non_null(out_json, "out_json")?;
        let id = cstr(id, "id")?;
        let sticker = registry::with_store(handle, |store| Ok(store.get_sticker(id)?))?;
        buf::write(out, to_json(&sticker)?);
        Ok(())
    })
}

/// Writes the stickers of one document as a JSON array, oldest first. An unknown
/// document gives `[]`.
///
/// # Safety
/// `partial_md5` must be a NUL-terminated string. `out_json_array` must be a valid
/// `KaiaBuf` pointer.
#[no_mangle]
pub unsafe extern "C" fn kaia_sticker_list(
    handle: u64,
    partial_md5: *const c_char,
    out_json_array: *mut KaiaBuf,
) -> i32 {
    guard(|| {
        let out = non_null(out_json_array, "out_json_array")?;
        let md5 = cstr(partial_md5, "partial_md5")?;
        let list = registry::with_store(handle, |store| Ok(store.list_stickers(md5)?))?;
        buf::write(out, to_json(&list)?);
        Ok(())
    })
}

/// Writes every sticker as a JSON array, grouped by document.
///
/// # Safety
/// `out_json_array` must be a valid `KaiaBuf` pointer.
#[no_mangle]
pub unsafe extern "C" fn kaia_sticker_list_all(handle: u64, out_json_array: *mut KaiaBuf) -> i32 {
    guard(|| {
        let out = non_null(out_json_array, "out_json_array")?;
        let list = registry::with_store(handle, |store| Ok(store.list_all()?))?;
        buf::write(out, to_json(&list)?);
        Ok(())
    })
}

/// Deletes a sticker. Its asset file stays until `kaia_store_gc`.
///
/// # Safety
/// `id` must be a NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn kaia_sticker_delete(handle: u64, id: *const c_char) -> i32 {
    guard(|| {
        let id = cstr(id, "id")?;
        registry::with_store(handle, |store| Ok(store.delete_sticker(id)?))
    })
}

/// Sets a sticker state to `attached`, `needs_review` or `orphaned`.
///
/// # Safety
/// `id` and `state` must be NUL-terminated strings.
#[no_mangle]
pub unsafe extern "C" fn kaia_sticker_set_state(handle: u64, id: *const c_char, state: *const c_char) -> i32 {
    guard(|| {
        let id = cstr(id, "id")?;
        let state = cstr(state, "state")?;
        let state = StickerState::parse(state)
            .ok_or_else(|| ValidationError::new("state", "must be attached, needs_review or orphaned"))?;
        registry::with_store(handle, |store| Ok(store.set_state(id, state)?))
    })
}

/// Copies a PNG or JPEG file into the store and writes its `AssetRef` as JSON.
///
/// # Safety
/// `path` and `license` must be NUL-terminated strings. `out_json` must be a valid
/// `KaiaBuf` pointer.
#[no_mangle]
pub unsafe extern "C" fn kaia_asset_add_file(
    handle: u64,
    path: *const c_char,
    license: *const c_char,
    out_json: *mut KaiaBuf,
) -> i32 {
    guard(|| {
        let out = non_null(out_json, "out_json")?;
        let path = cstr(path, "path")?;
        let license = cstr(license, "license")?;
        let asset = registry::with_store(
            handle,
            |store| Ok(store.add_asset_file(Path::new(path), license)?),
        )?;
        buf::write(out, to_json(&asset)?);
        Ok(())
    })
}

/// Writes the file path of an asset, as UTF-8 text without a terminating NUL.
///
/// # Safety
/// `sha256` must be a NUL-terminated string. `out_path` must be a valid `KaiaBuf` pointer.
#[no_mangle]
pub unsafe extern "C" fn kaia_asset_path(handle: u64, sha256: *const c_char, out_path: *mut KaiaBuf) -> i32 {
    guard(|| {
        let out = non_null(out_path, "out_path")?;
        let sha256 = cstr(sha256, "sha256")?;
        let path = registry::with_store(handle, |store| Ok(store.asset_path(sha256)?))?;
        buf::write(out, path_bytes(&path)?);
        Ok(())
    })
}

/// Removes unreferenced assets and writes the count to `out_removed`.
///
/// # Safety
/// `out_removed` must be valid for a `u32` write.
#[no_mangle]
pub unsafe extern "C" fn kaia_store_gc(handle: u64, out_removed: *mut u32) -> i32 {
    guard(|| {
        let out = non_null(out_removed, "out_removed")?;
        let removed = registry::with_store(handle, |store| Ok(store.gc()?))?;
        *out = removed;
        Ok(())
    })
}

/// Records that a document was opened. Writes 1 to `out_changed` when the stored
/// `file_size` differs, and then moves its attached stickers to `needs_review`.
/// An empty `title` means no title.
///
/// # Safety
/// `partial_md5` and `title` must be NUL-terminated strings. `out_changed` must be
/// valid for an `i32` write.
#[no_mangle]
pub unsafe extern "C" fn kaia_document_seen(
    handle: u64,
    partial_md5: *const c_char,
    file_size: u64,
    title: *const c_char,
    out_changed: *mut i32,
) -> i32 {
    guard(|| {
        let out = non_null(out_changed, "out_changed")?;
        let md5 = cstr(partial_md5, "partial_md5")?;
        let title = cstr(title, "title")?;
        let title = if title.is_empty() { None } else { Some(title) };
        let changed = registry::with_store(handle, |store| Ok(store.document_seen(md5, file_size, title)?))?;
        *out = i32::from(changed);
        Ok(())
    })
}

/// Writes the export directory path, which is `exports/kaia-export-<time>/` in the store.
///
/// # Safety
/// `out_dir` must be a valid `KaiaBuf` pointer.
#[no_mangle]
pub unsafe extern "C" fn kaia_export(handle: u64, out_dir: *mut KaiaBuf) -> i32 {
    guard(|| {
        let out = non_null(out_dir, "out_dir")?;
        let dir = registry::with_store(handle, |store| Ok(store.export()?))?;
        buf::write(out, path_bytes(&dir)?);
        Ok(())
    })
}

/// Imports a directory written by `kaia_export` and writes the counts as JSON.
///
/// # Safety
/// `dir` must be a NUL-terminated string. `out_report_json` must be a valid `KaiaBuf` pointer.
#[no_mangle]
pub unsafe extern "C" fn kaia_import(handle: u64, dir: *const c_char, out_report_json: *mut KaiaBuf) -> i32 {
    guard(|| {
        let out = non_null(out_report_json, "out_report_json")?;
        let dir = cstr(dir, "dir")?;
        let report = registry::with_store(handle, |store| Ok(store.import(Path::new(dir))?))?;
        buf::write(out, to_json(&report)?);
        Ok(())
    })
}

/// Finds `exact` in `haystack` with the rules in `STORAGE.md`. Writes the char
/// index, or -1 when there is no match. This needs no store.
///
/// # Safety
/// All four strings must be NUL-terminated. `out_char_index` must be valid for an
/// `i64` write.
#[no_mangle]
pub unsafe extern "C" fn kaia_quote_locate(
    haystack: *const c_char,
    exact: *const c_char,
    prefix: *const c_char,
    suffix: *const c_char,
    out_char_index: *mut i64,
) -> i32 {
    guard(|| {
        let out = non_null(out_char_index, "out_char_index")?;
        let haystack = cstr(haystack, "haystack")?;
        let exact = cstr(exact, "exact")?;
        let prefix = cstr(prefix, "prefix")?;
        let suffix = cstr(suffix, "suffix")?;
        let index = match quote::locate(haystack, exact, prefix, suffix) {
            Some(index) => i64::try_from(index)
                .map_err(|_| Failure::new(KAIA_E_INVALID_ARG, "match index does not fit in i64"))?,
            None => -1,
        };
        *out = index;
        Ok(())
    })
}

/// Writes the message of the most recent error on this thread. Returns
/// `KAIA_E_NOT_FOUND` when there is none. Reading does not change the message.
///
/// # Safety
/// `out` must be a valid `KaiaBuf` pointer.
#[no_mangle]
pub unsafe extern "C" fn kaia_last_error(out: *mut KaiaBuf) -> i32 {
    catch_unwind(|| {
        if out.is_null() {
            return KAIA_E_INVALID_ARG;
        }
        match error::last_error() {
            Some(msg) => {
                buf::write(out, msg.into_bytes());
                KAIA_OK
            }
            None => KAIA_E_NOT_FOUND,
        }
    })
    .unwrap_or(KAIA_E_PANIC)
}

/// Frees the bytes of a buffer and zeroes the struct. NULL, zeroed and already
/// freed buffers are no-ops.
///
/// # Safety
/// `buf` is NULL, or points to a `KaiaBuf` that libkaia wrote or that is zeroed.
#[no_mangle]
pub unsafe extern "C" fn kaia_buf_free(buf: *mut KaiaBuf) {
    let _ = guard(|| {
        buf::free(buf);
        Ok(())
    });
}

/// Panics on purpose. Test builds only, to check that a panic becomes `KAIA_E_PANIC`.
#[cfg(feature = "test-panic")]
#[no_mangle]
pub extern "C" fn kaia_debug_panic() -> i32 {
    guard(|| panic!("kaia debug panic requested"))
}
