//! Passes the MuPDF library directories to this crate's link steps as run-time
//! search paths. The binaries and the integration tests load the shared
//! dependencies (freetype, harfbuzz, ...) from the KOReader staging tree, the
//! same way `kaia-doc` does. Link arguments do not pass to dependents, so the
//! list is read again here.

use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=KAIA_LUAJIT_LIB");
    if env::var_os("CARGO_FEATURE_LUA").is_some() {
        if env::var("HOST") == env::var("TARGET") {
            if let Some(dir) = env::var_os("KAIA_LUAJIT_LIB") {
                println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.to_string_lossy());
            }
        } else {
            println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/libs");
        }
    }
    println!("cargo:rerun-if-env-changed=DEP_KAIA_MUPDF_RPATH_DIRS");
    if let Some(list) = env::var_os("DEP_KAIA_MUPDF_RPATH_DIRS") {
        for dir in env::split_paths(&list).filter(|d| !d.as_os_str().is_empty()) {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
        }
    }
}
