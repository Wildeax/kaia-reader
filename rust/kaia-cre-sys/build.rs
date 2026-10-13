//! Builds the C++ shim and links KOReader's CREngine build.
//!
//! Locations, in order of precedence:
//! - `KAIA_CRE_INCLUDE`: directory with CREngine's `lvdocview.h` (the engine's `crengine/include`).
//! - `KAIA_CRE_LIB`: directory with `libcrengine.a`. `libantiword.a` and
//!   `libchmlib.a` are read from its `crengine/thirdparty/...` subdirectories.
//! - `KAIA_CRE_SETUP`: directory with the generated `crsetup.h`.
//! - `KAIA_CRE_DEP_LIBS`: path list of dependency library directories
//!   (freetype, harfbuzz, ...). Defaults to the `KAIA_CRE_LIB` root's staging.
//! - `KAIA_CRE_DEP_INCLUDE`: path list of dependency header directories.
//!
//! When none is set, the crate uses the desktop build in `../../base/build/*`
//! that has `thirdparty/crengine/build/libcrengine.a`, the same search as
//! `kaia-mupdf-sys`. Set `KAIA_CRE_ROOT` to a build directory to pick one.
//!
//! The defaults for the data directories (CSS, hyphenation, fonts) are exported
//! as `kaia_cre_sys::DATA_DIR` and `kaia_cre_sys::FONTS_DIR`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const VARS: &[&str] = &[
    "KAIA_CRE_ROOT",
    "KAIA_CRE_INCLUDE",
    "KAIA_CRE_LIB",
    "KAIA_CRE_SETUP",
    "KAIA_CRE_DEP_LIBS",
    "KAIA_CRE_DEP_INCLUDE",
    "KAIA_CRE_DATA_DIR",
    "KAIA_CRE_FONTS_DIR",
];

/// Shared dependencies. A shared library is preferred; a static archive is
/// used when no shared copy exists. The list follows the `koreader-cre`
/// link rule in `cmake/koreader/build/build.ninja`.
const DEPS: &[&str] = &[
    "brotlicommon",
    "brotlidec",
    "freetype",
    "harfbuzz",
    "unibreak",
    "fribidi",
    "jpeg",
    "png16",
    "webp",
    "webpdemux",
    "lunasvg",
    "md4c-html",
    "md4c",
    "utf8proc",
    "xxhash",
    "z",
    "zstd",
];

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo = manifest.join("../..");
    for var in VARS {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-changed=csrc/kaia_cre.cpp");
    println!("cargo:rerun-if-changed=csrc/kaia_cre.h");

    let paths = locate(&repo);

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .pic(true)
        .file(manifest.join("csrc/kaia_cre.cpp"))
        .include(&paths.include)
        .include(&paths.setup)
        .include(&paths.staging_include)
        .include(paths.staging_include.join("freetype2"))
        .include(paths.staging_include.join("harfbuzz"))
        .include(paths.staging_include.join("fribidi"))
        .include(repo.join("base/thirdparty/srell"))
        .include(manifest.join("csrc"))
        .flag("-include")
        .flag(paths.setup.join("crsetup.h"))
        // CREngine headers have many unused parameters in inline stubs. Their
        // warnings are noise for this crate.
        .warnings(false)
        .extra_warnings(false)
        .flag_if_supported("-Wno-unused-parameter");
    // CREngine is built with DEBUG in debug profiles (see koreader-cre in
    // the CMake build). Match it so the class layouts agree.
    if env::var("PROFILE").map(|p| p == "debug").unwrap_or(true) {
        build.define("DEBUG", None);
    }
    build.compile("kaia_cre");

    for dir in &paths.lib_dirs {
        println!("cargo:rustc-link-search=native={}", dir.display());
    }
    println!("cargo:rustc-link-lib=static=crengine");
    println!("cargo:rustc-link-lib=static=antiword");
    println!("cargo:rustc-link-lib=static=chmlib");
    for name in DEPS {
        match find_library(&paths.dep_dirs, name) {
            Some(Kind::Shared) => println!("cargo:rustc-link-lib=dylib={name}"),
            Some(Kind::Static) => println!("cargo:rustc-link-lib=static={name}"),
            None => panic!(
                "kaia-cre-sys: lib{name} not found in {}. Set KAIA_CRE_DEP_LIBS.",
                join_paths(&paths.dep_dirs)
            ),
        }
    }
    let target = env::var("TARGET").unwrap_or_default();
    if target.contains("apple") {
        println!("cargo:rustc-link-lib=c++");
    } else {
        println!("cargo:rustc-link-lib=stdc++");
    }

    println!("cargo:rustc-env=KAIA_CRE_DATA_DIR={}", paths.data_dir.display());
    println!("cargo:rustc-env=KAIA_CRE_FONTS_DIR={}", paths.fonts_dir.display());
}

struct Paths {
    include: PathBuf,
    setup: PathBuf,
    staging_include: PathBuf,
    lib_dirs: Vec<PathBuf>,
    dep_dirs: Vec<PathBuf>,
    data_dir: PathBuf,
    fonts_dir: PathBuf,
}

fn locate(repo: &Path) -> Paths {
    let root = env_path("KAIA_CRE_ROOT")
        .or_else(|| desktop_build(repo))
        .unwrap_or_else(|| {
            panic!(
                "kaia-cre-sys: no KOReader build with thirdparty/crengine/build/libcrengine.a \
                 found under base/build. Build the desktop emulator (./kodev build) or set \
                 KAIA_CRE_ROOT, or set KAIA_CRE_INCLUDE, KAIA_CRE_LIB and KAIA_CRE_DEP_LIBS."
            )
        });
    let crengine_build = root.join("thirdparty/crengine/build");
    let staging = root.join("staging");

    let include = env_path("KAIA_CRE_INCLUDE")
        .unwrap_or_else(|| repo.join("base/thirdparty/kpvcrlib/crengine/crengine/include"));
    let lib = env_path("KAIA_CRE_LIB").unwrap_or_else(|| crengine_build.clone());
    let setup = env_path("KAIA_CRE_SETUP").unwrap_or_else(|| crengine_build.clone());
    let dep_dirs: Vec<PathBuf> = match env::var_os("KAIA_CRE_DEP_LIBS") {
        Some(v) => env::split_paths(&v).collect(),
        None => vec![staging.join("lib")],
    };
    let staging_include = match env::var_os("KAIA_CRE_DEP_INCLUDE") {
        Some(v) => env::split_paths(&v)
            .next()
            .unwrap_or_else(|| staging.join("include")),
        None => staging.join("include"),
    };

    for (what, file) in [
        ("KAIA_CRE_INCLUDE", include.join("lvdocview.h")),
        ("KAIA_CRE_SETUP", setup.join("crsetup.h")),
    ] {
        if !file.exists() {
            panic!("kaia-cre-sys: {} not found. Check {what}.", file.display());
        }
    }
    let archive = lib.join("libcrengine.a");
    if !archive.exists() {
        panic!(
            "kaia-cre-sys: {} not found. Check KAIA_CRE_LIB.",
            archive.display()
        );
    }

    // libantiword.a and libchmlib.a are built next to libcrengine.a, in
    // crengine/thirdparty/<name>/.
    let mut lib_dirs = vec![lib.clone()];
    for sub in ["antiword", "chmlib"] {
        lib_dirs.push(lib.join("crengine/thirdparty").join(sub));
    }
    for d in &dep_dirs {
        if !lib_dirs.contains(d) {
            lib_dirs.push(d.clone());
        }
    }

    let data_dir = env_path("KAIA_CRE_DATA_DIR")
        .unwrap_or_else(|| repo.join("base/thirdparty/kpvcrlib/crengine/cr3gui/data"));
    let fonts_dir = env_path("KAIA_CRE_FONTS_DIR").unwrap_or_else(|| repo.join("resources/fonts"));

    Paths {
        include,
        setup,
        staging_include,
        lib_dirs,
        dep_dirs,
        data_dir,
        fonts_dir,
    }
}

enum Kind {
    Shared,
    Static,
}

fn find_library(dirs: &[PathBuf], name: &str) -> Option<Kind> {
    for dir in dirs {
        for file in [format!("lib{name}.dylib"), format!("lib{name}.so")] {
            if dir.join(&file).exists() {
                return Some(Kind::Shared);
            }
        }
    }
    for dir in dirs {
        if dir.join(format!("lib{name}.a")).exists() {
            return Some(Kind::Static);
        }
    }
    None
}

fn join_paths(dirs: &[PathBuf]) -> String {
    dirs.iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn env_path(var: &str) -> Option<PathBuf> {
    env::var_os(var).map(PathBuf::from)
}

/// Finds a desktop KOReader build root (the directory that holds `thirdparty/`
/// and `staging/`) by globbing `base/build`.
fn desktop_build(repo: &Path) -> Option<PathBuf> {
    let host = env::var("HOST").unwrap_or_default();
    let host_tag = if host.contains("apple-darwin") {
        "apple-darwin"
    } else if host.contains("linux") {
        "linux"
    } else {
        return None;
    };
    let mut candidates: Vec<PathBuf> = fs::read_dir(repo.join("base/build"))
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.contains(host_tag) && !name.contains("kobo")
        })
        .filter(|p| p.join("thirdparty/crengine/build/libcrengine.a").exists())
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}
