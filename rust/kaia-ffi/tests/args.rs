//! Required test 3: NULL pointers and invalid UTF-8 return `KAIA_E_INVALID_ARG`.
//! Also checks the error messages and the thread-local last error.

mod common;

use std::ffi::CString;

use common::{cstr, get, last_error, list, open, put, sticker_json, take_string, temp_store, zeroed, MD5_A};
use kaia::{
    kaia_document_seen, kaia_export, kaia_last_error, kaia_quote_locate, kaia_sticker_get,
    kaia_sticker_set_state, kaia_store_open, KAIA_E_INVALID_ARG, KAIA_E_NOT_FOUND, KAIA_E_VALIDATION,
    KAIA_OK,
};

#[test]
fn null_string_arguments_are_invalid() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    let id = cstr("x");
    let mut out = zeroed();
    let mut handle_out = 0u64;

    // SAFETY: the NULL cases are the point of the test. Non-NULL arguments are valid.
    unsafe {
        assert_eq!(
            kaia_store_open(std::ptr::null(), &mut handle_out),
            KAIA_E_INVALID_ARG
        );
        assert_eq!(
            kaia_sticker_get(handle, std::ptr::null(), &mut out),
            KAIA_E_INVALID_ARG
        );
        assert_eq!(
            kaia_sticker_get(handle, id.as_ptr(), std::ptr::null_mut()),
            KAIA_E_INVALID_ARG
        );
        assert_eq!(
            kaia_sticker_set_state(handle, id.as_ptr(), std::ptr::null()),
            KAIA_E_INVALID_ARG
        );
        assert_eq!(
            kaia_document_seen(handle, std::ptr::null(), 1, id.as_ptr(), &mut 0i32),
            KAIA_E_INVALID_ARG
        );
        // An empty title is allowed. A NULL title is not.
        assert_eq!(
            kaia_document_seen(handle, cstr(MD5_A).as_ptr(), 1, std::ptr::null(), &mut 0i32),
            KAIA_E_INVALID_ARG
        );
        assert_eq!(kaia_export(handle, std::ptr::null_mut()), KAIA_E_INVALID_ARG);
        assert_eq!(
            kaia_quote_locate(id.as_ptr(), std::ptr::null(), id.as_ptr(), id.as_ptr(), &mut 0i64),
            KAIA_E_INVALID_ARG
        );
        assert_eq!(
            kaia_quote_locate(
                id.as_ptr(),
                id.as_ptr(),
                id.as_ptr(),
                id.as_ptr(),
                std::ptr::null_mut()
            ),
            KAIA_E_INVALID_ARG
        );
        assert_eq!(
            kaia_store_open(id.as_ptr(), std::ptr::null_mut()),
            KAIA_E_INVALID_ARG
        );
    }
    assert_eq!(list(handle, MD5_A).0, KAIA_OK);
    assert_eq!(common::close(handle), KAIA_OK);
}

#[test]
fn null_json_with_length_is_invalid() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    let mut out = zeroed();
    // SAFETY: a NULL json pointer with len > 0 is the case under test.
    let rc = unsafe { kaia::kaia_sticker_put(handle, std::ptr::null(), 10, &mut out) };
    assert_eq!(rc, KAIA_E_INVALID_ARG);
    assert!(last_error().contains("NULL"), "message: {}", last_error());
    assert_eq!(common::close(handle), KAIA_OK);
}

#[test]
fn invalid_utf8_strings_are_invalid() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    // 0xFF and 0xFE can never appear in UTF-8.
    let bad = CString::new(vec![b'a', 0xFF, 0xFE]).expect("no interior NUL");
    let mut out = zeroed();
    let mut handle_out = 0u64;

    // SAFETY: bad is NUL-terminated. Only its UTF-8 validity is wrong.
    unsafe {
        assert_eq!(kaia_store_open(bad.as_ptr(), &mut handle_out), KAIA_E_INVALID_ARG);
        assert_eq!(
            kaia_sticker_get(handle, bad.as_ptr(), &mut out),
            KAIA_E_INVALID_ARG
        );
        assert_eq!(
            kaia_sticker_set_state(handle, cstr("id").as_ptr(), bad.as_ptr()),
            KAIA_E_INVALID_ARG
        );
        assert_eq!(
            kaia_quote_locate(
                bad.as_ptr(),
                cstr("x").as_ptr(),
                cstr("").as_ptr(),
                cstr("").as_ptr(),
                &mut 0i64
            ),
            KAIA_E_INVALID_ARG
        );
    }
    let (rc, _) = list(handle, MD5_A);
    assert_eq!(rc, KAIA_OK);

    // The JSON bytes are text too, so invalid UTF-8 there is invalid argument.
    let bytes: &[u8] = &[b'{', 0xFF, b'}'];
    let mut out = zeroed();
    // SAFETY: bytes is valid for its length.
    let rc = unsafe { kaia::kaia_sticker_put(handle, bytes.as_ptr(), bytes.len(), &mut out) };
    assert_eq!(rc, KAIA_E_INVALID_ARG);
    assert_eq!(common::close(handle), KAIA_OK);
}

#[test]
fn malformed_json_is_validation_error_with_serde_message() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    let (rc, _) = put(handle, "{\"doc\": ");
    assert_eq!(rc, KAIA_E_VALIDATION);
    let message = last_error();
    assert!(message.contains("EOF"), "serde message expected, got: {message}");
    assert_eq!(common::close(handle), KAIA_OK);
}

#[test]
fn unknown_sticker_state_is_validation_error() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    let (rc, stored) = put(handle, &sticker_json(MD5_A, "quote text"));
    assert_eq!(rc, KAIA_OK);
    let id = common::sticker_id(&stored);
    let id = cstr(&id);
    // SAFETY: both strings are NUL-terminated.
    let rc = unsafe { kaia_sticker_set_state(handle, id.as_ptr(), cstr("archived").as_ptr()) };
    assert_eq!(rc, KAIA_E_VALIDATION);
    assert!(
        last_error().contains("state"),
        "message names the field: {}",
        last_error()
    );
    assert_eq!(common::close(handle), KAIA_OK);
}

#[test]
fn a_failed_call_sets_the_last_error_and_reading_keeps_it() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    let (rc, _) = get(handle, "00000000-0000-4000-8000-000000000000");
    assert_eq!(rc, KAIA_E_NOT_FOUND);
    let first = last_error();
    assert!(first.contains("not found"), "got: {first}");
    // Reading the last error does not change it.
    assert_eq!(last_error(), first);
    assert_eq!(common::close(handle), KAIA_OK);
}

#[test]
fn last_error_is_per_thread_and_reports_none_when_empty() {
    let code = std::thread::spawn(|| {
        let mut out = zeroed();
        // SAFETY: out is a valid buffer.
        unsafe { kaia_last_error(&mut out) }
    })
    .join()
    .expect("thread finishes");
    assert_eq!(code, KAIA_E_NOT_FOUND);

    // An error on this thread is invisible to a new thread.
    let (rc, _) = get(0, "x");
    assert_ne!(rc, KAIA_OK);
    let other = std::thread::spawn(|| {
        let mut out = zeroed();
        // SAFETY: out is a valid buffer.
        let rc = unsafe { kaia_last_error(&mut out) };
        (rc, out.len)
    })
    .join()
    .expect("thread finishes");
    assert_eq!(other.0, KAIA_E_NOT_FOUND);
    let mut out = zeroed();
    // SAFETY: out is a valid buffer.
    assert_eq!(unsafe { kaia_last_error(&mut out) }, KAIA_OK);
    assert!(take_string(&mut out).contains("handle"));
}

#[test]
fn last_error_rejects_null_out() {
    // SAFETY: NULL is the case under test.
    assert_eq!(
        unsafe { kaia_last_error(std::ptr::null_mut()) },
        KAIA_E_INVALID_ARG
    );
}
