//! Importing a KOReader statistics database.

mod common;

use common::{add_book, add_span, koreader_db, legacy_db, local, span_count, Scratch};
use kaia_stats::{Error, Stats};
use rusqlite::Connection;

/// Source: "Alpha" with two spans, "Solo" with one span, "Empty" with no spans.
/// Target: "Alpha" with one span the source also has (same start time) and one the source lacks.
fn databases(dir: &Scratch) -> (std::path::PathBuf, std::path::PathBuf, i64) {
    let source_path = dir.path("koreader.sqlite3");
    let source = koreader_db(&source_path);
    let alpha = add_book(&source, "Alpha", Some("X"), "aaaa", 100);
    add_span(&source, alpha, 1, local("2024-05-10 08:00:00"), 60, 100);
    add_span(&source, alpha, 2, local("2024-05-11 08:00:00"), 30, 100);
    let solo = add_book(&source, "Solo", Some(""), "ssss", 20);
    add_span(&source, solo, 3, local("2024-05-12 08:00:00"), 50, 20);
    add_book(&source, "Empty", Some("Z"), "eeee", 5);
    drop(source);

    let target_path = dir.path("stats.sqlite3");
    let target = koreader_db(&target_path);
    let target_alpha = add_book(&target, "Alpha", Some("X"), "aaaa", 100);
    add_span(&target, target_alpha, 1, local("2024-05-10 08:00:00"), 60, 100);
    add_span(&target, target_alpha, 9, local("2024-05-20 08:00:00"), 10, 100);
    drop(target);
    (source_path, target_path, target_alpha)
}

#[test]
fn import_adds_new_books_and_spans_without_duplicates() {
    let dir = Scratch::new();
    let (source, target_path, alpha) = databases(&dir);
    let mut stats = Stats::open(&target_path).unwrap();

    let report = stats.import_koreader(&source).unwrap();
    assert_eq!(
        report.books_added, 1,
        "Solo is new; Empty has no spans and is not imported"
    );
    assert_eq!(report.books_matched, 1, "Alpha matches on title, authors and md5");
    assert_eq!(report.spans_added, 2, "Alpha page 2 and Solo page 3");
    assert_eq!(report.spans_skipped, 1, "Alpha page 1 is already in the target");

    // Alpha: 60 (shared) + 30 (imported) + 10 (target only).
    let totals = stats.book_totals(alpha).unwrap().unwrap();
    assert_eq!(totals.total_seconds, 100);
    assert_eq!(totals.distinct_pages, 3);
    assert_eq!(totals.days_read, 3);
    let solo = stats
        .books()
        .unwrap()
        .into_iter()
        .find(|b| b.title == "Solo")
        .unwrap();
    assert_eq!((solo.seconds, solo.distinct_pages), (50, 1));
    assert_eq!(
        stats.books().unwrap().len(),
        2,
        "Alpha and Solo; Empty is not added"
    );
}

#[test]
fn importing_the_same_file_again_adds_nothing() {
    let dir = Scratch::new();
    let (source, target_path, alpha) = databases(&dir);
    let mut stats = Stats::open(&target_path).unwrap();
    stats.import_koreader(&source).unwrap();
    let before = stats.book_totals(alpha).unwrap().unwrap();

    let report = stats.import_koreader(&source).unwrap();
    assert_eq!((report.books_added, report.books_matched), (0, 2));
    assert_eq!((report.spans_added, report.spans_skipped), (0, 3));
    assert_eq!(stats.book_totals(alpha).unwrap().unwrap(), before);
    let conn = Connection::open(&target_path).unwrap();
    assert_eq!(span_count(&conn, alpha), 3);
}

#[test]
fn the_source_file_is_never_written() {
    let dir = Scratch::new();
    let (source, target_path, _) = databases(&dir);
    let before = std::fs::read(&source).unwrap();

    let mut stats = Stats::open(&target_path).unwrap();
    stats.import_koreader(&source).unwrap();

    assert_eq!(std::fs::read(&source).unwrap(), before);
    assert!(!dir.path("koreader.sqlite3-journal").exists());
    assert!(!dir.path("koreader.sqlite3-wal").exists());
}

#[test]
fn a_source_at_schema_zero_is_refused() {
    let dir = Scratch::new();
    let source = dir.path("legacy.sqlite3");
    drop(legacy_db(&source));
    let target_path = dir.path("stats.sqlite3");
    let target = koreader_db(&target_path);
    let alpha = add_book(&target, "Alpha", Some("X"), "aaaa", 100);
    add_span(&target, alpha, 1, local("2024-05-10 08:00:00"), 60, 100);
    drop(target);

    let mut stats = Stats::open(&target_path).unwrap();
    let err = stats.import_koreader(&source).unwrap_err();
    assert!(matches!(err, Error::UnsupportedSource { found: 0 }), "{err:?}");
    let conn = Connection::open(&target_path).unwrap();
    assert_eq!(span_count(&conn, alpha), 1);
}

#[test]
fn a_missing_source_is_an_error_and_is_not_created() {
    let dir = Scratch::new();
    let target_path = dir.path("stats.sqlite3");
    let mut stats = Stats::open(&target_path).unwrap();
    let missing = dir.path("missing.sqlite3");
    assert!(stats.import_koreader(&missing).is_err());
    assert!(!missing.exists());
}

#[test]
fn a_database_cannot_import_itself() {
    let dir = Scratch::new();
    let (_, target_path, _) = databases(&dir);
    let mut stats = Stats::open(&target_path).unwrap();
    let err = stats.import_koreader(&target_path).unwrap_err();
    assert!(matches!(err, Error::SameDatabase(_)), "{err:?}");
}

#[test]
fn an_open_book_is_written_before_the_import() {
    let dir = Scratch::new();
    let (source, target_path, _) = databases(&dir);
    let mut stats = Stats::open(&target_path).unwrap();
    let id = stats
        .start_book("open", "Open", "Me", 10, 1, local("2024-05-16 09:00:00"))
        .unwrap();
    stats.page_turn(2, local("2024-05-16 09:01:00")).unwrap();

    stats.import_koreader(&source).unwrap();
    assert_eq!(stats.book_totals(id).unwrap().unwrap().total_seconds, 60);
}
