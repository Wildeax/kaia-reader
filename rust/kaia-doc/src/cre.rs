//! CREngine backend: KOReader's typography for reflowable books.
//!
//! The engine is reached through `kaia-cre-sys`, which owns all `unsafe`
//! code. This module maps the engine calls onto the [`Document`] trait. The
//! calls follow `base/cre.cpp` and `frontend/document/credocument.lua`.
//!
//! Units: a layout's width, height and em are device pixels for CREngine, and
//! page sizes reported by this backend use the same numbers. Rendering at a
//! scale other than 1 renders at the layout size and resamples the result.
//! Line positions returned by `page_text` and `search` are in those units.
//!
//! The default pixel layout is Noto Serif at 24 px, 100% line spacing, engine
//! margins of 12 px left/right and 6 px top/bottom, CSS DPI 96, embedded styles
//! and fonts enabled, and hyphenation disabled. The app supplies its own layout
//! and hyphenation choice. KOReader's frontend scales its 22-unit font and
//! 10/15-unit margins with Screen:scaleBySize before calling the engine. That
//! gives 47 px text and 22/32 px margins at 1264x1680, before footer space.
//!
//! The stock constructor alone uses Arial, DejaVu Sans and an engine header.
//! The automated comparison in tools/kaia/cre_compare.py applies matched
//! reading layouts to this backend and KOReader's Lua wrapper. It records
//! page counts, DOM text between page anchors and grayscale renders.
//!
//! Threads: CREngine keeps the hyphenation dictionary and the preferred font
//! in process-wide state, and the shim's lock covers only one call. Two
//! threads that each open documents (the library scan and the reader) could
//! change each other's settings between calls. So [`ENGINE`] is held for each
//! whole document operation: [`CreDocument::doc_mut`] takes it, applies this
//! document's hyphenation dictionary and font face, and the caller then goes
//! to a page and renders or extracts under the same guard. Every operation
//! applies the settings again, so a document never sees another's.
//!
//! Live documents: CREngine's document table holds 16 entries, and a 17th
//! document corrupts memory. The shim caps open documents below that and makes
//! [`CreDocument::open`] wait for one to close. The wait happens outside
//! [`ENGINE`]. It fails only when no document closes for 10 seconds.
//!
//! Locations: `Location::Page` and `Location::Engine` (an xpointer) both
//! resolve. Internal links resolve to pages when the target id exists.

use std::cell::RefCell;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use kaia_cre_sys::{CharBox, Doc, DocFormat, Meta};
use kaia_gfx::image::scale_pixmap;
use kaia_gfx::{PixelFormat, Pixmap, Rect, Size};

use crate::document::Document;
use crate::error::{DocError, Result};
use crate::types::{
    Layout, Link, LinkTarget, Location, Metadata, PageSize, PageText, PointF, Quad, RectF, TextChar,
    TextLine, TocEntry,
};

/// Page size before a layout is applied. Matches the MuPDF backend's tests.
const DEFAULT_WIDTH: u32 = 1264;
const DEFAULT_HEIGHT: u32 = 1680;
const DEFAULT_EM: f32 = 24.0;
/// The face KOReader uses when a book does not name one.
const DEFAULT_FONT: &str = "Noto Serif";
/// The base font weight of regular text, in CSS units.
const REGULAR_FONT_WEIGHT: u32 = 400;
/// Hits returned by one search call.
const SEARCH_MAX_HITS: u32 = 500;

/// The kind of book. The kind picks the stylesheet, as in credocument.lua.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Epub,
    Fb2,
    Html,
    Txt,
    Rtf,
    Doc,
    Chm,
}

impl Kind {
    /// Maps a file extension, without the dot and in any case.
    pub fn from_extension(ext: &str) -> Option<Kind> {
        match ext.to_ascii_lowercase().as_str() {
            "epub" => Some(Kind::Epub),
            "fb2" => Some(Kind::Fb2),
            "htm" | "html" | "xhtml" => Some(Kind::Html),
            "txt" => Some(Kind::Txt),
            "rtf" => Some(Kind::Rtf),
            "doc" => Some(Kind::Doc),
            "chm" => Some(Kind::Chm),
            _ => None,
        }
    }

    /// True when CREngine's detected format is one this kind can be.
    fn accepts(self, found: DocFormat) -> bool {
        match self {
            Kind::Epub => found == DocFormat::Epub,
            Kind::Fb2 => found == DocFormat::Fb2,
            Kind::Html => found == DocFormat::Html,
            Kind::Txt => matches!(found, DocFormat::Txt | DocFormat::Html),
            Kind::Rtf => found == DocFormat::Rtf,
            Kind::Doc => matches!(found, DocFormat::Doc | DocFormat::Rtf),
            Kind::Chm => found == DocFormat::Chm,
        }
    }

    /// The stylesheet KOReader applies by default. FB2 and text files use
    /// fb2.css. Everything else uses epub.css.
    fn default_css(self) -> &'static str {
        match self {
            Kind::Fb2 | Kind::Txt => "fb2.css",
            _ => "epub.css",
        }
    }
}

/// Held for each whole document operation. See the module docs.
static ENGINE: Mutex<()> = Mutex::new(());

/// Takes [`ENGINE`]. A panic in another thread leaves no half-applied state
/// that matters here, because every operation applies its settings first.
fn engine_lock() -> MutexGuard<'static, ()> {
    ENGINE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One document operation: the engine lock and the document's handle, with
/// this document's process-wide settings applied. Dropping it releases both.
struct Session<'a> {
    // Declared before the lock so the handle is released first.
    doc: std::cell::RefMut<'a, Doc>,
    _engine: MutexGuard<'static, ()>,
}

impl Deref for Session<'_> {
    type Target = Doc;
    fn deref(&self) -> &Doc {
        &self.doc
    }
}

impl DerefMut for Session<'_> {
    fn deref_mut(&mut self) -> &mut Doc {
        &mut self.doc
    }
}

/// Runtime resource folders for the CREngine backend.
pub use kaia_cre_sys::RuntimeOptions as CreOptions;

/// Sets the user's font folder. Kaia calls it once at start, with `<data>/fonts`.
/// Its fonts join the built-in ones from the next engine start or rescan.
pub use kaia_cre_sys::set_user_font_dir;

/// Called while ENGINE is held. Cache only successful initialization so callers
/// can correct missing resource paths and retry.
fn engine_ready(options: &CreOptions) -> Result<()> {
    ready_state(options, false)
}

/// Initializes the engine with `options`. With `force`, the engine registers the
/// fonts in the options again, which adds the files that appeared since the last call.
fn ready_state(options: &CreOptions, force: bool) -> Result<()> {
    static READY: Mutex<Option<CreOptions>> = Mutex::new(None);
    let mut ready = READY.lock().unwrap_or_else(|p| p.into_inner());
    if force || ready.as_ref() != Some(options) {
        kaia_cre_sys::init_with_options(options).map_err(engine_err)?;
        *ready = Some(options.clone());
    }
    Ok(())
}

/// Registers the fonts in the user folder that CREngine has not seen yet. Call
/// it when a list of faces opens. CREngine cannot unregister a font, so a file
/// deleted from the folder stays listed until Kaia restarts.
pub fn rescan_fonts() -> Result<()> {
    let _engine = engine_lock();
    ready_state(&CreOptions::default(), true)
}

/// The font families that CREngine registered from the user folder, sorted by
/// name. A family counts when its regular face is a file in that folder, so a
/// copy of a built-in face can appear here. Empty when no user folder is set or
/// CREngine does not start.
pub fn user_font_faces() -> Vec<String> {
    let Some(dir) = kaia_cre_sys::user_font_dir() else {
        return Vec::new();
    };
    let _engine = engine_lock();
    if engine_ready(&CreOptions::default()).is_err() {
        return Vec::new();
    }
    let Ok(fonts) = kaia_cre_sys::registered_fonts() else {
        return Vec::new();
    };
    let mut names: Vec<String> = fonts
        .into_iter()
        .filter(|f| f.file.starts_with(&dir))
        .map(|f| f.family)
        .collect();
    names.sort_by_key(|name| (name.to_lowercase(), name.clone()));
    names.dedup();
    names
}

/// The installed data directory, with a desktop build tree fallback.
pub fn data_dir() -> PathBuf {
    CreOptions::default().data_dir
}

fn engine_err(e: kaia_cre_sys::Error) -> DocError {
    DocError::Engine(e.to_string())
}

/// An open CREngine document.
pub struct CreDocument {
    doc: RefCell<Doc>,
    options: Box<CreOptions>,
    kind: Kind,
    path: PathBuf,
    /// The layout last applied to the engine. None until the first `set_layout`.
    applied: Option<Layout>,
    /// The hyphenation dictionary this document uses. CREngine keeps the
    /// active dictionary process-wide, so `doc_mut` applies it before work.
    hyphenation: Option<String>,
    /// The font face this document uses. CREngine keeps the preferred font
    /// process-wide, so `doc_mut` applies it before work.
    font_face: String,
    /// The base font weight, in CSS units. CREngine keeps it process-wide too,
    /// so `doc_mut` applies it before work.
    font_weight: u32,
}

impl CreDocument {
    /// Opens a file. The kind comes from the extension and defaults to EPUB.
    /// Fails with an engine error when CREngine cannot read the file.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_options(path, &CreOptions::default())
    }

    /// Opens a file with runtime font and data folders.
    pub fn open_with_options(path: &Path, options: &CreOptions) -> Result<Self> {
        let kind = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(Kind::from_extension)
            .unwrap_or(Kind::Epub);
        Self::open_as_with_options(path, kind, options)
    }

    /// Opens a file as the given kind. CREngine must read it as a format that
    /// kind accepts.
    pub fn open_as(path: &Path, kind: Kind) -> Result<Self> {
        Self::open_as_with_options(path, kind, &CreOptions::default())
    }

    /// Opens a known format with runtime font and data folders.
    pub fn open_as_with_options(path: &Path, kind: Kind, options: &CreOptions) -> Result<Self> {
        {
            let _engine = engine_lock();
            engine_ready(options)?;
        }
        // Admission may wait for another thread to drop a document. Do not
        // hold ENGINE here: that thread needs it to finish its extraction.
        // The shim locks construction itself and reserves temporary DOM slots.
        let mut doc = Doc::new(DEFAULT_WIDTH, DEFAULT_HEIGHT).map_err(engine_err)?;
        let doc = {
            // Loading can lay the book out, so it runs under the engine lock
            // with this document's settings. Restore options after admission,
            // since another thread may have initialized different resources.
            let _engine = engine_lock();
            engine_ready(options)?;
            doc.set_hyphenation(None).map_err(engine_err)?;
            if !doc.load(path).map_err(engine_err)? {
                return Err(DocError::Engine(format!(
                    "CREngine cannot read {}",
                    path.display()
                )));
            }
            // CREngine reads unidentified bytes as plain text. A file whose
            // extension names another format is damaged, so refuse it.
            let found = doc.doc_format().map_err(engine_err)?;
            if !kind.accepts(found) {
                return Err(DocError::Engine(format!(
                    "CREngine read {} as {found:?}, not as {kind:?}",
                    path.display()
                )));
            }
            doc.set_font_face(DEFAULT_FONT).map_err(engine_err)?;
            doc
        };
        let mut this = Self {
            doc: RefCell::new(doc),
            options: Box::new(options.clone()),
            kind,
            path: path.to_path_buf(),
            applied: None,
            hyphenation: None,
            font_face: DEFAULT_FONT.to_string(),
            font_weight: REGULAR_FONT_WEIGHT,
        };
        let default = Layout::new(DEFAULT_WIDTH as f32, DEFAULT_HEIGHT as f32, DEFAULT_EM);
        this.set_layout(&default)?;
        Ok(this)
    }

    /// Starts one operation: takes the engine lock, then applies this
    /// document's hyphenation dictionary and font face. The engine re-lays
    /// out only when one of them differs from the last call. Hold the result
    /// for the whole operation and do not call another `doc_mut` meanwhile:
    /// the lock is not reentrant.
    fn doc_mut(&self) -> Result<Session<'_>> {
        let engine = engine_lock();
        engine_ready(&self.options)?;
        let mut doc = self.doc.borrow_mut();
        doc.set_hyphenation(self.hyphenation.as_deref())
            .map_err(engine_err)?;
        doc.set_font_face(&self.font_face).map_err(engine_err)?;
        doc.set_font_weight(self.font_weight).map_err(engine_err)?;
        Ok(Session { doc, _engine: engine })
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Selects a hyphenation dictionary, such as `English_US.pattern`. None turns hyphenation off.
    pub fn set_hyphenation(&mut self, dict: Option<&str>) -> Result<()> {
        self.hyphenation = dict.map(str::to_string);
        self.doc_mut().map(drop)
    }

    /// Sets the base font weight in CSS units. 400 is regular and 700 is bold.
    /// Weights between faces are synthesized, so any value from 1 to 999 works.
    /// The engine lays the book out again when the weight changes.
    pub fn set_font_weight(&mut self, weight: u32) -> Result<()> {
        if !(1..=999).contains(&weight) {
            return Err(DocError::InvalidArgument(format!(
                "font weight {weight} is outside 1 to 999"
            )));
        }
        self.font_weight = weight;
        self.doc_mut().map(drop)
    }

    /// Turns the book's own CSS on or off. Off ignores the book's styles.
    pub fn set_embedded_styles(&mut self, enabled: bool) -> Result<()> {
        self.doc_mut()?.set_embedded_styles(enabled).map_err(engine_err)
    }

    /// DOM text between anchors, with block separators as newlines. This follows
    /// KOReader's getTextFromXPointers; page_text instead returns visual lines.
    pub fn range_text(&self, start: &str, end: &str) -> Result<String> {
        self.doc_mut()?.range_text(start, end).map_err(engine_err)
    }

    /// KOReader text queries used by reader plugins. Coordinates are page pixels.
    pub fn plugin_query(&self, method: &str, args: &[String], page: u32) -> Result<kaia_events::Value> {
        use kaia_events::{Key, Value};
        let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
        let string = |s: Option<String>| s.map(Value::from).unwrap_or_default();
        match method {
            "findAllText" => {
                let fields = [
                    "start",
                    "end",
                    "matched_text",
                    "matched_word_prefix",
                    "matched_word_prefix_start",
                    "matched_word_suffix",
                    "matched_word_suffix_end",
                    "prev_text",
                    "next_text",
                ];
                let hits = self
                    .doc_mut()?
                    .find_all_text(
                        arg(0),
                        arg(1) == "true",
                        arg(4) == "true",
                        arg(2).parse().unwrap_or(0),
                        arg(3).parse().unwrap_or(1000),
                    )
                    .map_err(engine_err)?;
                if hits.is_empty() {
                    return Ok(Value::Nil);
                }
                Ok(Value::array(hits.into_iter().map(|hit| {
                    Value::Table(
                        fields
                            .iter()
                            .zip(hit)
                            .filter_map(|(key, value)| value.map(|v| (Key::from(*key), Value::from(v))))
                            .collect(),
                    )
                })))
            }
            "getTextFromXPointers" => Ok(string(
                self.doc_mut()?.text_range(arg(0), arg(1)).map_err(engine_err)?,
            )),
            "getPrevVisibleWordStart" | "getNextVisibleWordEnd" => Ok(string(
                self.doc_mut()?
                    .word_boundary(arg(0), method == "getNextVisibleWordEnd")
                    .map_err(engine_err)?,
            )),
            "compareXPointers" => Ok(self
                .compare_xpointers(arg(0), arg(1))?
                .map(|n| Value::from(n as i64))
                .unwrap_or_default()),
            "getPageXPointer" => Ok(Value::from(
                self.page_xpointer(arg(0).parse::<u32>().unwrap_or(1).saturating_sub(1))?,
            )),
            "getScreenBoxesFromPositions" => {
                let boxes = self.char_boxes(page, arg(0), Some(arg(1)))?;
                let mut lines: Vec<(i32, [f32; 4])> = Vec::new();
                for b in boxes {
                    if let Some((line, rect)) = lines.last_mut().filter(|(line, _)| *line == b.line) {
                        let _ = line;
                        rect[0] = rect[0].min(b.rect[0]);
                        rect[1] = rect[1].min(b.rect[1]);
                        rect[2] = rect[2].max(b.rect[2]);
                        rect[3] = rect[3].max(b.rect[3]);
                    } else {
                        lines.push((b.line, b.rect));
                    }
                }
                Ok(Value::array(lines.into_iter().map(|(_, r)| {
                    Value::Table(
                        [
                            (Key::from("x"), Value::from(r[0] as f64)),
                            (Key::from("y"), Value::from(r[1] as f64)),
                            (Key::from("w"), Value::from((r[2] - r[0]) as f64)),
                            (Key::from("h"), Value::from((r[3] - r[1]) as f64)),
                        ]
                        .into(),
                    )
                })))
            }
            _ => Err(DocError::InvalidArgument(format!(
                "plugin document method {method}"
            ))),
        }
    }

    /// The xpointer at the top of a page.
    pub fn page_xpointer(&self, page: u32) -> Result<String> {
        self.check_page(page)?;
        self.doc_mut()?.page_xpointer(page).map_err(engine_err)
    }

    /// The page that holds an xpointer, or None when it is not in the document.
    pub fn xpointer_page(&self, xpointer: &str) -> Result<Option<u32>> {
        self.doc_mut()?.xpointer_page(xpointer).map_err(engine_err)
    }

    /// KOReader's comparison: positive if `a` precedes `b`, negative if it
    /// follows, zero if equal. None if either position is invalid.
    pub fn compare_xpointers(&self, a: &str, b: &str) -> Result<Option<i32>> {
        self.doc_mut()?.compare_xpointers(a, b).map_err(engine_err)
    }

    /// The xpointer of the node under a point on the given page.
    pub fn xpointer_at(&self, page: u32, x: i32, y: i32) -> Result<Option<String>> {
        self.check_page(page)?;
        let mut doc = self.doc_mut()?;
        doc.goto_page(page).map_err(engine_err)?;
        doc.xpointer_at(x, y).map_err(engine_err)
    }

    /// Character boxes between two xpointers on the page on screen. Each box is
    /// in page units. Call after [`goto_page`](Self::goto_page) for the page.
    pub fn char_boxes(&self, page: u32, start: &str, end: Option<&str>) -> Result<Vec<CharBox>> {
        self.check_page(page)?;
        let mut doc = self.doc_mut()?;
        doc.goto_page(page).map_err(engine_err)?;
        doc.char_boxes(start, end).map_err(engine_err)
    }

    /// The first character on the page on screen at or after an xpointer, with its
    /// box in page units. None when no visible character follows it. Call
    /// [`goto_page`](Self::goto_page) for the page first. Unlike `char_boxes` with no
    /// end, the cost does not grow with the document.
    pub fn first_char_box(&self, page: u32, start: &str) -> Result<Option<CharBox>> {
        self.check_page(page)?;
        let mut doc = self.doc_mut()?;
        doc.goto_page(page).map_err(engine_err)?;
        doc.first_char_box(start).map_err(engine_err)
    }

    /// The share of a page covered by images, from 0 to 1, measured against the
    /// view of the current layout. None when CREngine cannot measure it.
    pub fn image_share(&self, page: u32) -> Result<Option<f32>> {
        self.check_page(page)?;
        let mut doc = self.doc_mut()?;
        doc.goto_page(page).map_err(engine_err)?;
        doc.page_image_share().map_err(engine_err)
    }

    /// The cover image bytes as stored in the book, or None when it has none.
    pub fn cover_bytes(&self) -> Result<Option<Vec<u8>>> {
        self.doc_mut()?.cover_bytes().map_err(engine_err)
    }

    /// The layout's page size, or the default before a layout is applied.
    fn view_size(&self) -> (u32, u32) {
        match &self.applied {
            Some(l) => (round_px(l.width), round_px(l.height)),
            None => (DEFAULT_WIDTH, DEFAULT_HEIGHT),
        }
    }

    fn check_page(&self, page: u32) -> Result<()> {
        let count = self.doc_mut()?.page_count().map_err(engine_err)? as usize;
        if page as usize >= count {
            return Err(DocError::PageOutOfRange { page, count });
        }
        Ok(())
    }

    /// Renders a page at the layout size, with no scaling.
    fn render_raw(&self, page: u32, format: PixelFormat) -> Result<Pixmap> {
        let (w, h) = self.view_size();
        let color = match format {
            PixelFormat::Gray8 => false,
            PixelFormat::Rgb24 => true,
        };
        let data = {
            let mut doc = self.doc_mut()?;
            doc.goto_page(page).map_err(engine_err)?;
            doc.render(w, h, color).map_err(engine_err)?
        };
        Pixmap::from_data(w, h, format, data)
            .ok_or_else(|| DocError::Engine("rendered buffer does not match its size".to_string()))
    }
}

fn round_px(v: f32) -> u32 {
    v.round().max(1.0) as u32
}

/// Groups character boxes into lines and text. Each line ends with a newline in
/// `text`. Newlines have no entry in `chars`, as the trait requires.
fn page_text_from(boxes: &[CharBox]) -> PageText {
    let mut out = PageText::default();
    let mut current: Option<i32> = None;
    let mut line_start = 0usize;
    let mut line_box: Option<RectF> = None;
    for b in boxes {
        if current != Some(b.line) {
            if current.is_some() {
                finish_line(&mut out, line_start, line_box.take());
            }
            current = Some(b.line);
            line_start = out.chars.len();
        }
        out.text.push(b.ch);
        let bbox = rect_of(b.rect);
        out.chars.push(TextChar { ch: b.ch, bbox });
        line_box = Some(match line_box {
            Some(lb) => union(lb, bbox),
            None => bbox,
        });
    }
    if current.is_some() {
        finish_line(&mut out, line_start, line_box);
    }
    out
}

fn finish_line(out: &mut PageText, first_char: usize, bbox: Option<RectF>) {
    let char_count = out.chars.len() - first_char;
    out.lines.push(TextLine {
        bbox: bbox.unwrap_or_default(),
        first_char,
        char_count,
    });
    out.text.push('\n');
}

fn rect_of(r: [f32; 4]) -> RectF {
    RectF::new(r[0], r[1], r[2], r[3])
}

fn union(a: RectF, b: RectF) -> RectF {
    RectF::new(a.x0.min(b.x0), a.y0.min(b.y0), a.x1.max(b.x1), a.y1.max(b.y1))
}

/// One quad per line of a boxed range, in reading order.
fn quads_from(boxes: &[CharBox]) -> Vec<Quad> {
    let mut quads = Vec::new();
    let mut i = 0;
    while i < boxes.len() {
        let line = boxes[i].line;
        let mut bounds = rect_of(boxes[i].rect);
        while i < boxes.len() && boxes[i].line == line {
            bounds = union(bounds, rect_of(boxes[i].rect));
            i += 1;
        }
        quads.push(Quad {
            ul: PointF {
                x: bounds.x0,
                y: bounds.y0,
            },
            ur: PointF {
                x: bounds.x1,
                y: bounds.y0,
            },
            ll: PointF {
                x: bounds.x0,
                y: bounds.y1,
            },
            lr: PointF {
                x: bounds.x1,
                y: bounds.y1,
            },
        });
    }
    quads
}

/// Fills a clip with white, then copies the part of `src` that it covers.
fn crop_white(src: &Pixmap, clip: Rect) -> Pixmap {
    let bpp = src.format().bytes_per_pixel();
    let white = vec![255u8; clip.w as usize * clip.h as usize * bpp];
    let mut out = Pixmap::from_data(clip.w, clip.h, src.format(), white)
        .unwrap_or_else(|| Pixmap::new(clip.w, clip.h, src.format()));
    let sb = src.bounds();
    let x0 = clip.x.max(sb.x);
    let y0 = clip.y.max(sb.y);
    let x1 = (clip.x + clip.w as i32).min(sb.x + sb.w as i32);
    let y1 = (clip.y + clip.h as i32).min(sb.y + sb.h as i32);
    if x0 >= x1 || y0 >= y1 {
        return out;
    }
    let row = (x1 - x0) as usize * bpp;
    let src_stride = src.stride();
    let dst_stride = out.stride();
    for y in y0..y1 {
        let s = y as usize * src_stride + x0 as usize * bpp;
        let d = (y - clip.y) as usize * dst_stride + (x0 - clip.x) as usize * bpp;
        out.data_mut()[d..d + row].copy_from_slice(&src.data()[s..s + row]);
    }
    out
}

impl Document for CreDocument {
    fn compare_xpointers(&self, a: &str, b: &str) -> Result<Option<i32>> {
        CreDocument::compare_xpointers(self, a, b)
    }

    fn page_count(&self) -> usize {
        self.doc_mut()
            .and_then(|mut d| d.page_count().map_err(engine_err))
            .map_or(0, |n| n as usize)
    }

    fn page_size(&self, page: u32) -> Result<PageSize> {
        self.check_page(page)?;
        let (w, h) = self.view_size();
        Ok(PageSize {
            width: w as f32,
            height: h as f32,
        })
    }

    fn render(&self, page: u32, scale: f32, format: PixelFormat) -> Result<Pixmap> {
        self.check_page(page)?;
        if !(scale.is_finite() && scale > 0.0) {
            return Err(DocError::InvalidArgument(format!("scale {scale}")));
        }
        let base = self.render_raw(page, format)?;
        if scale == 1.0 {
            return Ok(base);
        }
        let size = Size {
            w: ((base.width() as f32 * scale).round() as u32).max(1),
            h: ((base.height() as f32 * scale).round() as u32).max(1),
        };
        Ok(scale_pixmap(&base, size))
    }

    fn render_clip(&self, page: u32, scale: f32, format: PixelFormat, clip: Rect) -> Result<Pixmap> {
        let full = self.render(page, scale, format)?;
        Ok(crop_white(&full, clip))
    }

    fn toc(&self) -> Vec<TocEntry> {
        self.doc_mut()
            .and_then(|mut d| d.toc().map_err(engine_err))
            .unwrap_or_default()
            .into_iter()
            .map(|t| TocEntry {
                xpointer: (!t.xpointer.is_empty()).then_some(t.xpointer),
                level: t.level,
                title: t.title,
                page: t.page,
            })
            .collect()
    }

    fn metadata(&self) -> Metadata {
        let Meta {
            title,
            authors,
            language,
        } = self
            .doc_mut()
            .and_then(|mut d| d.metadata().map_err(engine_err))
            .unwrap_or_default();
        Metadata {
            title: title.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            author: authors.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
            language: language.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        }
    }

    fn page_text(&self, page: u32) -> Result<PageText> {
        let count = self.page_count() as u32;
        self.check_page(page)?;
        let start = self.page_xpointer(page)?;
        let end = if page + 1 < count {
            Some(self.page_xpointer(page + 1)?)
        } else {
            None
        };
        let boxes = self.char_boxes(page, &start, end.as_deref())?;
        Ok(page_text_from(&boxes))
    }

    fn search(&self, page: u32, needle: &str) -> Result<Vec<Quad>> {
        self.check_page(page)?;
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let hits = self
            .doc_mut()?
            .search(needle, true, SEARCH_MAX_HITS)
            .map_err(engine_err)?;
        let mut quads = Vec::new();
        for hit in hits {
            if self.xpointer_page(&hit.start)? != Some(page) {
                continue;
            }
            let boxes = self.char_boxes(page, &hit.start, Some(&hit.end))?;
            quads.extend(quads_from(&boxes));
        }
        Ok(quads)
    }

    fn links(&self, page: u32) -> Result<Vec<Link>> {
        self.check_page(page)?;
        let boxes = {
            let mut doc = self.doc_mut()?;
            doc.goto_page(page).map_err(engine_err)?;
            doc.page_links().map_err(engine_err)?
        };
        // Keep each line's box separate so whitespace between lines is not clickable.
        Ok(boxes
            .into_iter()
            .map(|b| Link {
                bbox: rect_of(b.rect),
                target: b.page.map(LinkTarget::Page).unwrap_or(LinkTarget::Uri(b.href)),
                location: b.xpointer.map(Location::Engine),
                note: b.note,
            })
            .collect())
    }

    fn is_reflowable(&self) -> bool {
        true
    }

    fn set_layout(&mut self, layout: &Layout) -> Result<()> {
        let finite_positive = |v: f32| v.is_finite() && v > 0.0;
        if !(finite_positive(layout.width) && finite_positive(layout.height) && finite_positive(layout.em)) {
            return Err(DocError::InvalidArgument(
                "layout width, height and em must be positive".to_string(),
            ));
        }
        let prev = self.applied.clone();
        let css_path = self.options.data_dir.join(self.kind.default_css());
        if let Some(face) = &layout.font_family {
            // `doc_mut` applies the face with the other process-wide settings.
            self.font_face.clone_from(face);
        }
        if let Some(weight) = layout.font_weight {
            // `set_font_weight` checks the range and `doc_mut` applies the weight.
            self.set_font_weight(weight)?;
        }
        let mut doc = self.doc_mut()?;

        // Stylesheet first. It re-lays out the book, so the other settings follow.
        let css_extra_changed = prev.as_ref().is_none_or(|p| p.css_extra != layout.css_extra);
        if css_extra_changed {
            doc.set_stylesheet(Some(&css_path), Some(&layout.css_extra))
                .map_err(engine_err)?;
        }
        let size_changed = prev.as_ref().is_none_or(|p| {
            round_px(p.width) != round_px(layout.width) || round_px(p.height) != round_px(layout.height)
        });
        if size_changed {
            doc.set_size(round_px(layout.width), round_px(layout.height))
                .map_err(engine_err)?;
        }
        let em_changed = prev
            .as_ref()
            .is_none_or(|p| round_px(p.em) != round_px(layout.em));
        if em_changed {
            doc.set_font_size(round_px(layout.em)).map_err(engine_err)?;
        }
        if let Some(m) = layout.margins {
            let same = prev.as_ref().and_then(|p| p.margins) == Some(m);
            if !same {
                let px = |v: f32| v.round().max(0.0) as u32;
                doc.set_margins(px(m.left), px(m.top), px(m.right), px(m.bottom))
                    .map_err(engine_err)?;
            }
        }
        let percent = layout
            .line_height
            .map(|lh| (lh * 100.0).round().max(1.0) as u32)
            .unwrap_or(100);
        let prev_percent = prev.as_ref().map(|p| {
            p.line_height
                .map(|lh| (lh * 100.0).round().max(1.0) as u32)
                .unwrap_or(100)
        });
        if prev_percent != Some(percent) {
            doc.set_interline_percent(percent).map_err(engine_err)?;
        }
        drop(doc);
        self.applied = Some(layout.clone());
        Ok(())
    }

    fn cover(&self, max_size: Size) -> Option<Pixmap> {
        if max_size.w == 0 || max_size.h == 0 || self.page_count() == 0 {
            return None;
        }
        let size = self.page_size(0).ok()?;
        if !(size.width > 0.0 && size.height > 0.0) {
            return None;
        }
        let scale = (max_size.w as f32 / size.width).min(max_size.h as f32 / size.height);
        if !(scale.is_finite() && scale > 0.0) {
            return None;
        }
        self.render(0, scale, PixelFormat::Rgb24).ok()
    }

    fn resolve(&self, location: &Location) -> Result<u32> {
        match location {
            Location::Page(p) => {
                self.check_page(*p)?;
                Ok(*p)
            }
            Location::Engine(xp) => match self.xpointer_page(xp)? {
                Some(p) if (p as usize) < self.page_count() => Ok(p),
                _ => Err(DocError::UnresolvedLocation(xp.clone())),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_gfx::png::write_pixmap;
    use std::fs;

    fn repo_file(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
    }

    fn juliet() -> PathBuf {
        repo_file("test/juliet.epub")
    }

    /// Artifacts from the tests: `rust/target/kaia-cre-test`.
    fn out_dir() -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/kaia-cre-test");
        fs::create_dir_all(&dir).expect("create the test output directory");
        dir
    }

    /// EPUBs the developer keeps outside the repository. Empty when missing.
    fn library_epubs() -> Vec<PathBuf> {
        let Some(home) = std::env::var_os("HOME") else {
            return Vec::new();
        };
        let Ok(entries) = fs::read_dir(PathBuf::from(home).join("kaia-test-library")) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "epub"))
            .collect();
        files.sort();
        files
    }

    fn open_at(path: &Path, width: f32, height: f32, em: f32) -> CreDocument {
        let mut doc = CreDocument::open(path).expect("open with CREngine");
        doc.set_layout(&Layout::new(width, height, em))
            .expect("apply layout");
        doc
    }

    fn inside_page(doc: &CreDocument, page: u32, b: RectF) -> bool {
        let size = doc.page_size(page).expect("page size");
        RectF::new(0.0, 0.0, size.width, size.height).contains_rect(&b)
    }

    /// A word on the page that the search can find: the longest word of the page text.
    fn longest_word(text: &PageText) -> String {
        text.text
            .split(|c: char| !c.is_alphabetic())
            .max_by_key(|w| w.chars().count())
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn epub_opens_and_has_pages() {
        let doc = open_at(&juliet(), 1264.0, 1680.0, 24.0);
        assert!(doc.is_reflowable());
        assert!(doc.page_count() > 1, "pages: {}", doc.page_count());
    }

    #[test]
    fn page_count_changes_with_font_size() {
        let small = open_at(&juliet(), 1264.0, 1680.0, 20.0).page_count();
        let large = open_at(&juliet(), 1264.0, 1680.0, 40.0).page_count();
        assert!(large > small, "20 px gives {small} pages, 40 px gives {large}");
    }

    #[test]
    fn set_layout_rejects_bad_values() {
        let mut doc = CreDocument::open(&juliet()).unwrap();
        assert!(matches!(
            doc.set_layout(&Layout::new(0.0, 1680.0, 24.0)),
            Err(DocError::InvalidArgument(_))
        ));
        assert!(matches!(
            doc.set_layout(&Layout::new(1264.0, 1680.0, f32::NAN)),
            Err(DocError::InvalidArgument(_))
        ));
    }

    #[test]
    fn renders_pages_to_png_with_ink() {
        let doc = open_at(&juliet(), 1264.0, 1680.0, 24.0);
        let count = doc.page_count() as u32;
        for page in [1.min(count - 1), (count / 2).min(count - 1)] {
            let pm = doc.render(page, 1.0, PixelFormat::Gray8).expect("render gray");
            assert_eq!((pm.width(), pm.height()), (1264, 1680));
            let dark = pm.data().iter().filter(|&&v| v < 128).count();
            assert!(dark > 1000, "page {page} has too little ink: {dark} dark pixels");
            write_pixmap(&pm, &out_dir().join(format!("juliet-p{page}-gray.png"))).expect("write png");
        }
    }

    /// Dark pixels on page 1 of Juliet at a base font weight.
    fn dark_pixels_at_weight(weight: u32) -> usize {
        let mut doc = open_at(&juliet(), 1264.0, 1680.0, 24.0);
        doc.set_font_weight(weight).expect("set weight");
        let pm = doc.render(1, 1.0, PixelFormat::Gray8).expect("render gray");
        pm.data().iter().filter(|&&v| v < 128).count()
    }

    #[test]
    fn heavier_font_weight_renders_more_ink() {
        let regular = dark_pixels_at_weight(REGULAR_FONT_WEIGHT);
        let heavy = dark_pixels_at_weight(480);
        assert!(
            heavy > regular,
            "400 gives {regular} dark pixels, 480 gives {heavy}"
        );
    }

    #[test]
    fn set_font_weight_rejects_values_outside_the_engine_range() {
        let mut doc = open_at(&juliet(), 1264.0, 1680.0, 24.0);
        assert!(matches!(
            doc.set_font_weight(0),
            Err(DocError::InvalidArgument(_))
        ));
        assert!(matches!(
            doc.set_font_weight(1000),
            Err(DocError::InvalidArgument(_))
        ));
        doc.set_font_weight(999).expect("999 is the top of the range");
    }

    #[test]
    fn renders_color_and_clip() {
        let doc = open_at(&juliet(), 800.0, 1000.0, 24.0);
        let pm = doc.render(0, 1.0, PixelFormat::Rgb24).expect("render rgb");
        assert_eq!(pm.data().len(), 800 * 1000 * 3);
        let clip = Rect {
            x: 10,
            y: 20,
            w: 300,
            h: 200,
        };
        let part = doc.render_clip(0, 1.0, PixelFormat::Gray8, clip).expect("clip");
        assert_eq!((part.width(), part.height()), (300, 200));
    }

    /// The first character from the walk to the end of the document, which is the
    /// old ink anchor, must equal the new first-character lookup on each page.
    #[test]
    fn first_char_box_matches_the_first_box_of_the_full_walk() {
        let doc = open_at(&juliet(), 1264.0, 1680.0, 24.0);
        let count = doc.page_count() as u32;
        for page in [0, count / 2] {
            let xp = doc.page_xpointer(page).expect("xpointer");
            let full = doc.char_boxes(page, &xp, None).expect("full walk");
            let first = doc.first_char_box(page, &xp).expect("first box");
            assert_eq!(first, full.first().copied(), "page {page} xpointer {xp}");
        }
    }

    #[test]
    fn xpointer_round_trip_returns_the_page() {
        let doc = open_at(&juliet(), 1264.0, 1680.0, 24.0);
        let count = doc.page_count() as u32;
        for page in [0, 1, count / 2, count - 1] {
            let xp = doc.page_xpointer(page).expect("xpointer");
            assert!(xp.starts_with('/'), "xpointer {xp}");
            assert_eq!(
                doc.xpointer_page(&xp).expect("resolve"),
                Some(page),
                "xpointer {xp}"
            );
            assert_eq!(doc.resolve(&Location::Engine(xp)).expect("location"), page);
        }
        // The center of a character on page 2 maps back to page 2.
        let ch = doc.page_text(2).expect("page text").chars[0].bbox;
        let (cx, cy) = (((ch.x0 + ch.x1) / 2.0) as i32, ((ch.y0 + ch.y1) / 2.0) as i32);
        let at = doc
            .xpointer_at(2, cx, cy)
            .expect("point lookup")
            .expect("a node under the character");
        assert_eq!(doc.xpointer_page(&at).expect("resolve point"), Some(2));
        assert_eq!(doc.xpointer_page("/no/such/node").expect("lookup"), None);
        assert!(matches!(
            doc.resolve(&Location::Engine("/no/such/node".into())),
            Err(DocError::UnresolvedLocation(_))
        ));
    }

    #[test]
    fn page_text_has_words_and_boxes_inside_the_page() {
        let doc = open_at(&juliet(), 1264.0, 1680.0, 24.0);
        let text = doc.page_text(2).expect("page text");
        assert!(!text.lines.is_empty());
        assert!(text.text.contains(' '), "words need spaces between them");
        assert_eq!(text.chars.len(), text.text.chars().filter(|&c| c != '\n').count());
        for c in &text.chars {
            assert!(
                inside_page(&doc, 2, c.bbox),
                "char {:?} at {:?} is outside the page",
                c.ch,
                c.bbox
            );
        }
    }

    #[test]
    fn search_hits_lie_inside_the_page() {
        let doc = open_at(&juliet(), 1264.0, 1680.0, 24.0);
        let page = 2;
        let word = longest_word(&doc.page_text(page).expect("page text"));
        assert!(word.len() >= 4, "no long word on page {page}");
        let quads = doc.search(page, &word).expect("search");
        assert!(
            !quads.is_empty(),
            "search for {word:?} found nothing on page {page}"
        );
        for q in &quads {
            assert!(
                inside_page(&doc, page, q.bounds()),
                "hit {:?} is outside the page",
                q.bounds()
            );
        }
        assert!(doc.search(page, "").expect("empty needle").is_empty());
    }

    #[test]
    fn toc_metadata_and_cover() {
        let doc = CreDocument::open(&juliet()).expect("open");
        let meta = doc.metadata();
        assert!(meta.title.is_some(), "juliet.epub has a title");
        let toc = doc.toc();
        assert!(!toc.is_empty(), "juliet.epub has a table of contents");
        assert!(toc.iter().all(|t| t.level >= 1));
        // Raw access to the engine handle: the cover is optional, so this only checks that the call succeeds.
        let _ = doc.cover_bytes().expect("cover call");
    }

    #[test]
    fn cover_is_rendered_to_fit_the_box() {
        let doc = open_at(&juliet(), 1264.0, 1680.0, 24.0);
        let cover = doc.cover(Size { w: 200, h: 300 }).expect("cover page");
        assert!(cover.width() <= 200 && cover.height() <= 300);
        assert_eq!(cover.format(), PixelFormat::Rgb24);
    }

    #[test]
    fn hyphenation_changes_line_breaks_on_a_narrow_column() {
        let books = library_epubs();
        let Some(path) = books.first() else {
            eprintln!("skipping: no EPUBs in ~/kaia-test-library");
            return;
        };
        let mut doc = open_at(path, 420.0, 1680.0, 24.0);
        let mid = doc.page_count() as u32 / 2;
        let plain = doc.page_text(mid).expect("text without hyphenation").text;
        doc.set_hyphenation(Some("English_US.pattern"))
            .expect("enable hyphenation");
        let hyphenated = doc.page_text(mid).expect("text with hyphenation").text;
        assert_ne!(
            plain, hyphenated,
            "hyphenation should move at least one line break"
        );
        doc.set_hyphenation(None).expect("disable hyphenation");
        assert_eq!(doc.page_text(mid).expect("text again").text, plain);
    }

    /// What one thread reads back from a book under one settings pair.
    #[derive(Debug, PartialEq)]
    struct Snapshot {
        pages: usize,
        text: Vec<String>,
        /// A hash per page, so a failure prints a short message.
        pixel_hashes: Vec<u64>,
    }

    /// Opens juliet.epub on a narrow column with the given hyphenation
    /// dictionary and font face, and reads pages from the middle of the book.
    fn snapshot(hyphenation: Option<&str>, face: &str) -> Snapshot {
        let mut doc = CreDocument::open(&juliet()).expect("open juliet");
        let mut layout = Layout::new(300.0, 500.0, 26.0);
        layout.font_family = Some(face.to_string());
        doc.set_layout(&layout).expect("layout");
        doc.set_hyphenation(hyphenation).expect("hyphenation");
        let pages = doc.page_count();
        let first = (pages / 2) as u32;
        let mut text = Vec::new();
        let mut pixel_hashes = Vec::new();
        for page in first..first + 3 {
            text.push(doc.page_text(page).expect("page text").text);
            let pix = doc.render(page, 1.0, PixelFormat::Gray8).expect("render");
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(pix.data(), &mut hasher);
            pixel_hashes.push(std::hash::Hasher::finish(&hasher));
        }
        Snapshot {
            pages,
            text,
            pixel_hashes,
        }
    }

    #[test]
    fn concurrent_documents_keep_their_own_hyphenation_and_font() {
        const ROUNDS: usize = 12;
        let a = ("English_US.pattern", "Noto Serif");
        let b = ("@none", "Noto Sans");
        fn hyph(name: &str) -> Option<&str> {
            (name != "@none").then_some(name)
        }

        // Single-threaded references.
        let want_a = snapshot(hyph(a.0), a.1);
        let want_b = snapshot(hyph(b.0), b.1);
        assert_ne!(want_a, want_b, "the two settings must give different pages");

        let (got_a, got_b) = std::thread::scope(|scope| {
            let run = |name: &'static str, face: &'static str| {
                scope.spawn(move || {
                    (0..ROUNDS)
                        .map(|_| snapshot(hyph(name), face))
                        .collect::<Vec<_>>()
                })
            };
            let ta = run(a.0, a.1);
            let tb = run(b.0, b.1);
            (ta.join().expect("thread A"), tb.join().expect("thread B"))
        });
        for (round, got) in got_a.iter().enumerate() {
            assert_eq!(got, &want_a, "thread A, round {round}");
        }
        for (round, got) in got_b.iter().enumerate() {
            assert_eq!(got, &want_b, "thread B, round {round}");
        }
    }

    #[test]
    fn two_open_documents_on_one_thread_keep_their_settings() {
        // Same rule on one thread: use A, then B, then A again.
        let mut a = CreDocument::open(&juliet()).expect("open a");
        let mut b = CreDocument::open(&juliet()).expect("open b");
        let mut la = Layout::new(300.0, 500.0, 26.0);
        la.font_family = Some("Noto Serif".to_string());
        let mut lb = Layout::new(300.0, 500.0, 26.0);
        lb.font_family = Some("Noto Sans".to_string());
        a.set_layout(&la).expect("layout a");
        b.set_layout(&lb).expect("layout b");
        a.set_hyphenation(Some("English_US.pattern")).expect("hyph a");
        let mid = (a.page_count() / 2) as u32;
        let first = a.page_text(mid).expect("a text").text;
        let _ = b.page_text(mid).expect("b text");
        b.set_hyphenation(None).expect("hyph b");
        assert_eq!(a.page_text(mid).expect("a again").text, first);
    }

    #[test]
    fn corrupt_file_is_an_error_not_a_crash() {
        let dir = out_dir();
        let garbage = dir.join("corrupt-garbage.epub");
        fs::write(&garbage, vec![0x5au8; 4096]).expect("write garbage");
        assert!(CreDocument::open(&garbage).is_err());

        let bytes = fs::read(juliet()).expect("read juliet");
        let truncated = dir.join("corrupt-truncated.epub");
        fs::write(&truncated, &bytes[..bytes.len() / 3]).expect("write truncated");
        assert!(CreDocument::open(&truncated).is_err());

        let missing = dir.join("does-not-exist.epub");
        assert!(CreDocument::open(&missing).is_err());
    }

    #[test]
    fn plain_text_opens_with_cre() {
        let txt = repo_file("test/sample.txt");
        let doc = open_at(&txt, 1264.0, 1680.0, 24.0);
        assert!(doc.page_count() >= 1);
    }

    #[test]
    fn library_epubs_open_and_render_when_present() {
        let books = library_epubs();
        if books.is_empty() {
            eprintln!("skipping: no EPUBs in ~/kaia-test-library");
            return;
        }
        for path in books.iter().take(3) {
            let mut doc = open_at(path, 1264.0, 1680.0, 24.0);
            assert!(doc.page_count() > 0, "{} has no pages", path.display());
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("book")
                .to_string();
            let pm = doc.render(0, 1.0, PixelFormat::Gray8).expect("render first page");
            write_pixmap(&pm, &out_dir().join(format!("library-{name}-p0.png"))).expect("write png");
            // A body page with hyphenation on, for the typography check.
            doc.set_hyphenation(Some("English_US.pattern"))
                .expect("hyphenation");
            let mid = doc.page_count() as u32 / 2;
            let pm = doc
                .render(mid, 1.0, PixelFormat::Gray8)
                .expect("render middle page");
            write_pixmap(&pm, &out_dir().join(format!("library-{name}-p{mid}-hyph.png"))).expect("write png");
        }
    }

    #[test]
    fn library_cover_bytes_are_nonempty_when_present() {
        for path in library_epubs().iter().take(3) {
            let doc = CreDocument::open(path).expect("open");
            if let Some(bytes) = doc.cover_bytes().expect("cover call") {
                assert!(!bytes.is_empty(), "{}", path.display());
            }
        }
    }
}
