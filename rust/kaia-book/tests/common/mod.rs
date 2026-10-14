//! Shared helpers for the integration tests.

#![allow(dead_code)]

pub mod epub;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Once;

use kaia_book::{BookSession, OpenOptions};
use tempfile::TempDir;

static ENGINE: Once = Once::new();

/// Points MuPDF at the fallback fonts. The build script sets
/// `KAIA_BOOK_FONT_DIR` from the MuPDF build. An explicit `FONTDIR` wins.
pub fn init_engine() {
    ENGINE.call_once(|| {
        if std::env::var_os("FONTDIR").is_none() {
            match option_env!("KAIA_BOOK_FONT_DIR") {
                Some(fonts) => std::env::set_var("FONTDIR", fonts),
                None => panic!("the build script did not publish the MuPDF font directory"),
            }
        }
    });
}

/// A file in the repository's `test/` directory.
pub fn repo_file(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../test")
        .join(name)
}

/// The developer's EPUB library outside the repository. None when it is missing,
/// so the tests that use it skip.
pub fn library_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join("kaia-test-library");
    dir.is_dir().then_some(dir)
}

/// Copies a file into `dir`, keeping its name. Returns the copy.
pub fn copy_into(src: &Path, dir: &Path) -> PathBuf {
    let dst = dir.join(src.file_name().expect("file name"));
    fs::copy(src, &dst).expect("copy fixture");
    dst
}

/// Copies a file and its KOReader sidecar directory (`<stem>.sdr`) into `dir`.
pub fn copy_book_with_sidecar(book: &Path, dir: &Path) -> PathBuf {
    let copy = copy_into(book, dir);
    let stem = book.file_stem().expect("stem").to_string_lossy().into_owned();
    let sdr_src = book.with_file_name(format!("{stem}.sdr"));
    let sdr_dst = dir.join(format!("{stem}.sdr"));
    fs::create_dir_all(&sdr_dst).expect("create sidecar dir");
    for entry in fs::read_dir(&sdr_src).expect("read sidecar dir") {
        let entry = entry.expect("entry");
        fs::copy(entry.path(), sdr_dst.join(entry.file_name())).expect("copy sidecar file");
    }
    copy
}

/// Options that keep all Kaia data in a temporary directory.
pub fn options(data: &TempDir) -> OpenOptions {
    let mut opts = OpenOptions::new(data.path());
    opts.sidecars = Vec::new();
    opts
}

pub fn open(path: &Path, data: &TempDir) -> BookSession {
    init_engine();
    BookSession::open(path, options(data)).expect("open book")
}

/// The first word that a grid of screen points selects on the current page.
pub fn first_word(s: &mut BookSession, viewport: kaia_gfx::Size) -> (kaia_gfx::Point, kaia_book::Selection) {
    for y in (80..viewport.h as i32).step_by(30) {
        for x in (60..viewport.w as i32).step_by(40) {
            let p = kaia_gfx::Point { x, y };
            if let Some(sel) = s.select_word_at(p, viewport).expect("select") {
                return (p, sel);
            }
        }
    }
    panic!("no word found on page {}", s.current().page);
}

/// Waits for the layout that is running to finish, and returns the page count.
pub fn wait_layout(s: &mut BookSession) -> usize {
    for _ in 0..600 {
        for event in s.poll_timeout(std::time::Duration::from_millis(50)) {
            match event {
                kaia_book::BookEvent::LayoutApplied { page_count } => return page_count,
                kaia_book::BookEvent::LayoutFailed { message } => panic!("layout failed: {message}"),
                _ => {}
            }
        }
    }
    panic!("layout did not finish");
}

/// Waits for the running search to finish. Returns true when it was cancelled.
pub fn wait_search(s: &mut BookSession) -> bool {
    for _ in 0..1200 {
        for event in s.poll_timeout(std::time::Duration::from_millis(50)) {
            if let kaia_book::BookEvent::SearchFinished { cancelled } = event {
                return cancelled;
            }
        }
    }
    panic!("search did not finish");
}
