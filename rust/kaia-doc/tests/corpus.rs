//! Corpus tests: real PDF and EPUB files through the public `kaia_doc` API.
//!
//! Fixtures come from the repository's `test/` directory. The Project
//! Gutenberg EPUBs in `~/kaia-test-library` are optional. Tests that need them
//! return early when the directory is missing.

mod common;

use std::fs;

use common::{ensure_font_dir, fixture, gutenberg_epubs, out_dir, write_png};
use kaia_doc::{open, DocError, Document, Layout, LinkTarget, Location};
use kaia_gfx::{PixelFormat, Rect, Size};

const PAGE_W: f32 = 1264.0;
const PAGE_H: f32 = 1680.0;
const EM: f32 = 24.0;

fn open_fixture(name: &str) -> Box<dyn Document> {
    ensure_font_dir();
    open(&fixture(name)).unwrap_or_else(|e| panic!("open {name}: {e}"))
}

fn reflow(doc: &mut dyn Document) {
    doc.set_layout(&Layout::new(PAGE_W, PAGE_H, EM)).expect("layout");
}

/// First page whose text contains `needle`. Lays out the book first when it reflows.
fn find_page(doc: &dyn Document, needle: &str) -> Option<u32> {
    (0..doc.page_count() as u32).find(|&p| doc.page_text(p).map(|t| t.text.contains(needle)).unwrap_or(false))
}

#[test]
fn pdf_page_count_is_stable_across_calls() {
    let doc = open_fixture("sample.pdf");
    let first = doc.page_count();
    assert_eq!(first, 45, "sample.pdf declares 45 pages");
    assert_eq!(doc.page_count(), first);
    assert!(!doc.is_reflowable());
}

#[test]
fn pdf_page_renders_to_gray_and_rgb() {
    let doc = open_fixture("2col.pdf");
    let size = doc.page_size(0).expect("page size");
    assert!(size.width > 0.0 && size.height > 0.0);

    let scale = 1.5;
    let gray = doc.render(0, scale, PixelFormat::Gray8).expect("gray render");
    let rgb = doc.render(0, scale, PixelFormat::Rgb24).expect("rgb render");

    for px in [&gray, &rgb] {
        assert_eq!(px.width(), (size.width * scale).round() as u32);
        assert_eq!(px.height(), (size.height * scale).round() as u32);
    }
    assert_eq!(gray.format(), PixelFormat::Gray8);
    assert_eq!(rgb.format(), PixelFormat::Rgb24);
    assert_eq!(gray.data().len(), (gray.width() * gray.height()) as usize);
    assert_eq!(rgb.data().len(), (rgb.width() * rgb.height() * 3) as usize);

    // The page has ink: some pixel is clearly darker than white.
    assert!(gray.data().iter().any(|&v| v < 128), "gray render has no ink");

    // Both renders come from the same display list, so the RGB luma must match
    // the gray output. The tolerance covers rounding in the two color paths.
    let rgb_px = rgb.data();
    for i in 0..gray.data().len() {
        let (r, g_, b) = (
            rgb_px[3 * i] as u32,
            rgb_px[3 * i + 1] as u32,
            rgb_px[3 * i + 2] as u32,
        );
        let luma = (r * 299 + g_ * 587 + b * 114 + 500) / 1000;
        let g = gray.data()[i] as u32;
        assert!(luma.abs_diff(g) <= 2, "pixel {i}: rgb luma {luma} vs gray {g}");
    }

    let dir = out_dir();
    write_png(
        &dir.join("2col-p1-gray.png"),
        gray.width(),
        gray.height(),
        true,
        gray.data(),
    );
    write_png(
        &dir.join("2col-p1-rgb.png"),
        rgb.width(),
        rgb.height(),
        false,
        rgb.data(),
    );
}

#[test]
fn pdf_clip_render_has_the_clip_size() {
    let doc = open_fixture("2col.pdf");
    let clip = Rect::new(10, 20, 64, 48);
    let px = doc
        .render_clip(0, 1.0, PixelFormat::Gray8, clip)
        .expect("clip render");
    assert_eq!((px.width(), px.height()), (64, 48));
}

#[test]
fn pdf_page_sample_renders_to_png() {
    let doc = open_fixture("sample.pdf");
    let px = doc.render(0, 1.0, PixelFormat::Gray8).expect("render");
    write_png(
        &out_dir().join("sample-p1-gray.png"),
        px.width(),
        px.height(),
        true,
        px.data(),
    );
}

#[test]
fn epub_relayout_changes_page_count_with_em() {
    let mut doc = open_fixture("juliet.epub");
    assert!(doc.is_reflowable());

    doc.set_layout(&Layout::new(PAGE_W, PAGE_H, 12.0)).unwrap();
    let small = doc.page_count();
    doc.set_layout(&Layout::new(PAGE_W, PAGE_H, 24.0)).unwrap();
    let large = doc.page_count();

    assert!(small > 1, "small type gives several pages, got {small}");
    assert!(
        large > small,
        "larger em must give more pages: em 12 -> {small}, em 24 -> {large}"
    );
}

#[test]
fn epub_toc_is_not_empty_and_points_into_the_book() {
    let mut doc = open_fixture("juliet.epub");
    reflow(doc.as_mut());
    let toc = doc.toc();
    assert!(!toc.is_empty(), "juliet.epub has a table of contents");
    assert!(toc.iter().all(|t| t.level >= 1));
    let count = doc.page_count();
    let resolved: Vec<u32> = toc.iter().filter_map(|t| t.page).collect();
    assert!(!resolved.is_empty(), "some entries resolve to pages");
    assert!(
        resolved.iter().all(|&p| (p as usize) < count),
        "every target page is inside the {count} pages"
    );
    assert!(toc.iter().any(|t| t.title.starts_with("Prologue")));
}

#[test]
fn epub_page_text_contains_expected_words() {
    let mut doc = open_fixture("juliet.epub");
    reflow(doc.as_mut());
    let page = find_page(doc.as_ref(), "Verona").expect("some page has the word Verona");
    let text = doc.page_text(page).unwrap();
    assert!(text.text.contains("households"), "page {page}: {}", text.text);
    assert!(!text.chars.is_empty());
    assert_eq!(
        text.chars.len(),
        text.text.chars().filter(|&c| c != '\n').count(),
        "chars and text agree on the character count"
    );
}

#[test]
fn epub_search_finds_word_with_quad_inside_page() {
    let mut doc = open_fixture("juliet.epub");
    reflow(doc.as_mut());
    let page = find_page(doc.as_ref(), "Verona").expect("page with Verona");
    let size = doc.page_size(page).unwrap();
    let hits = doc.search(page, "Verona").expect("search");
    assert!(!hits.is_empty(), "search finds Verona on page {page}");

    for q in &hits {
        let b = q.bounds();
        assert!(b.width() > 0.0 && b.height() > 0.0, "quad {b:?} has area");
        const SLOP: f32 = 1.0;
        assert!(
            b.x0 >= -SLOP && b.y0 >= -SLOP && b.x1 <= size.width + SLOP && b.y1 <= size.height + SLOP,
            "quad {b:?} lies inside the page {size:?}"
        );
    }
    assert!(
        doc.search(page, "").unwrap().is_empty(),
        "empty needle has no hits"
    );
}

#[test]
fn epub_metadata_has_title_and_author() {
    let doc = open_fixture("juliet.epub");
    let meta = doc.metadata();
    assert_eq!(meta.title.as_deref(), Some("Romeo and Juliet"));
    assert_eq!(meta.author.as_deref(), Some("William Shakespeare"));
}

#[test]
fn epub_cover_fits_the_requested_box() {
    let doc = open_fixture("juliet.epub");
    let cover = doc.cover(Size::new(200, 300)).expect("cover is rendered");
    assert!(cover.width() <= 200 && cover.height() <= 300);
    assert!(cover.width() > 0 && cover.height() > 0);
    assert_eq!(cover.format(), PixelFormat::Rgb24);
}

#[test]
fn epub_links_load_and_external_targets_are_uris() {
    let doc = open_fixture("juliet.epub");
    let mut all = Vec::new();
    for page in 0..doc.page_count() as u32 {
        all.extend(doc.links(page).expect("links load on every page"));
    }
    // The title pages link to the publisher's site.
    assert!(
        all.iter()
            .any(|l| matches!(&l.target, LinkTarget::Uri(u) if u.starts_with("http://"))),
        "an external link is reported as a URI"
    );
}

#[test]
fn location_resolves_pages_and_rejects_unknown_targets() {
    let mut doc = open_fixture("juliet.epub");
    reflow(doc.as_mut());
    let count = doc.page_count();

    assert_eq!(doc.resolve(&Location::Page(0)).unwrap(), 0);
    assert!(matches!(
        doc.resolve(&Location::Page(count as u32)),
        Err(DocError::PageOutOfRange { .. })
    ));
    assert!(matches!(
        doc.resolve(&Location::Engine("#no-such-anchor-kaia".to_string())),
        Err(DocError::UnresolvedLocation(_))
    ));
}

#[test]
fn fixed_layout_rejects_reflow() {
    let mut doc = open_fixture("2col.pdf");
    let err = doc
        .set_layout(&Layout::new(PAGE_W, PAGE_H, EM))
        .expect_err("PDF is fixed layout");
    assert!(matches!(err, DocError::NotReflowable));
}

#[test]
fn page_out_of_range_is_an_error() {
    let doc = open_fixture("2col.pdf");
    let count = doc.page_count();
    assert!(matches!(
        doc.page_text(count as u32),
        Err(DocError::PageOutOfRange { .. })
    ));
    assert!(matches!(
        doc.render(count as u32, 1.0, PixelFormat::Gray8),
        Err(DocError::PageOutOfRange { .. })
    ));
}

#[test]
fn page_text_boxes_lie_inside_the_page() {
    // Page 1 of sample.pdf has text. Page 0 is a blank cover, and 2col.pdf has
    // no text layer at all.
    let doc = open_fixture("sample.pdf");
    let size = doc.page_size(1).unwrap();
    let text = doc.page_text(1).unwrap();
    assert!(!text.text.trim().is_empty(), "sample.pdf page 2 has text");
    assert!(text.chars.len() > 100);
    for c in &text.chars {
        assert!(
            c.bbox.x0 >= -1.0
                && c.bbox.y0 >= -1.0
                && c.bbox.x1 <= size.width + 1.0
                && c.bbox.y1 <= size.height + 1.0,
            "char {:?} at {:?} is outside the page",
            c.ch,
            c.bbox
        );
    }
    assert!(text
        .lines
        .iter()
        .all(|l| l.first_char + l.char_count <= text.chars.len()));
}

#[test]
fn text_free_pdf_has_no_text_layer() {
    let doc = open_fixture("2col.pdf");
    assert!(doc.page_text(0).unwrap().text.is_empty());
}

#[test]
fn missing_file_is_an_io_error() {
    let err = match open(&fixture("does-not-exist.pdf")) {
        Ok(_) => panic!("missing file must not open"),
        Err(e) => e,
    };
    assert!(matches!(err, DocError::Io { .. }), "got {err:?}");
}

#[test]
fn corrupt_files_are_errors_not_crashes() {
    ensure_font_dir();
    let dir = out_dir();

    let bad_pdf = dir.join("corrupt-header.pdf");
    fs::write(&bad_pdf, b"%PDF-1.4\n1 0 obj << garbage\nxref\n").unwrap();
    assert!(open(&bad_pdf).is_err(), "truncated PDF must not open");

    let full = fs::read(fixture("juliet.epub")).unwrap();
    let truncated = dir.join("truncated.epub");
    fs::write(&truncated, &full[..full.len() / 8]).unwrap();
    assert!(open(&truncated).is_err(), "truncated EPUB must not open");

    let unknown = dir.join("notes.xyz");
    fs::write(&unknown, b"plain bytes").unwrap();
    assert!(matches!(open(&unknown), Err(DocError::UnknownFormat(_))));
}

#[test]
fn gutenberg_epubs_open_lay_out_and_render() {
    let files = gutenberg_epubs();
    if files.is_empty() {
        eprintln!("skipped: no EPUBs in ~/kaia-test-library");
        return;
    }
    ensure_font_dir();
    for path in files {
        let mut doc = open(&path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
        assert!(doc.page_count() > 0, "{} has pages", path.display());
        doc.set_layout(&Layout::new(PAGE_W, PAGE_H, EM))
            .unwrap_or_else(|e| panic!("layout {}: {e}", path.display()));
        let pages = doc.page_count();
        assert!(pages > 0, "{} has pages after layout", path.display());
        let px = doc
            .render(0, 0.5, PixelFormat::Rgb24)
            .unwrap_or_else(|e| panic!("render {}: {e}", path.display()));
        assert!(px.width() > 0 && px.height() > 0);
    }
}

#[test]
fn gutenberg_epub_png_from_first_book() {
    let files = gutenberg_epubs();
    let Some(path) = files.first() else {
        eprintln!("skipped: no EPUBs in ~/kaia-test-library");
        return;
    };
    ensure_font_dir();
    let mut doc = open(path).unwrap();
    doc.set_layout(&Layout::new(PAGE_W, PAGE_H, EM)).unwrap();
    let px = doc.render(0, 0.6, PixelFormat::Gray8).unwrap();
    write_png(
        &out_dir().join("gutenberg-first-p1-gray.png"),
        px.width(),
        px.height(),
        true,
        px.data(),
    );
}

const FB2_SAMPLE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
<description><title-info><book-title>Kaia Sample</book-title><author><first-name>Ann</first-name><last-name>Writer</last-name></author><lang>en</lang></title-info></description>
<body><section><title><p>Chapter One</p></title><p>The quick brown fox jumps over the lazy dog.</p></section></body>
</FictionBook>
"#;

const HTML_SAMPLE: &str = "<!DOCTYPE html>\n<html><head><title>Kaia</title></head>\n<body><p>Hello from an HTML page.</p></body></html>\n";

#[test]
fn fb2_opens_by_content_even_without_a_known_extension() {
    ensure_font_dir();
    let path = out_dir().join("sample-fb2.bin");
    fs::write(&path, FB2_SAMPLE).unwrap();
    let doc = open(&path).expect("FB2 is sniffed from its content");
    assert!(doc.is_reflowable());
    let text = doc.page_text(0).unwrap().text;
    assert!(text.contains("quick brown fox"), "text: {text}");
}

#[test]
fn html_opens_by_extension_and_reports_no_toc() {
    ensure_font_dir();
    let path = out_dir().join("sample.html");
    fs::write(&path, HTML_SAMPLE).unwrap();
    let mut doc = open(&path).expect("HTML opens");
    doc.set_layout(&Layout::new(PAGE_W, PAGE_H, EM)).unwrap();
    let page = find_page(doc.as_ref(), "Hello").expect("the HTML text is on a page");
    assert!(page < doc.page_count() as u32);
    assert_eq!(doc.metadata().title.as_deref(), Some("Kaia"));
}
