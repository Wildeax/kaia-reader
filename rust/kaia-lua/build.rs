//! Checks that the LuaJIT linked into this crate is KOReader's.
//!
//! mlua-sys links the library named by `LUA_LIB` and `LUA_LIB_NAME` (see
//! `env.sh`). This script finds the same library, from `KAIA_LUAJIT_LIB` or the
//! desktop build under `base/build/*/libs`, and fails if the two disagree. It
//! also reads the version from KOReader's `luajit.h`, so a test can compare it
//! with the running runtime.
//!
//! The dynamic library is linked, not the static archive. KOReader's native
//! modules (`libs/libkoreader-lfs.so` and others) bind to `libs/libluajit.dylib`.
//! A static copy inside this binary would give the process a second LuaJIT
//! image, with its own globals. Linking the same dynamic library keeps one image.
//! Test binaries get an rpath to that directory.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    for var in [
        "KAIA_LUAJIT_LIB",
        "KAIA_LUAJIT_INCLUDE",
        "LUA_LIB",
        "LUA_LIB_NAME",
        "LUA_LINK",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-changed=build.rs");

    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let host = env::var("HOST").unwrap_or_default();
    let target = env::var("TARGET").unwrap_or_default();
    if host != target
        && (env::var_os("KAIA_LUAJIT_LIB").is_none() || env::var_os("KAIA_LUAJIT_INCLUDE").is_none())
    {
        panic!("Cross builds need the target KOReader KAIA_LUAJIT_LIB and KAIA_LUAJIT_INCLUDE");
    }

    let lib_dir = locate_lib_dir(&manifest, &host);
    let include_dir = env::var_os("KAIA_LUAJIT_INCLUDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            lib_dir
                .parent()
                .unwrap_or(&lib_dir)
                .join("staging/include/luajit-2.1")
        });
    let version = read_version(&include_dir);

    check_mlua_env(&lib_dir);

    // Test binaries load the dynamic library from the KOReader build.
    if host == target {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
    }
    if target.contains("linux") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,$ORIGIN/libs");
    }
    println!("cargo:rustc-env=KAIA_LUAJIT_VERSION={version}");
}

fn locate_lib_dir(manifest: &Path, host: &str) -> PathBuf {
    if let Some(dir) = env::var_os("KAIA_LUAJIT_LIB").map(PathBuf::from) {
        if !has_luajit(&dir) {
            panic!(
                "kaia-lua: KAIA_LUAJIT_LIB={} has no libluajit.dylib or libluajit.so",
                dir.display()
            );
        }
        return dir;
    }
    let tag = if host.contains("apple-darwin") {
        "apple-darwin"
    } else if host.contains("linux") {
        "linux"
    } else {
        panic!("kaia-lua: unsupported host {host}");
    };
    let build_root = manifest.join("../../base/build");
    let mut candidates: Vec<PathBuf> = fs::read_dir(&build_root)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path().join("libs")))
                .filter(|dir| {
                    let name = dir.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str());
                    name.is_some_and(|n| n.contains(tag) && !n.contains("kobo"))
                })
                .filter(|dir| has_luajit(dir))
                .collect()
        })
        .unwrap_or_default();
    candidates.sort();
    candidates.into_iter().next().unwrap_or_else(|| {
        panic!(
            "kaia-lua: no KOReader LuaJIT under base/build/*/libs. Build the emulator (./kodev build) \
             or set KAIA_LUAJIT_LIB."
        )
    })
}

fn has_luajit(dir: &Path) -> bool {
    dir.join("libluajit.dylib").is_file() || dir.join("libluajit.so").is_file()
}

/// Reads `LUAJIT_VERSION` from `luajit.h`, for example "LuaJIT 2.1.1783773675".
fn read_version(include_dir: &Path) -> String {
    let header = include_dir.join("luajit.h");
    let text = fs::read_to_string(&header).unwrap_or_else(|e| {
        panic!(
            "kaia-lua: cannot read {} ({e}). Set KAIA_LUAJIT_INCLUDE.",
            header.display()
        )
    });
    text.lines()
        .find_map(|line| {
            let rest = line.trim().strip_prefix("#define LUAJIT_VERSION")?;
            let start = rest.find('"')? + 1;
            let end = rest[start..].find('"')? + start;
            Some(rest[start..end].to_owned())
        })
        .unwrap_or_else(|| panic!("kaia-lua: no LUAJIT_VERSION in {}", header.display()))
}

/// mlua-sys links what LUA_LIB names. It must be the directory found above.
fn check_mlua_env(lib_dir: &Path) {
    let lua_lib = env::var("LUA_LIB").unwrap_or_default();
    let lua_lib_name = env::var("LUA_LIB_NAME").unwrap_or_default();
    let same_dir = fs::canonicalize(&lua_lib)
        .ok()
        .zip(fs::canonicalize(lib_dir).ok())
        .is_some_and(|(a, b)| a == b);
    if !same_dir || lua_lib_name != "luajit" {
        panic!(
            "kaia-lua: mlua-sys must link {}. Run `. rust/kaia-lua/env.sh` before cargo \
             (LUA_LIB={lua_lib:?}, LUA_LIB_NAME={lua_lib_name:?}).",
            lib_dir.display()
        );
    }
}
