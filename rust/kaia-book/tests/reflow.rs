//! Reflowable books through CREngine: navigation, rendering, selection,
//! relayout, annotations and persistence.

mod common;

use std::time::Duration;

use kaia_book::*;
use kaia_gfx::{Point, Size};

const VIEW: Size = Size { w: 1264, h: 1680 };

#[test]
fn opens_with_cre_and_reports_chapters() {
    let data = tempfile::tempdir().unwrap();
    let s = common::open(&common::repo_file("juliet.epub"), &data);
    assert!(s.uses_cre());
    assert!(s.is_reflowable());
    assert!(
        s.page_count() > 50,
        "Romeo and Juliet has many pages, got {}",
        s.page_count()
    );
    let info = s.current();
    assert_eq!(info.page, 0);
    assert_eq!(info.location, Location::Engine(info.location_xpointer()));
    assert!(s.toc().iter().any(|t| t.title == "Act I"));
}

#[test]
fn page_turns_percent_and_chapter_info() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    assert!(s.next_page().unwrap());
    assert!(s.next_page().unwrap());
    assert_eq!(s.current().page, 2);
    assert!(s.prev_page().unwrap());
    assert_eq!(s.current().page, 1);

    s.go_to_percent(1.0).unwrap();
    let last = s.current();
    assert_eq!(last.page as usize, s.page_count() - 1);
    assert!((last.percent - 1.0).abs() < 1e-9);
    assert!(!s.next_page().unwrap(), "no page after the last");

    let act = s.toc().iter().position(|t| t.title == "Act I").expect("Act I");
    assert!(s.go_to_toc_entry(act).unwrap());
    let info = s.current();
    let page = s.toc()[act].page.expect("page");
    assert_eq!(info.page, page);
    // KOReader uses the first TOC entry on this page.
    assert_eq!(info.chapter.as_deref(), Some("Act I"));
    assert!(info.pages_left_in_chapter.is_some());
}

#[test]
fn render_and_prerender_cache_hit_after_page_turn() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    s.go_to(Location::Page(5)).unwrap();
    let first = s.render_current(VIEW).unwrap();
    assert_eq!((first.width(), first.height()), (VIEW.w, VIEW.h));
    // The engine prerenders the next page once it is idle. Wait for the cache to hold it.
    for _ in 0..400 {
        if s.cache_stats().entries >= 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(s.cache_stats().entries >= 2, "next page was not prerendered");
    let before = s.cache_stats();
    assert!(s.next_page().unwrap());
    let hit = s.cached_current(VIEW);
    assert!(hit.is_some(), "page 6 should come from the prerender cache");
    let after = s.cache_stats();
    assert_eq!(after.hits, before.hits + 1);
}

#[test]
fn word_selection_and_extension() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    s.go_to(Location::Page(5)).unwrap();
    let (p, word) = common::first_word(&mut s, VIEW);
    assert!(
        word.text.chars().all(|c| c.is_alphabetic()),
        "word {:?}",
        word.text
    );
    assert!(matches!(word.pos0, Pos::Xpointer(_)));
    assert!(!word.screen_rects.is_empty());

    let extended = s
        .extend_selection(Point { x: p.x + 300, y: p.y }, VIEW)
        .unwrap()
        .expect("extension");
    assert!(extended.text.len() > word.text.len());
    assert!(extended.text.starts_with(&word.text));
    // Dragging back to the first word shrinks the selection to that word.
    let back = s.extend_selection(p, VIEW).unwrap().expect("shrink");
    assert_eq!(back.text, word.text);
}

#[test]
fn highlight_survives_relayout_on_the_same_text() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    s.go_to(Location::Page(5)).unwrap();
    let (_, word) = common::first_word(&mut s, VIEW);
    let id = s
        .add_highlight(&word, Drawer::Underscore, "red", Some("check"))
        .unwrap();
    let here = s.current().location;

    let mut layout = s.layout().clone();
    layout.font_size += 6.0;
    layout.margins.left += 12.0;
    s.set_layout(layout).unwrap();
    let pages = common::wait_layout(&mut s);
    assert!(pages > 0);
    assert_eq!(s.current().location, here, "the xpointer keeps the place");

    assert_eq!(s.annotation_text(id).unwrap(), word.text);
    let on_page: Vec<_> = s
        .annotations_on_page(VIEW)
        .unwrap()
        .into_iter()
        .filter(|a| a.annotation.id == id)
        .collect();
    assert_eq!(on_page.len(), 1, "the highlight is on the page the reader is on");
    assert!(!on_page[0].rects.is_empty());
    assert_eq!(on_page[0].annotation.note.as_deref(), Some("check"));
}

#[test]
fn highlights_and_bookmarks_persist_across_reopen() {
    let data = tempfile::tempdir().unwrap();
    let book = common::repo_file("juliet.epub");
    let (text, here) = {
        let mut s = common::open(&book, &data);
        s.go_to(Location::Page(5)).unwrap();
        let (_, word) = common::first_word(&mut s, VIEW);
        s.add_highlight(&word, Drawer::Lighten, "yellow", None).unwrap();
        assert!(s.toggle_bookmark().unwrap());
        s.set_status(Some("reading")).unwrap();
        s.set_rating(4).unwrap();
        s.save().unwrap();
        (word.text, s.current().location)
    };
    let state_file = data.path().join("kaia").join("books");
    assert!(state_file
        .join(format!("{}.json", kaia_book::partial_md5(&book).unwrap()))
        .exists());
    assert!(state_file.join("paths.json").exists());
    let saved = kaia_events::Settings::open(
        &state_file.join(format!("{}.json", kaia_book::partial_md5(&book).unwrap())),
    );
    let kaia_events::Value::Table(saved_annotations) = saved.read("annotations") else {
        panic!("annotations array");
    };
    let first = saved_annotations.values().next().expect("first annotation");
    assert!(first.get("drawer").is_nil(), "bookmark sorts before highlight");
    assert!(first.get("text").as_str().unwrap_or("").starts_with("in "));

    let mut s = common::open(&book, &data);
    assert_eq!(s.current().location, here);
    assert_eq!(s.current().page, 5);
    let all = s.annotations().unwrap();
    assert!(all[0].is_bookmark());
    let highlight = all.iter().find(|a| !a.is_bookmark()).expect("highlight");
    assert_eq!(highlight.text.as_deref(), Some(text.as_str()));
    assert_eq!(highlight.drawer.as_deref(), Some("lighten"));
    assert_eq!(highlight.color.as_deref(), Some("yellow"));
    assert_eq!(s.bookmarks().unwrap().len(), 1);
    assert!(s.is_bookmarked().unwrap());
    assert_eq!(s.status(), Some("reading"));
    assert_eq!(s.rating(), Some(4));
    assert!(s.import_report().is_none());
}

#[test]
fn bookmark_toggles_on_the_current_page() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    s.go_to(Location::Page(3)).unwrap();
    assert!(!s.is_bookmarked().unwrap());
    assert!(s.toggle_bookmark().unwrap());
    assert!(s.is_bookmarked().unwrap());
    s.next_page().unwrap();
    assert!(!s.is_bookmarked().unwrap(), "bookmarks are per page");
    s.prev_page().unwrap();
    assert!(!s.toggle_bookmark().unwrap(), "second toggle removes it");
    assert!(s.bookmarks().unwrap().is_empty());
}

#[test]
fn search_finds_hits_in_page_order_and_steps_through_them() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    s.start_search("Romeo").unwrap();
    assert!(!common::wait_search(&mut s));
    let hits = s.search_results().to_vec();
    assert!(hits.len() >= 3, "expected several hits, got {}", hits.len());
    assert!(hits.windows(2).all(|w| w[0].page <= w[1].page));
    let first = &hits[0];
    assert!(first.snippet.to_lowercase().contains("romeo"));
    assert_eq!(first.snippet[first.match_range.clone()].to_lowercase(), "romeo");
    assert!(!first.rects.is_empty());

    let jump = s.next_result().unwrap().expect("first hit");
    assert_eq!(jump.index, 0);
    assert_eq!(s.current().page, jump.page);
    let second = s.next_result().unwrap().expect("second hit");
    assert_eq!(second.index, 1);
    let back = s.prev_result().unwrap().expect("back to first");
    assert_eq!(back.index, 0);
    assert!(s.prev_result().unwrap().is_none());
}

#[test]
fn search_cancel_stops_the_running_search() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    s.start_search("the").unwrap();
    s.cancel_search().unwrap();
    assert!(!s.search_running());
    // A cancelled search reports so. A search that already finished reports completion.
    let finished_cancelled = common::wait_search(&mut s);
    let count = s.search_results().len();
    std::thread::sleep(Duration::from_millis(100));
    s.poll();
    assert_eq!(s.search_results().len(), count, "no hits arrive after the cancel");
    let _ = finished_cancelled;
}

#[test]
fn links_jump_and_back_restores_the_place() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("leaves.epub"), &data);
    let mut found = None;
    for page in 0..s.page_count() as u32 {
        s.go_to(Location::Page(page)).unwrap();
        let links = s.links().unwrap();
        if let Some(link) = links
            .into_iter()
            .find(|l| matches!(l.target, kaia_doc::LinkTarget::Page(p) if p != page))
        {
            found = Some((page, link));
            break;
        }
    }
    let (from, link) = found.expect("leaves.epub has internal page links");
    let kaia_doc::LinkTarget::Page(to) = link.target else {
        unreachable!()
    };
    let before = s.current().location;
    assert!(s.follow_link(&link).unwrap());
    assert_eq!(s.current().page, to);
    assert!(s.back().unwrap());
    assert_eq!(s.current().page, from);
    assert_eq!(s.current().location, before);
    assert!(!s.back().unwrap(), "history is empty");
}

#[test]
fn reading_time_is_recorded_per_visit() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    std::thread::sleep(Duration::from_millis(20));
    s.next_page().unwrap();
    std::thread::sleep(Duration::from_millis(20));
    s.next_page().unwrap();
    let times = s.reading_time();
    assert_eq!(times.len(), 2);
    assert_eq!(times[0].page, 1);
    assert!(times.iter().all(|t| t.seconds > 0.0));
}

#[test]
fn corrupt_epub_is_an_error_and_writes_no_state() {
    let data = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let bad = dir.path().join("broken.epub");
    std::fs::write(&bad, b"this is not a zip container at all").unwrap();
    common::init_engine();
    let err = BookSession::open(&bad, common::options(&data))
        .err()
        .expect("open must fail");
    assert!(matches!(err, BookError::Doc(_)), "got {err:?}");
    assert!(!data.path().join("kaia").exists());
}

#[test]
fn leaves_epub_opens_and_renders() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("leaves.epub"), &data);
    assert!(s.page_count() > 1);
    let img = s.render_current(VIEW).unwrap();
    assert_eq!(img.width(), VIEW.w);
}

trait LocationXpointer {
    fn location_xpointer(&self) -> String;
}

impl LocationXpointer for PageInfo {
    fn location_xpointer(&self) -> String {
        match &self.location {
            Location::Engine(xp) => xp.clone(),
            other => panic!("expected an xpointer, got {other:?}"),
        }
    }
}

#[test]
fn session_is_send_and_relayout_returns_at_once() {
    fn assert_send<T: Send>() {}
    assert_send::<BookSession>();

    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    let mut layout = s.layout().clone();
    layout.font_size += 4.0;
    let started = std::time::Instant::now();
    s.set_layout(layout).unwrap();
    let call = started.elapsed();
    assert!(
        call < Duration::from_millis(100),
        "set_layout took {call:?}; it must not wait for the layout"
    );
    let pages = common::wait_layout(&mut s);
    assert_eq!(pages, s.page_count());
}

#[test]
fn request_render_reports_when_the_page_is_cached() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("juliet.epub"), &data);
    s.request_render(VIEW).unwrap();
    let mut rendered = false;
    for _ in 0..200 {
        if s.poll_timeout(Duration::from_millis(20))
            .iter()
            .any(|e| matches!(e, BookEvent::PageRendered { page: 0 }))
        {
            rendered = true;
            break;
        }
    }
    assert!(rendered);
    assert!(s.cached_current(VIEW).is_some());
}

#[test]
fn state_file_uses_koreader_field_names() {
    let data = tempfile::tempdir().unwrap();
    let book = common::repo_file("juliet.epub");
    let mut s = common::open(&book, &data);
    s.go_to(Location::Page(5)).unwrap();
    let (_, word) = common::first_word(&mut s, VIEW);
    s.add_highlight(&word, Drawer::Lighten, "yellow", Some("a note"))
        .unwrap();
    s.save().unwrap();
    let md5 = kaia_book::partial_md5(&book).unwrap();
    let text = std::fs::read_to_string(data.path().join("kaia/books").join(format!("{md5}.json"))).unwrap();
    for key in [
        "\"last_xpointer\"",
        "\"percent_finished\"",
        "\"annotations\"",
        "\"datetime\"",
        "\"drawer\": \"lighten\"",
        "\"color\": \"yellow\"",
        "\"note\": \"a note\"",
        "\"pos0\"",
        "\"pos1\"",
        "\"summary\"",
        "\"partial_md5\"",
    ] {
        assert!(text.contains(key), "state file lacks {key}");
    }
}

#[test]
fn reader_alignment_overrides_book_css_and_off_restores_it() {
    let data = tempfile::tempdir().unwrap();
    let path = data.path().join("alignment.epub");
    common::epub::write_epub(&path, "<p style=\"text-align: right\">A short line.</p>", &[]);
    let mut s = common::open(&path, &data);
    let book_layout = s.layout().clone();
    let original = s.render_current(VIEW).unwrap();
    let (_, right) = common::first_word(&mut s, VIEW);
    let right = right.screen_rects[0];
    let mut left = book_layout.clone();
    left.align_left = true;
    s.set_layout(left).unwrap();
    common::wait_layout(&mut s);
    let (_, left) = common::first_word(&mut s, VIEW);
    let left = left.screen_rects[0];
    assert!(left.x < right.x - 200, "left {left:?}, book {right:?}");
    s.set_layout(book_layout).unwrap();
    common::wait_layout(&mut s);
    assert_eq!(s.render_current(VIEW).unwrap().data(), original.data());
}
