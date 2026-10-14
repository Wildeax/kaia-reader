#[path = "../../kaia-archive-sys/tests/common/mod.rs"]
mod common;
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use kaia_doc::{open, open_with, CbrDocument, DocError, Engine, Format, Layout, Location};
use kaia_gfx::{PixelFormat, Rect, Size};
use std::io::Cursor;

fn image(width: u32, height: u32, rgb: [u8; 3], format: ImageFormat) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb(rgb)))
        .write_to(&mut bytes, format)
        .unwrap();
    bytes.into_inner()
}

fn fixture(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("comic.CBR");
    std::fs::write(&path, bytes).unwrap();
    (dir, path)
}

#[test]
fn cbr_opens_pages_in_natural_order_renders_and_reopens() {
    let first = image(8, 12, [255, 0, 0], ImageFormat::Png);
    let second = image(10, 14, [0, 0, 0], ImageFormat::Jpeg);
    let last = image(12, 16, [0, 0, 255], ImageFormat::Png);
    let (_dir, path) = fixture(&common::rar(&[
        ("page10.png", &last, last.len() as u32),
        ("PAGE2.JPEG", &second, second.len() as u32),
        ("page1.png", &first, first.len() as u32),
        ("readme.txt", b"ignored", 7),
    ]));
    let mut document = open(&path).unwrap();
    assert_eq!(document.page_count(), 3);
    for (page, width) in [(0, 8.0), (1, 10.0), (2, 12.0), (0, 8.0)] {
        assert_eq!(document.page_size(page).unwrap().width, width);
        assert_eq!(document.resolve(&Location::Page(page)).unwrap(), page);
        assert_eq!(
            document.render(page, 1.0, PixelFormat::Rgb24).unwrap().width(),
            width as u32
        );
    }
    let gray = document.render(0, 0.5, PixelFormat::Gray8).unwrap();
    assert_eq!(gray.size(), Size::new(4, 6));
    assert!(gray.data().iter().all(|v| *v == 76));
    let clip = document
        .render_clip(0, 1.0, PixelFormat::Rgb24, Rect::new(1, 2, 3, 4))
        .unwrap();
    assert_eq!(clip.size(), Size::new(3, 4));
    assert!(clip
        .data()
        .as_chunks::<3>()
        .0
        .iter()
        .all(|rgb| *rgb == [255, 0, 0]));
    assert_eq!(document.cover(Size::new(4, 6)).unwrap().size(), Size::new(4, 6));
    assert!(document.page_text(0).unwrap().text.is_empty());
    assert!(document.search(0, "anything").unwrap().is_empty());
    assert!(document.links(0).unwrap().is_empty());
    assert!(!document.is_reflowable());
    assert!(matches!(
        document.set_layout(&Layout::new(600.0, 800.0, 16.0)),
        Err(DocError::NotReflowable)
    ));
    assert!(matches!(
        document.page_size(3),
        Err(DocError::PageOutOfRange { .. })
    ));
    assert!(open_with(&path, Engine::Mupdf).is_err());
    assert!(open_with(&path, Engine::Cre).is_err());
    drop(document);
    assert_eq!(open(&path).unwrap().page_count(), 3);
    assert_eq!(Format::from_extension("CBR"), Some(Format::Cbr));
    assert_eq!(
        kaia_doc::format::sniff(b"Rar!\x1a\x07\x01\x00"),
        Some(Format::Cbr)
    );
}

#[test]
fn cbr_rejects_bad_scales_clips_and_render_expansion() {
    let png = image(8, 12, [0, 0, 0], ImageFormat::Png);
    let (_dir, path) = fixture(&common::rar(&[("page.png", &png, png.len() as u32)]));
    let doc = open(&path).unwrap();
    for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::MAX, 10000.0] {
        assert!(doc.render(0, scale, PixelFormat::Rgb24).is_err());
    }
    for clip in [
        Rect::new(-1, 0, 1, 1),
        Rect::new(0, 0, 0, 1),
        Rect::new(7, 11, 2, 2),
        Rect::new(i32::MAX, 0, u32::MAX, 1),
    ] {
        assert!(doc.render_clip(0, 1.0, PixelFormat::Gray8, clip).is_err());
    }
}

#[test]
fn cbr_rejects_oversize_image_header_and_corrupt_images() {
    let mut png = image(8, 12, [0, 0, 0], ImageFormat::Png);
    // Change IHDR width and repair its CRC so rejection is due to dimensions.
    png[16..20].copy_from_slice(&5000u32.to_be_bytes());
    let crc = crc32fast::hash(&png[12..29]);
    png[29..33].copy_from_slice(&crc.to_be_bytes());
    for bytes in [png, b"not an image".to_vec()] {
        let (_dir, path) = fixture(&common::rar(&[("page.png", &bytes, bytes.len() as u32)]));
        let doc = open(&path).unwrap();
        assert!(doc.page_size(0).is_err());
        assert!(doc.render(0, 1.0, PixelFormat::Rgb24).is_err());
        assert!(doc.cover(Size::new(10, 10)).is_none());
    }
}

#[test]
fn cbr_rejects_malformed_empty_oversize_and_unsafe_archives() {
    let png = image(8, 12, [0, 0, 0], ImageFormat::Png);
    let mut truncated = common::rar(&[("page.png", &png, png.len() as u32)]);
    truncated.truncate(truncated.len() - 20);
    for bytes in [
        truncated,
        b"not a rar".to_vec(),
        common::rar(&[]),
        common::rar(&[("page.png", b"", 64 * 1024 * 1024 + 1)]),
        common::rar(&[("page.png", &png, png.len() as u32), ("../ignored.txt", b"x", 1)]),
    ] {
        let (_dir, path) = fixture(&bytes);
        assert!(CbrDocument::open(&path).is_err());
    }
}
