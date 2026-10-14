mod common;

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use common::*;
use kaia_library::{Book, Filter, Library, LibraryError, Sort, SortKey, Status};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    fs::create_dir_all(&root).unwrap();
    (tmp, root)
}

fn all(lib: &kaia_library::Library, sort: Sort, filter: &Filter) -> Vec<Book> {
    lib.books(sort, filter, 0, 1000).unwrap()
}

fn titles(books: &[Book]) -> Vec<String> {
    books
        .iter()
        .map(|b| b.title.clone().unwrap_or_default())
        .collect()
}

/// The number of history rows of a book, read straight from the database.
fn history_rows(dir: &Path, book_id: i64) -> i64 {
    raw_connection(dir)
        .query_row(
            "SELECT COUNT(*) FROM history WHERE book_id = ?1",
            [book_id],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn titles_sort_ascending_and_descending() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let asc = titles(&all(&lib, Sort::new(SortKey::Title, false), &Filter::default()));
    let mut sorted = asc.clone();
    sorted.sort_by_key(|t| t.to_lowercase());
    assert_eq!(asc, sorted);
    let desc = titles(&all(&lib, Sort::new(SortKey::Title, true), &Filter::default()));
    let mut rev = asc.clone();
    rev.reverse();
    assert_eq!(desc, rev);
}

#[test]
fn progress_and_last_read_sorts_put_unread_last() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let a = lib.book_by_path(&files[0]).unwrap().unwrap().id;
    let b = lib.book_by_path(&files[1]).unwrap().unwrap().id;
    lib.record_open(a, 0.2).unwrap();
    lib.record_open(b, 0.9).unwrap();

    let by_progress = all(&lib, Sort::new(SortKey::Progress, true), &Filter::default());
    assert_eq!(by_progress[0].id, b);
    assert_eq!(by_progress[1].id, a);

    let by_read = all(&lib, Sort::new(SortKey::LastRead, true), &Filter::default());
    assert_eq!(by_read[0].id, b, "opened last, comes first when descending");
    assert_eq!(by_read[1].id, a);
    assert!(by_read[2].last_opened_at.is_none());
    assert!(by_read[3].last_opened_at.is_none());
    let by_read_asc = all(&lib, Sort::new(SortKey::LastRead, false), &Filter::default());
    assert!(
        by_read_asc[2].last_opened_at.is_none(),
        "never-opened books stay last"
    );
}

#[test]
fn added_sort_and_offset_limit_paging() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let ids: Vec<i64> = all(&lib, Sort::new(SortKey::Added, false), &Filter::default())
        .iter()
        .map(|b| b.id)
        .collect();
    let page = lib
        .books(Sort::new(SortKey::Added, false), &Filter::default(), 1, 2)
        .unwrap();
    assert_eq!(page.iter().map(|b| b.id).collect::<Vec<_>>(), ids[1..3]);
    assert!(lib
        .books(Sort::new(SortKey::Added, false), &Filter::default(), 10, 2)
        .unwrap()
        .is_empty());
}

/// Books indexed in the same second have equal added times. Newest first then
/// lists the book indexed last at the top, so the ties follow the direction.
#[test]
fn added_sort_breaks_ties_in_the_same_direction() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    raw_connection(tmp.path())
        .execute("UPDATE books SET added_at = 100", [])
        .unwrap();
    let ascending = all(&lib, Sort::new(SortKey::Added, false), &Filter::default());
    let descending = all(&lib, Sort::new(SortKey::Added, true), &Filter::default());
    let mut reversed: Vec<i64> = ascending.iter().map(|b| b.id).collect();
    reversed.reverse();
    assert_eq!(descending.iter().map(|b| b.id).collect::<Vec<_>>(), reversed);
}

#[test]
fn filters_by_status_format_folder_and_collection() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let sub = root.join("series");
    fs::create_dir_all(&sub).unwrap();
    let inner = copy_fixture("2col.pdf", &sub, "inner.pdf");
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let s = Sort::new(SortKey::Title, false);

    let epubs = all(
        &lib,
        s,
        &Filter {
            format: Some("EPUB".into()),
            ..Filter::default()
        },
    );
    assert_eq!(epubs.len(), 2, "format match ignores case in the filter");
    assert!(epubs.iter().all(|b| b.format == "epub"));

    let in_sub = all(
        &lib,
        s,
        &Filter {
            folder: Some(sub.clone()),
            ..Filter::default()
        },
    );
    assert_eq!(in_sub.len(), 1);
    assert_eq!(in_sub[0].path, fs::canonicalize(&inner).unwrap());
    let in_root = all(
        &lib,
        s,
        &Filter {
            folder: Some(root.clone()),
            ..Filter::default()
        },
    );
    assert_eq!(in_root.len(), 5, "a folder includes its subfolders");

    let id = lib.book_by_path(&files[0]).unwrap().unwrap().id;
    lib.set_status(id, Status::Complete).unwrap();
    let done = all(
        &lib,
        s,
        &Filter {
            status: Some(Status::Complete),
            ..Filter::default()
        },
    );
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].id, id);
    assert_eq!(
        all(
            &lib,
            s,
            &Filter {
                status: Some(Status::New),
                ..Filter::default()
            }
        )
        .len(),
        4
    );

    lib.add_to_collection(id, "Favorites").unwrap();
    let coll = lib.collections().unwrap();
    let fav = coll.iter().find(|c| c.name == "Favorites").unwrap().id;
    let in_fav = all(
        &lib,
        s,
        &Filter {
            collection: Some(fav),
            ..Filter::default()
        },
    );
    assert_eq!(in_fav.len(), 1);
}

#[test]
fn formats_lists_each_format_in_the_library_once_in_order() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let mut lib = open_library(tmp.path());
    assert!(
        lib.formats().unwrap().is_empty(),
        "an empty library has no formats"
    );
    scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(
        lib.formats().unwrap(),
        vec!["epub".to_string(), "pdf".to_string()]
    );
}

#[test]
fn manual_collection_order_follows_insertion() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let ids: Vec<i64> = files
        .iter()
        .map(|f| lib.book_by_path(f).unwrap().unwrap().id)
        .collect();
    for &id in [ids[3], ids[0], ids[2]].iter() {
        lib.add_to_collection(id, "Reading order").unwrap();
    }
    let cid = lib
        .collections()
        .unwrap()
        .iter()
        .find(|c| c.name == "Reading order")
        .unwrap()
        .id;
    let manual = all(
        &lib,
        Sort::new(SortKey::Manual, false),
        &Filter {
            collection: Some(cid),
            ..Filter::default()
        },
    );
    assert_eq!(
        manual.iter().map(|b| b.id).collect::<Vec<_>>(),
        vec![ids[3], ids[0], ids[2]]
    );
}

#[test]
fn series_sort_uses_name_index_and_title_then_unassigned_last() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let ids: Vec<i64> = files
        .iter()
        .map(|path| lib.book_by_path(path).unwrap().unwrap().id)
        .collect();
    let raw = raw_connection(tmp.path());
    raw.execute(
        "UPDATE books SET series = 'B', series_index = 1 WHERE id = ?1",
        [ids[0]],
    )
    .unwrap();
    raw.execute(
        "UPDATE books SET series = 'A', series_index = 2 WHERE id = ?1",
        [ids[1]],
    )
    .unwrap();
    raw.execute(
        "UPDATE books SET series = 'A', series_index = 1 WHERE id = ?1",
        [ids[2]],
    )
    .unwrap();
    let sorted = all(&lib, Sort::new(SortKey::Series, false), &Filter::default());
    assert_eq!(
        sorted.iter().map(|book| book.id).collect::<Vec<_>>(),
        [ids[2], ids[1], ids[0], ids[3]]
    );
}

#[test]
fn search_matches_prefixes_across_fields() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&files[2]).unwrap().unwrap().id;

    // Metadata the engine does not supply yet is set directly in the table, which
    // exercises the FTS triggers.
    let raw = raw_connection(tmp.path());
    raw.execute(
        "UPDATE books SET title = ?2, authors = ?3, series = ?4, description = ?5 WHERE id = ?1",
        rusqlite::params![
            id,
            "Dune Messiah",
            "Frank Herbert",
            "Dune Chronicles",
            "A sequel set on a desert planet, with spice."
        ],
    )
    .unwrap();

    let hits = |q: &str| {
        lib.search(q, 10)
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(hits("dun"), vec![id], "title prefix");
    assert_eq!(hits("Messi"), vec![id], "prefix is case-insensitive");
    assert_eq!(hits("herb"), vec![id], "author prefix");
    assert_eq!(hits("chron"), vec![id], "series prefix");
    assert_eq!(hits("spi"), vec![id], "description prefix");
    assert_eq!(hits("dune messiah"), vec![id], "all words must match");
    assert!(hits("dune foundation").is_empty());
    assert!(hits("").is_empty());
    assert!(
        hits(" \" * -- ").is_empty(),
        "syntax characters do not break the query"
    );

    // Changed metadata is searchable under the new text only.
    raw.execute(
        "UPDATE books SET title = 'Foundation', series = NULL, description = NULL WHERE id = ?1",
        [id],
    )
    .unwrap();
    assert!(hits("dune").is_empty());
    assert_eq!(hits("found"), vec![id]);

    // A removed book leaves no hits behind.
    fs::remove_file(&files[2]).unwrap();
    let rows_before = lib.search("found", 10).unwrap().len();
    assert_eq!(rows_before, 1);
    scan(&mut lib, std::slice::from_ref(&root));
    assert!(lib.search("found", 10).unwrap().is_empty());
}

#[test]
fn search_handles_non_ascii_text() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let a = lib.book_by_path(&files[0]).unwrap().unwrap().id;
    let b = lib.book_by_path(&files[1]).unwrap().unwrap().id;
    let raw = raw_connection(tmp.path());
    raw.execute("UPDATE books SET title = 'Ünïcödé Tëst' WHERE id = ?1", [a])
        .unwrap();
    raw.execute(
        "UPDATE books SET title = '吾輩は猫である', authors = 'Sōseki Natsume' WHERE id = ?1",
        [b],
    )
    .unwrap();

    let hits = |q: &str| {
        lib.search(q, 10)
            .unwrap()
            .into_iter()
            .map(|b| b.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(hits("unicode"), vec![a], "diacritics are folded");
    assert_eq!(hits("ÜNÏ"), vec![a], "query accents fold too");
    assert_eq!(hits("tës"), vec![a]);
    assert_eq!(hits("吾輩"), vec![b], "CJK prefix");
    assert_eq!(hits("sōseki"), vec![b]);
    assert_eq!(hits("soseki"), vec![b], "macron folds to base letter");
}

#[test]
fn collections_add_remove_and_count() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let a = lib.book_by_path(&files[0]).unwrap().unwrap().id;
    let b = lib.book_by_path(&files[1]).unwrap().unwrap().id;

    lib.add_to_collection(a, "Favorites").unwrap();
    lib.add_to_collection(b, "Favorites").unwrap();
    lib.add_to_collection(a, "Favorites").unwrap();
    lib.add_to_collection(a, "  Work  ").unwrap();
    let colls = lib.collections().unwrap();
    let names: Vec<(&str, u64)> = colls.iter().map(|c| (c.name.as_str(), c.books)).collect();
    assert_eq!(
        names,
        vec![("Favorites", 2), ("Work", 1)],
        "sorted by name, duplicates ignored"
    );

    assert!(lib.remove_from_collection(b, "Favorites").unwrap());
    assert!(
        !lib.remove_from_collection(b, "Favorites").unwrap(),
        "already removed"
    );
    assert!(!lib.remove_from_collection(a, "No such collection").unwrap());
    let colls = lib.collections().unwrap();
    assert_eq!(colls.iter().find(|c| c.name == "Favorites").unwrap().books, 1);

    assert!(matches!(
        lib.add_to_collection(a, "   "),
        Err(LibraryError::InvalidArgument(_))
    ));
    assert!(matches!(
        lib.add_to_collection(9999, "Work"),
        Err(LibraryError::NotFound(9999))
    ));
}

#[test]
fn collection_management_enforces_names_and_order() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&files[0]).unwrap().unwrap().id;
    let first = lib.create_collection("  First  ").unwrap();
    let second = lib.create_collection("Second").unwrap();
    assert_eq!(first.name, "First");
    assert!(matches!(
        lib.create_collection("first"),
        Err(LibraryError::CollectionNameExists(_))
    ));
    lib.add_to_collection(id, "SECOND").unwrap();
    assert!(matches!(
        lib.rename_collection(second.id, "fIrSt"),
        Err(LibraryError::CollectionNameExists(_))
    ));
    lib.rename_collection(second.id, "Renamed").unwrap();
    lib.reorder_collection(second.id, 0).unwrap();
    let collections = lib.collections().unwrap();
    assert_eq!(
        collections.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["Renamed", "First"]
    );
    assert_eq!(collections[0].books, 1);
    assert!(lib.delete_collection(second.id).unwrap());
    assert!(!lib.delete_collection(second.id).unwrap());
    assert_eq!(lib.collections().unwrap().len(), 1);
}

#[test]
fn history_removal_and_book_removal_only_change_the_index() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let first = lib.book_by_path(&files[0]).unwrap().unwrap().id;
    let second = lib.book_by_path(&files[1]).unwrap().unwrap().id;
    lib.record_open(first, 0.6).unwrap();
    lib.record_open(second, 0.2).unwrap();

    lib.remove_from_history(first).unwrap();
    let removed = lib.book(first).unwrap().unwrap();
    assert_eq!(removed.last_opened_at, None);
    assert_eq!(removed.percent, 0.6, "progress is kept");
    assert_eq!(removed.status, Status::Reading, "status is kept");
    assert_eq!(history_rows(tmp.path(), first), 0);
    assert_eq!(history_rows(tmp.path(), second), 1);
    let recent: Vec<i64> = lib.recent(10).unwrap().iter().map(|b| b.id).collect();
    assert_eq!(recent, [second]);
    assert!(files[0].exists());

    lib.clear_history().unwrap();
    let cleared = lib.book(second).unwrap().unwrap();
    assert_eq!(cleared.last_opened_at, None);
    assert_eq!(cleared.percent, 0.2, "progress is kept");
    assert_eq!(cleared.status, Status::Reading, "status is kept");
    assert_eq!(history_rows(tmp.path(), second), 0);
    assert!(lib.recent(10).unwrap().is_empty());
    assert!(lib.remove_book(first).unwrap());
    assert!(!lib.remove_book(first).unwrap());
    assert!(lib.book(first).unwrap().is_none());
    assert!(files[0].exists());
}

#[test]
fn record_open_sets_progress_status_and_history() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&files[1]).unwrap().unwrap().id;
    assert_eq!(lib.book(id).unwrap().unwrap().status, Status::New);

    lib.record_open(id, 1.7).unwrap();
    let b = lib.book(id).unwrap().unwrap();
    assert_eq!(b.percent, 1.0, "clamped to 1");
    assert_eq!(b.status, Status::Reading, "new book starts reading");
    assert!(b.last_opened_at.is_some());

    lib.set_status(id, Status::Abandoned).unwrap();
    lib.record_open(id, 0.3).unwrap();
    let b = lib.book(id).unwrap().unwrap();
    assert_eq!(b.status, Status::Abandoned, "a set status is kept on reopen");
    assert!((b.percent - 0.3).abs() < 1e-9);

    let raw = raw_connection(tmp.path());
    let n: i64 = raw
        .query_row("SELECT COUNT(*) FROM history WHERE book_id = ?1", [id], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 2, "one history row per open");

    assert!(matches!(
        lib.record_open(id, f64::NAN),
        Err(LibraryError::InvalidArgument(_))
    ));
    assert!(matches!(
        lib.record_open(4242, 0.1),
        Err(LibraryError::NotFound(4242))
    ));
    assert!(matches!(
        lib.set_status(4242, Status::Complete),
        Err(LibraryError::NotFound(4242))
    ));
}

#[test]
fn recent_lists_newest_first_and_book_by_path_resolves_links() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let a = lib.book_by_path(&files[0]).unwrap().unwrap().id;
    let b = lib.book_by_path(&files[1]).unwrap().unwrap().id;
    lib.record_open(a, 0.1).unwrap();
    lib.record_open(b, 0.1).unwrap();
    // Opened in the same second, so the id breaks the tie.
    let recent = lib.recent(10).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].id, b);
    assert_eq!(lib.recent(1).unwrap().len(), 1);

    let through_link = root.join("..").join("books").join("tall.pdf");
    assert_eq!(lib.book_by_path(&through_link).unwrap().unwrap().id, b);
    assert!(lib.book_by_path(&root.join("missing.pdf")).unwrap().is_none());
}

#[test]
fn cover_is_none_without_a_cover_row() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&files[0]).unwrap().unwrap().id;
    assert!(lib.cover(id, kaia_library::CoverSize::Large).unwrap().is_some());
    assert!(lib.cover(4242, kaia_library::CoverSize::Large).unwrap().is_none());
}

/// Writes a one-chapter EPUB whose package lists each of `creators` as a
/// `dc:creator`, so the engine reads several authors for one book.
fn write_epub_with_creators(path: &Path, creators: &[&str]) {
    const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    const CHAPTER: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>One</title></head>
<body><h1>Chapter one</h1><p>It was a dark and stormy night.</p></body></html>"#;
    let names: String = creators
        .iter()
        .map(|c| format!("<dc:creator>{c}</dc:creator>"))
        .collect();
    let opf = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:test:creators</dc:identifier>
    <dc:title>Two authors</dc:title>
    <dc:language>en</dc:language>
    {names}
  </metadata>
  <manifest><item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/></manifest>
  <spine><itemref idref="chapter"/></spine>
</package>"#
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

#[test]
fn the_engine_joins_several_creators_with_a_line_feed_that_authors_split_on() {
    let (tmp, root) = setup();
    write_epub_with_creators(&root.join("pair.epub"), &["Ada Lovelace", "Charles Babbage"]);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let books = all(&lib, Sort::new(SortKey::Title, false), &Filter::default());
    assert_eq!(books.len(), 1);
    assert_eq!(
        books[0].authors.as_deref(),
        Some("Ada Lovelace\nCharles Babbage"),
        "the scanner stores the engine's field as written"
    );
    assert_eq!(
        Library::split_authors(books[0].authors.as_deref().unwrap()),
        vec!["Ada Lovelace", "Charles Babbage"]
    );
    assert_eq!(
        Library::author_index(&books),
        vec![
            ("Ada Lovelace".to_string(), 1),
            ("Charles Babbage".to_string(), 1)
        ]
    );
    let by_babbage = Library::books_by_author(&books, "charles babbage");
    assert_eq!(by_babbage.len(), 1, "the second author finds the book too");
}

/// Adds a book row straight to the table, with the given metadata. The search
/// and suggestion tables update through their triggers.
fn add_book(
    conn: &rusqlite::Connection,
    title: &str,
    authors: Option<&str>,
    series: Option<&str>,
    opened: Option<i64>,
) -> i64 {
    let n: i64 = conn
        .query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM books", [], |r| r.get(0))
        .unwrap();
    conn.execute(
        "INSERT INTO books (path, size, mtime, partial_md5, format, added_at, title, authors, series, last_opened_at)
         VALUES (?1, 1, 1, ?1, 'epub', 1, ?2, ?3, ?4, ?5)",
        rusqlite::params![format!("/books/{n}.epub"), title, authors, series, opened],
    )
    .unwrap();
    n
}

fn suggestion_texts(lib: &Library, query: &str, limit: usize) -> Vec<(String, &'static str)> {
    lib.suggestions(query, limit)
        .unwrap()
        .into_iter()
        .map(|s| (s.text, s.kind))
        .collect()
}

#[test]
fn suggestions_put_prefixes_first_then_word_starts_then_recent_books() {
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    let raw = raw_connection(tmp.path());
    add_book(&raw, "Dune", Some("Frank Herbert"), None, Some(10));
    add_book(&raw, "Dunwich Horror", None, None, None);
    add_book(&raw, "The Dunes", None, None, Some(50));
    // An author who starts with "dun", opened most recently, comes first.
    add_book(&raw, "The Gods", Some("Dunsany"), None, Some(500));

    assert_eq!(
        suggestion_texts(&lib, "dun", 10),
        [
            ("Dunsany".to_string(), "author"),
            ("Dune".to_string(), "title"),
            ("Dunwich Horror".to_string(), "title"),
            ("The Dunes".to_string(), "title"),
        ],
        "prefix matches by recency, never-opened last, then the word match"
    );
    assert_eq!(
        suggestion_texts(&lib, "dun", 2).len(),
        2,
        "the limit cuts the list"
    );
}

#[test]
fn suggestions_ignore_case_and_match_each_word_at_a_word_start() {
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    let raw = raw_connection(tmp.path());
    add_book(&raw, "Dune", Some("Frank Herbert"), Some("Dune Saga"), None);

    let upper = suggestion_texts(&lib, "DUN", 10);
    assert_eq!(upper, suggestion_texts(&lib, "dun", 10), "case does not matter");
    assert_eq!(
        upper,
        [("Dune".to_string(), "title"), ("Dune Saga".to_string(), "series"),],
        "a title and a series that start with the text"
    );
    assert_eq!(
        suggestion_texts(&lib, "herb", 10),
        [("Frank Herbert".to_string(), "author")],
        "a word inside an author name matches"
    );
    assert_eq!(
        suggestion_texts(&lib, "frank h", 10),
        [("Frank Herbert".to_string(), "author")],
        "a phrase at the start of the name matches"
    );
    assert_eq!(
        suggestion_texts(&lib, "herbert frank", 10),
        [("Frank Herbert".to_string(), "author")],
        "word order does not matter for a word match"
    );
    assert!(
        suggestion_texts(&lib, "ert", 10).is_empty(),
        "a text inside a word does not match"
    );
}

#[test]
fn suggestions_count_titles_authors_and_series_but_not_descriptions() {
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    let raw = raw_connection(tmp.path());
    let id = add_book(&raw, "Plain", None, None, None);
    raw.execute(
        "UPDATE books SET description = 'Arrakis spice trade' WHERE id = ?1",
        [id],
    )
    .unwrap();
    assert!(
        suggestion_texts(&lib, "arr", 10).is_empty(),
        "a word only in the description suggests nothing"
    );
    assert_eq!(
        lib.search("arr", 10).unwrap().len(),
        1,
        "the search still finds it"
    );
}

#[test]
fn suggestions_list_each_author_once_and_skip_missing_books() {
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    let raw = raw_connection(tmp.path());
    add_book(
        &raw,
        "One",
        Some("Ursula K. Le Guin\nAnnie Proulx"),
        None,
        Some(1),
    );
    add_book(&raw, "Two", Some("Ursula K. Le Guin"), None, Some(2));
    let gone = add_book(&raw, "Lost Guin", Some("Ursula K. Le Guin"), None, Some(3));
    raw.execute("UPDATE books SET missing_since = 1 WHERE id = ?1", [gone])
        .unwrap();

    assert_eq!(
        suggestion_texts(&lib, "ursula", 10),
        [("Ursula K. Le Guin".to_string(), "author")],
        "one suggestion for the author of two books, and none from the missing book"
    );
    assert_eq!(
        suggestion_texts(&lib, "ann", 10),
        [("Annie Proulx".to_string(), "author")],
        "each name of a book with two authors is its own suggestion"
    );
}

#[test]
fn suggestions_are_empty_for_blank_or_symbol_only_text() {
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    add_book(&raw_connection(tmp.path()), "Dune", None, None, None);
    assert!(lib.suggestions("", 5).unwrap().is_empty());
    assert!(lib.suggestions("   ", 5).unwrap().is_empty());
    assert!(lib.suggestions(" -- ** ", 5).unwrap().is_empty());
    assert!(lib.suggestions("dune", 0).unwrap().is_empty());
}

#[test]
fn suggestions_fold_latin_accents_as_the_search_index_does() {
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    let raw = raw_connection(tmp.path());
    add_book(&raw, "Éclair", None, None, None);
    add_book(&raw, "Canción", Some("Ana Peña"), None, None);
    add_book(&raw, "Naïve", None, None, None);
    add_book(&raw, "Mi Señor", Some("Señor Pérez"), None, None);

    let eclair = [("Éclair".to_string(), "title")];
    assert_eq!(
        suggestion_texts(&lib, "ecl", 5),
        eclair,
        "plain letters find the accented title"
    );
    assert_eq!(
        suggestion_texts(&lib, "Écl", 5),
        eclair,
        "the same accent matches"
    );
    assert_eq!(
        suggestion_texts(&lib, "éclair", 5),
        eclair,
        "a whole word matches"
    );
    assert_eq!(
        suggestion_texts(&lib, "cancion", 5),
        [("Canción".to_string(), "title")]
    );
    assert_eq!(
        suggestion_texts(&lib, "CANCIÓN", 5),
        suggestion_texts(&lib, "cancion", 5),
        "accents and case together"
    );
    assert_eq!(
        suggestion_texts(&lib, "naive", 5),
        [("Naïve".to_string(), "title")]
    );
    assert_eq!(
        suggestion_texts(&lib, "pena", 5),
        [("Ana Peña".to_string(), "author")],
        "a word inside an author name matches without the accent"
    );
    assert_eq!(
        suggestion_texts(&lib, "senor", 5),
        [
            ("Señor Pérez".to_string(), "author"),
            ("Mi Señor".to_string(), "title"),
        ],
        "a prefix of an author name first, then a word of a title"
    );
    assert_eq!(
        lib.search("ecl", 5).unwrap().len(),
        1,
        "the search index folds accents too"
    );
}

#[test]
fn suggestions_keep_texts_that_differ_only_in_accents() {
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    let raw = raw_connection(tmp.path());
    add_book(&raw, "Cafe", None, None, None);
    add_book(&raw, "Café", None, None, None);
    assert_eq!(
        suggestion_texts(&lib, "caf", 5),
        [("Cafe".to_string(), "title"), ("Café".to_string(), "title")],
        "two titles, two suggestions"
    );
}

#[test]
fn suggestions_keep_the_accents_of_other_scripts() {
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    let raw = raw_connection(tmp.path());
    add_book(&raw, "Οδύσσεια", None, None, None);
    add_book(&raw, "がっこう", None, None, None);

    let odyssey = [("Οδύσσεια".to_string(), "title")];
    assert_eq!(
        suggestion_texts(&lib, "Οδύσσεια", 5),
        odyssey,
        "a Greek title suggests itself"
    );
    assert_eq!(
        suggestion_texts(&lib, "οδύσσεια", 5),
        odyssey,
        "case does not matter"
    );
    assert!(
        suggestion_texts(&lib, "Οδυσσεια", 5).is_empty(),
        "Greek tonos is kept, as the index keeps it"
    );
    assert_eq!(
        suggestion_texts(&lib, "がっこう", 5),
        [("がっこう".to_string(), "title")],
        "a Japanese title suggests itself"
    );
    assert!(
        suggestion_texts(&lib, "かっこう", 5).is_empty(),
        "the dakuten is kept, as the index keeps it"
    );
}

#[test]
fn books_titled_finds_the_books_with_one_title() {
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    let raw = raw_connection(tmp.path());
    let one = add_book(&raw, "Leaves", None, None, None);
    add_book(&raw, "Dune", None, None, None);
    add_book(&raw, "dune", None, None, None);
    let gone = add_book(&raw, "Gone", None, None, None);
    raw.execute("UPDATE books SET missing_since = 1 WHERE id = ?1", [gone])
        .unwrap();

    let ids = |t: &str| -> Vec<i64> { lib.books_titled(t).unwrap().into_iter().map(|b| b.id).collect() };
    assert_eq!(ids("LEAVES"), vec![one], "one book, any case");
    assert_eq!(ids("Dune").len(), 2, "two books with the title");
    assert!(ids("Leave").is_empty(), "a prefix is not the title");
    assert!(ids("Gone").is_empty(), "a missing book is left out");
}

#[test]
fn suggestions_find_an_accented_prefix_past_the_candidate_limit() {
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    let raw = raw_connection(tmp.path());
    // More books match than one query reads. The accented title has the
    // highest id and no opens, so only the SQL prefix test can keep it.
    for i in 0..300 {
        add_book(&raw, &format!("Cafe {i:03}"), None, None, None);
    }
    add_book(&raw, "Café crème", None, None, None);
    assert_eq!(
        suggestion_texts(&lib, "cafe c", 5).first(),
        Some(&("Café crème".to_string(), "title")),
        "the accented title is a candidate and ranks first"
    );
}

/// Keystrokes on a library of 2,000 books each take under 50 ms. The test
/// prints the time of each query.
#[test]
fn suggestions_on_2000_books_take_under_50_ms() {
    const WORDS: [&str; 10] = [
        "silent", "river", "garden", "winter", "ocean", "lantern", "harbor", "meadow", "copper", "signal",
    ];
    let (tmp, _root) = setup();
    let lib = open_library(tmp.path());
    let mut raw = raw_connection(tmp.path());
    let tx = raw.transaction().unwrap();
    for i in 0..2000i64 {
        let title = format!(
            "{} {} {}",
            WORDS[(i % 10) as usize],
            WORDS[((i / 10) % 10) as usize],
            i
        );
        let author = format!("Author {:03}", i % 150);
        let series = (i % 3 == 0).then(|| format!("Series {}", i % 40));
        let opened = (i % 4 == 0).then_some(i);
        tx.execute(
            "INSERT INTO books (path, size, mtime, partial_md5, format, added_at, title, authors, series, last_opened_at)
             VALUES (?1, 1, 1, ?1, 'epub', 1, ?2, ?3, ?4, ?5)",
            rusqlite::params![format!("/books/{i}.epub"), title, author, series, opened],
        )
        .unwrap();
    }
    tx.commit().unwrap();
    // "silent" opens 380 titles: every tenth book, and the books whose tens digit is 0.
    assert_eq!(lib.search("silent", 5000).unwrap().len(), 380);

    let mut worst = std::time::Duration::ZERO;
    for query in [
        "s",
        "si",
        "silent ri",
        "author 1",
        "series 2",
        "lan",
        "meadow copper 1",
    ] {
        let start = std::time::Instant::now();
        let found = lib.suggestions(query, 5).unwrap();
        let took = start.elapsed();
        println!(
            "suggestions {query:?}: {} rows in {:.2} ms",
            found.len(),
            took.as_secs_f64() * 1e3
        );
        worst = worst.max(took);
    }
    assert!(worst.as_millis() < 50, "slowest suggestion took {worst:?}");
}
