mod common;
#[path = "../../kaia-doc/tests/support/links_fixture.rs"]
mod fixture;
use kaia_book::{LinkTarget, Location};

#[test]
fn epub_notes_chapter_links_external_links_and_history_survive_reflow() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("links.epub");
    fixture::epub(&path);
    let mut book = common::open(&path, &data);
    let links = book.links().unwrap();
    let notes: Vec<_> = links.iter().filter_map(|l| l.note.as_deref()).collect();
    assert!(
        notes
            .iter()
            .any(|n| n.contains("First note paragraph.") && n.contains("Second note paragraph.")),
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("An aside without a type.")),
        "{notes:?}"
    );
    assert!(
        notes.iter().any(|n| n.contains("A short numbered note.")),
        "{notes:?}"
    );
    assert!(!book.can_go_back(), "reading note text does not navigate");
    let external = links
        .iter()
        .find(|l| matches!(&l.target, LinkTarget::Uri(u) if u.starts_with("https:")))
        .unwrap();
    let before = book.current().location;
    assert!(!book.follow_link(external).unwrap());
    assert_eq!(book.current().location, before);
    let chapter = links
        .iter()
        .find(|l| matches!(l.target, LinkTarget::Page(_)) && l.note.is_none())
        .unwrap();
    assert!(chapter.location.is_some());
    book.follow_link(chapter).unwrap();
    assert!(book.current().page > 0);
    let chapter_position = book.current().location;
    book.jump_to(Location::Page(book.page_count() as u32 - 1))
        .unwrap();
    assert!(book.can_go_back());
    let mut layout = book.layout().clone();
    layout.font_size += 5.0;
    book.set_layout(layout).unwrap();
    common::wait_layout(&mut book);
    assert!(book.back().unwrap());
    assert_eq!(book.current().location, chapter_position);
    assert!(book.back().unwrap());
    assert_eq!(book.current().location, before);
    assert!(!book.back().unwrap());
}

#[test]
fn pdf_links_and_explicit_jumps_share_history_but_turns_and_failures_do_not() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("links.pdf");
    fixture::pdf(&path);
    let mut book = common::open(&path, &data);
    let links = book.links().unwrap();
    assert_eq!(links.len(), 2);
    let internal = links.iter().find(|l| l.target == LinkTarget::Page(1)).unwrap();
    assert!(links.iter().all(|l| l.note.is_none()));
    book.follow_link(internal).unwrap();
    assert_eq!(book.current().page, 1);
    assert!(!book.next_page().unwrap());
    assert!(book.back().unwrap());
    assert_eq!(book.current().page, 0);
    book.jump_to(Location::Page(0)).unwrap();
    assert!(!book.can_go_back());
    assert!(book.jump_to(Location::Page(999)).is_err());
    assert!(!book.can_go_back());
    book.next_page().unwrap();
    assert!(!book.can_go_back());
    book.jump_to(Location::Page(0)).unwrap();
    assert!(book.back().unwrap());
    assert_eq!(book.current().page, 1);
}

#[test]
fn fb2_notes_return_text_and_cbz_pages_have_no_links() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("notes.fb2");
    std::fs::write(&path, r##"<?xml version="1.0"?><FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink"><description><title-info><book-title>Notes</book-title><lang>en</lang></title-info></description><body><section><p>Read this <a l:href="#note" type="note">1</a>.</p></section></body><body name="notes"><section id="note"><p>An FB2 note.</p></section></body></FictionBook>"##).unwrap();
    let mut book = common::open(&path, &data);
    assert!(book
        .links()
        .unwrap()
        .iter()
        .any(|l| l.note.as_deref().is_some_and(|t| t.contains("An FB2 note."))));
    let path = data.path().join("comic.cbz");
    // A valid one-pixel GIF is enough for the comic engine.
    let gif = b"GIF89a\x01\0\x01\0\x80\0\0\0\0\0\xff\xff\xff,\0\0\0\0\x01\0\x01\0\0\x02\x02D\x01\0;";
    fixture::zip(&path, &[("page.gif", gif)]);
    let mut comic = common::open(&path, &data);
    assert_eq!(comic.page_count(), 1);
    assert!(comic.links().unwrap().is_empty());
}
