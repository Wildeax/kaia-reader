//! The ink anchors of a page: the origin of the page text and the origins of stored
//! strokes. The anchor must not walk the rest of the document.

mod common;

use std::time::Instant;

use kaia_doc::{Location, PointF};

#[test]
fn page_origin_near_the_end_of_leaves_is_fast() {
    let data = tempfile::tempdir().unwrap();
    let mut session = common::open(&common::repo_file("leaves.epub"), &data);
    let count = session.page_count() as u32;
    assert!(count > 100, "leaves has {count} pages");
    let page = count - 5;
    session.go_to(Location::Page(page)).expect("go to page");
    let Location::Engine(locator) = session.current().location else {
        panic!("leaves is reflowable and has text positions");
    };
    let started = Instant::now();
    let (anchor_page, origin) = session
        .ink_anchor(&locator)
        .expect("anchor")
        .expect("the locator is in the document");
    let elapsed = started.elapsed();
    println!("page origin of page {page} of {count}: {elapsed:?}");
    assert_eq!(anchor_page, page);
    assert!(origin.x >= 0.0 && origin.y >= 0.0, "origin {origin:?}");
    assert!(
        elapsed.as_millis() < 20,
        "page origin took {elapsed:?}, limit 20 ms in debug builds"
    );
}

#[test]
fn batch_origins_match_single_anchors_and_skip_other_pages() {
    let data = tempfile::tempdir().unwrap();
    let mut session = common::open(&common::repo_file("juliet.epub"), &data);
    let first = session.current().location;
    let Location::Engine(here) = first else {
        panic!("juliet is reflowable and has text positions");
    };
    session.go_to(Location::Page(4)).expect("go to page");
    let Location::Engine(later) = session.current().location else {
        panic!("juliet has text positions");
    };
    session.go_to(Location::Page(0)).expect("go to page");
    let page = session.current().page;
    let locators = vec![here.clone(), later.clone(), "/no/such/node".to_string()];
    let batch = session.ink_origins(page, &locators).expect("batch");
    assert_eq!(batch.len(), 3);
    let single: Option<(u32, PointF)> = session.ink_anchor(&here).expect("anchor");
    let (single_page, single_origin) = single.expect("here is in the document");
    assert_eq!(single_page, page);
    assert_eq!(batch[0], Some(single_origin), "same origin as the single anchor");
    assert_eq!(batch[1], None, "a locator on page 4 is not on page 0");
    assert_eq!(batch[2], None, "a missing locator is not on the page");
}
