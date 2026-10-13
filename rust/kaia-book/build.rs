//! Makes the MuPDF library directories available to this crate's test binaries.
//!
//! `kaia-doc` already does this for its own tests. Its link arguments do not
//! reach dependent crates, so this script repeats the step for `kaia-book`.
//! It reads the metadata that `kaia-mupdf-sys` publishes to direct dependents
//! and exposes the fallback font directory to the tests.

use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=DEP_KAIA_MUPDF_RPATH_DIRS");
    println!("cargo:rerun-if-env-changed=DEP_KAIA_MUPDF_FONTS_DIR");

    if let Some(list) = env::var_os("DEP_KAIA_MUPDF_RPATH_DIRS") {
        for dir in env::split_paths(&list).filter(|d| !d.as_os_str().is_empty()) {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
        }
    }
    if let Some(fonts) = env::var_os("DEP_KAIA_MUPDF_FONTS_DIR") {
        println!("cargo:rustc-env=KAIA_BOOK_FONT_DIR={}", fonts.to_string_lossy());
    }
}
