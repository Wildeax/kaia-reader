//! Favorites and tags. A favorite is a member of the collection `favorites`, the
//! one KOReader keeps, so a KOReader import and Kaia share one store. A tag is
//! free text on a book, with a trimmed, case-insensitive key, a length limit and
//! a count limit per book.

mod common;

use std::fs;
use std::path::PathBuf;

use common::*;
use kaia_library::{
    Book, Filter, Library, LibraryError, Sort, SortKey, Status, MAX_TAGS_PER_BOOK, MAX_TAG_CHARS,
};

/// A library with the two EPUBs of the repository scanned in. Returns the library
/// folder, the database handle and the ids of `juliet` and `leaves`.
fn indexed(name: &str) -> (tempfile::TempDir, PathBuf, Library, i64, i64) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(name);
    let books = dir.join("books");
    fs::create_dir_all(&books).unwrap();
    copy_fixture("juliet.epub", &books, "juliet.epub");
    copy_fixture("leaves.epub", &books, "leaves.epub");
    let mut lib = open_library(&dir);
    scan(&mut lib, std::slice::from_ref(&books));
    let juliet = id_of(&lib, "juliet.epub");
    let leaves = id_of(&lib, "leaves.epub");
    (tmp, dir, lib, juliet, leaves)
}

/// The id of the book whose file name is `file`.
fn id_of(lib: &Library, file: &str) -> i64 {
    all(lib)
        .into_iter()
        .find(|b| b.path.file_name().is_some_and(|f| f == file))
        .unwrap_or_else(|| panic!("{file} is not indexed"))
        .id
}

fn all(lib: &Library) -> Vec<Book> {
    lib.books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
        .unwrap()
}

/// The ids of the books the filter shows, sorted.
fn shown(lib: &Library, filter: &Filter) -> Vec<i64> {
    let mut ids: Vec<i64> = lib
        .books(Sort::new(SortKey::Title, false), filter, 0, 100)
        .unwrap()
        .into_iter()
        .map(|b| b.id)
        .collect();
    ids.sort_unstable();
    ids
}

fn favorites() -> Filter {
    Filter {
        favorite: true,
        ..Filter::default()
    }
}

fn tagged(text: &str) -> Filter {
    Filter {
        tag: Some(text.to_string()),
        ..Filter::default()
    }
}

#[test]
fn marking_a_favorite_and_unmarking_it_filters_the_list() {
    let (_tmp, _dir, mut lib, juliet, leaves) = indexed("favorite-filter");
    assert!(!lib.is_favorite(juliet).unwrap());
    assert!(shown(&lib, &favorites()).is_empty(), "no favorites yet");

    lib.set_favorite(juliet, true).unwrap();
    assert!(lib.is_favorite(juliet).unwrap());
    assert!(!lib.is_favorite(leaves).unwrap());
    assert_eq!(shown(&lib, &favorites()), vec![juliet]);

    lib.set_favorite(juliet, true).unwrap();
    let collections = lib.collections().unwrap();
    assert_eq!(collections.len(), 1, "marking twice keeps one collection");
    assert_eq!(collections[0].books, 1, "marking twice keeps one membership");
    assert_eq!(collections[0].name, "Favorites");

    lib.set_favorite(juliet, false).unwrap();
    assert!(!lib.is_favorite(juliet).unwrap());
    assert!(shown(&lib, &favorites()).is_empty());
    lib.set_favorite(juliet, false).unwrap();
    assert_eq!(
        lib.collections().unwrap()[0].books,
        0,
        "unmarking twice is harmless"
    );
}

#[test]
fn a_favorite_filter_combines_with_the_status_filter() {
    let (_tmp, _dir, mut lib, juliet, leaves) = indexed("favorite-status");
    lib.set_favorite(juliet, true).unwrap();
    lib.set_favorite(leaves, true).unwrap();
    lib.record_open(juliet, 0.3).unwrap();

    let reading = Filter {
        favorite: true,
        status: Some(Status::Reading),
        ..Filter::default()
    };
    assert_eq!(shown(&lib, &reading), vec![juliet]);
}

#[test]
fn an_import_of_koreader_favorites_is_the_same_store() {
    let (tmp, dir, mut lib, juliet, leaves) = indexed("favorite-koreader");
    let collection = tmp.path().join("collection.lua");
    fs::write(
        &collection,
        format!(
            r#"return {{
    ["favorites"] = {{
        [1] = {{ ["file"] = "{d}/books/juliet.epub", ["order"] = 1 }},
    }},
}}
"#,
            d = dir.display()
        ),
    )
    .unwrap();
    lib.import_koreader_collections(&collection).unwrap();

    assert!(
        lib.is_favorite(juliet).unwrap(),
        "KOReader's favorite is Kaia's favorite"
    );
    assert!(!lib.is_favorite(leaves).unwrap());
    lib.set_favorite(leaves, true).unwrap();
    let collections = lib.collections().unwrap();
    assert_eq!(collections.len(), 1, "one favorites collection, not two");
    assert_eq!(collections[0].name, "favorites", "the imported name is kept");
    assert_eq!(collections[0].books, 2);
}

#[test]
fn the_favorites_collection_cannot_be_renamed_or_deleted() {
    let (_tmp, _dir, mut lib, juliet, _leaves) = indexed("favorite-fixed");
    lib.set_favorite(juliet, true).unwrap();
    let id = lib.collections().unwrap()[0].id;
    assert!(matches!(
        lib.rename_collection(id, "Best"),
        Err(LibraryError::InvalidArgument(_))
    ));
    assert!(matches!(
        lib.delete_collection(id),
        Err(LibraryError::InvalidArgument(_))
    ));
    assert!(lib.is_favorite(juliet).unwrap(), "the favorite is kept");

    let other = lib.create_collection("Shelf").unwrap();
    lib.rename_collection(other.id, "Desk").unwrap();
    assert!(
        lib.delete_collection(other.id).unwrap(),
        "other collections still go"
    );
}

#[test]
fn a_tag_is_trimmed_and_matched_without_regard_to_case() {
    let (_tmp, _dir, mut lib, juliet, leaves) = indexed("tag-dedupe");
    let first = lib.add_tag(juliet, "  Sci-Fi  ").unwrap();
    assert_eq!(first.name, "Sci-Fi", "the text is trimmed");
    let again = lib.add_tag(juliet, "sci-fi").unwrap();
    assert_eq!(again.id, first.id, "a tag differing in case is the same tag");
    assert_eq!(again.name, "Sci-Fi", "the first spelling is the one shown");
    lib.add_tag(juliet, "Sci-Fi").unwrap();
    assert_eq!(lib.book_tags(juliet).unwrap().len(), 1, "a book keeps one copy");

    lib.add_tag(leaves, "SCI-fi").unwrap();
    lib.add_tag(leaves, "classic   novel").unwrap();
    let tags = lib.tags().unwrap();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].name, "classic novel", "inner spaces become one");
    assert_eq!(tags[0].books, 1);
    assert_eq!(tags[1].name, "Sci-Fi");
    assert_eq!(tags[1].books, 2, "the count is the number of books");
}

#[test]
fn removing_a_tag_drops_it_from_the_book_and_from_the_list_when_unused() {
    let (_tmp, _dir, mut lib, juliet, leaves) = indexed("tag-remove");
    let shared = lib.add_tag(juliet, "war").unwrap();
    lib.add_tag(leaves, "war").unwrap();
    let only_juliet = lib.add_tag(juliet, "romance").unwrap();

    assert!(lib.remove_tag(juliet, only_juliet.id).unwrap());
    assert!(
        !lib.remove_tag(juliet, only_juliet.id).unwrap(),
        "removing twice is false"
    );
    assert!(
        lib.tags().unwrap().iter().all(|t| t.name != "romance"),
        "a tag no book carries leaves the list"
    );
    let left: Vec<i64> = lib.book_tags(juliet).unwrap().iter().map(|t| t.id).collect();
    assert_eq!(left, vec![shared.id], "the shared tag stays on the book");

    assert!(lib.remove_tag(juliet, shared.id).unwrap());
    assert_eq!(lib.tags().unwrap()[0].books, 1, "the other book keeps the tag");
    assert!(lib.book_tags(juliet).unwrap().is_empty());
}

#[test]
fn a_tag_needs_text_and_fits_the_length_limit() {
    let (_tmp, _dir, mut lib, juliet, _leaves) = indexed("tag-length");
    assert!(matches!(lib.add_tag(juliet, ""), Err(LibraryError::TagEmpty)));
    assert!(matches!(
        lib.add_tag(juliet, "   \t "),
        Err(LibraryError::TagEmpty)
    ));

    let longest = "a".repeat(MAX_TAG_CHARS);
    lib.add_tag(juliet, &format!("  {longest}  ")).unwrap();
    assert_eq!(
        lib.book_tags(juliet).unwrap()[0].name,
        longest,
        "the spaces do not count"
    );

    let too_long = "a".repeat(MAX_TAG_CHARS + 1);
    assert!(matches!(
        lib.add_tag(juliet, &too_long),
        Err(LibraryError::TagTooLong(MAX_TAG_CHARS))
    ));
    // Characters count, not bytes: 40 accented letters are 80 bytes and still fit.
    let accented = "é".repeat(MAX_TAG_CHARS);
    lib.add_tag(juliet, &accented).unwrap();
    assert_eq!(lib.book_tags(juliet).unwrap().len(), 2);
}

#[test]
fn a_book_takes_at_most_twenty_tags() {
    let (_tmp, _dir, mut lib, juliet, leaves) = indexed("tag-count");
    for n in 0..MAX_TAGS_PER_BOOK {
        lib.add_tag(juliet, &format!("tag {n}")).unwrap();
    }
    assert_eq!(lib.book_tags(juliet).unwrap().len(), MAX_TAGS_PER_BOOK);

    assert!(matches!(
        lib.add_tag(juliet, "one too many"),
        Err(LibraryError::TooManyTags(MAX_TAGS_PER_BOOK))
    ));
    assert_eq!(
        lib.book_tags(juliet).unwrap().len(),
        MAX_TAGS_PER_BOOK,
        "the refused tag is not stored"
    );
    assert!(
        lib.tags().unwrap().iter().all(|t| t.name != "one too many"),
        "a refused tag does not appear in the list either"
    );

    // A tag the book already has is not a new tag, so the limit does not refuse it.
    lib.add_tag(juliet, "TAG 3").unwrap();
    assert_eq!(lib.book_tags(juliet).unwrap().len(), MAX_TAGS_PER_BOOK);

    // Another book has room for its own twenty.
    lib.add_tag(leaves, "one too many").unwrap();
}

#[test]
fn a_tag_filter_matches_without_regard_to_case_and_combines_with_favorites() {
    let (_tmp, _dir, mut lib, juliet, leaves) = indexed("tag-filter");
    lib.add_tag(juliet, "Romance").unwrap();
    lib.add_tag(leaves, "classic").unwrap();
    lib.set_favorite(juliet, true).unwrap();

    assert_eq!(shown(&lib, &tagged("romance")), vec![juliet]);
    assert_eq!(shown(&lib, &tagged("  ROMANCE ")), vec![juliet]);
    assert_eq!(shown(&lib, &tagged("classic")), vec![leaves]);
    assert!(shown(&lib, &tagged("missing")).is_empty());
    assert_eq!(
        shown(&lib, &tagged("   ")),
        vec![juliet, leaves],
        "blank text does not filter"
    );

    let both = Filter {
        favorite: true,
        tag: Some("classic".into()),
        ..Filter::default()
    };
    assert!(
        shown(&lib, &both).is_empty(),
        "a favorite with that tag is not there"
    );
}

#[test]
fn tags_and_favorites_survive_reopening_the_library() {
    let (_tmp, dir, mut lib, juliet, leaves) = indexed("reopen");
    lib.set_favorite(juliet, true).unwrap();
    lib.add_tag(juliet, "Sci-Fi").unwrap();
    lib.add_tag(juliet, "classic").unwrap();
    drop(lib);

    let lib = Library::open(&db_path(&dir)).unwrap();
    assert!(lib.is_favorite(juliet).unwrap());
    assert!(!lib.is_favorite(leaves).unwrap());
    let names: Vec<String> = lib
        .book_tags(juliet)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names, vec!["classic", "Sci-Fi"], "tags come back in name order");
    assert_eq!(shown(&lib, &favorites()), vec![juliet]);
    assert_eq!(shown(&lib, &tagged("sci-fi")), vec![juliet]);
}

#[test]
fn removing_a_book_takes_its_tags_with_it() {
    let (_tmp, _dir, mut lib, juliet, leaves) = indexed("remove-book");
    lib.add_tag(juliet, "gone soon").unwrap();
    lib.add_tag(leaves, "kept").unwrap();
    assert!(lib.remove_book(juliet).unwrap());
    let names: Vec<String> = lib.tags().unwrap().into_iter().map(|t| t.name).collect();
    assert_eq!(
        names,
        vec!["kept"],
        "the tag of the removed book is gone from the list"
    );
}

#[test]
fn tag_and_favorite_calls_on_an_unknown_book_fail_and_change_nothing() {
    let (_tmp, _dir, mut lib, _juliet, _leaves) = indexed("unknown");
    assert!(matches!(
        lib.add_tag(9999, "x"),
        Err(LibraryError::NotFound(9999))
    ));
    assert!(matches!(
        lib.set_favorite(9999, true),
        Err(LibraryError::NotFound(9999))
    ));
    assert!(matches!(
        lib.set_favorite(9999, false),
        Err(LibraryError::NotFound(9999))
    ));
    assert!(lib.tags().unwrap().is_empty());
    assert!(lib.collections().unwrap().is_empty());
}
