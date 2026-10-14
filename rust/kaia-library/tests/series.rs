//! Series: the sort by series and number, the series filter, the book that
//! follows another in its series, and the number read from EPUB 3 metadata.

mod common;

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use common::*;
use kaia_library::{Book, Filter, Library, Sort, SortKey};
use rusqlite::params;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    fs::create_dir_all(&root).unwrap();
    (tmp, root)
}

/// Sets the series of a book straight in the table. The engine does not read
/// series from the fixtures, so the tests write it.
fn set_series(dir: &Path, id: i64, name: Option<&str>, index: Option<f64>) {
    raw_connection(dir)
        .execute(
            "UPDATE books SET series = ?2, series_index = ?3 WHERE id = ?1",
            params![id, name, index],
        )
        .unwrap();
}

fn set_title(dir: &Path, id: i64, title: &str) {
    raw_connection(dir)
        .execute("UPDATE books SET title = ?2 WHERE id = ?1", params![id, title])
        .unwrap();
}

fn ids_of(books: &[Book]) -> Vec<i64> {
    books.iter().map(|b| b.id).collect()
}

/// Writes an EPUB whose package metadata is `metadata`, extra XML such as the
/// series collection. The engine opens it, so it is a complete one-chapter book.
fn write_epub(path: &Path, metadata: &str) {
    const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    const CHAPTER: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>One</title></head>
<body><h1>Chapter one</h1><p>It was a dark and stormy night.</p></body></html>"#;
    let opf = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:test:{}</dc:identifier>
    <dc:title>Series test</dc:title>
    <dc:language>en</dc:language>
    {metadata}
  </metadata>
  <manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest>
  <spine><itemref idref="chapter"/></spine>
</package>"#,
        path.file_stem().unwrap().to_string_lossy()
    );
    let mut writer = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in [
        ("mimetype", &b"application/epub+zip"[..]),
        ("META-INF/container.xml", CONTAINER.as_bytes()),
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/chapter.xhtml", CHAPTER.as_bytes()),
    ] {
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file(name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

/// The EPUB 3 series collection, with `refinements` after it.
fn collection(refinements: &str) -> String {
    format!(
        r##"<meta id="series" property="belongs-to-collection">Earthsea</meta>
    <meta refines="#series" property="collection-type">series</meta>
    {refinements}"##
    )
}

/// Seven books: the four fixtures, two PDFs and one plain EPUB. The series
/// are set in the table, as the tests need them:
///
/// | book          | series | number |
/// |---------------|--------|--------|
/// | two-column    | Saga   | 10     |
/// | tall          | Saga   | 1.5    |
/// | leaves        | Saga   | none   |
/// | juliet        | Other  | 1      |
/// | paper         | Saga   | 1      |
/// | sample        | Saga   | 2      |
/// | plain         | none   | none   |
///
/// Returns the library and the ids in that table order.
fn saga_library(tmp: &tempfile::TempDir, root: &Path) -> (Library, [i64; 7]) {
    let mut files = fixture_set(root);
    files.push(copy_fixture("paper.pdf", root, "paper.pdf"));
    files.push(copy_fixture("sample.pdf", root, "sample.pdf"));
    let plain = root.join("plain.epub");
    write_epub(&plain, "");
    files.push(plain);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, &[root.to_path_buf()]);
    let ids: [i64; 7] = files
        .iter()
        .map(|f| lib.book_by_path(f).unwrap().unwrap().id)
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let dir = tmp.path();
    set_series(dir, ids[0], Some("Saga"), Some(10.0));
    set_series(dir, ids[1], Some("Saga"), Some(1.5));
    set_series(dir, ids[2], Some("Saga"), None);
    set_series(dir, ids[3], Some("Other"), Some(1.0));
    set_series(dir, ids[4], Some("Saga"), Some(1.0));
    set_series(dir, ids[5], Some("Saga"), Some(2.0));
    (lib, ids)
}

#[test]
fn series_sort_orders_decimal_numbers_and_puts_unnumbered_books_last() {
    let (tmp, root) = setup();
    let (lib, ids) = saga_library(&tmp, &root);
    let [ten, halfway, unnumbered, other, one, two, plain] = ids;

    let ascending = lib
        .books(Sort::new(SortKey::Series, false), &Filter::default(), 0, 100)
        .unwrap();
    assert_eq!(
        ids_of(&ascending),
        [other, one, halfway, two, ten, unnumbered, plain],
        "Other sorts before Saga. Within Saga, 1, 1.5, 2 and 10 sort as numbers, and the unnumbered book follows them. A book with no series is last."
    );
    let descending = lib
        .books(Sort::new(SortKey::Series, true), &Filter::default(), 0, 100)
        .unwrap();
    assert_eq!(
        ids_of(&descending),
        [ten, two, halfway, one, unnumbered, other, plain],
        "descending reverses the numbers, and the unnumbered book still comes after them"
    );
}

#[test]
fn series_filter_matches_one_series_in_order_without_regard_to_case() {
    let (tmp, root) = setup();
    let (lib, ids) = saga_library(&tmp, &root);
    let [ten, halfway, unnumbered, _, one, two, _] = ids;
    let saga = Filter {
        series: Some("  saga ".into()),
        ..Filter::default()
    };
    let books = lib
        .books(Sort::new(SortKey::Series, false), &saga, 0, 100)
        .unwrap();
    assert_eq!(
        ids_of(&books),
        [one, halfway, two, ten, unnumbered],
        "only the Saga books, by number"
    );
    let missing = Filter {
        series: Some("Missing".into()),
        ..Filter::default()
    };
    assert!(lib
        .books(Sort::new(SortKey::Series, false), &missing, 0, 100)
        .unwrap()
        .is_empty());
}

#[test]
fn next_in_series_takes_the_smallest_higher_number() {
    let (tmp, root) = setup();
    let (lib, ids) = saga_library(&tmp, &root);
    let [ten, halfway, unnumbered, other, one, two, plain] = ids;

    let next = |id| lib.next_in_series(id).unwrap().map(|b| b.id);
    assert_eq!(next(one), Some(halfway), "1 is followed by 1.5");
    assert_eq!(next(halfway), Some(two), "1.5 is followed by 2");
    assert_eq!(next(two), Some(ten), "2 is followed by 10");
    assert_eq!(next(ten), None, "the last number has no follower");
    assert_eq!(next(unnumbered), None, "a book with no number has no follower");
    assert_eq!(next(other), None, "the only book of its series has no follower");
    assert_eq!(next(plain), None, "a book with no series has no follower");
    assert_eq!(next(9999), None, "an unknown book has no follower");
}

#[test]
fn next_in_series_breaks_ties_by_title_and_skips_missing_books() {
    let (tmp, root) = setup();
    let (lib, ids) = saga_library(&tmp, &root);
    let [_, halfway, _, juliet, _, sample, _] = ids;
    // Two books share number 2. The one whose title sorts first is the next book.
    set_series(tmp.path(), juliet, Some("Saga"), Some(2.0));
    set_title(tmp.path(), juliet, "Aardvark");
    set_title(tmp.path(), sample, "Zebra");
    let next = |id| lib.next_in_series(id).unwrap().map(|b| b.id);
    assert_eq!(next(halfway), Some(juliet), "equal numbers go by title");

    raw_connection(tmp.path())
        .execute("UPDATE books SET missing_since = 1 WHERE id = ?1", [juliet])
        .unwrap();
    assert_eq!(
        next(halfway),
        Some(sample),
        "a book marked missing is not the next one"
    );
}

#[test]
fn epub_three_group_position_gives_the_series_number() {
    let (tmp, root) = setup();
    let file = root.join("earthsea-2.epub");
    write_epub(
        &file,
        &collection(r##"<meta refines="#series" property="group-position">2.5</meta>"##),
    );
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let book = lib.book_by_path(&file).unwrap().unwrap();
    assert_eq!(book.series.as_deref(), Some("Earthsea"));
    assert_eq!(book.series_index, Some(2.5));
}

/// A version 6 library holds an EPUB 3 series book with its name and no number.
/// Version 7 queues the book, and the next scan reads the number.
#[test]
fn version_six_series_books_without_a_number_are_queued_again() {
    let (tmp, root) = setup();
    let file = root.join("earthsea-3.epub");
    write_epub(
        &file,
        &collection(r##"<meta refines="#series" property="group-position">3</meta>"##),
    );
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&file).unwrap().unwrap().id;
    assert_eq!(lib.book(id).unwrap().unwrap().series_index, Some(3.0));
    drop(lib);

    let raw = raw_connection(tmp.path());
    raw.execute(
        "UPDATE books SET series_index = NULL, extracted = 1, attempts = 0 WHERE id = ?1",
        [id],
    )
    .unwrap();
    raw.pragma_update(None, "user_version", 6).unwrap();
    drop(raw);

    let mut lib = open_library(tmp.path());
    assert!(
        !lib.book(id).unwrap().unwrap().extracted,
        "version 7 queues the book"
    );
    let report = scan(&mut lib, &[root]);
    assert_eq!(report.extracted, 1, "the book is extracted again");
    assert_eq!(lib.book(id).unwrap().unwrap().series_index, Some(3.0));
}
