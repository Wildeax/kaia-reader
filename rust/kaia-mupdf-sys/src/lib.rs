//! C shim and bindings to KOReader's MuPDF build.
//!
//! This crate is the only place in Kaia that contains `unsafe` code for MuPDF.
//! [`Document`] owns one MuPDF context and one document, and frees both on
//! drop. Its methods return plain Rust values. A MuPDF exception never crosses
//! into Rust: the C shim catches it and returns an [`Error`] with MuPDF's
//! message.
//!
//! Threading: a `Document` is not `Send` or `Sync`. Use it from one thread.
//! Fallback fonts are read from the directory named by the `FONTDIR`
//! environment variable (KOReader's convention; the default is `./fonts`).
//!
//! Build configuration is described in `README.md`.

pub mod ffi;

use std::ffi::{CStr, CString};
use std::fmt;
use std::path::Path;
use std::ptr::{self, NonNull};

use ffi::*;

/// Why a call failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// MuPDF raised an exception: a corrupt file, an unsupported feature.
    Mupdf,
    /// Bad argument, for example a path with a NUL byte or an empty search.
    Argument,
    /// Allocation failed.
    OutOfMemory,
    /// The page number is outside the document.
    PageRange,
}

/// An error from the shim. `message` is MuPDF's text when the kind is `Mupdf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Largest side of a rendered area in device pixels. Matches `KAIA_MAX_SIDE` in
/// the C shim.
const MAX_RENDER_SIDE: i64 = 20_000;

/// Rendered pixels. Rows are tightly packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub gray: bool,
    pub data: Vec<u8>,
}

impl Pixels {
    pub fn stride(&self) -> usize {
        self.width as usize * if self.gray { 1 } else { 3 }
    }
}

/// One outline entry, flattened depth first. `level` starts at 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocItem {
    pub level: u32,
    pub page: Option<u32>,
    pub title: String,
}

/// A link on a page. `bbox` is `[x0, y0, x1, y1]` in points.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkItem {
    pub bbox: [f32; 4],
    pub page: Option<u32>,
    pub uri: String,
}

/// Structured text block. `kind` is 0 for text, 1 for an image, 2 for other.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StextBlock {
    pub kind: u32,
    pub bbox: [f32; 4],
    pub first_line: usize,
    pub line_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StextLine {
    pub bbox: [f32; 4],
    pub first_char: usize,
    pub char_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StextChar {
    pub unicode: u32,
    pub bbox: [f32; 4],
}

/// Structured text for one page. Lines and chars are flat arrays that blocks
/// and lines index into.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StextPage {
    pub blocks: Vec<StextBlock>,
    pub lines: Vec<StextLine>,
    pub chars: Vec<StextChar>,
}

/// An owned MuPDF document with its own context.
pub struct Document {
    raw: NonNull<kaia_doc>,
}

impl Document {
    /// Opens a file and lets MuPDF pick the handler from the file name.
    pub fn open(path: &Path) -> Result<Self> {
        let doc = Self::new_handle()?;
        let c = path_to_cstring(path)?;
        // SAFETY: `doc.raw` is a live handle owned by this function. `c` is a
        // NUL-terminated string that outlives the call.
        let status = unsafe { kaia_doc_open(doc.raw.as_ptr(), c.as_ptr()) };
        doc.check(status)?;
        Ok(doc)
    }

    /// Opens a file with an explicit handler. `magic` is an extension such as
    /// `.epub` or a mime type.
    pub fn open_as(path: &Path, magic: &str) -> Result<Self> {
        let doc = Self::new_handle()?;
        let c = path_to_cstring(path)?;
        let m = to_cstring(magic)?;
        // SAFETY: `doc.raw` is a live handle. Both strings are NUL-terminated and
        // outlive the call.
        let status = unsafe { kaia_doc_open_as(doc.raw.as_ptr(), c.as_ptr(), m.as_ptr()) };
        doc.check(status)?;
        Ok(doc)
    }

    fn new_handle() -> Result<Self> {
        let mut raw: *mut kaia_doc = ptr::null_mut();
        // SAFETY: `raw` is a valid place to write one pointer. On success the
        // returned handle is owned by the `Document` built below.
        let status = unsafe { kaia_doc_new(&mut raw) };
        if status != KAIA_OK || raw.is_null() {
            return Err(Error {
                kind: kind_of(status),
                message: "cannot create MuPDF context".to_string(),
            });
        }
        Ok(Document {
            raw: NonNull::new(raw).expect("checked non-null above"),
        })
    }

    /// Converts a status into a `Result`. Reads MuPDF's message on failure.
    fn check(&self, status: i32) -> Result<()> {
        if status == KAIA_OK {
            return Ok(());
        }
        // SAFETY: `self.raw` is live, and `kaia_doc_error` returns a pointer to a
        // NUL-terminated buffer inside the handle. We copy it before any other
        // call on the handle.
        let message = unsafe {
            let p = kaia_doc_error(self.raw.as_ptr());
            if p.is_null() {
                String::new()
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };
        Err(Error {
            kind: kind_of(status),
            message,
        })
    }

    fn page_arg(page: u32) -> Result<i32> {
        i32::try_from(page).map_err(|_| Error {
            kind: ErrorKind::PageRange,
            message: "page number out of range".to_string(),
        })
    }

    /// Number of pages at the current layout.
    pub fn page_count(&self) -> Result<u32> {
        let mut n: i32 = 0;
        // SAFETY: `self.raw` is live and `n` is a valid out pointer.
        let status = unsafe { kaia_doc_page_count(self.raw.as_ptr(), &mut n) };
        self.check(status)?;
        Ok(n.max(0) as u32)
    }

    pub fn is_reflowable(&self) -> Result<bool> {
        let mut r: i32 = 0;
        // SAFETY: `self.raw` is live and `r` is a valid out pointer.
        let status = unsafe { kaia_doc_is_reflowable(self.raw.as_ptr(), &mut r) };
        self.check(status)?;
        Ok(r != 0)
    }

    /// Sets the user stylesheet for HTML and EPUB layout. Call before `layout`.
    pub fn set_user_css(&mut self, css: &str) -> Result<()> {
        let c = to_cstring(css)?;
        // SAFETY: `self.raw` is live. The shim copies `c` before returning.
        let status = unsafe { kaia_doc_set_user_css(self.raw.as_ptr(), c.as_ptr()) };
        self.check(status)
    }

    /// Lays out a reflowable document. Fixed-layout documents return an error.
    pub fn layout(&mut self, width: f32, height: f32, em: f32) -> Result<()> {
        // SAFETY: `self.raw` is live. The floats are plain values.
        let status = unsafe { kaia_doc_layout(self.raw.as_ptr(), width, height, em) };
        self.check(status)
    }

    /// Page box `[x0, y0, x1, y1]` in points.
    pub fn page_bounds(&self, page: u32) -> Result<[f32; 4]> {
        let page = Self::page_arg(page)?;
        let mut b = [0f32; 4];
        // SAFETY: `self.raw` is live. `b` has room for four floats.
        let status = unsafe { kaia_doc_page_bounds(self.raw.as_ptr(), page, b.as_mut_ptr()) };
        self.check(status)?;
        Ok(b)
    }

    /// Renders a page. `clip` is `[x0, y0, x1, y1]` in scaled device pixels;
    /// `None` renders the whole page. Scale 1.0 is 72 dpi.
    pub fn render(&self, page: u32, scale: f32, gray: bool, clip: Option<[i32; 4]>) -> Result<Pixels> {
        let page = Self::page_arg(page)?;
        if let Some([x0, y0, x1, y1]) = clip {
            // Subtract in i64: the corners can be anywhere in the i32 range.
            let w = i64::from(x1) - i64::from(x0);
            let h = i64::from(y1) - i64::from(y0);
            if w <= 0 || h <= 0 {
                return Err(Error {
                    kind: ErrorKind::Argument,
                    message: "render clip is empty".to_string(),
                });
            }
            if w > MAX_RENDER_SIDE || h > MAX_RENDER_SIDE {
                return Err(Error {
                    kind: ErrorKind::Argument,
                    message: "render clip is too large".to_string(),
                });
            }
        }
        let mut out = kaia_pixels {
            width: 0,
            height: 0,
            stride: 0,
            format: 0,
            len: 0,
            data: ptr::null_mut(),
        };
        let clip_ptr = clip.as_ref().map_or(ptr::null(), |c| c.as_ptr());
        // SAFETY: `self.raw` is live. `clip_ptr` is either null or points at four
        // ints that live until the call returns. `out` is a valid out pointer.
        let status =
            unsafe { kaia_doc_render(self.raw.as_ptr(), page, scale, clip_ptr, gray as i32, &mut out) };
        self.check(status)?;
        let data = take_buffer(out.data, out.len);
        Ok(Pixels {
            width: out.width.max(0) as u32,
            height: out.height.max(0) as u32,
            gray: out.format == 0,
            data,
        })
    }

    /// Metadata value for a MuPDF key such as `info:Title` or `info:Author`.
    pub fn metadata(&self, key: &str) -> Result<Option<String>> {
        let k = to_cstring(key)?;
        let mut out: *mut std::os::raw::c_char = ptr::null_mut();
        // SAFETY: `self.raw` is live. `k` is NUL-terminated. `out` is a valid
        // out pointer; on success it holds a malloc'd string or null.
        let status = unsafe { kaia_doc_metadata(self.raw.as_ptr(), k.as_ptr(), &mut out) };
        self.check(status)?;
        Ok(take_c_string(out))
    }

    /// Flattened table of contents. Empty when the document has none.
    pub fn toc(&self) -> Result<Vec<TocItem>> {
        let mut items: *mut kaia_toc_item = ptr::null_mut();
        let mut count: i32 = 0;
        // SAFETY: `self.raw` is live. Out pointers are valid. On success the
        // array is freed below with `kaia_toc_free`.
        let status = unsafe { kaia_doc_toc(self.raw.as_ptr(), &mut items, &mut count) };
        self.check(status)?;
        let mut result = Vec::with_capacity(count.max(0) as usize);
        if !items.is_null() {
            for i in 0..count.max(0) as usize {
                // SAFETY: the shim wrote `count` initialized items at `items`.
                let it = unsafe { *items.add(i) };
                result.push(TocItem {
                    level: it.level.max(0) as u32,
                    page: to_page(it.page),
                    title: copy_c_string(it.title).unwrap_or_default(),
                });
            }
            // SAFETY: `items` and `count` came from `kaia_doc_toc`. The titles are
            // already copied, so freeing them here is the only release.
            unsafe { kaia_toc_free(items, count) };
        }
        Ok(result)
    }

    /// Links on a page.
    pub fn links(&self, page: u32) -> Result<Vec<LinkItem>> {
        let page = Self::page_arg(page)?;
        let mut links: *mut kaia_link = ptr::null_mut();
        let mut count: i32 = 0;
        // SAFETY: `self.raw` is live. Out pointers are valid. The array is freed
        // below with `kaia_links_free`.
        let status = unsafe { kaia_doc_links(self.raw.as_ptr(), page, &mut links, &mut count) };
        self.check(status)?;
        let mut result = Vec::with_capacity(count.max(0) as usize);
        if !links.is_null() {
            for i in 0..count.max(0) as usize {
                // SAFETY: the shim wrote `count` initialized links at `links`.
                let l = unsafe { *links.add(i) };
                result.push(LinkItem {
                    bbox: [l.x0, l.y0, l.x1, l.y1],
                    page: to_page(l.page),
                    uri: copy_c_string(l.uri).unwrap_or_default(),
                });
            }
            // SAFETY: `links` and `count` came from `kaia_doc_links`.
            unsafe { kaia_links_free(links, count) };
        }
        Ok(result)
    }

    /// Structured text for a page, with boxes for every character.
    pub fn stext(&self, page: u32) -> Result<StextPage> {
        let page = Self::page_arg(page)?;
        let mut raw = kaia_stext {
            block_count: 0,
            blocks: ptr::null_mut(),
            line_count: 0,
            lines: ptr::null_mut(),
            char_count: 0,
            chars: ptr::null_mut(),
        };
        // SAFETY: `self.raw` is live and `raw` is a valid out pointer. The shim
        // fills it and `kaia_stext_free` releases it below.
        let status = unsafe { kaia_doc_stext(self.raw.as_ptr(), page, &mut raw) };
        self.check(status)?;
        let page_text = copy_stext(&raw);
        // SAFETY: `raw` was filled by `kaia_doc_stext` and is freed exactly once.
        unsafe { kaia_stext_free(&mut raw) };
        Ok(page_text)
    }

    /// Hit quads for `needle` on a page, at most `max_hits` of them. The shim
    /// clamps `max_hits` to 65536. Each quad is `[ul.x, ul.y, ur.x, ur.y, ll.x, ll.y, lr.x, lr.y]`.
    pub fn search(&self, page: u32, needle: &str, max_hits: u32) -> Result<Vec<[f32; 8]>> {
        let page = Self::page_arg(page)?;
        let n = to_cstring(needle)?;
        let max = i32::try_from(max_hits).unwrap_or(i32::MAX);
        let mut quads: *mut f32 = ptr::null_mut();
        let mut count: i32 = 0;
        // SAFETY: `self.raw` is live. `n` is NUL-terminated. Out pointers are
        // valid. On success `quads` holds `count * 8` floats and is freed below.
        let status =
            unsafe { kaia_doc_search(self.raw.as_ptr(), page, n.as_ptr(), max, &mut quads, &mut count) };
        self.check(status)?;
        let mut result = Vec::with_capacity(count.max(0) as usize);
        if !quads.is_null() {
            for i in 0..count.max(0) as usize {
                let mut q = [0f32; 8];
                // SAFETY: the shim wrote `count * 8` floats at `quads`.
                unsafe { ptr::copy_nonoverlapping(quads.add(i * 8), q.as_mut_ptr(), 8) };
                result.push(q);
            }
            // SAFETY: `quads` was allocated by the shim with malloc.
            unsafe { kaia_free(quads.cast()) };
        }
        Ok(result)
    }

    /// Page number that an internal URI such as `#chapter3` points to.
    pub fn resolve_uri(&self, uri: &str) -> Result<Option<u32>> {
        let u = to_cstring(uri)?;
        let mut page: i32 = -1;
        // SAFETY: `self.raw` is live. `u` is NUL-terminated. `page` is a valid
        // out pointer.
        let status = unsafe { kaia_doc_resolve_uri(self.raw.as_ptr(), u.as_ptr(), &mut page) };
        self.check(status)?;
        Ok(to_page(page))
    }

    /// Page number for a MuPDF chapter and page location.
    pub fn location_page(&self, chapter: i32, page: i32) -> Result<Option<u32>> {
        let mut out: i32 = -1;
        // SAFETY: `self.raw` is live. `out` is a valid out pointer.
        let status = unsafe { kaia_doc_location_page(self.raw.as_ptr(), chapter, page, &mut out) };
        self.check(status)?;
        Ok(to_page(out))
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        // SAFETY: `self.raw` came from `kaia_doc_new` and has not been freed.
        // After this call the pointer is not used again.
        unsafe { kaia_doc_free(self.raw.as_ptr()) };
    }
}

fn kind_of(status: i32) -> ErrorKind {
    match status {
        KAIA_ERR_NOMEM => ErrorKind::OutOfMemory,
        KAIA_ERR_RANGE => ErrorKind::PageRange,
        KAIA_ERR_ARG => ErrorKind::Argument,
        _ => ErrorKind::Mupdf,
    }
}

fn to_page(v: i32) -> Option<u32> {
    (v >= 0).then_some(v as u32)
}

fn to_cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error {
        kind: ErrorKind::Argument,
        message: "string contains a NUL byte".to_string(),
    })
}

fn path_to_cstring(path: &Path) -> Result<CString> {
    let s = path.to_str().ok_or_else(|| Error {
        kind: ErrorKind::Argument,
        message: format!("path is not UTF-8: {}", path.display()),
    })?;
    to_cstring(s)
}

/// Copies a C string owned by a shim array. The array's free function releases it.
fn copy_c_string(p: *mut std::os::raw::c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: `p` is a NUL-terminated string owned by a shim array that stays
    // alive until the caller frees the array.
    Some(unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() })
}

/// Copies a malloc'd C string into a Rust `String` and frees the original.
fn take_c_string(p: *mut std::os::raw::c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: `p` is a NUL-terminated string allocated by the shim. It is read
    // once here and freed once below.
    let s = unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() };
    // SAFETY: `p` came from malloc in the shim and is not used after this.
    unsafe { kaia_free(p.cast()) };
    Some(s)
}

/// Copies `len` bytes from a malloc'd buffer and frees it.
fn take_buffer(p: *mut u8, len: usize) -> Vec<u8> {
    if p.is_null() {
        return Vec::new();
    }
    // SAFETY: the shim allocated `len` bytes at `p` and wrote all of them before
    // returning success. We copy them, then free the original exactly once.
    let v = unsafe { std::slice::from_raw_parts(p, len).to_vec() };
    // SAFETY: `p` came from malloc in the shim and is not used after this.
    unsafe { kaia_free(p.cast()) };
    v
}

fn copy_stext(raw: &kaia_stext) -> StextPage {
    let mut page = StextPage::default();
    if !raw.chars.is_null() {
        for i in 0..raw.char_count.max(0) as usize {
            // SAFETY: the shim allocated `char_count` chars and filled them.
            let c = unsafe { *raw.chars.add(i) };
            page.chars.push(StextChar {
                unicode: c.unicode.max(0) as u32,
                bbox: [c.x0, c.y0, c.x1, c.y1],
            });
        }
    }
    if !raw.lines.is_null() {
        for i in 0..raw.line_count.max(0) as usize {
            // SAFETY: the shim allocated `line_count` lines and filled them.
            let l = unsafe { *raw.lines.add(i) };
            page.lines.push(StextLine {
                bbox: [l.x0, l.y0, l.x1, l.y1],
                first_char: l.first_char.max(0) as usize,
                char_count: l.char_count.max(0) as usize,
            });
        }
    }
    if !raw.blocks.is_null() {
        for i in 0..raw.block_count.max(0) as usize {
            // SAFETY: the shim allocated `block_count` blocks and filled them.
            let b = unsafe { *raw.blocks.add(i) };
            page.blocks.push(StextBlock {
                kind: b.kind.max(0) as u32,
                bbox: [b.x0, b.y0, b.x1, b.y1],
                first_line: b.first_line.max(0) as usize,
                line_count: b.line_count.max(0) as usize,
            });
        }
    }
    page
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_an_error_not_a_crash() {
        let err = Document::open(Path::new("/nonexistent/kaia/none.pdf"))
            .err()
            .expect("opening a missing file must fail");
        assert_eq!(err.kind, ErrorKind::Mupdf);
        assert!(!err.message.is_empty());
    }

    #[test]
    fn nul_in_path_is_an_argument_error() {
        let err = Document::open(Path::new("a\0b.pdf")).err().expect("must fail");
        assert_eq!(err.kind, ErrorKind::Argument);
    }

    #[test]
    fn page_numbers_beyond_i32_are_rejected() {
        assert_eq!(
            Document::page_arg(u32::MAX).unwrap_err().kind,
            ErrorKind::PageRange
        );
        assert_eq!(Document::page_arg(7).unwrap(), 7);
    }

    fn sample_pdf() -> Document {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/sample.pdf");
        Document::open(&path).expect("open test/sample.pdf")
    }

    #[test]
    fn extreme_clips_are_rejected_in_rust() {
        let doc = sample_pdf();
        for clip in [
            [i32::MIN, 0, i32::MAX, 10],
            [0, i32::MIN, 10, i32::MAX],
            [i32::MAX, 0, i32::MIN, 10],
            [0, 0, 0, 10],
            [0, 0, 20_001, 10],
        ] {
            let err = doc.render(0, 1.0, true, Some(clip)).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Argument, "clip {clip:?}");
        }
        let ok = doc
            .render(0, 1.0, true, Some([0, 0, 20, 10]))
            .expect("small clip");
        assert_eq!((ok.width, ok.height), (20, 10));
    }

    #[test]
    fn extreme_clips_are_rejected_in_c_without_overflow() {
        let doc = sample_pdf();
        // Call the shim directly: the Rust wrapper would reject these first.
        for clip in [
            [i32::MIN, i32::MIN, i32::MAX, i32::MAX],
            [i32::MIN, 0, i32::MAX, 10],
            [i32::MAX, 0, i32::MIN, 10],
        ] {
            let mut out = kaia_pixels {
                width: 0,
                height: 0,
                stride: 0,
                format: 0,
                len: 0,
                data: ptr::null_mut(),
            };
            // SAFETY: `doc.raw` is live, `clip` holds four ints for the call and
            // `out` is a valid out pointer.
            let status = unsafe { kaia_doc_render(doc.raw.as_ptr(), 0, 1.0, clip.as_ptr(), 1, &mut out) };
            assert_ne!(status, KAIA_OK, "clip {clip:?}");
            assert!(out.data.is_null());
        }
    }

    #[test]
    fn huge_hit_limit_is_clamped_not_overflowed() {
        let doc = sample_pdf();
        // The result is whatever the page holds; the call must neither fail nor
        // try to allocate max_hits * sizeof(fz_quad) bytes.
        let mut out: *mut f32 = ptr::null_mut();
        let mut count: i32 = 0;
        let needle = CString::new("the").unwrap();
        // SAFETY: `doc.raw` is live, `needle` is NUL-terminated and the out
        // pointers are valid. On success `out` is a malloc'd array or null.
        let status = unsafe {
            kaia_doc_search(
                doc.raw.as_ptr(),
                0,
                needle.as_ptr(),
                i32::MAX,
                &mut out,
                &mut count,
            )
        };
        assert_eq!(status, KAIA_OK);
        assert!(count >= 0);
        // SAFETY: `out` came from the shim's malloc, or is null (kaia_free accepts it).
        unsafe { kaia_free(out.cast()) };
        assert!(doc.search(0, "the", u32::MAX).is_ok());
    }
}
