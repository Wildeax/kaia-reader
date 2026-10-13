#![cfg(feature = "cre")]

mod common;

use kaia_book::{BookSession, Drawer, Location, Pos, Selection};

fn range(page: u32, start: &str, end: &str) -> Selection {
    Selection {
        page,
        text: "Act I".into(),
        pos0: Pos::Xpointer(start.into()),
        pos1: Pos::Xpointer(end.into()),
        page_rects: Vec::new(),
        screen_rects: Vec::new(),
    }
}

#[test]
fn epub_annotations_use_engine_order_end_ties_and_chapters_after_reflow() {
    let data = tempfile::tempdir().unwrap();
    let book = common::repo_file("juliet.epub");
    let mut session = common::open(&book, &data);
    let toc = session.toc();
    let act = toc.iter().position(|t| t.title == "Act I").unwrap();
    let page = toc[act].page.unwrap();
    assert_eq!(toc[act + 1].title, "Prologue");
    assert_eq!(toc[act + 1].page, Some(page));
    let a = toc[act].xpointer.clone().unwrap();
    let b = toc[act + 1].xpointer.clone().unwrap();
    let c = toc[act + 2].xpointer.clone().unwrap();
    let alias = a.replacen("/body/", "/body[1]/", 1);
    // Both starts name the same DOM node. The shorter range comes first even
    // though its spelling sorts after the longer range's start as text.
    assert!(a < alias);
    let long = session
        .add_highlight(&range(page, &a, &c), Drawer::Lighten, "yellow", None)
        .unwrap();
    let short = session
        .add_highlight(&range(page, &alias, &b), Drawer::Lighten, "yellow", None)
        .unwrap();
    let prologue = session
        .add_highlight(&range(page, &b, &c), Drawer::Lighten, "yellow", None)
        .unwrap();
    session.go_to(Location::Engine(b.clone())).unwrap();
    assert!(session.toggle_bookmark().unwrap());

    let verify = |session: &mut BookSession| {
        let all = session.annotations().unwrap();
        assert_eq!(all.len(), 4);
        assert!(all[0].is_bookmark(), "bookmark precedes highlights on its page");
        assert_eq!(all[0].chapter.as_deref(), Some("Prologue"));
        assert_eq!(all[0].text.as_deref(), Some("in Prologue"));
        assert_eq!(
            all[1..].iter().map(|a| a.id).collect::<Vec<_>>(),
            vec![short, long, prologue]
        );
        assert_eq!(all[1].chapter.as_deref(), Some("Act I"));
        assert_eq!(all[2].chapter.as_deref(), Some("Act I"));
        assert_eq!(all[3].chapter.as_deref(), Some("Prologue"));
    };
    verify(&mut session);
    let original_page = session.toc()[act].page;
    let mut layout = session.layout().clone();
    layout.font_size = 48.0;
    session.set_layout(layout).unwrap();
    common::wait_layout(&mut session);
    assert_ne!(session.toc()[act].page, original_page);
    assert_eq!(session.toc()[act].xpointer.as_deref(), Some(a.as_str()));
    verify(&mut session);
    session.save().unwrap();
    drop(session);
    let mut reopened = common::open(&book, &data);
    verify(&mut reopened);
}
