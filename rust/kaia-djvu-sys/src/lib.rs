//! Safe access to KOReader's pinned DjVuLibre build.

use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void, CStr, CString};
use std::fmt;
use std::path::Path;
use std::ptr::{self, NonNull};
use std::thread;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(15);
const POLL: Duration = Duration::from_millis(5);
const JOB_OK: c_int = 2;
const JOB_FAILED: c_int = 3;
const MESSAGE_ERROR: c_int = 0;
const DUMMY: usize = 2;
const MAX_EXPR_ITEMS: usize = 100_000;

#[derive(Debug, Clone)]
pub struct Error(pub String);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}

type Expr = *mut c_void;
#[repr(C)]
struct Context {
    _private: [u8; 0],
}
#[repr(C)]
struct RawDocument {
    _private: [u8; 0],
}
#[repr(C)]
struct RawPage {
    _private: [u8; 0],
}
#[repr(C)]
struct RawFormat {
    _private: [u8; 0],
}
#[repr(C)]
struct MessageAny {
    tag: c_int,
    context: *mut Context,
    document: *mut RawDocument,
    page: *mut RawPage,
    job: *mut c_void,
}
#[repr(C)]
struct MessageError {
    any: MessageAny,
    message: *const c_char,
    function: *const c_char,
    filename: *const c_char,
    lineno: c_int,
}
#[repr(C)]
struct PageInfo {
    width: c_int,
    height: c_int,
    dpi: c_int,
    rotation: c_int,
    version: c_int,
}
#[repr(C)]
struct RawRect {
    x: c_int,
    y: c_int,
    w: c_uint,
    h: c_uint,
}

unsafe extern "C" {
    fn ddjvu_context_create(name: *const c_char) -> *mut Context;
    fn ddjvu_context_release(ctx: *mut Context);
    fn ddjvu_document_create_by_filename_utf8(
        ctx: *mut Context,
        path: *const c_char,
        cache: c_int,
    ) -> *mut RawDocument;
    fn ddjvu_document_job(doc: *mut RawDocument) -> *mut c_void;
    fn ddjvu_page_job(page: *mut RawPage) -> *mut c_void;
    fn ddjvu_job_status(job: *mut c_void) -> c_int;
    fn ddjvu_job_release(job: *mut c_void);
    fn ddjvu_document_get_pagenum(doc: *mut RawDocument) -> c_int;
    fn ddjvu_document_get_pageinfo_imp(
        doc: *mut RawDocument,
        page: c_int,
        info: *mut PageInfo,
        size: c_uint,
    ) -> c_int;
    fn ddjvu_page_create_by_pageno(doc: *mut RawDocument, page: c_int) -> *mut RawPage;
    fn ddjvu_format_create(style: c_int, nargs: c_int, args: *mut c_uint) -> *mut RawFormat;
    fn ddjvu_format_set_row_order(format: *mut RawFormat, top: c_int);
    fn ddjvu_format_set_y_direction(format: *mut RawFormat, top: c_int);
    fn ddjvu_format_release(format: *mut RawFormat);
    fn ddjvu_page_render(
        page: *mut RawPage,
        mode: c_int,
        page_rect: *const RawRect,
        render_rect: *const RawRect,
        format: *const RawFormat,
        row_size: c_ulong,
        pixels: *mut c_char,
    ) -> c_int;
    fn ddjvu_message_peek(ctx: *mut Context) -> *mut MessageAny;
    fn ddjvu_message_pop(ctx: *mut Context);
    fn ddjvu_document_get_outline(doc: *mut RawDocument) -> Expr;
    fn ddjvu_document_get_pagetext(doc: *mut RawDocument, page: c_int, detail: *const c_char) -> Expr;
    fn ddjvu_document_get_anno(doc: *mut RawDocument, compat: c_int) -> Expr;
    fn ddjvu_document_get_pageanno(doc: *mut RawDocument, page: c_int) -> Expr;
    fn ddjvu_anno_get_metadata_keys(anno: Expr) -> *mut Expr;
    fn ddjvu_anno_get_metadata(anno: Expr, key: Expr) -> *const c_char;
    fn ddjvu_anno_get_hyperlinks(anno: Expr) -> *mut Expr;
    fn ddjvu_miniexp_release(doc: *mut RawDocument, expr: Expr);
    fn miniexp_length(expr: Expr) -> c_int;
    fn miniexp_nth(index: c_int, expr: Expr) -> Expr;
    fn miniexp_to_str(expr: Expr) -> *const c_char;
    fn miniexp_to_name(expr: Expr) -> *const c_char;
    fn free(ptr: *mut c_void);
}

#[derive(Debug, Clone, Copy)]
pub struct Info {
    pub width: u32,
    pub height: u32,
    pub dpi: u32,
}
#[derive(Debug, Clone)]
pub enum Sexp {
    Nil,
    Number(i32),
    Symbol(String),
    String(String),
    List(Vec<Sexp>),
}

pub struct Document {
    ctx: NonNull<Context>,
    raw: NonNull<RawDocument>,
    count: u32,
}
impl Document {
    pub fn open(path: &Path) -> Result<Self, Error> {
        let path = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| Error("path contains a null byte".into()))?;
        // SAFETY: The static name is null terminated and the context is owned below.
        let ctx = NonNull::new(unsafe { ddjvu_context_create(c"kaia-reader".as_ptr()) })
            .ok_or_else(|| Error("cannot create DjVu context".into()))?;
        // SAFETY: Both pointers remain valid during the call. DjVuLibre copies the path.
        let raw =
            NonNull::new(unsafe { ddjvu_document_create_by_filename_utf8(ctx.as_ptr(), path.as_ptr(), 1) });
        let Some(raw) = raw else {
            let error = drain(ctx.as_ptr())
                .err()
                .unwrap_or_else(|| Error("cannot open DjVu file".into()));
            // SAFETY: ctx was created above and has no owned document.
            unsafe { ddjvu_context_release(ctx.as_ptr()) };
            return Err(error);
        };
        let mut doc = Self { ctx, raw, count: 0 };
        doc.wait_for(|| {
            // SAFETY: The document pointer remains valid until doc drops.
            unsafe { ddjvu_job_status(ddjvu_document_job(doc.raw.as_ptr())) }
        })?;
        // SAFETY: Header decoding finished, and the document remains alive.
        let count = unsafe { ddjvu_document_get_pagenum(raw.as_ptr()) };
        if count <= 0 {
            return Err(Error("DjVu document has no pages".into()));
        }
        doc.count = count as u32;
        Ok(doc)
    }
    pub fn page_count(&self) -> u32 {
        self.count
    }
    fn check_page(&self, page: u32) -> Result<c_int, Error> {
        if page >= self.count {
            Err(Error(format!("page {page} is outside the document")))
        } else {
            Ok(page as c_int)
        }
    }
    fn wait_for(&self, mut status: impl FnMut() -> c_int) -> Result<(), Error> {
        let start = Instant::now();
        loop {
            drain(self.ctx.as_ptr())?;
            let state = status();
            if state == JOB_OK {
                return Ok(());
            }
            if state >= JOB_FAILED {
                return Err(Error("DjVu decoding failed".into()));
            }
            if start.elapsed() >= TIMEOUT {
                return Err(Error("DjVu decoding timed out".into()));
            }
            thread::sleep(POLL);
        }
    }
    pub fn page_info(&self, page: u32) -> Result<Info, Error> {
        let page = self.check_page(page)?;
        let mut info = PageInfo {
            width: 0,
            height: 0,
            dpi: 0,
            rotation: 0,
            version: 0,
        };
        self.wait_for(|| {
            // SAFETY: The info buffer has the size passed to the API.
            unsafe {
                ddjvu_document_get_pageinfo_imp(
                    self.raw.as_ptr(),
                    page,
                    &mut info,
                    std::mem::size_of::<PageInfo>() as c_uint,
                )
            }
        })?;
        if info.width <= 0 || info.height <= 0 || info.dpi <= 0 {
            return Err(Error("invalid DjVu page size or DPI".into()));
        }
        Ok(Info {
            width: info.width as u32,
            height: info.height as u32,
            dpi: info.dpi as u32,
        })
    }
    pub fn render(
        &self,
        page: u32,
        width: u32,
        height: u32,
        gray: bool,
        clip: [u32; 4],
    ) -> Result<Vec<u8>, Error> {
        let page = self.check_page(page)?;
        let right = clip[0]
            .checked_add(clip[2])
            .ok_or_else(|| Error("clip overflow".into()))?;
        let bottom = clip[1]
            .checked_add(clip[3])
            .ok_or_else(|| Error("clip overflow".into()))?;
        if width == 0
            || height == 0
            || clip[2] == 0
            || clip[3] == 0
            || right > width
            || bottom > height
            || [width, height, right, bottom]
                .iter()
                .any(|n| *n > i32::MAX as u32)
        {
            return Err(Error("invalid render size or clip".into()));
        }
        let bytes = if gray { 1usize } else { 3 };
        let row = (clip[2] as usize)
            .checked_mul(bytes)
            .ok_or_else(|| Error("render size overflow".into()))?;
        let len = row
            .checked_mul(clip[3] as usize)
            .filter(|n| *n <= 512 * 1024 * 1024)
            .ok_or_else(|| Error("render is too large".into()))?;
        // SAFETY: The document is alive, and page is in range.
        let raw_page = NonNull::new(unsafe { ddjvu_page_create_by_pageno(self.raw.as_ptr(), page) })
            .ok_or_else(|| Error("cannot create DjVu page".into()))?;
        let result = (|| {
            self.wait_for(|| {
                // SAFETY: raw_page stays alive until the closure returns.
                unsafe { ddjvu_job_status(ddjvu_page_job(raw_page.as_ptr())) }
            })?;
            // SAFETY: Format style constants match ddjvuapi.h. No arguments are needed.
            let format =
                NonNull::new(unsafe { ddjvu_format_create(if gray { 4 } else { 1 }, 0, ptr::null_mut()) })
                    .ok_or_else(|| Error("cannot create DjVu pixel format".into()))?;
            // SAFETY: format stays valid through render and is released below.
            unsafe {
                ddjvu_format_set_row_order(format.as_ptr(), 1);
                ddjvu_format_set_y_direction(format.as_ptr(), 1);
            }
            let page_rect = RawRect {
                x: 0,
                y: 0,
                w: width,
                h: height,
            };
            let render_rect = RawRect {
                x: clip[0] as i32,
                y: clip[1] as i32,
                w: clip[2],
                h: clip[3],
            };
            let mut pixels = vec![255; len];
            // SAFETY: pixels has row * height bytes and both rectangles are in bounds.
            let ok = unsafe {
                ddjvu_page_render(
                    raw_page.as_ptr(),
                    0,
                    &page_rect,
                    &render_rect,
                    format.as_ptr(),
                    row as c_ulong,
                    pixels.as_mut_ptr().cast(),
                )
            };
            // SAFETY: format was created above and is no longer used.
            unsafe { ddjvu_format_release(format.as_ptr()) };
            if ok == 0 {
                return Err(Error("DjVu page render failed".into()));
            }
            Ok(pixels)
        })();
        // SAFETY: raw_page was created above and no longer has users.
        unsafe { ddjvu_job_release(ddjvu_page_job(raw_page.as_ptr())) };
        result
    }
    fn expression(&self, mut get: impl FnMut() -> Expr) -> Result<Sexp, Error> {
        let start = Instant::now();
        loop {
            drain(self.ctx.as_ptr())?;
            let expr = get();
            if expr as usize != DUMMY {
                let value = parse(expr, 0);
                if !expr.is_null() {
                    // SAFETY: expr came from this document and is no longer referenced.
                    unsafe { ddjvu_miniexp_release(self.raw.as_ptr(), expr) };
                }
                return value;
            }
            if start.elapsed() >= TIMEOUT {
                return Err(Error("DjVu data timed out".into()));
            }
            thread::sleep(POLL);
        }
    }
    pub fn outline(&self) -> Result<Sexp, Error> {
        self.expression(|| {
            // SAFETY: The document remains alive while the expression is parsed.
            unsafe { ddjvu_document_get_outline(self.raw.as_ptr()) }
        })
    }
    pub fn page_text(&self, page: u32) -> Result<Sexp, Error> {
        let page = self.check_page(page)?;
        self.expression(|| {
            // SAFETY: page is in range and the document remains alive.
            unsafe { ddjvu_document_get_pagetext(self.raw.as_ptr(), page, c"word".as_ptr()) }
        })
    }
    pub fn metadata(&self) -> Result<Vec<(String, String)>, Error> {
        let mut out = Vec::new();
        let _ = self.expression(|| {
            // SAFETY: The document remains alive while the callback reads metadata.
            let expr = unsafe { ddjvu_document_get_anno(self.raw.as_ptr(), 1) };
            if expr as usize != DUMMY && !expr.is_null() {
                // SAFETY: DjVuLibre returns a null terminated array, freed below.
                let keys = unsafe { ddjvu_anno_get_metadata_keys(expr) };
                if !keys.is_null() {
                    for index in 0..MAX_EXPR_ITEMS {
                        // SAFETY: The array ends at a null expression.
                        let key = unsafe { *keys.add(index) };
                        if key.is_null() {
                            break;
                        }
                        // SAFETY: key and expr remain alive through string copies.
                        let name = unsafe { copy_cstr(miniexp_to_name(key)) };
                        let value = unsafe { copy_cstr(ddjvu_anno_get_metadata(expr, key)) };
                        if let (Some(name), Some(value)) = (name, value) {
                            out.push((name, value));
                        }
                    }
                    // SAFETY: keys was allocated by DjVuLibre with malloc.
                    unsafe { free(keys.cast()) };
                }
            }
            expr
        })?;
        Ok(out)
    }
    pub fn page_links(&self, page: u32) -> Result<Vec<Sexp>, Error> {
        let page = self.check_page(page)?;
        let mut links = Vec::new();
        let mut parse_error = None;
        let _ = self.expression(|| {
            // SAFETY: page is in range and the document remains alive.
            let expr = unsafe { ddjvu_document_get_pageanno(self.raw.as_ptr(), page) };
            if expr as usize != DUMMY && !expr.is_null() {
                // SAFETY: The array is null terminated and stays valid through parsing.
                let areas = unsafe { ddjvu_anno_get_hyperlinks(expr) };
                if !areas.is_null() {
                    for index in 0..MAX_EXPR_ITEMS {
                        // SAFETY: The array ends at a null expression.
                        let area = unsafe { *areas.add(index) };
                        if area.is_null() {
                            break;
                        }
                        match parse(area, 0) {
                            Ok(link) => links.push(link),
                            Err(error) => {
                                parse_error = Some(error);
                                break;
                            }
                        }
                    }
                    // SAFETY: areas was allocated by DjVuLibre with malloc.
                    unsafe { free(areas.cast()) };
                }
            }
            expr
        })?;
        if let Some(error) = parse_error {
            return Err(error);
        }
        Ok(links)
    }
}
impl Drop for Document {
    fn drop(&mut self) {
        // SAFETY: These owned pointers are valid and no page remains alive.
        unsafe {
            ddjvu_job_release(ddjvu_document_job(self.raw.as_ptr()));
            ddjvu_context_release(self.ctx.as_ptr());
        }
    }
}
fn drain(ctx: *mut Context) -> Result<(), Error> {
    // SAFETY: ctx is live and each message remains valid until it is popped.
    unsafe {
        for _ in 0..MAX_EXPR_ITEMS {
            let Some(message) = NonNull::new(ddjvu_message_peek(ctx)) else {
                return Ok(());
            };
            let error = if message.as_ref().tag == MESSAGE_ERROR {
                let detail = &*message.as_ptr().cast::<MessageError>();
                Some(Error(
                    copy_cstr(detail.message).unwrap_or_else(|| "DjVu decoding failed".into()),
                ))
            } else {
                None
            };
            ddjvu_message_pop(ctx);
            if let Some(error) = error {
                return Err(error);
            }
        }
    }
    Err(Error("DjVu message queue exceeded limit".into()))
}
fn parse(expr: Expr, depth: usize) -> Result<Sexp, Error> {
    if expr.is_null() {
        return Ok(Sexp::Nil);
    }
    if depth > 64 {
        return Err(Error("DjVu expression is too deep".into()));
    }
    let tag = (expr as usize) & 3;
    if tag == 3 {
        return Ok(Sexp::Number((expr as isize >> 2) as i32));
    }
    if tag == 2 {
        // SAFETY: The tag marks a symbol and the returned name is owned by DjVuLibre.
        return Ok(Sexp::Symbol(
            unsafe { copy_cstr(miniexp_to_name(expr)) }.unwrap_or_default(),
        ));
    }
    // SAFETY: miniexp_to_str checks the object type before returning a pointer.
    if let Some(s) = unsafe { copy_cstr(miniexp_to_str(expr)) } {
        return Ok(Sexp::String(s));
    }
    // SAFETY: miniexp_length accepts any expression and returns zero for non lists.
    let len = unsafe { miniexp_length(expr) };
    if len < 0 || len as usize > MAX_EXPR_ITEMS {
        return Err(Error("invalid DjVu expression length".into()));
    }
    let mut items = Vec::with_capacity(len as usize);
    for index in 0..len {
        // SAFETY: index is within the reported list length.
        items.push(parse(unsafe { miniexp_nth(index, expr) }, depth + 1)?);
    }
    Ok(Sexp::List(items))
}
unsafe fn copy_cstr(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        // SAFETY: DjVuLibre returns null terminated strings valid during this call.
        Some(unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned())
    }
}
