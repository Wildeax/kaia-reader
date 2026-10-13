//! The `kaia_buf` struct. Bytes written into a buffer belong to libkaia until
//! `kaia_buf_free` runs.

use std::ptr;

/// Mirrors `kaia_buf` in `include/kaia.h`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct KaiaBuf {
    /// Owned by libkaia. NULL when `len` is 0.
    pub ptr: *mut u8,
    pub len: usize,
    /// Allocation size, which is `len` for every buffer this crate writes.
    pub cap: usize,
}

impl KaiaBuf {
    pub const fn zeroed() -> Self {
        KaiaBuf {
            ptr: ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
}

/// Stores `bytes` in `*out`. The buffer owns them until `free` runs.
///
/// # Safety
/// `out` must be valid for writes. Bytes the struct already owns are dropped
/// from the struct without being freed, so callers pass a zeroed or freed buffer.
pub unsafe fn write(out: *mut KaiaBuf, bytes: Vec<u8>) {
    let boxed = bytes.into_boxed_slice();
    let len = boxed.len();
    let value = if len == 0 {
        KaiaBuf::zeroed()
    } else {
        KaiaBuf {
            ptr: Box::into_raw(boxed).cast::<u8>(),
            len,
            cap: len,
        }
    };
    ptr::write(out, value);
}

/// Frees the bytes and zeroes the struct. Safe on a zeroed struct, and safe to
/// call twice on the same struct because the second call sees the zeroed value.
///
/// # Safety
/// `buf` is NULL, or points to a `KaiaBuf` that this crate wrote or that is zeroed.
pub unsafe fn free(buf: *mut KaiaBuf) {
    if buf.is_null() {
        return;
    }
    let owned = *buf;
    if !owned.ptr.is_null() {
        drop(Box::from_raw(ptr::slice_from_raw_parts_mut(owned.ptr, owned.cap)));
    }
    ptr::write(buf, KaiaBuf::zeroed());
}
