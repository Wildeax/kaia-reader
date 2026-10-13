//! Required test 6: `kaia_buf_free` is safe on a zeroed buffer and on a second free.

mod common;

use common::{cstr, open, temp_store, zeroed, MD5_A};
use kaia::{kaia_buf_free, kaia_sticker_list, KaiaBuf, KAIA_OK};

fn is_zeroed(buf: &KaiaBuf) -> bool {
    buf.ptr.is_null() && buf.len == 0 && buf.cap == 0
}

#[test]
fn free_on_a_zeroed_buffer_is_a_no_op() {
    let mut buf = zeroed();
    // SAFETY: buf is a valid zeroed struct.
    unsafe { kaia_buf_free(&mut buf) };
    assert!(is_zeroed(&buf));
    // SAFETY: as above, a second time.
    unsafe { kaia_buf_free(&mut buf) };
    assert!(is_zeroed(&buf));
}

#[test]
fn free_twice_on_the_same_written_buffer_is_safe() {
    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    let md5 = cstr(MD5_A);
    let mut buf = zeroed();
    // SAFETY: md5 is NUL-terminated and buf is a valid out buffer.
    let rc = unsafe { kaia_sticker_list(handle, md5.as_ptr(), &mut buf) };
    assert_eq!(rc, KAIA_OK);
    // An empty list still gives "[]", so the buffer has bytes.
    assert_eq!(buf.len, 2);
    assert!(!buf.ptr.is_null());

    // SAFETY: buf was written by libkaia.
    unsafe { kaia_buf_free(&mut buf) };
    assert!(is_zeroed(&buf), "free zeroes the struct");
    // SAFETY: the struct is zeroed now, so this is the double free.
    unsafe { kaia_buf_free(&mut buf) };
    assert!(is_zeroed(&buf));
    assert_eq!(common::close(handle), KAIA_OK);
}

#[test]
fn free_of_null_is_a_no_op() {
    // SAFETY: NULL is documented as a no-op.
    unsafe { kaia_buf_free(std::ptr::null_mut()) };
}
