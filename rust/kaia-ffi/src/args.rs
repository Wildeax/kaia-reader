//! Checks on the raw arguments that Lua passes. Each check returns
//! `KAIA_E_INVALID_ARG` on failure.

use std::ffi::{c_char, CStr};
use std::slice;
use std::str;

use crate::error::Failure;

/// Largest JSON input accepted by `kaia_sticker_put`, in bytes.
pub const JSON_MAX_BYTES: usize = 1 << 20;

/// Rejects a NULL out pointer.
pub fn non_null<T>(ptr: *mut T, what: &str) -> Result<*mut T, Failure> {
    if ptr.is_null() {
        Err(Failure::invalid(format!("{what} is NULL")))
    } else {
        Ok(ptr)
    }
}

/// Reads a NUL-terminated UTF-8 string.
///
/// # Safety
/// A non-NULL `ptr` must point to a NUL-terminated string that stays valid for `'a`.
pub unsafe fn cstr<'a>(ptr: *const c_char, what: &str) -> Result<&'a str, Failure> {
    if ptr.is_null() {
        return Err(Failure::invalid(format!("{what} is NULL")));
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map_err(|_| Failure::invalid(format!("{what} is not valid UTF-8")))
}

/// Reads a JSON input of `len` bytes. Inputs above `JSON_MAX_BYTES` are refused
/// before any byte is read or parsed.
///
/// # Safety
/// When `len` is non-zero, `ptr` must be valid for `len` bytes.
pub unsafe fn json_text<'a>(ptr: *const u8, len: usize) -> Result<&'a str, Failure> {
    if len > JSON_MAX_BYTES {
        return Err(Failure::invalid(format!(
            "JSON input is {len} bytes, the limit is {JSON_MAX_BYTES}"
        )));
    }
    if len == 0 {
        return Ok("");
    }
    if ptr.is_null() {
        return Err(Failure::invalid("json is NULL"));
    }
    let bytes = slice::from_raw_parts(ptr, len);
    str::from_utf8(bytes).map_err(|_| Failure::invalid("json is not valid UTF-8"))
}
