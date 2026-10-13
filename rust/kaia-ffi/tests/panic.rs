//! Required test 4: a panic inside an export returns `KAIA_E_PANIC` and the process survives.
//! Built only with `--features test-panic`.

#![cfg(feature = "test-panic")]

mod common;

use common::{last_error, list, open, temp_store, MD5_A};
use kaia::{kaia_debug_panic, kaia_store_close, KAIA_E_PANIC, KAIA_OK};

#[test]
fn panic_returns_kaia_e_panic_and_records_the_message() {
    let rc = kaia_debug_panic();
    assert_eq!(rc, KAIA_E_PANIC);
    assert_eq!(last_error(), "panic: kaia debug panic requested");
}

#[test]
fn the_process_keeps_working_after_a_panic() {
    assert_eq!(kaia_debug_panic(), KAIA_E_PANIC);

    let (_tmp, dir) = temp_store();
    let handle = open(&dir);
    let (rc, _) = list(handle, MD5_A);
    assert_eq!(rc, KAIA_OK);
    assert_eq!(kaia_store_close(handle), KAIA_OK);
}

#[test]
fn panic_does_not_poison_the_registry() {
    // A panic inside a locked section would poison the registry mutex. Opening and
    // closing after a panic must still work, which shows the lock recovers.
    assert_eq!(kaia_debug_panic(), KAIA_E_PANIC);
    for _ in 0..3 {
        let (_tmp, dir) = temp_store();
        let handle = open(&dir);
        assert_eq!(kaia_store_close(handle), KAIA_OK);
    }
}
