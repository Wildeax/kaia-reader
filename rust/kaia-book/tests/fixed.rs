//! Fixed-layout books through MuPDF: zoom, point positions, highlights and search.

mod common;

use kaia_book::*;
use kaia_gfx::{Point, Size};

const VIEW: Size = Size { w: 900, h: 1200 };

#[test]
fn pdf_is_fixed_and_refuses_relayout() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("2col.pdf"), &data);
    assert!(!s.is_reflowable());
    assert!(!s.uses_cre());
    assert!(s.page_count() >= 1);
    let err = s.set_layout(TextLayout::default()).expect_err("refused");
    assert!(
        matches!(err, BookError::Doc(kaia_doc::DocError::NotReflowable)),
        "got {err:?}"
    );
}

#[test]
fn navigation_uses_page_numbers_and_percent() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("tall.pdf"), &data);
    let count = s.page_count();
    assert_eq!(s.current().location, Location::Page(0));
    s.go_to_percent(0.0).unwrap();
    assert_eq!(s.current().page, 0);
    s.go_to_percent(1.0).unwrap();
    assert_eq!(s.current().page as usize, count - 1);
    assert!(!s.next_page().unwrap());
    assert!(s.go_to_percent(1.5).is_err());
}

#[test]
fn zoom_modes_change_the_page_image() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("tall.pdf"), &data);
    let fit = s.page_transform(VIEW).unwrap();
    assert!(fit.image.w <= VIEW.w && fit.image.h <= VIEW.h);

    s.set_zoom(ZoomMode::FitWidth).unwrap();
    let width = s.page_transform(VIEW).unwrap();
    assert_eq!(width.image.w, VIEW.w);

    s.set_zoom(ZoomMode::Manual { percent: 200 }).unwrap();
    let double = s.page_transform(VIEW).unwrap();
    assert!((double.scale - fit.scale * 2.0).abs() < 1e-4);

    let img = s.render_current(VIEW).unwrap();
    assert!((img.width() as i64 - double.image.w as i64).abs() <= 1);
    assert!(s.set_zoom(ZoomMode::Manual { percent: 5 }).is_err());
}

#[test]
fn point_highlight_persists_and_reads_back_its_text() {
    let data = tempfile::tempdir().unwrap();
    let book = common::repo_file("paper.pdf");
    let (word, page) = {
        let mut s = common::open(&book, &data);
        let (_, word) = common::first_word(&mut s, VIEW);
        assert!(matches!(word.pos0, Pos::Point { .. }), "PDF positions are points");
        let id = s.add_highlight(&word, Drawer::Strikeout, "red", None).unwrap();
        assert_eq!(s.annotation_text(id).unwrap(), word.text);
        let shown = s.annotations_on_page(VIEW).unwrap();
        assert_eq!(shown.len(), 1);
        assert!(!shown[0].rects.is_empty());
        s.save().unwrap();
        (word.text, s.current().page)
    };
    let mut s = common::open(&book, &data);
    let all = s.annotations().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].pageno, Some(page + 1));
    assert!(matches!(all[0].page, Location::Page(p) if p == page));
    assert_eq!(s.annotation_text(all[0].id).unwrap(), word);
}

#[test]
fn search_in_pdf_finds_a_selected_word() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("paper.pdf"), &data);
    let (_, word) = common::first_word(&mut s, VIEW);
    let needle = word.text.clone();
    s.start_search(&needle).unwrap();
    assert!(!common::wait_search(&mut s));
    let hits = s.search_results();
    assert!(!hits.is_empty(), "the word {needle:?} is on the page");
    assert!(hits[0].snippet.to_lowercase().contains(&needle.to_lowercase()));
    assert!(!hits[0].rects.is_empty());
}

#[test]
fn two_column_pdf_opens_and_renders() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("2col.pdf"), &data);
    assert_eq!(s.page_count(), 2);
    let img = s.render_current(VIEW).unwrap();
    assert!(img.width() <= VIEW.w && img.height() <= VIEW.h);
}

#[test]
fn selection_outside_text_is_none() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("tall.pdf"), &data);
    let corner = s.select_word_at(Point { x: 1, y: 1 }, VIEW).unwrap();
    assert!(corner.is_none());
}

#[test]
fn corrupt_pdf_is_an_error() {
    let data = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("broken.pdf");
    std::fs::write(&bad, b"%PDF-1.4\n%garbage that is not a document").unwrap();
    common::init_engine();
    let err = BookSession::open(&bad, common::options(&data))
        .err()
        .expect("open must fail");
    assert!(matches!(err, BookError::Doc(_)), "got {err:?}");
}
