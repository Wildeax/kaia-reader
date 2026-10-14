//! The image share of a page: the fraction of the page area that images cover.
//! The reader uses it to choose a full refresh for image pages.

mod common;

use kaia_book::*;
use kaia_gfx::Size;

const VIEW: Size = Size { w: 1264, h: 1680 };

/// One text page, a page with a picture across the width, a text page, a page
/// with a small inline figure, and a text page. Each section starts a page.
const BOOK: &str = r#"<h1 id="ch1">Chapter one</h1>
<p>Some text on the first page.</p>
<div style="page-break-before: always"><img src="pic.png" style="width: 100%"/></div>
<p style="page-break-before: always">Text after the picture.</p>
<p style="page-break-before: always">A small figure <img src="pic.png"/> in a line of text.</p>
<p style="page-break-before: always">Plain text again.</p>"#;

/// Opens an EPUB with `body` in a fresh directory. `engine` picks the backend.
fn open_book(body: &str, engine: Engine) -> (tempfile::TempDir, BookSession) {
    let data = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let book = dir.path().join("image.epub");
    common::epub::write_epub(&book, body, &[("Chapter one", "ch1")]);
    common::init_engine();
    let mut opts = common::options(&data);
    opts.viewport = VIEW;
    opts.engine = engine;
    let session = BookSession::open(&book, opts).expect("open the image book");
    (data, session)
}

/// The share of each page of the book, in order, after rendering it.
fn shares_of(session: &mut BookSession) -> Vec<Option<f32>> {
    (0..session.page_count() as u32)
        .map(|page| {
            session.go_to(Location::Page(page)).expect("go to page");
            session.render_current(VIEW).expect("render page");
            session.page_image_share()
        })
        .collect()
}

#[test]
fn cre_reports_the_picture_share_and_text_pages_have_none() {
    let (_data, mut session) = open_book(BOOK, Engine::Auto);
    assert!(session.uses_cre());
    let shares = shares_of(&mut session);
    assert_eq!(shares.len(), 5, "one page per section, got {shares:?}");
    let text = [0, 3, 4];
    for page in text {
        let share = shares[page].expect("CREngine measures the page");
        assert!(share < 0.01, "text page {page} has share {share}");
    }
    // The picture fills the width and most of the height of the page.
    let picture = shares[1].expect("CREngine measures the picture page");
    assert!(
        (0.5..=1.0).contains(&picture),
        "picture page share {picture} is an image page"
    );
    // A figure in a line of text covers a small part of its page. CREngine
    // gives an inline image no box in every case, so the share may read as 0.
    let figure = shares[2].expect("CREngine measures the figure page");
    assert!(figure < 0.1, "figure page share {figure} is a text page");
}

#[test]
fn share_follows_the_page_on_screen_after_a_jump() {
    let (_data, mut session) = open_book(BOOK, Engine::Auto);
    session.go_to(Location::Page(1)).unwrap();
    session.render_current(VIEW).unwrap();
    assert!(session.page_image_share().unwrap() >= 0.5);
    session.go_to(Location::Page(0)).unwrap();
    session.render_current(VIEW).unwrap();
    assert!(session.page_image_share().unwrap() < 0.01);
}

#[test]
fn fixed_layout_pages_report_a_full_share() {
    let data = tempfile::tempdir().unwrap();
    let mut session = common::open(&common::repo_file("sample.pdf"), &data);
    assert!(!session.is_reflowable());
    session.render_current(VIEW).unwrap();
    assert_eq!(session.page_image_share(), Some(1.0));
}

#[test]
fn mupdf_reflow_reports_no_share() {
    // The app never opens EPUB with MuPDF, but the rule should stay honest:
    // MuPDF reflow has no cheap image test, so the share is unknown.
    let (_data, mut session) = open_book(BOOK, Engine::Mupdf);
    assert!(session.is_reflowable());
    session.render_current(VIEW).unwrap();
    assert_eq!(session.page_image_share(), None);
}
