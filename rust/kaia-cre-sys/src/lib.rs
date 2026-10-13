//! C++ shim and bindings to KOReader's CREngine build.
//!
//! The C ABI is in `csrc/kaia_cre.h`. The raw declarations are in [`ffi`]. The
//! [`Doc`] type wraps one open document: it owns the handle, frees C strings
//! and arrays, and turns return codes into [`Error`]. CREngine keeps global
//! state, so the shim serializes calls with one lock. [`init`] must run before
//! any document is opened: [`Doc::new`] returns an error until it has.
//!
//! The shim lock covers one call. CREngine's hyphenation dictionary and
//! preferred font are process-wide, so a caller that uses documents from
//! several threads must hold its own lock across a whole operation and set
//! those two before it works. `kaia-doc` does this.
//!
//! This crate is the only place with `unsafe` code for CREngine. Every
//! `unsafe` block states the invariant it relies on.

use std::ffi::{c_char, c_int, CStr, CString};
use std::fmt;
use std::path::{Path, PathBuf};
use std::ptr;

/// CSS, hyphenation dictionaries and other data files that ship with KOReader.
/// The default is the desktop tree in this repository.
pub const DATA_DIR: &str = env!("KAIA_CRE_DATA_DIR");

/// Font files for CREngine's font manager. The default is `resources/fonts`.
pub const FONTS_DIR: &str = env!("KAIA_CRE_FONTS_DIR");

/// Largest width or height that [`Doc::render`] accepts, in pixels.
pub const MAX_RENDER_SIDE: u32 = 16384;

/// Raw declarations for `csrc/kaia_cre.h`.
#[allow(non_camel_case_types, missing_docs)]
pub mod ffi {
    use std::ffi::{c_char, c_int};

    pub const KAIA_CRE_OK: c_int = 0;
    pub const KAIA_CRE_NOT_FOUND: c_int = 1;
    pub const KAIA_CRE_ERROR: c_int = -1;

    #[repr(C)]
    pub struct kaia_cre_doc {
        _private: [u8; 0],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct kaia_cre_char_box {
        pub x0: f32,
        pub y0: f32,
        pub x1: f32,
        pub y1: f32,
        pub line: i32,
        pub codepoint: u32,
    }

    #[repr(C)]
    pub struct kaia_cre_toc_entry {
        pub level: i32,
        pub page: i32,
        pub title: *mut c_char,
        pub xpointer: *mut c_char,
    }

    #[repr(C)]
    pub struct kaia_cre_link_box {
        pub x0: f32,
        pub y0: f32,
        pub x1: f32,
        pub y1: f32,
        pub page: i32,
        pub href: *mut c_char,
    }

    #[repr(C)]
    pub struct kaia_cre_range {
        pub start: *mut c_char,
        pub end: *mut c_char,
    }

    extern "C" {
        pub fn kaia_cre_last_error() -> *const c_char;
        pub fn kaia_cre_init(
            font_files: *const *const c_char,
            font_count: usize,
            hyph_dir: *const c_char,
            registered: *mut c_int,
        ) -> c_int;
        pub fn kaia_cre_doc_new(width: c_int, height: c_int, out: *mut *mut kaia_cre_doc) -> c_int;
        pub fn kaia_cre_doc_free(doc: *mut kaia_cre_doc);
        pub fn kaia_cre_doc_load(doc: *mut kaia_cre_doc, path: *const c_char, loaded: *mut c_int) -> c_int;
        pub fn kaia_cre_doc_format(doc: *mut kaia_cre_doc, out: *mut i32) -> c_int;
        pub fn kaia_cre_set_size(doc: *mut kaia_cre_doc, width: c_int, height: c_int) -> c_int;
        pub fn kaia_cre_set_font_size(doc: *mut kaia_cre_doc, px: c_int) -> c_int;
        pub fn kaia_cre_set_font_face(doc: *mut kaia_cre_doc, face: *const c_char) -> c_int;
        pub fn kaia_cre_set_interline_percent(doc: *mut kaia_cre_doc, percent: c_int) -> c_int;
        pub fn kaia_cre_set_margins(
            doc: *mut kaia_cre_doc,
            left: c_int,
            top: c_int,
            right: c_int,
            bottom: c_int,
        ) -> c_int;
        pub fn kaia_cre_set_embedded_styles(doc: *mut kaia_cre_doc, enabled: c_int) -> c_int;
        pub fn kaia_cre_set_embedded_fonts(doc: *mut kaia_cre_doc, enabled: c_int) -> c_int;
        pub fn kaia_cre_set_hyphenation(doc: *mut kaia_cre_doc, dict: *const c_char) -> c_int;
        pub fn kaia_cre_set_stylesheet(
            doc: *mut kaia_cre_doc,
            css_file: *const c_char,
            css_extra: *const c_char,
        ) -> c_int;
        pub fn kaia_cre_render(
            doc: *mut kaia_cre_doc,
            width: c_int,
            height: c_int,
            color: c_int,
            out: *mut u8,
            out_len: usize,
        ) -> c_int;
        pub fn kaia_cre_page_count(doc: *mut kaia_cre_doc, out: *mut i32) -> c_int;
        pub fn kaia_cre_goto_page(doc: *mut kaia_cre_doc, page: i32) -> c_int;
        pub fn kaia_cre_current_page(doc: *mut kaia_cre_doc, out: *mut i32) -> c_int;
        pub fn kaia_cre_page_xpointer(doc: *mut kaia_cre_doc, page: i32, out: *mut *mut c_char) -> c_int;
        pub fn kaia_cre_xpointer_page(
            doc: *mut kaia_cre_doc,
            xpointer: *const c_char,
            out: *mut i32,
        ) -> c_int;
        pub fn kaia_cre_xpointer_at(doc: *mut kaia_cre_doc, x: i32, y: i32, out: *mut *mut c_char) -> c_int;
        pub fn kaia_cre_page_links(
            doc: *mut kaia_cre_doc,
            out: *mut *mut kaia_cre_link_box,
            count: *mut usize,
        ) -> c_int;
        pub fn kaia_cre_free_link_boxes(boxes: *mut kaia_cre_link_box, count: usize);
        pub fn kaia_cre_toc(
            doc: *mut kaia_cre_doc,
            out: *mut *mut kaia_cre_toc_entry,
            count: *mut usize,
        ) -> c_int;
        pub fn kaia_cre_free_toc(entries: *mut kaia_cre_toc_entry, count: usize);
        pub fn kaia_cre_char_boxes(
            doc: *mut kaia_cre_doc,
            xp0: *const c_char,
            xp1: *const c_char,
            out: *mut *mut kaia_cre_char_box,
            count: *mut usize,
        ) -> c_int;
        pub fn kaia_cre_free_boxes(boxes: *mut kaia_cre_char_box);
        pub fn kaia_cre_search(
            doc: *mut kaia_cre_doc,
            pattern: *const c_char,
            case_insensitive: c_int,
            max_hits: c_int,
            out: *mut *mut kaia_cre_range,
            count: *mut usize,
        ) -> c_int;
        pub fn kaia_cre_free_ranges(ranges: *mut kaia_cre_range, count: usize);
        pub fn kaia_cre_metadata(
            doc: *mut kaia_cre_doc,
            title: *mut *mut c_char,
            authors: *mut *mut c_char,
            language: *mut *mut c_char,
        ) -> c_int;
        pub fn kaia_cre_cover(doc: *mut kaia_cre_doc, out: *mut *mut u8, len: *mut usize) -> c_int;
        pub fn kaia_cre_free(ptr: *mut std::ffi::c_void);
    }
}

/// An error from the CREngine shim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// CREngine or the shim failed. The message comes from the shim.
    Engine(String),
    /// A path or string contains a NUL byte or is not valid for the engine.
    BadInput(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Engine(msg) => write!(f, "CREngine: {msg}"),
            Error::BadInput(msg) => write!(f, "CREngine input: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Reads the calling thread's last shim error.
fn last_error() -> String {
    // SAFETY: kaia_cre_last_error returns a pointer to a thread-local string
    // that stays valid until the next shim call on this thread. We copy it
    // at once.
    unsafe {
        let p = ffi::kaia_cre_last_error();
        if p.is_null() {
            return "unknown error".to_string();
        }
        CStr::from_ptr(p).to_string_lossy().into_owned()
    }
}

/// Converts a shim return code into a result.
fn check(code: c_int) -> Result<()> {
    if code == ffi::KAIA_CRE_OK {
        Ok(())
    } else {
        Err(Error::Engine(last_error()))
    }
}

fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::BadInput(format!("contains a NUL byte: {s:?}")))
}

fn path_cstring(path: &Path) -> Result<CString> {
    let s = path
        .to_str()
        .ok_or_else(|| Error::BadInput(format!("path is not UTF-8: {}", path.display())))?;
    cstring(s)
}

/// Takes ownership of a C string from the shim and frees it.
///
/// # Safety
/// `p` must be a non-null string allocated by the shim with malloc.
unsafe fn take_string(p: *mut c_char) -> String {
    let s = CStr::from_ptr(p).to_string_lossy().into_owned();
    ffi::kaia_cre_free(p.cast());
    s
}

/// Takes ownership of an optional C string. Null maps to None.
///
/// # Safety
/// `p` must be null or a string allocated by the shim with malloc.
unsafe fn take_opt_string(p: *mut c_char) -> Option<String> {
    if p.is_null() {
        None
    } else {
        let s = take_string(p);
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    }
}

/// Registers font files and sets the hyphenation directory. Only the first call
/// does work. Returns the number of fonts CREngine accepted.
pub fn init(fonts: &[PathBuf], hyph_dir: &Path) -> Result<usize> {
    let owned: Vec<CString> = fonts.iter().map(|p| path_cstring(p)).collect::<Result<_>>()?;
    let ptrs: Vec<*const c_char> = owned.iter().map(|c| c.as_ptr()).collect();
    let hyph = path_cstring(hyph_dir)?;
    let mut registered: c_int = 0;
    // SAFETY: `ptrs` holds `owned.len()` valid NUL-terminated strings that
    // outlive the call. `hyph` outlives the call. `registered` is a valid out
    // pointer.
    let code = unsafe { ffi::kaia_cre_init(ptrs.as_ptr(), ptrs.len(), hyph.as_ptr(), &mut registered) };
    check(code)?;
    Ok(registered.max(0) as usize)
}

/// CREngine's `doc_format_t` values that this crate names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    None,
    Fb2,
    Txt,
    Rtf,
    Epub,
    Html,
    Chm,
    Doc,
    /// Any other value. Carries the raw number.
    Other(i32),
}

impl DocFormat {
    fn from_raw(v: i32) -> DocFormat {
        match v {
            0 => DocFormat::None,
            1 => DocFormat::Fb2,
            3 => DocFormat::Txt,
            4 => DocFormat::Rtf,
            5 => DocFormat::Epub,
            6 => DocFormat::Html,
            8 => DocFormat::Chm,
            9 => DocFormat::Doc,
            other => DocFormat::Other(other),
        }
    }
}

/// One rectangle of a link on the page on screen.
#[derive(Debug, Clone, PartialEq)]
pub struct LinkBox {
    /// Left, top, right and bottom.
    pub rect: [f32; 4],
    /// The target page of an internal link that resolves.
    pub page: Option<u32>,
    /// The link target as written in the book.
    pub href: String,
}

/// One flattened table of contents entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocItem {
    pub level: u32,
    pub page: Option<u32>,
    pub title: String,
    pub xpointer: String,
}

/// One character of the page window with its box in view pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharBox {
    /// Left, top, right and bottom.
    pub rect: [f32; 4],
    pub line: i32,
    pub ch: char,
}

/// A search hit as two xpointers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub start: String,
    pub end: String,
}

/// Metadata strings. Empty values are None.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    pub title: Option<String>,
    pub authors: Option<String>,
    pub language: Option<String>,
}

/// One open CREngine document with its own view.
pub struct Doc {
    ptr: ptr::NonNull<ffi::kaia_cre_doc>,
}

impl Doc {
    /// Creates a view of the given size. Load a file before using it. Fails
    /// until [`init`] has succeeded.
    pub fn new(width: u32, height: u32) -> Result<Doc> {
        let w = c_int::try_from(width).map_err(|_| Error::BadInput("width too large".into()))?;
        let h = c_int::try_from(height).map_err(|_| Error::BadInput("height too large".into()))?;
        let mut out: *mut ffi::kaia_cre_doc = ptr::null_mut();
        // SAFETY: `out` is a valid out pointer. On success the shim stores a
        // handle that we own until Drop.
        check(unsafe { ffi::kaia_cre_doc_new(w, h, &mut out) })?;
        let ptr =
            ptr::NonNull::new(out).ok_or_else(|| Error::Engine("shim returned a null handle".into()))?;
        Ok(Doc { ptr })
    }

    fn raw(&self) -> *mut ffi::kaia_cre_doc {
        self.ptr.as_ptr()
    }

    /// Loads a file. Returns Ok(false) when CREngine rejects it, and Err when
    /// the shim itself fails.
    pub fn load(&mut self, path: &Path) -> Result<bool> {
        let p = path_cstring(path)?;
        let mut loaded: c_int = 0;
        // SAFETY: `self.raw()` is a live handle. `p` is a valid C string for
        // the call. `loaded` is a valid out pointer.
        check(unsafe { ffi::kaia_cre_doc_load(self.raw(), p.as_ptr(), &mut loaded) })?;
        Ok(loaded != 0)
    }

    pub fn set_size(&mut self, width: u32, height: u32) -> Result<()> {
        // SAFETY: live handle, plain integer arguments.
        check(unsafe { ffi::kaia_cre_set_size(self.raw(), width as c_int, height as c_int) })
    }

    pub fn set_font_size(&mut self, px: u32) -> Result<()> {
        // SAFETY: live handle, plain integer argument.
        check(unsafe { ffi::kaia_cre_set_font_size(self.raw(), px as c_int) })
    }

    pub fn set_font_face(&mut self, face: &str) -> Result<()> {
        let f = cstring(face)?;
        // SAFETY: live handle; `f` is a valid C string for the call.
        check(unsafe { ffi::kaia_cre_set_font_face(self.raw(), f.as_ptr()) })
    }

    /// Line spacing in percent of the font size. 100 is single spacing.
    pub fn set_interline_percent(&mut self, percent: u32) -> Result<()> {
        // SAFETY: live handle, plain integer argument.
        check(unsafe { ffi::kaia_cre_set_interline_percent(self.raw(), percent as c_int) })
    }

    pub fn set_margins(&mut self, left: u32, top: u32, right: u32, bottom: u32) -> Result<()> {
        // SAFETY: live handle, plain integer arguments.
        check(unsafe {
            ffi::kaia_cre_set_margins(
                self.raw(),
                left as c_int,
                top as c_int,
                right as c_int,
                bottom as c_int,
            )
        })
    }

    /// The format CREngine detected for the loaded file. A file it cannot
    /// identify is read as plain text, so check this after loading.
    pub fn doc_format(&mut self) -> Result<DocFormat> {
        let mut raw: i32 = 0;
        // SAFETY: live handle; `raw` is a valid out pointer.
        check(unsafe { ffi::kaia_cre_doc_format(self.raw(), &mut raw) })?;
        Ok(DocFormat::from_raw(raw))
    }

    /// Turns the book's own styles on or off.
    pub fn set_embedded_styles(&mut self, enabled: bool) -> Result<()> {
        // SAFETY: live handle, plain integer argument.
        check(unsafe { ffi::kaia_cre_set_embedded_styles(self.raw(), enabled as c_int) })
    }

    /// Turns the book's embedded fonts on or off.
    pub fn set_embedded_fonts(&mut self, enabled: bool) -> Result<()> {
        // SAFETY: live handle, plain integer argument.
        check(unsafe { ffi::kaia_cre_set_embedded_fonts(self.raw(), enabled as c_int) })
    }

    /// Selects a hyphenation dictionary by id, such as `English_US.pattern`.
    /// None turns hyphenation off.
    pub fn set_hyphenation(&mut self, dict: Option<&str>) -> Result<()> {
        let d = dict.map(cstring).transpose()?;
        let p = d.as_ref().map_or(ptr::null(), |c| c.as_ptr());
        // SAFETY: live handle; `p` is null or a valid C string that lives for the call.
        check(unsafe { ffi::kaia_cre_set_hyphenation(self.raw(), p) })
    }

    /// Sets the user style sheet. `css_file` is read from disk when given.
    /// `css_extra` is appended after it.
    pub fn set_stylesheet(&mut self, css_file: Option<&Path>, css_extra: Option<&str>) -> Result<()> {
        let f = css_file.map(path_cstring).transpose()?;
        let e = css_extra.map(cstring).transpose()?;
        let fp = f.as_ref().map_or(ptr::null(), |c| c.as_ptr());
        let ep = e.as_ref().map_or(ptr::null(), |c| c.as_ptr());
        // SAFETY: live handle; both pointers are null or valid C strings for the call.
        check(unsafe { ffi::kaia_cre_set_stylesheet(self.raw(), fp, ep) })
    }

    /// Renders the current page. Returns gray8 bytes (`width*height`) or rgb24
    /// bytes (`width*height*3`). Each side is at most [`MAX_RENDER_SIDE`].
    pub fn render(&mut self, width: u32, height: u32, color: bool) -> Result<Vec<u8>> {
        if width == 0 || height == 0 || width > MAX_RENDER_SIDE || height > MAX_RENDER_SIDE {
            return Err(Error::BadInput(format!(
                "render size {width}x{height} is outside 1..={MAX_RENDER_SIDE}"
            )));
        }
        let len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(if color { 3 } else { 1 }))
            .ok_or_else(|| Error::BadInput(format!("render size {width}x{height} overflows")))?;
        let mut buf = vec![0u8; len];
        // SAFETY: `buf` holds `len` bytes, the size the shim checks against.
        // The shim writes at most `len` bytes and returns before we read.
        check(unsafe {
            ffi::kaia_cre_render(
                self.raw(),
                width as c_int,
                height as c_int,
                color as c_int,
                buf.as_mut_ptr(),
                buf.len(),
            )
        })?;
        Ok(buf)
    }

    pub fn page_count(&mut self) -> Result<u32> {
        let mut n: i32 = 0;
        // SAFETY: live handle; `n` is a valid out pointer.
        check(unsafe { ffi::kaia_cre_page_count(self.raw(), &mut n) })?;
        Ok(n.max(0) as u32)
    }

    pub fn goto_page(&mut self, page: u32) -> Result<()> {
        // SAFETY: live handle, plain integer argument.
        check(unsafe { ffi::kaia_cre_goto_page(self.raw(), page as i32) })
    }

    pub fn current_page(&mut self) -> Result<u32> {
        let mut n: i32 = 0;
        // SAFETY: live handle; `n` is a valid out pointer.
        check(unsafe { ffi::kaia_cre_current_page(self.raw(), &mut n) })?;
        Ok(n.max(0) as u32)
    }

    /// The xpointer at the top of a page.
    pub fn page_xpointer(&mut self, page: u32) -> Result<String> {
        let mut out: *mut c_char = ptr::null_mut();
        // SAFETY: live handle; `out` is a valid out pointer. On success the
        // shim allocates the string, which take_string frees.
        let code = unsafe { ffi::kaia_cre_page_xpointer(self.raw(), page as i32, &mut out) };
        match code {
            ffi::KAIA_CRE_OK => {
                // SAFETY: the shim set `out` to a malloc'd string on success.
                Ok(unsafe { take_string(out) })
            }
            ffi::KAIA_CRE_NOT_FOUND => Err(Error::Engine(format!("no xpointer for page {page}"))),
            _ => Err(Error::Engine(last_error())),
        }
    }

    /// The page that holds an xpointer. None when the xpointer is not in the document.
    pub fn xpointer_page(&mut self, xpointer: &str) -> Result<Option<u32>> {
        let x = cstring(xpointer)?;
        let mut page: i32 = 0;
        // SAFETY: live handle; `x` is a valid C string for the call; `page` is a valid out pointer.
        let code = unsafe { ffi::kaia_cre_xpointer_page(self.raw(), x.as_ptr(), &mut page) };
        match code {
            ffi::KAIA_CRE_OK => Ok(Some(page.max(0) as u32)),
            ffi::KAIA_CRE_NOT_FOUND => Ok(None),
            _ => Err(Error::Engine(last_error())),
        }
    }

    /// The xpointer of the node under a view point, or None.
    pub fn xpointer_at(&mut self, x: i32, y: i32) -> Result<Option<String>> {
        let mut out: *mut c_char = ptr::null_mut();
        // SAFETY: live handle; `out` is a valid out pointer.
        let code = unsafe { ffi::kaia_cre_xpointer_at(self.raw(), x, y, &mut out) };
        match code {
            ffi::KAIA_CRE_OK => {
                // SAFETY: the shim set `out` to a malloc'd string on success.
                Ok(Some(unsafe { take_string(out) }))
            }
            ffi::KAIA_CRE_NOT_FOUND => Ok(None),
            _ => Err(Error::Engine(last_error())),
        }
    }

    /// The link rectangles on the page on screen. Call [`Doc::goto_page`] first.
    pub fn page_links(&mut self) -> Result<Vec<LinkBox>> {
        let mut out: *mut ffi::kaia_cre_link_box = ptr::null_mut();
        let mut count: usize = 0;
        // SAFETY: live handle; out pointers are valid.
        check(unsafe { ffi::kaia_cre_page_links(self.raw(), &mut out, &mut count) })?;
        let mut links = Vec::with_capacity(count);
        if !out.is_null() {
            for i in 0..count {
                // SAFETY: the shim allocated `count` boxes at `out`. Each href is
                // detached below, before the array is freed.
                let b = unsafe { &*out.add(i) };
                links.push(LinkBox {
                    rect: [b.x0, b.y0, b.x1, b.y1],
                    page: (b.page >= 0).then_some(b.page as u32),
                    // SAFETY: href is a malloc'd string set by the shim.
                    href: unsafe { take_opt_string(b.href) }.unwrap_or_default(),
                });
            }
            // SAFETY: `out` came from kaia_cre_page_links. Its hrefs were freed
            // above, so only the array is released here.
            unsafe { ffi::kaia_cre_free(out.cast()) };
        }
        Ok(links)
    }

    /// The table of contents, flattened depth first.
    pub fn toc(&mut self) -> Result<Vec<TocItem>> {
        let mut entries: *mut ffi::kaia_cre_toc_entry = ptr::null_mut();
        let mut count: usize = 0;
        // SAFETY: live handle; both out pointers are valid.
        check(unsafe { ffi::kaia_cre_toc(self.raw(), &mut entries, &mut count) })?;
        let mut out = Vec::with_capacity(count);
        if !entries.is_null() {
            for i in 0..count {
                // SAFETY: the shim allocated `count` entries at `entries`, and
                // we read each one before the single free below.
                let e = unsafe { &*entries.add(i) };
                out.push(TocItem {
                    level: e.level.max(0) as u32,
                    page: (e.page >= 0).then_some(e.page as u32),
                    // SAFETY: the shim sets title and xpointer to malloc'd strings.
                    title: unsafe { take_opt_string(e.title) }.unwrap_or_default(),
                    xpointer: unsafe { take_opt_string(e.xpointer) }.unwrap_or_default(),
                });
            }
            // SAFETY: `entries` came from kaia_cre_toc and is freed once. Its
            // strings were detached above, so only the array is released here.
            unsafe { ffi::kaia_cre_free(entries.cast()) };
        }
        Ok(out)
    }

    /// Character boxes for the text between two xpointers, limited to the
    /// page on screen. Call [`Doc::goto_page`] first. `end` None means the end of the document.
    pub fn char_boxes(&mut self, start: &str, end: Option<&str>) -> Result<Vec<CharBox>> {
        let s = cstring(start)?;
        let e = end.map(cstring).transpose()?;
        let ep = e.as_ref().map_or(ptr::null(), |c| c.as_ptr());
        let mut out: *mut ffi::kaia_cre_char_box = ptr::null_mut();
        let mut count: usize = 0;
        // SAFETY: live handle; `s` and `ep` are valid C strings or null for the call; out pointers are valid.
        check(unsafe { ffi::kaia_cre_char_boxes(self.raw(), s.as_ptr(), ep, &mut out, &mut count) })?;
        let mut boxes = Vec::with_capacity(count);
        if !out.is_null() {
            for i in 0..count {
                // SAFETY: the shim allocated `count` boxes at `out`.
                let b = unsafe { *out.add(i) };
                boxes.push(CharBox {
                    rect: [b.x0, b.y0, b.x1, b.y1],
                    line: b.line,
                    ch: char::from_u32(b.codepoint).unwrap_or('\u{fffd}'),
                });
            }
            // SAFETY: `out` came from kaia_cre_char_boxes and is freed once.
            unsafe { ffi::kaia_cre_free_boxes(out) };
        }
        Ok(boxes)
    }

    /// Searches the whole document. Hits come in reading order.
    pub fn search(&mut self, pattern: &str, case_insensitive: bool, max_hits: u32) -> Result<Vec<SearchHit>> {
        let p = cstring(pattern)?;
        let mut out: *mut ffi::kaia_cre_range = ptr::null_mut();
        let mut count: usize = 0;
        // SAFETY: live handle; `p` is a valid C string for the call; out pointers are valid.
        check(unsafe {
            ffi::kaia_cre_search(
                self.raw(),
                p.as_ptr(),
                case_insensitive as c_int,
                max_hits as c_int,
                &mut out,
                &mut count,
            )
        })?;
        let mut hits = Vec::with_capacity(count);
        if !out.is_null() {
            for i in 0..count {
                // SAFETY: the shim allocated `count` ranges at `out`. The strings
                // are detached once each, below.
                let r = unsafe { &*out.add(i) };
                // SAFETY: start and end are malloc'd strings set by the shim.
                let start = unsafe { take_string(r.start) };
                let end = unsafe { take_string(r.end) };
                hits.push(SearchHit { start, end });
            }
            // SAFETY: `out` came from kaia_cre_search. Its strings are already
            // freed, so only the array remains.
            unsafe { ffi::kaia_cre_free(out.cast()) };
        }
        Ok(hits)
    }

    pub fn metadata(&mut self) -> Result<Meta> {
        let mut title: *mut c_char = ptr::null_mut();
        let mut authors: *mut c_char = ptr::null_mut();
        let mut language: *mut c_char = ptr::null_mut();
        // SAFETY: live handle; each out pointer is valid. The shim sets each to
        // null or a malloc'd string.
        check(unsafe { ffi::kaia_cre_metadata(self.raw(), &mut title, &mut authors, &mut language) })?;
        // SAFETY: each pointer is null or a malloc'd string from the shim.
        Ok(Meta {
            title: unsafe { take_opt_string(title) },
            authors: unsafe { take_opt_string(authors) },
            language: unsafe { take_opt_string(language) },
        })
    }

    /// The cover image bytes as stored in the book (JPEG, PNG and so on).
    /// None when the book has no cover.
    pub fn cover_bytes(&mut self) -> Result<Option<Vec<u8>>> {
        let mut out: *mut u8 = ptr::null_mut();
        let mut len: usize = 0;
        // SAFETY: live handle; out pointers are valid.
        let code = unsafe { ffi::kaia_cre_cover(self.raw(), &mut out, &mut len) };
        match code {
            ffi::KAIA_CRE_OK => {
                // SAFETY: on success the shim returns `len` bytes at `out`, malloc'd.
                let bytes = unsafe { std::slice::from_raw_parts(out, len) }.to_vec();
                // SAFETY: `out` came from kaia_cre_cover and is freed once.
                unsafe { ffi::kaia_cre_free(out.cast()) };
                Ok(Some(bytes))
            }
            ffi::KAIA_CRE_NOT_FOUND => Ok(None),
            _ => Err(Error::Engine(last_error())),
        }
    }
}

impl Drop for Doc {
    fn drop(&mut self) {
        // SAFETY: the handle came from kaia_cre_doc_new and is freed once, here.
        unsafe { ffi::kaia_cre_doc_free(self.ptr.as_ptr()) };
    }
}
