#![cfg(feature = "cre")]

mod common;

use kaia_doc::cre::CreDocument;
use kaia_doc::{Document, Layout};

#[test]
fn engine_compares_toc_positions_and_aliases_across_reflow() {
    let mut doc = CreDocument::open(&common::fixture("juliet.epub")).unwrap();
    let toc = doc.toc();
    let act = toc.iter().position(|t| t.title == "Act I").unwrap();
    let a = toc[act].xpointer.as_deref().unwrap();
    let b = toc[act + 1].xpointer.as_deref().unwrap();
    eprintln!("Act I: {a}; Prologue: {b}");
    assert_eq!(doc.compare_xpointers(a, a).unwrap(), Some(0));
    assert!(doc.compare_xpointers(a, b).unwrap().unwrap() > 0);
    assert!(doc.compare_xpointers(b, a).unwrap().unwrap() < 0);
    assert_eq!(doc.compare_xpointers(a, "/missing").unwrap(), None);
    // An omitted first sibling index and [1] name the same node. A textual
    // comparison cannot establish that these positions are equal.
    let alias = a.replacen("/body/", "/body[1]/", 1);
    assert_ne!(a, alias);
    assert_eq!(doc.compare_xpointers(a, &alias).unwrap(), Some(0));
    let before = toc[act].page;
    doc.set_layout(&Layout::new(500.0, 700.0, 40.0)).unwrap();
    let after = doc.toc();
    assert_ne!(before, after[act].page);
    assert_eq!(toc[act].xpointer, after[act].xpointer);
    assert!(doc.compare_xpointers(a, b).unwrap().unwrap() > 0);
    let generic: &dyn Document = &doc;
    assert_eq!(generic.compare_xpointers(a, &alias).unwrap(), Some(0));
}

#[test]
fn juliet_pixel_layout_matches_stock_cre_with_the_same_settings() {
    // tests/koreader_layout.lua reproduces this count with stock KOReader.
    let doc = CreDocument::open(&common::fixture("juliet.epub")).unwrap();
    assert_eq!(doc.page_count(), 114);
}
