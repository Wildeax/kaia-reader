//! Reflowable text through MuPDF (`Engine::Mupdf`). MuPDF positions are points
//! and depend on the layout, so a relayout marks highlights for repair.

mod common;

use kaia_book::*;
use kaia_gfx::Size;

const VIEW: Size = Size { w: 1264, h: 1680 };

fn open_mupdf(data: &tempfile::TempDir) -> BookSession {
    common::init_engine();
    let mut opts = common::options(data);
    opts.engine = Engine::Mupdf;
    BookSession::open(common::repo_file("juliet.epub"), opts).expect("open with MuPDF")
}

#[test]
fn mupdf_reflow_relayouts_and_marks_point_highlights_for_repair() {
    let data = tempfile::tempdir().unwrap();
    let mut s = open_mupdf(&data);
    assert!(s.is_reflowable());
    assert!(!s.uses_cre());
    assert!(s.page_count() > 10);
    s.go_to(Location::Page(5)).unwrap();
    let (_, word) = common::first_word(&mut s, VIEW);
    assert!(matches!(word.pos0, Pos::Point { .. }));
    let id = s.add_highlight(&word, Drawer::Lighten, "yellow", None).unwrap();
    assert_eq!(s.annotation_text(id).unwrap(), word.text);
    assert!(!s.annotations_on_page(VIEW).unwrap()[0].rects.is_empty());

    let mut layout = s.layout().clone();
    layout.font_size += 8.0;
    s.set_layout(layout).unwrap();
    common::wait_layout(&mut s);
    let shown = s.annotations_on_page(VIEW).unwrap();
    let mine = shown.iter().find(|a| a.annotation.id == id);
    assert!(
        mine.is_none_or(|a| a.rects.is_empty()),
        "stale boxes are not drawn"
    );
    assert!(s.annotation(id).unwrap().orphaned);

    assert!(s.repair_annotation(id).unwrap(), "the text is still in the book");
    let fixed = s.annotation(id).unwrap();
    assert!(!fixed.orphaned);
    assert_eq!(s.annotation_text(id).unwrap(), word.text);
    // The highlight may sit on another page now. Its boxes show on that page.
    s.go_to(fixed.page.clone()).unwrap();
    let on_page = s.annotations_on_page(VIEW).unwrap();
    assert!(on_page
        .iter()
        .any(|a| a.annotation.id == id && !a.rects.is_empty()));
}

#[test]
fn point_highlight_is_orphaned_when_reopened_with_another_layout() {
    let data = tempfile::tempdir().unwrap();
    let book = common::repo_file("juliet.epub");
    let mut opts = common::options(&data);
    opts.engine = Engine::Mupdf;
    {
        let mut s = BookSession::open(&book, opts.clone()).unwrap();
        s.go_to(Location::Page(5)).unwrap();
        let (_, word) = common::first_word(&mut s, VIEW);
        s.add_highlight(&word, Drawer::Lighten, "yellow", None).unwrap();
        s.save().unwrap();
    }
    // The saved layout is the default. Another default size is another layout.
    opts.viewport = Size { w: 900, h: 1200 };
    let s = BookSession::open(&book, opts).unwrap();
    let highlight = s.annotation(1).expect("the highlight");
    assert!(highlight.orphaned, "positions made at another layout need repair");
}

#[test]
fn set_viewport_relayouts_mupdf_reflow_without_blocking() {
    let data = tempfile::tempdir().unwrap();
    let mut s = open_mupdf(&data);
    let before = s.page_count();
    s.set_viewport(Size { w: 800, h: 1000 }).unwrap();
    let after = common::wait_layout(&mut s);
    assert!(after > 0 && before > 0);
}
