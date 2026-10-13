use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=KAIA_DJVU_INCLUDE");
    println!("cargo:rerun-if-env-changed=KAIA_DJVU_LIB");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let include = env::var_os("KAIA_DJVU_INCLUDE").map(PathBuf::from);
    let lib = env::var_os("KAIA_DJVU_LIB").map(PathBuf::from);
    let staging = if include.is_none() || lib.is_none() {
        desktop_staging(&manifest)
    } else {
        None
    };
    let include = include
        .or_else(|| staging.as_ref().map(|p| p.join("include")))
        .expect("DjVu headers missing: set KAIA_DJVU_INCLUDE");
    let lib = lib
        .or_else(|| staging.as_ref().map(|p| p.join("lib")))
        .expect("DjVu library missing: set KAIA_DJVU_LIB");
    assert!(
        include.join("libdjvu/ddjvuapi.h").exists(),
        "DjVu header missing in {}",
        include.display()
    );
    assert!(
        lib.join("libdjvulibre.a").exists(),
        "DjVu archive missing in {}",
        lib.display()
    );
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=static=djvulibre");
    if env::var("TARGET").unwrap_or_default().contains("apple-darwin") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }
}

fn desktop_staging(manifest: &Path) -> Option<PathBuf> {
    let host = env::var("HOST").ok()?;
    let tag = if host.contains("apple-darwin") {
        "apple-darwin"
    } else if host.contains("linux") {
        "linux"
    } else {
        return None;
    };
    let mut candidates: Vec<_> = fs::read_dir(manifest.join("../../base/build"))
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.contains(tag) && !s.contains("kobo"))
        })
        .map(|path| path.join("staging"))
        .filter(|path| path.join("lib/libdjvulibre.a").exists())
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}
