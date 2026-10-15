//! Fixed-layout zoom geometry, clipped rendering and saved views.
mod common;

use kaia_book::*;
use kaia_doc::{Document, Format, MupdfDocument};
use std::time::Instant;

const VIEW: Size = Size { w: 900, h: 1200 };

#[test]
fn pinch_keeps_the_touch_anchor_and_renders_its_region() {
    let data = tempfile::tempdir().unwrap();
    let path = common::repo_file("sample.pdf");
    let mut session = common::open(&path, &data);
    session.go_to(Location::Page(1)).unwrap();
    let fit = session.page_transform(VIEW).unwrap();
    let touch = Point::new(360, 480);
    let page_point = fit.to_page(Point::new(
        touch.x - (VIEW.w - fit.image.w) as i32 / 2,
        touch.y - (VIEW.h - fit.image.h) as i32 / 2,
    ));
    session.pinch_zoom(2.0, touch, touch, VIEW).unwrap();
    let zoom = session.page_transform(VIEW).unwrap();
    assert!((zoom.scale / fit.scale - 2.0).abs() < 0.001);
    let actual = zoom.to_screen_point(page_point);
    assert!((actual.x - touch.x).abs() <= 1 && (actual.y - touch.y).abs() <= 1);
    let start = Instant::now();
    let clipped = session.render_current(VIEW).unwrap();
    let elapsed = start.elapsed();
    eprintln!(
        "sample.pdf 2x clipped render: {elapsed:?}, {}x{}",
        clipped.width(),
        clipped.height()
    );
    assert!(
        elapsed < std::time::Duration::from_millis(100),
        "2x render exceeded the page-turn budget: {elapsed:?}"
    );
    assert!(clipped.width() <= VIEW.w && clipped.height() <= VIEW.h);
    assert!(
        clipped.data().iter().any(|p| *p < 128),
        "the test must crop visible content"
    );
    let doc = MupdfDocument::open(&path, Format::Pdf).unwrap();
    // Compute the expected crop from the original centered image and the touch.
    let left = ((touch.x - (VIEW.w - fit.image.w) as i32 / 2) * 2 - touch.x).max(0);
    let top = ((touch.y - (VIEW.h - fit.image.h) as i32 / 2) * 2 - touch.y).max(0);
    let expected = doc
        .render_clip(
            1,
            fit.scale * 2.0,
            PixelFormat::Gray8,
            Rect::new(left, top, VIEW.w, VIEW.h),
        )
        .unwrap();
    assert!(
        clipped.data() == expected.data(),
        "pinch rendered the wrong page region"
    );
    session.pinch_zoom(100.0, touch, touch, VIEW).unwrap();
    assert!((session.page_transform(VIEW).unwrap().scale / fit.scale - 4.0).abs() < 0.001);
    session.pinch_zoom(0.01, touch, touch, VIEW).unwrap();
    assert_eq!(session.zoom(), ZoomMode::FitPage);
    assert_eq!(session.page_transform(VIEW).unwrap().offset, Point::new(0, 0));
}

#[test]
fn drag_clamps_and_cached_regions_are_distinct() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("2col.pdf"), &data);
    s.set_zoom(ZoomMode::Manual { percent: 200 }).unwrap();
    let first = s.render_current(VIEW).unwrap();
    s.pan_by(Point::new(-200, -300), VIEW).unwrap();
    assert_eq!(s.page_transform(VIEW).unwrap().offset, Point::new(200, 300));
    let moved = s.render_current(VIEW).unwrap();
    assert_ne!(first.data(), moved.data());
    s.pan_by(Point::new(i32::MIN, i32::MIN), VIEW).unwrap();
    let t = s.page_transform(VIEW).unwrap();
    assert_eq!(
        t.offset,
        Point::new((t.full_image.w - VIEW.w) as i32, (t.full_image.h - VIEW.h) as i32)
    );
    s.pan_by(Point::new(i32::MAX, i32::MAX), VIEW).unwrap();
    assert_eq!(s.page_transform(VIEW).unwrap().offset, Point::new(0, 0));
    assert_eq!(s.render_current(VIEW).unwrap().data(), first.data());
}

#[test]
fn turns_keep_zoom_and_offset_and_stop_at_physical_boundaries() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("2col.pdf"), &data);
    s.set_zoom(ZoomMode::Manual { percent: 200 }).unwrap();
    s.pan_by(Point::new(-200, -300), VIEW).unwrap();
    let before = s.page_transform(VIEW).unwrap();
    assert!(!s.can_turn_view(false, VIEW).unwrap());
    assert!(s.turn_view(true, VIEW).unwrap());
    assert_eq!(s.current().page, 1);
    let after = s.page_transform(VIEW).unwrap();
    assert_eq!(after.offset, before.offset);
    assert_eq!(after.scale, before.scale);
    assert!(s.turn_view(false, VIEW).unwrap());
    assert_eq!(s.current().page, 0);
    assert_eq!(s.page_transform(VIEW).unwrap(), before);
    assert!(!s.turn_view(false, VIEW).unwrap());
    s.go_to(Location::Page(s.page_count() as u32 - 1)).unwrap();
    assert!(!s.can_turn_view(true, VIEW).unwrap());
    assert!(!s.turn_view(true, VIEW).unwrap());
}

#[test]
fn region_and_pan_survive_reopen() {
    let data = tempfile::tempdir().unwrap();
    let path = common::repo_file("sample.pdf");
    let before = {
        let mut s = common::open(&path, &data);
        s.zoom_to_area(Point::new(200, 300), Point::new(650, 900), VIEW)
            .unwrap();
        s.pan_by(Point::new(-30, -40), VIEW).unwrap();
        assert_eq!(s.zoom(), ZoomMode::Region);
        s.save().unwrap();
        s.page_transform(VIEW).unwrap()
    };
    let mut s = common::open(&path, &data);
    assert_eq!(s.zoom(), ZoomMode::Region);
    assert_eq!(s.page_transform(VIEW).unwrap(), before);
    s.set_zoom(ZoomMode::FitPage).unwrap();
    assert_eq!(s.page_transform(VIEW).unwrap().offset, Point::new(0, 0));
}

#[test]
fn two_column_fit_goldens() {
    let data = tempfile::tempdir().unwrap();
    let path = common::repo_file("2col.pdf");
    let mut s = common::open(&path, &data);
    s.set_viewport(VIEW).unwrap();
    let fit = s.page_transform(VIEW).unwrap();
    for (mode, name) in [
        (ZoomMode::FitWidth, "pdf-fit-width"),
        (ZoomMode::FitColumns, "pdf-fit-columns"),
    ] {
        s.set_zoom(mode).unwrap();
        let t = s.page_transform(VIEW).unwrap();
        assert_eq!(s.zoom(), mode);
        if mode == ZoomMode::FitColumns {
            assert!(t.scale > fit.scale * 1.8, "column scale: {}", t.scale / fit.scale);
            assert!(t.offset.x > 0);
        } else {
            assert_eq!(t.full_image.w, VIEW.w);
        }
        let image = s.render_current(VIEW).unwrap();
        let golden =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.png"));
        if std::env::var_os("KAIA_ZOOM_REVIEW").is_some() {
            kaia_gfx::png::write_pixmap(&image, &std::path::PathBuf::from(format!("/tmp/{name}.png")))
                .unwrap();
        } else {
            kaia_gfx::golden::assert_matches_golden(&image, &golden, 3);
        }
    }
}

#[test]
fn djvu_zoom_uses_the_fixed_layout_backend() {
    let data = tempfile::tempdir().unwrap();
    let mut s = common::open(&common::repo_file("djvu3spec.djvu"), &data);
    let fit = s.page_transform(VIEW).unwrap();
    s.pinch_zoom(2.0, Point::new(450, 600), Point::new(450, 600), VIEW)
        .unwrap();
    let zoom = s.page_transform(VIEW).unwrap();
    assert!((zoom.scale / fit.scale - 2.0).abs() < 0.001);
    let image = s.render_current(VIEW).unwrap();
    assert_eq!(image.size(), zoom.image);
}

#[path = "../../kaia-archive-sys/tests/common/mod.rs"]
mod rar;

#[test]
fn cbr_session_zooms_and_pans() {
    let data = tempfile::tempdir().unwrap();
    let book = data.path().join("comic.cbr");
    let png = include_bytes!("golden/pdf-fit-width.png");
    std::fs::write(&book, rar::rar(&[("page.png", png, png.len() as u32)])).unwrap();
    let mut s = common::open(&book, &data);
    assert!(!s.is_reflowable());
    s.set_zoom(ZoomMode::Manual { percent: 400 }).unwrap();
    s.pan_by(Point::new(-900, -1200), VIEW).unwrap();
    assert_eq!(s.page_transform(VIEW).unwrap().offset, Point::new(900, 1200));
    assert_eq!(s.render_current(VIEW).unwrap().size(), VIEW);
}

#[test]
fn cbz_session_zooms_and_pans() {
    let data = tempfile::tempdir().unwrap();
    let book = data.path().join("comic.cbz");
    std::fs::write(
        &book,
        common::epub::zip(&[("page.png", include_bytes!("golden/pdf-fit-width.png").to_vec())]),
    )
    .unwrap();
    let mut s = common::open(&book, &data);
    assert!(!s.is_reflowable());
    s.set_zoom(ZoomMode::Manual { percent: 200 }).unwrap();
    s.pan_by(Point::new(-450, -600), VIEW).unwrap();
    assert_eq!(s.page_transform(VIEW).unwrap().offset, Point::new(450, 600));
    assert_eq!(s.render_current(VIEW).unwrap().size(), VIEW);
}
