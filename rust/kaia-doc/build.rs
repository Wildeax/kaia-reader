//! Makes the MuPDF library directories available to this crate.
//!
//! The shared dependencies (freetype, harfbuzz, ...) sit in the KOReader
//! staging tree. `kaia-mupdf-sys` publishes the directories through its `links`
//! metadata. This script passes them to link steps as run-time search paths and
//! exposes the fallback font directory to tests as `KAIA_DOC_FONT_DIR`.

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
        println!("cargo:rustc-env=KAIA_DOC_FONT_DIR={}", fonts.to_string_lossy());
    }
}
