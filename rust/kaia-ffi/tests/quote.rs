//! Required test 7: property tests for `kaia_quote_locate` with random text.
//!
//! The FFI result must equal `kaia_core::quote::locate`, and an inserted quote must be found.

mod common;

use std::ffi::CString;

use kaia::{kaia_quote_locate, KAIA_OK};
use proptest::prelude::*;

/// Calls the export and returns its char index, or -1.
fn ffi_locate(haystack: &str, exact: &str, prefix: &str, suffix: &str) -> i64 {
    let h = CString::new(haystack).expect("no NUL");
    let e = CString::new(exact).expect("no NUL");
    let p = CString::new(prefix).expect("no NUL");
    let s = CString::new(suffix).expect("no NUL");
    let mut index = -2i64;
    // SAFETY: all four strings are NUL-terminated and index is valid for a write.
    let rc = unsafe { kaia_quote_locate(h.as_ptr(), e.as_ptr(), p.as_ptr(), s.as_ptr(), &mut index) };
    assert_eq!(rc, KAIA_OK);
    index
}

fn chars_at(text: &str, index: usize, len: usize) -> String {
    text.chars().skip(index).take(len).collect()
}

proptest! {
    #[test]
    fn ffi_agrees_with_the_core_function(
        haystack in "\\PC{0,200}",
        exact in "\\PC{0,40}",
        prefix in "\\PC{0,20}",
        suffix in "\\PC{0,20}",
    ) {
        let got = ffi_locate(&haystack, &exact, &prefix, &suffix);
        let want = kaia_core::quote::locate(&haystack, &exact, &prefix, &suffix)
            .map_or(-1, |i| i64::try_from(i).expect("small index"));
        prop_assert_eq!(got, want);
    }

    #[test]
    fn a_match_points_at_the_quote(
        haystack in "\\PC{0,200}",
        exact in "\\PC{1,40}",
        prefix in "\\PC{0,20}",
        suffix in "\\PC{0,20}",
    ) {
        let index = ffi_locate(&haystack, &exact, &prefix, &suffix);
        if index >= 0 {
            let index = usize::try_from(index).expect("non-negative");
            prop_assert_eq!(chars_at(&haystack, index, exact.chars().count()), exact);
        }
    }

    #[test]
    fn an_inserted_quote_is_found(
        before in "\\PC{0,200}",
        exact in "\\PC{1,40}",
        after in "\\PC{0,200}",
    ) {
        let haystack = format!("{before}{exact}{after}");
        let b: Vec<char> = before.chars().collect();
        let a: Vec<char> = after.chars().collect();
        let prefix: String = b[b.len().saturating_sub(64)..].iter().collect();
        let suffix: String = a[..a.len().min(64)].iter().collect();

        let index = ffi_locate(&haystack, &exact, &prefix, &suffix);
        prop_assert!(index >= 0, "quote was inserted, so it must be found");
        let index = usize::try_from(index).expect("non-negative");
        prop_assert!(index <= b.len(), "the first full match is at or before the insertion");
        prop_assert_eq!(chars_at(&haystack, index, exact.chars().count()), exact);
    }
}

#[test]
fn empty_quote_and_missing_quote_return_minus_one() {
    assert_eq!(ffi_locate("abc", "", "", ""), -1);
    assert_eq!(ffi_locate("abc", "zz", "", ""), -1);
    assert_eq!(ffi_locate("the cat sat", "cat", "the ", " sat"), 4);
    assert_eq!(ffi_locate("éèà target", "target", "à ", ""), 4);
}
