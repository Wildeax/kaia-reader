//! Required test 1: 10,000 open, put, list, close cycles leave no live allocations behind.
//!
//! This file is its own test binary, so the counting allocator sees only this test.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};

use common::{close, delete, list, open, put, sticker_id, sticker_json, temp_store, MD5_A};

/// Bytes currently allocated by the process.
static LIVE_BYTES: AtomicIsize = AtomicIsize::new(0);

struct Counting;

// SAFETY: every call forwards to the system allocator with the same layout.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            LIVE_BYTES.fetch_add(layout.size() as isize, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE_BYTES.fetch_sub(layout.size() as isize, Ordering::SeqCst);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            LIVE_BYTES.fetch_add(new_size as isize - layout.size() as isize, Ordering::SeqCst);
        }
        new_ptr
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// One open, put, list, close cycle. The sticker is deleted before the store
/// closes, so each list sees one sticker and the work per cycle stays constant.
fn cycle(dir: &std::path::Path, json: &str) {
    let handle = open(dir);
    let (rc, stored) = put(handle, json);
    assert_eq!(rc, kaia::KAIA_OK);
    let (rc, _) = list(handle, MD5_A);
    assert_eq!(rc, kaia::KAIA_OK);
    let id = sticker_id(&stored);
    assert_eq!(delete(handle, &id), kaia::KAIA_OK);
    assert_eq!(close(handle), kaia::KAIA_OK);
}

#[test]
fn ten_thousand_cycles_do_not_grow_live_allocations() {
    const CYCLES: usize = 10_000;
    // Allowed growth after warm-up, in bytes. Covers allocator bookkeeping and
    // SQLite caches that the process keeps across opens.
    const BOUND: isize = 256 * 1024;

    let (_tmp, dir) = temp_store();
    let json = sticker_json(MD5_A, "quoted text");

    // Warm up once so one-time allocations (thread locals, the registry Vec,
    // the first SQLite open) happen before the measurement starts.
    cycle(&dir, &json);
    let before = LIVE_BYTES.load(Ordering::SeqCst);

    for _ in 0..CYCLES {
        cycle(&dir, &json);
    }

    let after = LIVE_BYTES.load(Ordering::SeqCst);
    let growth = after - before;
    assert!(
        growth <= BOUND,
        "live bytes grew by {growth} over {CYCLES} cycles (bound {BOUND})"
    );
}
