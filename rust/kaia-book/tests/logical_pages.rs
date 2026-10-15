mod common;
use kaia_book::{BookSession, Location, Margins, OpenOptions, Size};

#[test]
fn juliet_turns_cross_logical_blocks_in_epub_and_kepub() {
    common::init_engine();
    for name in ["juliet.epub", "juliet.kepub.epub"] {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join(name);
        std::fs::copy(common::repo_file("juliet.epub"), &book).unwrap();
        let mut opts = OpenOptions::new(dir.path().join("data"));
        opts.viewport = Size::new(1264, 1440);
        opts.default_layout.font_size = 76.0;
        opts.default_layout.line_spacing = 1.35;
        opts.default_layout.hyphenation = false;
        opts.default_layout.margins = Margins {
            top: 64.0,
            bottom: 64.0,
            left: 71.0,
            right: 71.0,
        };
        let mut session = BookSession::open(book, opts).unwrap();
        assert_eq!(session.logical_progress().unwrap().page, 1);
        assert_eq!(session.logical_progress().unwrap().total, 96);
        let about = session
            .toc()
            .iter()
            .find(|e| e.title == "About")
            .unwrap()
            .page
            .unwrap();
        let end = session
            .toc()
            .iter()
            .filter_map(|e| e.page)
            .filter(|&p| p > about)
            .min()
            .unwrap();
        let mut labels = Vec::new();
        for page in about..end {
            session.go_to(Location::Page(page)).unwrap();
            let logical = session.logical_progress().unwrap();
            assert_eq!((logical.total, logical.chapter_total), (96, 2));
            let expected = 3 + (2 * (page - about) / (end - about));
            assert_eq!(logical.page, expected);
            assert_eq!(logical.chapter_page, expected - 2);
            assert_eq!(session.current().page, page);
            assert!(
                (session.current().percent - f64::from(page + 1) / session.page_count() as f64).abs()
                    < 0.0001
            );
            labels.push(logical.page);
        }
        assert!(labels.windows(2).any(|w| w == [3, 3]));
        assert_eq!(labels.windows(2).filter(|w| w[0] != w[1]).count(), 1);
        println!("{name}: About screens {about}..{end}, logical pages {labels:?}, total 96");

        // Reflow replaces the cached screen boundaries without changing ZIP pages.
        let mut layout = session.layout().clone();
        layout.font_size = 40.0;
        session.set_layout(layout).unwrap();
        assert!(session.logical_progress().is_none());
        common::wait_layout(&mut session);
        let start = session
            .toc()
            .iter()
            .find(|e| e.title == "About")
            .unwrap()
            .page
            .unwrap();
        let next = session
            .toc()
            .iter()
            .filter_map(|e| e.page)
            .filter(|&p| p > start)
            .min()
            .unwrap();
        assert!(next - start < end - about);
        session.go_to(Location::Page(start)).unwrap();
        let first = session.logical_progress().unwrap();
        assert_eq!((first.page, first.total, first.chapter_total), (3, 96, 2));
        session
            .go_to(Location::Page(start + (next - start).div_ceil(2)))
            .unwrap();
        assert_eq!(session.logical_progress().unwrap().page, 4);
    }
}

#[test]
fn fixed_layout_keeps_document_pages() {
    let data = tempfile::tempdir().unwrap();
    let session = common::open(&common::repo_file("paper.pdf"), &data);
    assert!(session.logical_progress().is_none());
    assert_eq!(session.current().page_count, session.page_count());
}
