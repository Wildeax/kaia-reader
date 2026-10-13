//! Required test 2: closed, never-issued and zero handles return `KAIA_E_BAD_HANDLE`.

mod common;

use common::{close, cstr, get, list, open, put, sticker_json, temp_store, zeroed, MD5_A};
use kaia::{kaia_export, kaia_sticker_list_all, kaia_store_close, kaia_store_gc, KAIA_E_BAD_HANDLE, KAIA_OK};

/// Every store function, called with `handle`. Returns the first code that is not `KAIA_E_BAD_HANDLE`
/// or `KAIA_OK`, if any.
fn all_calls_codes(handle: u64) -> Vec<i32> {
    let id = cstr("00000000-0000-4000-8000-000000000000");
    let json = sticker_json(MD5_A, "q");
    let mut codes = Vec::new();
    codes.push(put(handle, &json).0);
    codes.push(get(handle, "00000000-0000-4000-8000-000000000000").0);
    codes.push(list(handle, MD5_A).0);
    let mut out = zeroed();
    // SAFETY: out is a valid buffer, handle is only checked, never dereferenced.
    codes.push(unsafe { kaia_sticker_list_all(handle, &mut out) });
    codes.push(unsafe { kaia::kaia_sticker_delete(handle, id.as_ptr()) });
    codes.push(unsafe { kaia::kaia_sticker_set_state(handle, id.as_ptr(), cstr("attached").as_ptr()) });
    let md5 = cstr(MD5_A);
    codes.push(unsafe { kaia::kaia_document_seen(handle, md5.as_ptr(), 1, cstr("").as_ptr(), &mut 0i32) });
    let mut removed = 0u32;
    // SAFETY: removed is a valid u32 out pointer.
    codes.push(unsafe { kaia_store_gc(handle, &mut removed) });
    let mut dir = zeroed();
    // SAFETY: dir is a valid buffer.
    codes.push(unsafe { kaia_export(handle, &mut dir) });
    codes.push(close(handle));
    codes
}

#[test]
fn handle_zero_is_never_valid() {
    for code in all_calls_codes(0) {
        assert_eq!(code, KAIA_E_BAD_HANDLE);
    }
}

#[test]
fn never_issued_handles_are_rejected() {
    // Generation 1, slot far beyond anything issued; generation 7 on slot 0;
    // and a value with only the generation bits set.
    for handle in [(1u64 << 32) | 0xFFFF, (7u64 << 32) | 1, 0xFFFF_FFFF_0000_0000] {
        for code in all_calls_codes(handle) {
            assert_eq!(code, KAIA_E_BAD_HANDLE, "handle {handle:#x}");
        }
    }
}

#[test]
fn closed_handle_is_rejected_and_second_close_fails() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    assert_eq!(close(handle), KAIA_OK);
    assert_eq!(close(handle), KAIA_E_BAD_HANDLE);
    for code in all_calls_codes(handle) {
        assert_eq!(code, KAIA_E_BAD_HANDLE);
    }
}

#[test]
fn stale_handle_does_not_reach_a_store_reusing_its_slot() {
    let (_tmp, dir) = temp_store();
    let old = open(&dir);
    assert_eq!(close(old), KAIA_OK);

    // The next open reuses the slot. The generation has moved on, so the old handle must not match.
    let new = open(&dir);
    assert_ne!(old, new);
    assert_ne!(new, 0);
    let (rc, _) = list(old, MD5_A);
    assert_eq!(rc, KAIA_E_BAD_HANDLE);
    let (rc, _) = list(new, MD5_A);
    assert_eq!(rc, KAIA_OK);
    assert_eq!(close(new), KAIA_OK);
}

#[test]
fn close_of_zero_and_unknown_handles_fail_cleanly() {
    assert_eq!(kaia_store_close(0), KAIA_E_BAD_HANDLE);
    assert_eq!(kaia_store_close(u64::MAX), KAIA_E_BAD_HANDLE);
}
