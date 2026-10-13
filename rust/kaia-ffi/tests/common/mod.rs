//! Helpers that call the exported C functions the way Lua does.

#![allow(dead_code)]

use std::ffi::CString;
use std::path::Path;
use std::slice;

use kaia::{
    kaia_buf_free, kaia_last_error, kaia_sticker_delete, kaia_sticker_get, kaia_sticker_list,
    kaia_sticker_put, kaia_store_close, kaia_store_open, KaiaBuf, KAIA_OK,
};

pub const MD5_A: &str = "0123456789abcdef0123456789abcdef";
pub const MD5_B: &str = "fedcba9876543210fedcba9876543210";

/// 1x1 red PNG, 69 bytes. Checked with `sips`.
pub const PNG_RED: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00,
    0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01,
    0x01, 0x00, 0xC9, 0xFE, 0x92, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
    0x82,
];

pub fn cstr(s: &str) -> CString {
    CString::new(s).expect("test strings have no NUL")
}

pub fn zeroed() -> KaiaBuf {
    KaiaBuf::zeroed()
}

/// Copies the bytes out of a buffer and frees it.
pub fn take_bytes(buf: &mut KaiaBuf) -> Vec<u8> {
    let bytes = if buf.ptr.is_null() {
        Vec::new()
    } else {
        // SAFETY: the buffer was written by libkaia, so ptr is valid for len bytes.
        unsafe { slice::from_raw_parts(buf.ptr, buf.len).to_vec() }
    };
    // SAFETY: buf is a struct that libkaia wrote.
    unsafe { kaia_buf_free(buf) };
    bytes
}

pub fn take_string(buf: &mut KaiaBuf) -> String {
    String::from_utf8(take_bytes(buf)).expect("libkaia writes UTF-8")
}

/// The last error message on the calling thread, or `none`.
pub fn last_error() -> String {
    let mut buf = zeroed();
    // SAFETY: buf is a valid out pointer.
    let rc = unsafe { kaia_last_error(&mut buf) };
    if rc != KAIA_OK {
        return format!("none (rc {rc})");
    }
    take_string(&mut buf)
}

/// Opens a store and asserts success.
pub fn open(dir: &Path) -> u64 {
    let dir = cstr(dir.to_str().expect("temp paths are UTF-8"));
    let mut handle = 0u64;
    // SAFETY: dir is NUL-terminated and handle is valid for a write.
    let rc = unsafe { kaia_store_open(dir.as_ptr(), &mut handle) };
    assert_eq!(rc, KAIA_OK, "open failed: {}", last_error());
    handle
}

pub fn close(handle: u64) -> i32 {
    kaia_store_close(handle)
}

/// Puts one sticker JSON and returns the return code plus the stored JSON
/// (empty on error).
pub fn put(handle: u64, json: &str) -> (i32, String) {
    let mut out = zeroed();
    // SAFETY: json is valid for its length and out is a valid buffer.
    let rc = unsafe { kaia_sticker_put(handle, json.as_ptr(), json.len(), &mut out) };
    if rc == KAIA_OK {
        (rc, take_string(&mut out))
    } else {
        (rc, String::new())
    }
}

pub fn get(handle: u64, id: &str) -> (i32, String) {
    let id = cstr(id);
    let mut out = zeroed();
    // SAFETY: id is NUL-terminated and out is a valid buffer.
    let rc = unsafe { kaia_sticker_get(handle, id.as_ptr(), &mut out) };
    if rc == KAIA_OK {
        (rc, take_string(&mut out))
    } else {
        (rc, String::new())
    }
}

pub fn list(handle: u64, md5: &str) -> (i32, String) {
    let md5 = cstr(md5);
    let mut out = zeroed();
    // SAFETY: md5 is NUL-terminated and out is a valid buffer.
    let rc = unsafe { kaia_sticker_list(handle, md5.as_ptr(), &mut out) };
    if rc == KAIA_OK {
        (rc, take_string(&mut out))
    } else {
        (rc, String::new())
    }
}

pub fn delete(handle: u64, id: &str) -> i32 {
    let id = cstr(id);
    // SAFETY: id is NUL-terminated.
    unsafe { kaia_sticker_delete(handle, id.as_ptr()) }
}

/// The `id` field of a stored sticker JSON. The stored form is known, so a
/// plain search is enough.
pub fn sticker_id(json: &str) -> String {
    let start = json.find("\"id\":\"").expect("stored sticker has an id") + 6;
    let end = start + json[start..].find('"').expect("id ends with a quote");
    json[start..end].to_string()
}

/// A text-only reflowable sticker as JSON, without an id.
pub fn sticker_json(md5: &str, exact: &str) -> String {
    format!(
        r#"{{"doc":{{"partial_md5":"{md5}","file_size":1000,"title":"Test book"}},
            "anchor":{{"type":"reflowable","locator":"/body/p[2]",
                "quote":{{"exact":"{exact}","prefix":"a ","suffix":" b"}},
                "offset":{{"dx":0,"dy":-1.2}}}},
            "asset":null,"text":"note",
            "size":{{"w":0.1,"h":0.07}},"rotation":0,"state":"attached"}}"#
    )
}

/// A fresh temp directory and the store path inside it.
pub fn temp_store() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let dir = tmp.path().join("kaia");
    (tmp, dir)
}
