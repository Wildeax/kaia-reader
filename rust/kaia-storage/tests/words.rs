//! The word list (schema version 3): saving, ordering, ignoring case, limits,
//! deleting, and upgrading a version 2 store.

mod common;

use kaia_storage::{Store, StoreError, DB_FILE, MIGRATIONS, WORD_MAX_CHARS};

use common::*;

fn user_version(dir: &std::path::Path) -> u32 {
    let conn = rusqlite::Connection::open(dir.join(DB_FILE)).expect("raw open");
    conn.pragma_query_value(None, "user_version", |r| r.get(0))
        .expect("user_version")
}

#[test]
fn a_saved_word_comes_back_with_its_book_context_and_time() {
    let (_tmp, dir) = temp_store_dir();
    let mut store = Store::open(&dir).expect("open");
    let id = store
        .add_word(
            "  serendipity ",
            Some("Dorian Gray"),
            Some("It was a serendipity, he said."),
            1_760_000_000,
        )
        .expect("add");
    let words = store.list_words().expect("list");
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].id, id);
    assert_eq!(words[0].word, "serendipity", "the word is trimmed");
    assert_eq!(words[0].book_title.as_deref(), Some("Dorian Gray"));
    assert_eq!(
        words[0].context.as_deref(),
        Some("It was a serendipity, he said.")
    );
    assert_eq!(words[0].created_at, 1_760_000_000);
    assert_eq!(store.get_word(id).expect("get"), Some(words[0].clone()));
    assert_eq!(store.get_word(id + 99).expect("get missing"), None);
}

#[test]
fn words_list_newest_first() {
    let (_tmp, dir) = temp_store_dir();
    let mut store = Store::open(&dir).expect("open");
    store.add_word("alpha", None, None, 100).expect("add");
    store.add_word("beta", None, None, 300).expect("add");
    store.add_word("gamma", None, None, 200).expect("add");
    let order: Vec<_> = store
        .list_words()
        .expect("list")
        .into_iter()
        .map(|w| w.word)
        .collect();
    assert_eq!(order, ["beta", "gamma", "alpha"]);
}

#[test]
fn a_word_saved_again_in_another_case_keeps_one_row_with_the_new_details() {
    let (_tmp, dir) = temp_store_dir();
    let mut store = Store::open(&dir).expect("open");
    let first = store
        .add_word("Their", Some("Book one"), Some("old sentence"), 100)
        .expect("add");
    let second = store
        .add_word("their", Some("Book two"), None, 500)
        .expect("add again");
    assert_eq!(first, second, "the same word keeps its id");
    let words = store.list_words().expect("list");
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].word, "Their", "the first spelling stays");
    assert_eq!(words[0].book_title.as_deref(), Some("Book two"));
    assert_eq!(words[0].context, None, "an empty context replaces the old one");
    assert_eq!(words[0].created_at, 500, "the new time moves it to the top");
}

#[test]
fn empty_and_long_words_are_refused_and_long_text_is_cut() {
    let (_tmp, dir) = temp_store_dir();
    let mut store = Store::open(&dir).expect("open");
    assert!(matches!(
        store.add_word("   ", None, None, 1),
        Err(StoreError::InvalidArg(_))
    ));
    let long = "x".repeat(WORD_MAX_CHARS + 1);
    assert!(matches!(
        store.add_word(&long, None, None, 1),
        Err(StoreError::InvalidArg(_))
    ));
    let title = "t".repeat(400);
    store
        .add_word("word", Some(&title), Some("   "), 1)
        .expect("add with a long title");
    let saved = store.list_words().expect("list").remove(0);
    assert_eq!(saved.book_title.map(|t| t.chars().count()), Some(300));
    assert_eq!(saved.context, None, "blank context is no context");
}

#[test]
fn deleting_removes_the_word_and_a_second_delete_fails() {
    let (_tmp, dir) = temp_store_dir();
    let mut store = Store::open(&dir).expect("open");
    let id = store.add_word("gone", None, None, 1).expect("add");
    store.delete_word(id).expect("delete");
    assert!(store.list_words().expect("list").is_empty());
    assert!(matches!(store.delete_word(id), Err(StoreError::NotFound(_))));
}

#[test]
fn the_word_list_survives_a_reopen() {
    let (_tmp, dir) = temp_store_dir();
    {
        let mut store = Store::open(&dir).expect("open");
        store
            .add_word("kept", Some("Book"), Some("A kept sentence."), 42)
            .expect("add");
    }
    let store = Store::open(&dir).expect("reopen");
    let words = store.list_words().expect("list");
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].word, "kept");
    assert_eq!(words[0].context.as_deref(), Some("A kept sentence."));
}

#[test]
fn a_version_2_store_upgrades_and_takes_words() {
    let (_tmp, dir) = temp_store_dir();
    drop(Store::open_with_migrations(&dir, &MIGRATIONS[..2]).expect("create v2"));
    assert_eq!(user_version(&dir), 2);
    let mut store = Store::open(&dir).expect("upgrade to the latest version");
    assert_eq!(user_version(&dir), MIGRATIONS.len() as u32);
    store
        .add_word("upgraded", None, None, 7)
        .expect("add after upgrade");
    assert_eq!(store.list_words().expect("list").len(), 1);
    let backups = std::fs::read_dir(dir.join("backups")).expect("backups").count();
    assert_eq!(backups, 1, "the upgrade takes a backup first");
}
