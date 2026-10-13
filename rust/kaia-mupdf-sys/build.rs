//! Builds the C shim and links KOReader's MuPDF build.
//!
//! Library locations, in order of precedence:
//! - `KAIA_MUPDF_INCLUDE`: directory that contains `mupdf/fitz.h`.
//! - `KAIA_MUPDF_LIB`: directory that contains `libmupdf.a` and `libmupdf-third.a`.
//! - `KAIA_MUPDF_DEP_LIBS`: path list of directories with the dependencies
//!   (freetype, harfbuzz, jpeg, webp, webpdemux, archive, z, aes). Defaults to
//!   the `KAIA_MUPDF_LIB` directory.
//!
//! When none of these is set, the crate looks for the desktop build of
//! KOReader in `../../base/build/*/staging`. It takes the first directory that
//! has `lib/libmupdf.a` and whose name matches the host OS. Kobo build
//! directories are skipped. See README.md for the cross build.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const VARS: &[&str] = &[
    "KAIA_MUPDF_INCLUDE",
    "KAIA_MUPDF_LIB",
    "KAIA_MUPDF_DEP_LIBS",
    "KAIA_MUPDF_FONTS",
];

/// Libraries that only exist as static archives in the KOReader build.
const MUPDF_STATIC: &[&str] = &["mupdf", "mupdf-third"];

/// Dependencies. Shared libraries are preferred; a static archive is used when
/// no shared copy exists. KOReader's own `libwrap-mupdf` links the shared set.
const DEPS: &[&str] = &[
    "freetype",
    "harfbuzz",
    "jpeg",
    "webp",
    "webpdemux",
    "archive",
    "z",
    "aes",
];

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    for var in VARS {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-changed=csrc/kaia_mupdf.c");
    println!("cargo:rerun-if-changed=csrc/kaia_mupdf.h");

    let (include_dir, lib, deps) = locate(&manifest);

    // Compile the shim first so its object files precede the MuPDF archives on
    // the link line. The defines match the flags KOReader passes to MuPDF.
    cc::Build::new()
        .file(manifest.join("csrc/kaia_mupdf.c"))
        .include(&include_dir)
        .include(include_dir.join("freetype2"))
        .include(include_dir.join("harfbuzz"))
        .define("NOBUILTINFONT", None)
        .define("FZ_ENABLE_HYPHEN", Some("0"))
        .define("FZ_PLOTTERS_CMYK", Some("0"))
        .define("HAVE_LIBAES", Some("1"))
        .define("HAVE_LIBARCHIVE", Some("1"))
        .define("HAVE_WEBP", Some("1"))
        .warnings(true)
        .compile("kaia_mupdf");

    // Fallback fonts that MuPDF loads through FONTDIR. Published so that test
    // binaries can set FONTDIR. The app sets FONTDIR itself on the device.
    // The repository's resources/fonts has the layout KOReader's MuPDF patch
    // expects (urw/, noto/, droid/, freefont/) with every face.
    let repo_fonts =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap_or_default()).join("../../resources/fonts");
    let fonts = env_path("KAIA_MUPDF_FONTS")
        .or_else(|| Some(repo_fonts.clone()))
        .filter(|p| p.join("urw").is_dir())
        .or_else(|| lib.parent().and_then(|p| p.parent()).map(|b| b.join("fonts")))
        .filter(|p| p.is_dir());
    if let Some(fonts) = fonts {
        println!("cargo:fonts_dir={}", fonts.display());
    }

    let host = env::var("HOST").unwrap_or_default();
    let target = env::var("TARGET").unwrap_or_default();
    let native = host == target;

    let mut search_dirs = vec![lib.clone()];
    for d in &deps {
        if !search_dirs.contains(d) {
            search_dirs.push(d.clone());
        }
    }
    let mut rpath_dirs: Vec<PathBuf> = Vec::new();
    for dir in &search_dirs {
        println!("cargo:rustc-link-search=native={}", dir.display());
        // Native test binaries need the shared dependencies at run time. Cargo
        // applies rustc-link-arg only to this crate, so dependents read the list
        // from DEP_KAIA_MUPDF_RPATH_DIRS (see kaia-doc/build.rs).
        if native && (host.contains("apple-darwin") || host.contains("linux")) {
            rpath_dirs.push(dir.clone());
        }
    }
    for dir in &rpath_dirs {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
    }
    let rpath_list = env::join_paths(&rpath_dirs)
        .expect("rpath directories contain no path separator")
        .to_string_lossy()
        .into_owned();
    println!("cargo:rpath_dirs={rpath_list}");

    for name in MUPDF_STATIC {
        if find_archive(&search_dirs, name).is_none() {
            panic!(
                "kaia-mupdf-sys: lib{name}.a not found in {}. Set KAIA_MUPDF_LIB.",
                lib.display()
            );
        }
        println!("cargo:rustc-link-lib=static={name}");
    }

    for name in DEPS {
        match find_library(&search_dirs, name) {
            Some(Kind::Shared) => println!("cargo:rustc-link-lib=dylib={name}"),
            Some(Kind::Static) => println!("cargo:rustc-link-lib=static={name}"),
            None if *name == "aes" => {
                // Only needed when MuPDF was built with HAVE_LIBAES and uses it.
            }
            None => panic!(
                "kaia-mupdf-sys: lib{name} not found in {}. Set KAIA_MUPDF_DEP_LIBS.",
                join_paths(&search_dirs)
            ),
        }
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
    find_archive(dirs, name).map(|_| Kind::Static)
}

fn find_archive(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter()
        .map(|d| d.join(format!("lib{name}.a")))
        .find(|p| p.exists())
}

fn join_paths(dirs: &[PathBuf]) -> String {
    dirs.iter()
        .map(|d| d.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Returns (include dir, MuPDF archive dir, dependency dirs).
fn locate(manifest: &Path) -> (PathBuf, PathBuf, Vec<PathBuf>) {
    let explicit_include = env_path("KAIA_MUPDF_INCLUDE");
    let explicit_lib = env_path("KAIA_MUPDF_LIB");
    let explicit_deps = env::var_os("KAIA_MUPDF_DEP_LIBS").map(|v| env::split_paths(&v).collect());

    if explicit_include.is_some() || explicit_lib.is_some() {
        let include = explicit_include.unwrap_or_else(|| {
            panic!("kaia-mupdf-sys: KAIA_MUPDF_LIB is set, so KAIA_MUPDF_INCLUDE must be too")
        });
        let lib = explicit_lib.unwrap_or_else(|| {
            panic!("kaia-mupdf-sys: KAIA_MUPDF_INCLUDE is set, so KAIA_MUPDF_LIB must be too")
        });
        let deps = explicit_deps.unwrap_or_else(|| vec![lib.clone()]);
        check_include(&include);
        return (include, lib, deps);
    }

    let staging = desktop_staging(manifest).unwrap_or_else(|| {
        panic!(
            "kaia-mupdf-sys: no KOReader build with staging/lib/libmupdf.a found under \
             base/build. Build the desktop emulator (./kodev build) or set KAIA_MUPDF_INCLUDE \
             and KAIA_MUPDF_LIB."
        )
    });
    println!(
        "cargo:warning=kaia-mupdf-sys: using MuPDF from {}",
        staging.display()
    );
    let include = staging.join("include");
    let lib = staging.join("lib");
    let deps = explicit_deps.unwrap_or_else(|| vec![lib.clone()]);
    check_include(&include);
    (include, lib, deps)
}

fn check_include(include: &Path) {
    if !include.join("mupdf/fitz.h").exists() {
        panic!(
            "kaia-mupdf-sys: mupdf/fitz.h not found in {}. Check KAIA_MUPDF_INCLUDE.",
            include.display()
        );
    }
}

fn env_path(var: &str) -> Option<PathBuf> {
    env::var_os(var).map(PathBuf::from)
}

/// Finds the desktop KOReader staging directory by globbing `base/build`.
fn desktop_staging(manifest: &Path) -> Option<PathBuf> {
    let build_root = manifest.join("../../base/build");
    let host = env::var("HOST").unwrap_or_default();
    let host_tag = if host.contains("apple-darwin") {
        "apple-darwin"
    } else if host.contains("linux") {
        "linux"
    } else {
        return None;
    };

    let mut candidates: Vec<PathBuf> = fs::read_dir(&build_root)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.contains(host_tag) && !name.contains("kobo")
        })
        .map(|p| p.join("staging"))
        .filter(|s| s.join("lib/libmupdf.a").exists())
        .collect();
    candidates.sort();
    candidates.into_iter().next()
}
