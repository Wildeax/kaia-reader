mod common;

use std::fs;
use std::path::PathBuf;

use common::*;
use kaia_library::{Book, Filter, LibraryError, Sort, SortKey, Status};

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
