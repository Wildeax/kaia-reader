//! Per-book dictionaries and the sentence around a selection, through a real
//! book: the saved state, the reopen, and the page text.

mod common;

use kaia_book::{Location, Selection};
use kaia_gfx::Size;

use common::*;

const VIEW: Size = Size { w: 1264, h: 1680 };

#[test]
fn preferred_dictionaries_keep_their_order_and_survive_a_reopen() {
    let data = tempfile::tempdir().unwrap();
    let book = repo_file("juliet.epub");
    {
        let mut s = open(&book, &data);
        assert!(
            s.preferred_dictionaries().is_empty(),
            "a new book has no preference"
        );
        s.set_preferred_dictionaries(vec![
            "Second Dictionary".into(),
            "  ".into(),
            "Test Dictionary".into(),
            "Second Dictionary".into(),
        ])
        .unwrap();
        assert_eq!(
            s.preferred_dictionaries(),
            ["Second Dictionary", "Test Dictionary"],
            "blank names and repeats are dropped, the first place is kept"
        );
    }
    let s = open(&book, &data);
    assert_eq!(
        s.preferred_dictionaries(),
        ["Second Dictionary", "Test Dictionary"],
        "the list is in the book's state file"
    );
}

#[test]
fn clearing_the_preference_empties_the_list() {
    let data = tempfile::tempdir().unwrap();
    let book = repo_file("juliet.epub");
    let mut s = open(&book, &data);
    s.set_preferred_dictionaries(vec!["Test Dictionary".into()])
        .unwrap();
    s.set_preferred_dictionaries(Vec::new()).unwrap();
    drop(s);
    let s = open(&book, &data);
    assert!(s.preferred_dictionaries().is_empty());
}

#[test]
fn the_sentence_of_a_selected_word_holds_the_word() {
    let data = tempfile::tempdir().unwrap();
    let mut s = open(&repo_file("juliet.epub"), &data);
    s.go_to(Location::Page(5)).unwrap();
    let (_, selection): (_, Selection) = first_word(&mut s, VIEW);
    let sentence = s
        .selection_sentence(&selection)
        .unwrap()
        .expect("the selected word is on the page");
    let word = selection.text.trim();
    assert!(
        sentence.contains(word),
        "the sentence {sentence:?} holds the word {word:?}"
    );
    assert!(
        sentence.len() < 2_000,
        "a sentence is a short window of text, got {} bytes",
        sentence.len()
    );
    assert!(
        !sentence.contains('\n'),
        "line breaks become spaces in the sentence"
    );
}

#[test]
fn a_book_has_highlights_only_once_one_is_added() {
    let data = tempfile::tempdir().unwrap();
    let mut s = open(&repo_file("juliet.epub"), &data);
    assert!(!s.has_highlights());
    s.toggle_bookmark().unwrap();
    assert!(!s.has_highlights(), "a bookmark is not a highlight");
    s.go_to(Location::Page(5)).unwrap();
    let (_, selection) = first_word(&mut s, VIEW);
    s.add_highlight(&selection, kaia_book::Drawer::Underscore, "green", None)
        .unwrap();
    assert!(s.has_highlights());
}
