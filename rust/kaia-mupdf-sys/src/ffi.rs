//! Raw declarations for `csrc/kaia_mupdf.h`, written by hand.
//!
//! Every function here is unsafe to call. Use the safe [`crate::Document`]
//! wrapper instead. The layouts below must match the C header field for field.

#![allow(non_camel_case_types)]

use std::os::raw::{c_char, c_float, c_int};

/// Status codes returned by every shim function.
pub const KAIA_OK: c_int = 0;
pub const KAIA_ERR_MUPDF: c_int = 1;
pub const KAIA_ERR_ARG: c_int = 2;
pub const KAIA_ERR_NOMEM: c_int = 3;
pub const KAIA_ERR_RANGE: c_int = 4;

/// Opaque handle: one fz_context, one document, one error buffer.
#[repr(C)]
pub struct kaia_doc {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kaia_pixels {
    pub width: c_int,
    pub height: c_int,
    pub stride: c_int,
    /// 0 is Gray8, 1 is Rgb24.
    pub format: c_int,
    pub len: usize,
    pub data: *mut u8,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kaia_toc_item {
    pub level: c_int,
    pub page: c_int,
    pub title: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kaia_link {
    pub x0: c_float,
    pub y0: c_float,
    pub x1: c_float,
    pub y1: c_float,
    pub page: c_int,
    pub uri: *mut c_char,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kaia_stext_block {
    pub kind: c_int,
    pub x0: c_float,
    pub y0: c_float,
    pub x1: c_float,
    pub y1: c_float,
    pub first_line: c_int,
    pub line_count: c_int,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kaia_stext_line {
    pub x0: c_float,
    pub y0: c_float,
    pub x1: c_float,
    pub y1: c_float,
    pub first_char: c_int,
    pub char_count: c_int,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct kaia_stext_char {
    pub unicode: c_int,
    pub x0: c_float,
    pub y0: c_float,
    pub x1: c_float,
    pub y1: c_float,
}

#[repr(C)]
#[derive(Debug)]
pub struct kaia_stext {
    pub block_count: c_int,
    pub blocks: *mut kaia_stext_block,
    pub line_count: c_int,
    pub lines: *mut kaia_stext_line,
    pub char_count: c_int,
    pub chars: *mut kaia_stext_char,
}

extern "C" {
    pub fn kaia_doc_new(out: *mut *mut kaia_doc) -> c_int;
    pub fn kaia_doc_free(d: *mut kaia_doc);
    pub fn kaia_doc_error(d: *const kaia_doc) -> *const c_char;

    pub fn kaia_doc_open(d: *mut kaia_doc, path: *const c_char) -> c_int;
    pub fn kaia_doc_open_as(d: *mut kaia_doc, path: *const c_char, magic: *const c_char) -> c_int;

    pub fn kaia_doc_page_count(d: *mut kaia_doc, out: *mut c_int) -> c_int;
    pub fn kaia_doc_is_reflowable(d: *mut kaia_doc, out: *mut c_int) -> c_int;
    pub fn kaia_doc_set_user_css(d: *mut kaia_doc, css: *const c_char) -> c_int;
    pub fn kaia_doc_layout(d: *mut kaia_doc, width: c_float, height: c_float, em: c_float) -> c_int;

    pub fn kaia_doc_page_bounds(d: *mut kaia_doc, page: c_int, box_: *mut c_float) -> c_int;
    pub fn kaia_doc_render(
        d: *mut kaia_doc,
        page: c_int,
        scale: c_float,
        clip: *const c_int,
        gray: c_int,
        out: *mut kaia_pixels,
    ) -> c_int;

    pub fn kaia_doc_metadata(d: *mut kaia_doc, key: *const c_char, out: *mut *mut c_char) -> c_int;

    pub fn kaia_doc_toc(d: *mut kaia_doc, items: *mut *mut kaia_toc_item, count: *mut c_int) -> c_int;
    pub fn kaia_toc_free(items: *mut kaia_toc_item, count: c_int);

    pub fn kaia_doc_links(
        d: *mut kaia_doc,
        page: c_int,
        links: *mut *mut kaia_link,
        count: *mut c_int,
    ) -> c_int;
    pub fn kaia_links_free(links: *mut kaia_link, count: c_int);

    pub fn kaia_doc_stext(d: *mut kaia_doc, page: c_int, out: *mut kaia_stext) -> c_int;
    pub fn kaia_stext_free(out: *mut kaia_stext);

    pub fn kaia_doc_search(
        d: *mut kaia_doc,
        page: c_int,
        needle: *const c_char,
        max_hits: c_int,
        quads: *mut *mut c_float,
        count: *mut c_int,
    ) -> c_int;

    pub fn kaia_doc_resolve_uri(d: *mut kaia_doc, uri: *const c_char, page: *mut c_int) -> c_int;
    pub fn kaia_doc_location_page(d: *mut kaia_doc, chapter: c_int, page: c_int, out: *mut c_int) -> c_int;

    pub fn kaia_free(p: *mut std::ffi::c_void);
}
