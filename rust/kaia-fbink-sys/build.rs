//! Links FBInk when `KAIA_FBINK_LIB` is set.
//!
//! `KAIA_FBINK_LIB` is a directory that holds `libfbink.a` or `libfbink.so`. The
//! static archive wins when both exist. When the variable is unset, the script emits
//! nothing: `cargo check` and the unit tests need no FBInk, and a final link that
//! calls FBInk fails with undefined symbols until the variable points at a build.
//!
//! The FBInk build that KOReader uses (`base/thirdparty/fbink/CMakeLists.txt`) sets
//! `MINIMAL=1` and does not enable image support. `fbink_print_raw_data` needs
//! image support, so build FBInk with `IMAGE=1` for Kaia.

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=KAIA_FBINK_LIB");

    let Some(dir) = env::var_os("KAIA_FBINK_LIB").map(PathBuf::from) else {
        return;
    };

    if !dir.is_dir() {
        panic!("KAIA_FBINK_LIB={} is not a directory", dir.display());
    }

    println!("cargo:rustc-link-search=native={}", dir.display());
    if dir.join("libfbink.a").is_file() {
        println!("cargo:rustc-link-lib=static=fbink");
    } else if dir.join("libfbink.so").is_file() {
        println!("cargo:rustc-link-lib=dylib=fbink");
    } else {
        panic!(
            "KAIA_FBINK_LIB={} has neither libfbink.a nor libfbink.so",
            dir.display()
        );
    }
}
