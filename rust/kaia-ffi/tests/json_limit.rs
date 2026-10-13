//! Required test 5: a 64 MiB JSON input is rejected before parsing. The limit is 1 MiB.

mod common;

use common::{last_error, open, temp_store, zeroed};
use kaia::{kaia_sticker_put, KAIA_E_INVALID_ARG, KAIA_E_VALIDATION, KAIA_OK};

const MIB: usize = 1 << 20;

#[test]
fn sixty_four_mib_input_is_refused_before_parsing() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    // Valid start, then a payload that only a parser would reject. A parser error
    // would give KAIA_E_VALIDATION, so INVALID_ARG proves the size check ran first.
    let mut json = vec![b' '; 64 * MIB];
    json[0] = b'{';
    let mut out = zeroed();
    // SAFETY: json is valid for its length. libkaia must not read past the limit.
    let rc = unsafe { kaia_sticker_put(handle, json.as_ptr(), json.len(), &mut out) };
    assert_eq!(rc, KAIA_E_INVALID_ARG);
    assert!(last_error().contains("limit is"), "message: {}", last_error());
    assert!(out.ptr.is_null(), "out is untouched on error");
    assert_eq!(common::close(handle), KAIA_OK);
}

#[test]
fn length_is_checked_even_with_a_null_pointer() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    let mut out = zeroed();
    // SAFETY: NULL with a huge length. libkaia must not dereference it.
    let rc = unsafe { kaia_sticker_put(handle, std::ptr::null(), 64 * MIB, &mut out) };
    assert_eq!(rc, KAIA_E_INVALID_ARG);
    assert_eq!(common::close(handle), KAIA_OK);
}

#[test]
fn exactly_one_mib_is_parsed_and_fails_as_validation() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    // At the limit the input is parsed. Spaces with no object give a serde error.
    let json = vec![b' '; MIB];
    let mut out = zeroed();
    // SAFETY: json is valid for its length.
    let rc = unsafe { kaia_sticker_put(handle, json.as_ptr(), json.len(), &mut out) };
    assert_eq!(rc, KAIA_E_VALIDATION, "message: {}", last_error());
    assert_eq!(common::close(handle), KAIA_OK);
}
