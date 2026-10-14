//! Recent searches: each distinct search once, newest first, at most the limit,
//! cleared on request, and kept by the migration that adds the table.

mod common;

use std::fs;
use std::path::PathBuf;

use common::*;
use kaia_library::RECENT_SEARCHES;

fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    fs::create_dir_all(&root).unwrap();
    (tmp, root)
}

#[test]
fn each_distinct_search_is_kept_once_newest_first() {
    let (tmp, _root) = setup();
    let mut lib = open_library(tmp.path());
    lib.record_search("dune").unwrap();
    lib.record_search("Leaves").unwrap();
    lib.record_search("  dune  ").unwrap();
    lib.record_search("Juliet").unwrap();
    assert_eq!(
        lib.recent_searches(10).unwrap(),
        ["Juliet", "dune", "Leaves"],
        "a repeated search moves to the top, trimmed"
    );
    lib.record_search("DUNE").unwrap();
    assert_eq!(
        lib.recent_searches(10).unwrap(),
        ["DUNE", "Juliet", "Leaves"],
        "a search that differs only by case replaces the older spelling"
    );
}

#[test]
fn only_the_newest_searches_are_kept() {
    let (tmp, _root) = setup();
    let mut lib = open_library(tmp.path());
    for i in 0..RECENT_SEARCHES + 2 {
        lib.record_search(&format!("search {i}")).unwrap();
    }
    let expected: Vec<String> = (2..RECENT_SEARCHES + 2)
        .rev()
        .map(|i| format!("search {i}"))
        .collect();
    assert_eq!(
        lib.recent_searches(50).unwrap(),
        expected,
        "the oldest two are gone"
    );
    assert_eq!(
        lib.recent_searches(3).unwrap(),
        &expected[..3],
        "the limit cuts the list"
    );
    let rows: i64 = raw_connection(tmp.path())
        .query_row("SELECT COUNT(*) FROM recent_searches", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, RECENT_SEARCHES as i64, "the table holds the limit");
}

#[test]
fn blank_searches_are_not_recorded() {
    let (tmp, _root) = setup();
    let mut lib = open_library(tmp.path());
    lib.record_search("").unwrap();
    lib.record_search("   ").unwrap();
    assert!(lib.recent_searches(10).unwrap().is_empty());
}

#[test]
fn clearing_removes_the_searches_and_no_book() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    lib.record_search("leaves").unwrap();
    lib.clear_recent_searches().unwrap();
    assert!(lib.recent_searches(10).unwrap().is_empty());
    assert_eq!(
        lib.books(
            kaia_library::Sort::new(kaia_library::SortKey::Title, false),
            &Default::default(),
            0,
            100
        )
        .unwrap()
        .len(),
        files.len(),
        "the books stay"
    );
}

#[test]
fn recent_searches_survive_a_reopen() {
    let (tmp, _root) = setup();
    {
        let mut lib = open_library(tmp.path());
        lib.record_search("juliet").unwrap();
        lib.record_search("leaves").unwrap();
    }
    let lib = open_library(tmp.path());
    assert_eq!(lib.recent_searches(10).unwrap(), ["leaves", "juliet"]);
}

/// A version 5 library has no search table. Opening it adds the table and keeps
/// its books.
#[test]
fn version_five_library_gains_the_table_and_keeps_its_books() {
    let (tmp, root) = setup();
    let files = fixture_set(&root);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    drop(lib);

    let raw = raw_connection(tmp.path());
    raw.execute_batch("DROP TABLE recent_searches;").unwrap();
    raw.pragma_update(None, "user_version", 5).unwrap();
    drop(raw);

    let mut lib = open_library(tmp.path());
    lib.record_search("tall").unwrap();
    assert_eq!(lib.recent_searches(10).unwrap(), ["tall"]);
    assert_eq!(
        lib.books(
            kaia_library::Sort::new(kaia_library::SortKey::Title, false),
            &Default::default(),
            0,
            100
        )
        .unwrap()
        .len(),
        files.len(),
        "the books are kept"
    );
    let version: u32 = raw_connection(tmp.path())
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 7);
}
