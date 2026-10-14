//! The text weight of a reflowable book: the layout's font weight reaches
//! CREngine, changes the page, and is kept with the book's state.

mod common;

use kaia_book::*;
use kaia_gfx::Size;

const VIEW: Size = Size { w: 1264, h: 1680 };

/// Sets the font weight and waits for the engine. Returns the number of bytes
/// under 128 on the current page. Each channel of a colour page counts as one
/// byte, so the numbers only compare with each other.
fn dark_bytes_at(s: &mut BookSession, font_weight: f32) -> usize {
    let mut layout = s.layout().clone();
    if layout.font_weight != font_weight {
        layout.font_weight = font_weight;
        s.set_layout(layout).expect("set layout");
        common::wait_layout(s);
    }
    let page = s.render_current(VIEW).expect("render");
    page.data().iter().filter(|&&v| v < 128).count()
}

#[test]
fn heavier_weight_renders_a_darker_page_than_regular() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    // Page 0 is the cover, which has no text to change weight.
    assert!(s.next_page().unwrap());
    let light = dark_bytes_at(&mut s, -1.0);
    let regular = dark_bytes_at(&mut s, 0.0);
    let heavy = dark_bytes_at(&mut s, 1.0);
    assert!(
        light < regular && regular < heavy,
        "weight -1 gives {light}, 0 gives {regular}, +1 gives {heavy} dark bytes"
    );
}

#[test]
fn font_weight_is_saved_with_the_book_and_read_back_on_reopen() {
    let data = tempfile::tempdir().unwrap();
    let book = common::repo_file("juliet.epub");
    {
        let mut s = common::open(&book, &data);
        let mut layout = s.layout().clone();
        layout.font_weight = 1.5;
        s.set_layout(layout).unwrap();
        common::wait_layout(&mut s);
        s.save().unwrap();
    }
    let s = common::open(&book, &data);
    assert_eq!(s.layout().font_weight, 1.5, "the saved weight comes back");
}
