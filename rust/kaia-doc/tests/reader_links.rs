#[path = "support/links_fixture.rs"]
mod fixture;
use kaia_doc::{Engine, Layout, LinkTarget, Location};

#[test]
fn cre_rejects_chapters_backlinks_and_long_targets_as_notes() {
    let dir = std::env::temp_dir().join(format!("kaia-doc-links-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("negative.epub");
    let content = format!(
        r##"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>Notes</title></head><body>
<h1>First chapter</h1><p id="reference">Text <a epub:type="noteref" href="#semantic">1</a> here.</p>
<p>Text <a href="#long">2</a> here.</p><p>Text <a href="#chapter">3</a> here.</p>
<p><a href="#missing">Missing target</a></p>
<h1 id="chapter">Second chapter</h1>
<aside id="semantic" epub:type="footnote"><p>The note.</p><a href="#reference">Back</a></aside>
<p id="long">{}</p><h1>End</h1></body></html>"##,
        "A long paragraph. ".repeat(700)
    );
    fixture::epub_content(&path, &content);
    let doc = kaia_doc::open_with(&path, Engine::Cre).unwrap();
    let mut links = Vec::new();
    for page in 0..doc.page_count() as u32 {
        links.extend(doc.links(page).unwrap());
    }
    let notes: Vec<_> = links.iter().filter_map(|l| l.note.as_deref()).collect();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("The note."));
    assert!(links.iter().any(
        |l| matches!(&l.target, LinkTarget::Uri(s) if s.contains("missing"))
            && l.note.is_none()
            && l.location.is_none()
    ));
    assert!(
        links
            .iter()
            .filter(|l| matches!(l.target, LinkTarget::Page(_)) && l.note.is_none())
            .count()
            >= 3
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn wrapped_links_keep_separate_line_rectangles_and_stable_targets() {
    let dir = std::env::temp_dir().join(format!("kaia-doc-wrap-links-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("wrapped.epub");
    fixture::epub_content(
        &path,
        r##"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>Links</title></head><body><p>Some text <a href="#chapter">A long chapter link which wraps onto several lines of text</a> after.</p><h1 id="chapter">Chapter</h1><p>Ordinary text.</p></body></html>"##,
    );
    let mut doc = kaia_doc::open_with(&path, Engine::Cre).unwrap();
    doc.set_layout(&Layout::new(240.0, 500.0, 24.0)).unwrap();
    let links = doc.links(0).unwrap();
    assert!(links.len() >= 2, "{links:?}");
    assert!(links.windows(2).any(|p| p[0].bbox.y0 != p[1].bbox.y0));
    let first = &links[0];
    assert!(matches!(&first.location, Some(Location::Engine(_))));
    assert!(links
        .iter()
        .all(|l| l.note.is_none() && l.location == first.location));
    std::fs::remove_dir_all(dir).unwrap();
}
