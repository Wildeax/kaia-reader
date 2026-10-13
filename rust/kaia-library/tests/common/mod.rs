//! Shared helpers for the integration tests.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Once;

use kaia_library::{Library, Progress, ScanReport};

static ENGINE: Once = Once::new();

/// Points MuPDF at the fallback fonts. The build script sets
/// `KAIA_LIBRARY_FONT_DIR` from the MuPDF build. An explicit `FONTDIR` wins.
pub fn init_engine() {
    ENGINE.call_once(|| {
        if std::env::var_os("FONTDIR").is_none() {
            match option_env!("KAIA_LIBRARY_FONT_DIR") {
                Some(fonts) => std::env::set_var("FONTDIR", fonts),
                None => panic!("the build script did not publish the MuPDF font directory"),
            }
        }
    });
}

/// A file in the repository's `test/` directory.
pub fn repo_test_file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test")
        .join(name)
}

/// Copies a repository fixture into `dir` under a new name.
pub fn copy_fixture(name: &str, dir: &Path, as_name: &str) -> PathBuf {
    let to = dir.join(as_name);
    fs::copy(repo_test_file(name), &to).expect("copy fixture");
    to
}

/// Four books of two formats, copied from the repository fixtures.
pub fn fixture_set(dir: &Path) -> Vec<PathBuf> {
    vec![
        copy_fixture("2col.pdf", dir, "two-column.pdf"),
        copy_fixture("tall.pdf", dir, "tall.pdf"),
        copy_fixture("leaves.epub", dir, "leaves.epub"),
        copy_fixture("juliet.epub", dir, "juliet.epub"),
    ]
}

/// A database file in `dir`. The caller keeps `dir` alive.
pub fn db_path(dir: &Path) -> PathBuf {
    dir.join("library").join("library.sqlite3")
}

pub fn open_library(dir: &Path) -> Library {
    init_engine();
    Library::open(&db_path(dir)).expect("open library")
}

/// A second connection on the same database, for checks and writes outside the library API.
pub fn raw_connection(dir: &Path) -> rusqlite::Connection {
    rusqlite::Connection::open(db_path(dir)).expect("raw connection")
}

pub fn scan(lib: &mut Library, roots: &[PathBuf]) -> ScanReport {
    let cancel = AtomicBool::new(false);
    lib.scan(roots, |_: Progress<'_>| {}, &cancel).expect("scan")
}

/// Reads the width and height from the first frame header (SOF) of a JPEG.
pub fn jpeg_size(bytes: &[u8]) -> (u32, u32) {
    assert_eq!(&bytes[..3], &[0xFF, 0xD8, 0xFF], "JPEG signature");
    let mut i = 2;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF || bytes[i + 1] == 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // SOF0 to SOF15, without DHT (C4), JPG (C8) and DAC (CC).
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            return (w, h);
        }
        let len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        i += 2 + len;
    }
    panic!("no frame header in JPEG");
}
