//! Tests that need a started engine. They share one process, so they all go
//! through `start`, which calls `init` once.

use std::ffi::c_int;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::Once;

use kaia_cre_sys::{ffi, init, Doc, Error, DATA_DIR, FONTS_DIR, MAX_RENDER_SIDE};

fn start() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let fonts: Vec<PathBuf> = ["NotoSerif-Regular.ttf", "NotoSerif-Bold.ttf"]
            .iter()
            .map(|f| Path::new(FONTS_DIR).join("noto").join(f))
            .filter(|p| p.exists())
            .collect();
        assert!(!fonts.is_empty(), "no Noto Serif in {FONTS_DIR}");
        let accepted = init(&fonts, &Path::new(DATA_DIR).join("hyph")).expect("init");
        assert!(accepted > 0, "CREngine accepted no fonts");
    });
}

fn sample_txt() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/sample.txt")
}

#[test]
fn doc_new_and_font_face_work_after_init() {
    start();
    let mut doc = Doc::new(300, 400).expect("doc after init");
    doc.set_font_face("Noto Serif").expect("font face");
    // The same face again is accepted (it skips the re-layout).
    doc.set_font_face("Noto Serif").expect("font face again");
    assert!(doc.load(&sample_txt()).expect("load"));
    assert!(doc.page_count().expect("pages") >= 1);
}

#[test]
fn render_returns_buffers_of_the_requested_size() {
    start();
    let mut doc = Doc::new(200, 300).expect("doc");
    assert!(doc.load(&sample_txt()).expect("load"));
    let gray = doc.render(200, 300, false).expect("gray");
    assert_eq!(gray.len(), 200 * 300);
    let rgb = doc.render(200, 300, true).expect("rgb");
    assert_eq!(rgb.len(), 200 * 300 * 3);
    assert!(gray.iter().any(|&b| b < 128), "page has no ink");
}

#[test]
fn render_rejects_absurd_sizes_in_rust() {
    start();
    let mut doc = Doc::new(200, 300).expect("doc");
    for (w, h) in [
        (0, 10),
        (10, 0),
        (MAX_RENDER_SIDE + 1, 10),
        (10, MAX_RENDER_SIDE + 1),
        (u32::MAX, u32::MAX),
        (65536, 65536),
    ] {
        assert!(
            matches!(doc.render(w, h, true), Err(Error::BadInput(_))),
            "{w}x{h} must be rejected before any allocation"
        );
    }
}

#[test]
fn render_rejects_absurd_sizes_in_the_shim() {
    start();
    // Call the shim directly. Its own checks must hold without the Rust ones.
    let mut raw: *mut ffi::kaia_cre_doc = ptr::null_mut();
    let mut buf = vec![0u8; 16];
    // SAFETY: `raw` is a valid out pointer and the handle is freed once below.
    // Every render call gets `buf` with its true length.
    unsafe {
        assert_eq!(ffi::kaia_cre_doc_new(200, 300, &mut raw), ffi::KAIA_CRE_OK);
        for (w, h, color) in [
            (MAX_RENDER_SIDE as c_int + 1, 1, 0),
            (1, MAX_RENDER_SIDE as c_int + 1, 1),
            (c_int::MAX, c_int::MAX, 1),
            (-1, 5, 0),
            (4, 5, 1), // within the limits, but the buffer is too small
        ] {
            let code = ffi::kaia_cre_render(raw, w, h, color, buf.as_mut_ptr(), buf.len());
            assert_eq!(code, ffi::KAIA_CRE_ERROR, "{w}x{h} color {color}");
        }
        ffi::kaia_cre_doc_free(raw);
    }
}

#[test]
fn font_face_rejects_null_in_the_shim() {
    start();
    let mut raw: *mut ffi::kaia_cre_doc = ptr::null_mut();
    // SAFETY: `raw` is a valid out pointer; the handle is freed once below.
    unsafe {
        assert_eq!(ffi::kaia_cre_doc_new(200, 300, &mut raw), ffi::KAIA_CRE_OK);
        assert_eq!(ffi::kaia_cre_set_font_face(raw, ptr::null()), ffi::KAIA_CRE_ERROR);
        ffi::kaia_cre_doc_free(raw);
    }
}
