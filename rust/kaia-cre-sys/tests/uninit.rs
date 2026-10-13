//! Runs in its own process, so no other test has called `init` before it.

use kaia_cre_sys::{Doc, Error};

#[test]
fn doc_new_before_init_is_an_error_not_a_crash() {
    // Before the fix this dereferenced the null font manager and segfaulted.
    match Doc::new(600, 800) {
        Err(Error::Engine(msg)) => assert!(msg.contains("not initialized"), "message: {msg}"),
        Err(other) => panic!("unexpected error: {other}"),
        Ok(_) => panic!("Doc::new must fail before init"),
    }
    // Still an error on a second try: nothing was left half built.
    assert!(Doc::new(600, 800).is_err());
}
