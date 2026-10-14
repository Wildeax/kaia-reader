//! Links the shared libarchive built by KOReader, also used by MuPDF.
use std::{env, fs, path::PathBuf};

fn main() {
    for key in ["KAIA_ARCHIVE_LIB", "KAIA_MUPDF_LIB", "KAIA_MUPDF_DEP_LIBS"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    let target = env::var("TARGET").expect("TARGET");
    let host = env::var("HOST").expect("HOST");
    let filename = if target.contains("apple-darwin") {
        "libarchive.dylib"
    } else {
        "libarchive.so"
    };
    let explicit = env::var_os("KAIA_ARCHIVE_LIB").map(PathBuf::from);
    let lib = explicit.unwrap_or_else(|| {
        let mut dirs: Vec<PathBuf> = env::var_os("KAIA_MUPDF_DEP_LIBS")
            .map(|list| env::split_paths(&list).collect())
            .unwrap_or_default();
        dirs.extend(env::var_os("KAIA_MUPDF_LIB").map(PathBuf::from));
        if dirs.is_empty() && host == target {
            let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
                .join("../../base/build");
            let tag = if host.contains("apple-darwin") { "apple-darwin" } else { "linux" };
            dirs = fs::read_dir(root).into_iter().flatten().filter_map(Result::ok)
                .filter(|entry| {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    name.contains(tag) && !name.contains("kobo")
                })
                .map(|entry| entry.path().join("staging/lib")).collect();
            dirs.sort();
        }
        dirs.into_iter().find(|dir| dir.join(filename).is_file())
            .expect("KOReader libarchive missing; build the emulator or set KAIA_ARCHIVE_LIB to the target staging/lib directory")
    });
    assert!(
        lib.join(filename).is_file(),
        "missing {filename} in {}",
        lib.display()
    );
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=archive");
    if host == target {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
        println!("cargo:rpath_dirs={}", lib.display());
    }
}
