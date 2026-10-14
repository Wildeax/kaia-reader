//! Text metadata and scan exclusions.

mod common;

use std::fs;
use std::path::Path;

use common::{open_library, scan};
use kaia_library::CoverSize;

fn write_text(root: &Path, name: &str) -> std::path::PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "A café beside the sea.\n".repeat(80)).unwrap();
    path
}

#[test]
fn text_uses_filename_without_author_or_cover_and_updates_after_move() {
    let tmp = tempfile::Builder::new().prefix("kaia-text-").tempdir().unwrap();
    let root = tmp.path().join("books");
    let path = write_text(&root, "A café.TXT");
    let mut library = open_library(tmp.path());
    let report = scan(&mut library, std::slice::from_ref(&root));
    assert_eq!((report.added, report.failed), (1, 0));
    let book = library.book_by_path(&path).unwrap().unwrap();
    assert_eq!(book.format, "txt");
    assert_eq!(book.title.as_deref(), Some("A café"));
    assert_eq!(book.authors, None);
    assert!(book.extracted);
    assert!(book.pages.unwrap() > 0);
    assert!(library.cover(book.id, CoverSize::Small).unwrap().is_none());
    assert!(library.cover(book.id, CoverSize::Large).unwrap().is_none());
    assert_eq!(scan(&mut library, std::slice::from_ref(&root)).unchanged, 1);

    let moved = root.join("Another title.txt");
    fs::rename(&path, &moved).unwrap();
    assert_eq!(scan(&mut library, &[root]).moved, 1);
    let after = library.book_by_path(&moved).unwrap().unwrap();
    assert_eq!(after.id, book.id);
    assert_eq!(after.title.as_deref(), Some("Another title"));
}

#[test]
fn text_history_import_keeps_progress_when_scan_adds_metadata() {
    let tmp = tempfile::Builder::new().prefix("kaia-text-").tempdir().unwrap();
    let root = tmp.path().join("books");
    let path = write_text(&root, "A café.txt");
    let history = tmp.path().join("history.lua");
    fs::write(
        &history,
        format!(
            "return {{ {{ time = 1700000300, file = [[{}]] }} }}",
            path.display()
        ),
    )
    .unwrap();
    let mut library = open_library(tmp.path());
    assert_eq!(library.import_koreader_history(&history).unwrap().added, 1);
    let imported = library.book_by_path(&path).unwrap().unwrap();
    library.record_open(imported.id, 0.4).unwrap();
    let report = scan(&mut library, &[root]);
    assert_eq!((report.added, report.extracted, report.failed), (0, 1, 0));
    let book = library.book_by_path(&path).unwrap().unwrap();
    assert_eq!(book.id, imported.id);
    assert_eq!(book.title.as_deref(), Some("A café"));
    assert_eq!(book.percent, 0.4);
    assert_eq!(library.recent(10).unwrap()[0].id, book.id);
}

#[test]
fn text_scan_skips_hidden_paths_reader_data_sidecars_and_size_outliers() {
    let tmp = tempfile::Builder::new().prefix("kaia-text-").tempdir().unwrap();
    let root = tmp.path().join("books");
    write_text(&root, "A book.txt");
    // A visible folder named KOReader holds the reader's own books. The apps keep
    // their data in hidden folders, which the hidden rule skips.
    let kept = write_text(&root, "KOReader/Moved book.txt");
    let mut skipped = Vec::new();
    for name in [
        ".hidden.txt",
        ".hidden/notes.txt",
        "Shelf/.nested/notes.txt",
        ".kaia/books/notes.txt",
        ".adds/koreader/README.txt",
        ".kobo/notes.txt",
        "Book.sdr/notes.txt",
        "Shelf/Book.SDR/notes.txt",
    ] {
        skipped.push(write_text(&root, name));
    }
    let empty = root.join("empty.txt");
    fs::write(&empty, []).unwrap();
    skipped.push(empty);
    let huge = root.join("huge.txt");
    fs::File::create(&huge)
        .unwrap()
        .set_len(32 * 1024 * 1024 + 1)
        .unwrap();
    skipped.push(huge);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&skipped[1], root.join("alias.txt")).unwrap();

    let mut library = open_library(tmp.path());
    // Explicit roots must not bypass the folder rules.
    let mut roots = vec![root.clone()];
    roots.extend(
        skipped
            .iter()
            .filter_map(|path| path.parent().map(Path::to_path_buf)),
    );
    let report = scan(&mut library, &roots);
    assert_eq!((report.files_seen, report.added, report.failed), (2, 2, 0));
    assert!(library.book_by_path(&kept).unwrap().is_some());
    for path in skipped {
        assert!(
            library.book_by_path(&path).unwrap().is_none(),
            "{}",
            path.display()
        );
        assert!(library.index_file(&path).is_err(), "{}", path.display());
    }
}
