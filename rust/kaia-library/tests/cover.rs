//! Covers from the book's own image (EPUB and CBZ), the page render they fall
//! back to, and the larger cover size.

mod common;

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::*;
use image::ImageEncoder;
use kaia_gfx::image::{decode_rgb, DecodeLimits};
use kaia_gfx::{Color, PixelFormat, Pixmap};
use kaia_library::{CoverSize, Library, DEFAULT_EXTRACT_BUDGET};
use rusqlite::params;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const RED: [u8; 3] = [200, 30, 30];
const BLUE: [u8; 3] = [30, 60, 200];
const GREEN: [u8; 3] = [30, 180, 60];

/// The EPUB 3 marking of a cover, for the package manifest.
const EPUB3_COVER: &str =
    r#"<item id="cover-img" href="images/cover.png" media-type="image/png" properties="cover-image"/>"#;

const CONTAINER_XML: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;

const CHAPTER_XHTML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>One</title></head>
<body><h1>Chapter one</h1><p>It was a dark and stormy night.</p></body></html>"#;

/// A temp dir with a `books` root. The database lives in the same temp dir.
fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    fs::create_dir_all(&root).unwrap();
    (tmp, root)
}

/// A PNG with the left half in `left` and the right half in `right`.
fn two_tone_png(w: u32, h: u32, left: [u8; 3], right: [u8; 3]) -> Vec<u8> {
    let mut pm = Pixmap::new(w, h, PixelFormat::Rgb24);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let [r, g, b] = if (x as u32) < w / 2 { left } else { right };
            pm.put(x, y, Color::rgb(r, g, b));
        }
    }
    kaia_gfx::png::encode_pixmap(&pm).unwrap()
}

/// A JPEG of one flat color, for an older cover that a test puts in place.
fn jpeg_bytes(w: u32, h: u32) -> Vec<u8> {
    let rgb = vec![120u8; (w * h * 3) as usize];
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// Writes a ZIP archive. Each entry is a name and its bytes.
fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let mut writer = ZipWriter::new(File::create(path).unwrap());
    for (name, bytes) in entries {
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap();
}

/// Writes a one-chapter EPUB. `metadata` and `manifest` add XML to the package
/// document. `files` add archive entries, named from the archive root.
fn write_epub(path: &Path, metadata: &str, manifest: &str, files: &[(&str, &[u8])]) {
    let opf = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="id">urn:test:cover</dc:identifier>
    <dc:title>Cover test</dc:title>
    <dc:language>en</dc:language>
    {metadata}
  </metadata>
  <manifest>
    <item id="chapter" href="chapter.xhtml" media-type="application/xhtml+xml"/>
    {manifest}
  </manifest>
  <spine><itemref idref="chapter"/></spine>
</package>"#
    );
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("mimetype", &b"application/epub+zip"[..]),
        ("META-INF/container.xml", CONTAINER_XML.as_bytes()),
        ("OEBPS/content.opf", opf.as_bytes()),
        ("OEBPS/chapter.xhtml", CHAPTER_XHTML.as_bytes()),
    ];
    entries.extend_from_slice(files);
    write_zip(path, &entries);
}

/// The stored cover of a size, decoded. Asserts the MIME type on the way.
fn decoded(lib: &Library, id: i64, size: CoverSize) -> Pixmap {
    let cover = lib.cover(id, size).unwrap().expect("book has a cover");
    assert_eq!(cover.mime, "image/jpeg");
    decode_rgb(&cover.bytes, DecodeLimits::default()).unwrap()
}

/// Checks each channel of a pixel. JPEG moves each value a little, so a
/// difference of up to 12 counts as the same color.
fn assert_color(px: Option<Color>, want: [u8; 3]) {
    let px = px.expect("pixel is inside the cover");
    let got = [px.r, px.g, px.b];
    for (g, w) in got.iter().zip(want) {
        assert!(g.abs_diff(w) <= 12, "pixel {got:?} is not {want:?}");
    }
}

/// The cover is the page render. Its top left pixel is the white page margin,
/// and it fits the large box.
fn assert_page_render(lib: &Library, id: i64) {
    let large = decoded(lib, id, CoverSize::Large);
    assert!(
        large.width() <= 1264 && large.height() <= 1680,
        "{}x{} fits the large box",
        large.width(),
        large.height()
    );
    assert!(
        large.height() > 480,
        "page render is {}x{}",
        large.width(),
        large.height()
    );
    let corner = large.get(2, 2).unwrap();
    assert!(
        corner.r >= 240 && corner.g >= 240 && corner.b >= 240,
        "top left pixel {corner:?} is white page margin"
    );
}

#[test]
fn epub_three_cover_image_is_stored_at_its_own_size() {
    let (tmp, root) = setup();
    let file = root.join("declared.epub");
    let cover = two_tone_png(600, 900, RED, BLUE);
    write_epub(&file, "", EPUB3_COVER, &[("OEBPS/images/cover.png", &cover)]);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let id = lib.book_by_path(&file).unwrap().unwrap().id;
    let large = decoded(&lib, id, CoverSize::Large);
    assert_eq!(
        (large.width(), large.height()),
        (600, 900),
        "not enlarged to the box"
    );
    assert_color(large.get(150, 450), RED);
    assert_color(large.get(450, 450), BLUE);

    let small = decoded(&lib, id, CoverSize::Small);
    assert_eq!((small.width(), small.height()), (107, 160));
}

#[test]
fn epub_two_meta_cover_with_an_escaped_href_is_found() {
    let (tmp, root) = setup();
    let file = root.join("meta.epub");
    let cover = two_tone_png(500, 750, GREEN, GREEN);
    write_epub(
        &file,
        r#"<meta name="cover" content="front"/>"#,
        r#"<item id="front" href="images/front%20cover.png" media-type="image/png"/>"#,
        &[("OEBPS/images/front cover.png", &cover)],
    );
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let id = lib.book_by_path(&file).unwrap().unwrap().id;
    let large = decoded(&lib, id, CoverSize::Large);
    assert_eq!((large.width(), large.height()), (500, 750));
    assert_color(large.get(250, 375), GREEN);
}

#[test]
fn epub_three_mark_wins_over_the_epub_two_meta() {
    let (tmp, root) = setup();
    let file = root.join("both.epub");
    let red = two_tone_png(300, 450, RED, RED);
    let blue = two_tone_png(300, 450, BLUE, BLUE);
    write_epub(
        &file,
        r#"<meta name="cover" content="old"/>"#,
        r#"<item id="old" href="old.png" media-type="image/png"/>
           <item id="new" href="new.png" media-type="image/png" properties="cover-image"/>"#,
        &[("OEBPS/old.png", &blue), ("OEBPS/new.png", &red)],
    );
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let id = lib.book_by_path(&file).unwrap().unwrap().id;
    assert_color(decoded(&lib, id, CoverSize::Large).get(150, 225), RED);
}

#[test]
fn epub_without_a_declared_cover_is_rendered_from_its_first_page() {
    let (tmp, root) = setup();
    let file = root.join("plain.epub");
    write_epub(&file, "", "", &[]);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let book = lib.book_by_path(&file).unwrap().unwrap();
    assert!(book.extracted);
    assert_page_render(&lib, book.id);
}

#[test]
fn corrupt_cover_image_falls_back_to_the_page_render() {
    let (tmp, root) = setup();
    let file = root.join("corrupt.epub");
    write_epub(
        &file,
        "",
        EPUB3_COVER,
        &[("OEBPS/images/cover.png", &b"this is not a PNG"[..])],
    );
    let mut lib = open_library(tmp.path());
    let report = scan(&mut lib, std::slice::from_ref(&root));

    assert_eq!(report.failed, 0, "the book itself is fine");
    let book = lib.book_by_path(&file).unwrap().unwrap();
    assert!(book.extracted && book.failure.is_none());
    assert_page_render(&lib, book.id);
}

#[test]
fn oversized_cover_image_falls_back_to_the_page_render() {
    let (tmp, root) = setup();
    let file = root.join("wide.epub");
    // Wider than the decoder limit. Used as the cover, it would be a thin strip.
    let wide = two_tone_png(4097, 40, BLUE, BLUE);
    write_epub(&file, "", EPUB3_COVER, &[("OEBPS/images/cover.png", &wide)]);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let book = lib.book_by_path(&file).unwrap().unwrap();
    assert!(book.extracted);
    assert_page_render(&lib, book.id);
}

#[test]
fn cbz_cover_is_the_first_page_in_name_order() {
    let (tmp, root) = setup();
    let file = root.join("comic.cbz");
    let red = two_tone_png(600, 900, RED, RED);
    let blue = two_tone_png(600, 900, BLUE, BLUE);
    write_zip(
        &file,
        &[
            ("002.png", &blue),
            ("001.png", &red),
            // ".000.png" sorts first, but a hidden name is not a page.
            (".000.png", &b"hidden"[..]),
            ("__MACOSX/001.png", &b"resource fork"[..]),
        ],
    );
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let book = lib.book_by_path(&file).unwrap().unwrap();
    assert!(book.extracted);
    let large = decoded(&lib, book.id, CoverSize::Large);
    assert_eq!((large.width(), large.height()), (600, 900));
    assert_color(large.get(300, 450), RED);
}

#[test]
fn corrupt_first_cbz_page_gives_no_cover_and_no_panic() {
    let (tmp, root) = setup();
    let file = root.join("broken.cbz");
    let blue = two_tone_png(600, 900, BLUE, BLUE);
    write_zip(&file, &[("001.png", &b"not a PNG"[..]), ("002.png", &blue)]);
    let mut lib = open_library(tmp.path());
    let report = scan(&mut lib, std::slice::from_ref(&root));

    assert_eq!(report.failed, 0);
    let book = lib.book_by_path(&file).unwrap().unwrap();
    assert!(book.extracted);
    // The page render of the first page fails, so the book has no cover. The
    // second page is not used in its place.
    assert!(lib.cover(book.id, CoverSize::Large).unwrap().is_none());
}

#[test]
fn repository_epubs_store_their_declared_covers_at_full_size() {
    let (tmp, root) = setup();
    let leaves = copy_fixture("leaves.epub", &root, "leaves.epub");
    let juliet = copy_fixture("juliet.epub", &root, "juliet.epub");
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    // leaves.epub declares a 442 x 616 JPEG and juliet.epub a 520 x 800 PNG.
    for (file, (w, h)) in [(&leaves, (442, 616)), (&juliet, (520, 800))] {
        let id = lib.book_by_path(file).unwrap().unwrap().id;
        let large = decoded(&lib, id, CoverSize::Large);
        assert_eq!((large.width(), large.height()), (w, h), "{}", file.display());
    }
}

#[test]
fn version_four_books_get_the_larger_cover_on_their_next_scan() {
    let (tmp, root) = setup();
    let file = root.join("declared.epub");
    let cover = two_tone_png(600, 900, RED, RED);
    write_epub(&file, "", EPUB3_COVER, &[("OEBPS/images/cover.png", &cover)]);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&file).unwrap().unwrap().id;
    drop(lib);

    // Puts the book in the state a version 4 library leaves behind. The cover
    // is an older 320 x 480 image, and the book is marked as extracted.
    let raw = raw_connection(tmp.path());
    raw.execute(
        "UPDATE covers SET large = ?1 WHERE book_id = ?2",
        params![jpeg_bytes(320, 480), id],
    )
    .unwrap();
    raw.execute("UPDATE books SET extracted = 1, attempts = 0 WHERE id = ?1", [id])
        .unwrap();
    raw.execute_batch("DROP TABLE recent_searches; DROP TABLE book_tags; DROP TABLE tags;")
        .unwrap();
    raw.pragma_update(None, "user_version", 4).unwrap();
    drop(raw);

    let mut lib = open_library(tmp.path());
    assert!(
        !lib.book(id).unwrap().unwrap().extracted,
        "version 5 queues the book"
    );
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(report.extracted, 1, "the book is extracted again");
    let large = decoded(&lib, id, CoverSize::Large);
    assert_eq!((large.width(), large.height()), (600, 900));
}

/// Indexes `file` with this build, then rewrites the database the way a version
/// 4 library holds it. The title and cover are older, and the book is marked
/// extracted. Returns the book id and the old cover bytes.
fn version_four_book(tmp: &Path, root: &Path, file: &Path) -> (i64, Vec<u8>) {
    let mut lib = open_library(tmp);
    scan(&mut lib, &[root.to_path_buf()]);
    let id = lib.book_by_path(file).unwrap().unwrap().id;
    drop(lib);

    let old_cover = jpeg_bytes(320, 480);
    let raw = raw_connection(tmp);
    raw.execute(
        "UPDATE covers SET large = ?1 WHERE book_id = ?2",
        params![old_cover, id],
    )
    .unwrap();
    raw.execute(
        "UPDATE books SET title = 'Old title', extracted = 1, attempts = 0 WHERE id = ?1",
        [id],
    )
    .unwrap();
    raw.execute_batch("DROP TABLE recent_searches; DROP TABLE book_tags; DROP TABLE tags;")
        .unwrap();
    raw.pragma_update(None, "user_version", 4).unwrap();
    (id, old_cover)
}

#[test]
fn failed_refresh_after_migration_keeps_the_title_and_cover() {
    let (tmp, root) = setup();
    let file = root.join("declared.epub");
    let cover = two_tone_png(600, 900, RED, RED);
    write_epub(&file, "", EPUB3_COVER, &[("OEBPS/images/cover.png", &cover)]);
    let (id, old_cover) = version_four_book(tmp.path(), &root, &file);

    // Version 5 queues the book. The new extraction runs out of time, and the
    // stored title and cover must survive it.
    let mut lib = open_library(tmp.path());
    assert!(
        !lib.book(id).unwrap().unwrap().extracted,
        "version 5 queues the book"
    );
    lib.set_extract_budget(Duration::ZERO);
    let failing = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!((failing.extracted, failing.failed), (1, 1));
    let book = lib.book(id).unwrap().unwrap();
    assert_eq!(book.title.as_deref(), Some("Old title"));
    assert!(!book.extracted);
    assert!(book.failure.is_none(), "a later scan tries again");
    assert_eq!(lib.cover(id, CoverSize::Large).unwrap().unwrap().bytes, old_cover);

    // The next scan has the normal budget. It replaces the title and the cover.
    lib.set_extract_budget(DEFAULT_EXTRACT_BUDGET);
    let retry = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(retry.extracted, 1);
    let book = lib.book(id).unwrap().unwrap();
    assert!(book.extracted);
    assert_eq!(book.title.as_deref(), Some("Cover test"));
    let large = decoded(&lib, id, CoverSize::Large);
    assert_eq!((large.width(), large.height()), (600, 900));
}

#[test]
fn refresh_that_keeps_failing_records_the_failure_after_three_tries() {
    let (tmp, root) = setup();
    let file = root.join("declared.epub");
    let cover = two_tone_png(600, 900, RED, RED);
    write_epub(&file, "", EPUB3_COVER, &[("OEBPS/images/cover.png", &cover)]);
    let (id, old_cover) = version_four_book(tmp.path(), &root, &file);

    let mut lib = open_library(tmp.path());
    lib.set_extract_budget(Duration::ZERO);
    for attempt in 1..=3 {
        let report = scan(&mut lib, std::slice::from_ref(&root));
        assert_eq!(report.extracted, 1, "try {attempt} calls the engine");
    }
    let book = lib.book(id).unwrap().unwrap();
    assert!(
        book.failure.as_deref().is_some_and(|m| m.contains("longer than")),
        "{:?}",
        book.failure
    );
    assert_eq!(book.title.as_deref(), Some("Old title"));
    assert_eq!(lib.cover(id, CoverSize::Large).unwrap().unwrap().bytes, old_cover);

    // A recorded failure is not tried again, so the stored data stays as it is.
    lib.set_extract_budget(DEFAULT_EXTRACT_BUDGET);
    let after = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!(after.extracted, 0);
    assert_eq!(lib.cover(id, CoverSize::Large).unwrap().unwrap().bytes, old_cover);
}

#[test]
fn refresh_keeps_the_stored_fields_the_new_extraction_does_not_produce() {
    let (tmp, root) = setup();
    // The first page is not an image, so the new extraction finds no cover.
    let file = root.join("comic.cbz");
    let blue = two_tone_png(600, 900, BLUE, BLUE);
    write_zip(&file, &[("001.png", &b"not a PNG"[..]), ("002.png", &blue)]);
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));
    let id = lib.book_by_path(&file).unwrap().unwrap().id;
    drop(lib);

    // The stored row of a version 4 library. The stored page count is wrong on
    // purpose, so the test shows that the new extraction replaces it.
    let old_cover = jpeg_bytes(320, 480);
    let old_small = jpeg_bytes(60, 90);
    let raw = raw_connection(tmp.path());
    raw.execute(
        "UPDATE covers SET source_w = 320, source_h = 480, small = ?1, large = ?2 WHERE book_id = ?3",
        params![old_small, old_cover, id],
    )
    .unwrap();
    raw.execute(
        "UPDATE books SET title = 'Old title', authors = 'Old author', series = 'Old series',
         series_index = 2, pages = 99, extracted = 1, attempts = 0 WHERE id = ?1",
        [id],
    )
    .unwrap();
    raw.execute_batch("DROP TABLE recent_searches; DROP TABLE book_tags; DROP TABLE tags;")
        .unwrap();
    raw.pragma_update(None, "user_version", 4).unwrap();
    drop(raw);

    let mut lib = open_library(tmp.path());
    let report = scan(&mut lib, std::slice::from_ref(&root));
    assert_eq!((report.extracted, report.failed), (1, 0));
    let book = lib.book(id).unwrap().unwrap();
    assert!(book.extracted && book.failure.is_none());
    assert_eq!(book.title.as_deref(), Some("Old title"));
    assert_eq!(book.authors.as_deref(), Some("Old author"));
    assert_eq!(book.series.as_deref(), Some("Old series"));
    assert_eq!(book.series_index, Some(2.0));
    assert_eq!(book.pages, Some(2), "the new extraction counts the pages");
    assert_eq!(lib.cover(id, CoverSize::Large).unwrap().unwrap().bytes, old_cover);
    assert_eq!(lib.cover(id, CoverSize::Small).unwrap().unwrap().bytes, old_small);
}

#[test]
fn exif_orientation_turns_a_jpeg_cover_upright() {
    let (tmp, root) = setup();
    let file = root.join("turned.epub");
    let manifest =
        r#"<item id="cover-jpg" href="images/cover.jpg" media-type="image/jpeg" properties="cover-image"/>"#;
    // Stored 200 x 400 with a red bottom left quarter. Orientation 6 turns the
    // image 90 degrees clockwise, so the red corner ends up at the top left.
    write_epub(
        &file,
        "",
        manifest,
        &[("OEBPS/images/cover.jpg", &corner_jpeg(200, 400, 6))],
    );
    let mut lib = open_library(tmp.path());
    scan(&mut lib, std::slice::from_ref(&root));

    let id = lib.book_by_path(&file).unwrap().unwrap().id;
    let large = decoded(&lib, id, CoverSize::Large);
    assert_eq!((large.width(), large.height()), (400, 200));
    assert_color(large.get(50, 50), RED);
    assert_color(large.get(350, 150), BLUE);
}

/// A JPEG of `w` x `h` with a red bottom left quarter and blue elsewhere. Its
/// EXIF orientation is `orientation`.
fn corner_jpeg(w: u32, h: u32, orientation: u16) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let [r, g, b] = if x < w / 2 && y >= h / 2 { RED } else { BLUE };
            rgb.extend_from_slice(&[r, g, b]);
        }
    }
    let mut out = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95);
    encoder.set_exif_metadata(tiff_orientation(orientation)).unwrap();
    encoder
        .write_image(&rgb, w, h, image::ExtendedColorType::Rgb8)
        .unwrap();
    out
}

/// An EXIF payload with one Orientation tag, in little endian byte order.
fn tiff_orientation(value: u16) -> Vec<u8> {
    let mut tiff = b"II*\0".to_vec();
    tiff.extend_from_slice(&8u32.to_le_bytes()); // offset of the first IFD
    tiff.extend_from_slice(&1u16.to_le_bytes()); // one entry
    tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    tiff.extend_from_slice(&1u32.to_le_bytes()); // one value
    tiff.extend_from_slice(&value.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]); // padding to four bytes
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    tiff
}
