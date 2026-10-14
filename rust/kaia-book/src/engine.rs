//! The engine thread. It owns the document and the work queue.
//!
//! The thread runs one job at a time. A job is either a request from the
//! session (it waits for a reply) or a fire-and-forget post. Between jobs the
//! thread runs background tasks: prerendering the next page, and search one
//! page per step. Before each background step it checks the queue, so a
//! request waits for at most one page render or one search page.

use std::collections::VecDeque;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use kaia_doc::{DocError, Engine as DocEngine, Location, Metadata, PageText, PointF, RectF, TocEntry};
use kaia_gfx::{PixelFormat, Pixmap, Size};

use crate::cache::{PageCache, RenderKey};
use crate::doc::EngineDoc;
use crate::error::{BookError, Result};
use crate::layout::TextLayout;
use crate::pos::Pos;
use crate::session::SearchResult;
use crate::text::{nearest_char, page_text_from_boxes, span_rects, span_selected_text, word_span, Flat};
use crate::view::{PageTransform, ZoomMode};

/// Pages of text kept for selection and links.
const TEXT_CACHE_PAGES: usize = 6;
/// Characters of context on each side of a search hit.
const SNIPPET_CONTEXT: usize = 40;

/// What the session learns when the document opens.
pub(crate) struct Facts {
    pub toc: Vec<TocEntry>,
    pub metadata: Metadata,
    pub reflowable: bool,
    pub cre: bool,
}

/// The result of a layout change.
#[derive(Debug)]
pub(crate) struct LayoutOutcome {
    /// The layout generation this outcome belongs to. The render cache keys on it.
    pub layout: u64,
    pub page: u32,
    pub page_count: usize,
    pub position: Location,
    pub toc: Vec<TocEntry>,
}

/// Messages from the engine thread to the session.
pub(crate) enum Event {
    Layout(std::result::Result<LayoutOutcome, String>),
    Rendered {
        page: u32,
    },
    SearchChunk {
        token: u64,
        page: u32,
        results: Vec<SearchResult>,
    },
    SearchDone {
        token: u64,
        cancelled: bool,
    },
}

/// Whether a background task has more steps.
pub(crate) enum Step {
    Continue,
    Done,
}

type Job = Box<dyn FnOnce(&mut Worker) + Send>;
type Background = Box<dyn FnMut(&mut Worker) -> Step>;

enum Msg {
    Job(Job),
    Shutdown,
}

/// Characters `first..=last` of one page's text. Empty when there are none.
pub(crate) struct Span {
    pub text: Arc<PageText>,
    pub first: usize,
    pub last: usize,
}

impl Span {
    fn empty() -> Self {
        Self {
            text: Arc::new(PageText::default()),
            first: 0,
            last: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.text.chars.is_empty() || self.first > self.last
    }
}

/// The position of a text in the reading order of a page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct OrderKey {
    pub page: u32,
    pub y: f32,
    pub x: f32,
}

/// A selection before the session turns it into a [`crate::session::Selection`].
pub(crate) struct RawSelection {
    pub first: usize,
    pub last: usize,
    pub text: String,
    pub pos0: Pos,
    pub pos1: Pos,
    pub page_rects: Vec<RectF>,
    pub transform: PageTransform,
}

/// Everything the engine thread owns. Jobs receive it as `&mut Worker`.
pub(crate) struct Worker {
    pub doc: EngineDoc,
    pub cache: Arc<Mutex<PageCache>>,
    events: Sender<Event>,
    /// Layout generation. Each layout change increments it.
    pub layout: u64,
    texts: VecDeque<((u64, u32), Arc<PageText>)>,
    background: VecDeque<Background>,
    /// The token of the newest prerender. Older prerenders stop when they run.
    prerender: u64,
    /// The token of the newest search. Older searches stop when they run.
    pub search: u64,
}

fn facts_of(doc: &EngineDoc) -> Facts {
    let d = doc.doc();
    Facts {
        toc: d.toc(),
        metadata: d.metadata(),
        reflowable: d.is_reflowable(),
        cre: doc.is_cre(),
    }
}

/// The page that a fraction of the book falls on. Percent `(page + 1) / count`
/// maps back to `page`, as KOReader's `percent_finished` does.
pub(crate) fn page_from_percent(percent: f64, count: usize) -> u32 {
    if count == 0 {
        return 0;
    }
    let p = (percent * count as f64).round() as i64 - 1;
    p.clamp(0, count as i64 - 1) as u32
}

impl Worker {
    pub fn send(&self, e: Event) {
        // The session may have gone away. The result is then unused.
        let _ = self.events.send(e);
    }

    pub fn page_count(&self) -> usize {
        self.doc.doc().page_count()
    }

    /// The share of a page covered by images, from 0 to 1, or None when the engine
    /// cannot tell. CREngine measures the page. A fixed-layout page is a picture,
    /// so it reports 1. MuPDF reflow has no cheap image test and reports None.
    pub fn image_share(&self, page: u32) -> std::result::Result<Option<f32>, DocError> {
        match &self.doc {
            #[cfg(feature = "cre")]
            EngineDoc::Cre(d) => d.image_share(page),
            EngineDoc::Mupdf(_) if self.doc.doc().is_reflowable() => Ok(None),
            EngineDoc::Mupdf(_) => Ok(Some(1.0)),
        }
    }

    /// The location that names a page: an xpointer for CREngine, a page for the rest.
    pub fn location_of_page(&self, page: u32) -> std::result::Result<Location, DocError> {
        Ok(match self.doc.page_xpointer(page)? {
            Some(xp) => Location::Engine(xp),
            None => Location::Page(page),
        })
    }

    /// The location to store for a page that a caller reached through `loc`.
    /// CREngine keeps an xpointer as given. A page number becomes the xpointer
    /// of the page top. Other engines store pages.
    pub fn normalize(&self, page: u32, loc: &Location) -> std::result::Result<Location, DocError> {
        if !self.doc.is_cre() {
            return Ok(Location::Page(page));
        }
        match loc {
            Location::Engine(xp) => Ok(Location::Engine(xp.clone())),
            Location::Page(_) => self.location_of_page(page),
        }
    }

    /// The text of a page, from a small cache.
    pub fn page_text(&mut self, page: u32) -> std::result::Result<Arc<PageText>, DocError> {
        let key = (self.layout, page);
        if let Some(i) = self.texts.iter().position(|(k, _)| *k == key) {
            let entry = self.texts.remove(i).expect("index from position");
            let text = entry.1.clone();
            self.texts.push_back(entry);
            return Ok(text);
        }
        let text = Arc::new(self.doc.doc().page_text(page)?);
        self.texts.push_back((key, text.clone()));
        while self.texts.len() > TEXT_CACHE_PAGES {
            self.texts.pop_front();
        }
        Ok(text)
    }

    /// The page-to-screen transform for a page in a viewport.
    pub fn transform(
        &self,
        page: u32,
        viewport: Size,
        zoom: ZoomMode,
    ) -> std::result::Result<PageTransform, DocError> {
        let size = self.doc.doc().page_size(page)?;
        let zoom = if self.doc.doc().is_reflowable() {
            None
        } else {
            Some(zoom)
        };
        Ok(PageTransform::new(size, viewport, zoom))
    }

    pub fn render_key(&self, page: u32, viewport: Size, zoom: ZoomMode, format: PixelFormat) -> RenderKey {
        RenderKey {
            layout: self.layout,
            page,
            viewport,
            zoom,
            format,
        }
    }

    /// Renders a page into the cache, unless it is there already. The cache
    /// lock is not held while the page renders.
    pub fn render_into_cache(
        &mut self,
        page: u32,
        viewport: Size,
        zoom: ZoomMode,
        format: PixelFormat,
    ) -> std::result::Result<Arc<Pixmap>, DocError> {
        let key = self.render_key(page, viewport, zoom, format);
        if let Some(hit) = self.lock_cache().peek(&key) {
            return Ok(hit);
        }
        let t = self.transform(page, viewport, zoom)?;
        let pixmap = self.doc.doc().render(page, t.scale, format)?;
        Ok(self.lock_cache().insert(key, pixmap))
    }

    /// Renders a page and queues the next page for prerendering.
    pub fn render_and_prefetch(
        &mut self,
        page: u32,
        viewport: Size,
        zoom: ZoomMode,
        format: PixelFormat,
    ) -> std::result::Result<Arc<Pixmap>, DocError> {
        let pixmap = self.render_into_cache(page, viewport, zoom, format)?;
        if (page as usize + 1) < self.page_count() {
            self.schedule_prerender(page + 1, viewport, zoom, format);
        }
        Ok(pixmap)
    }

    /// Queues a background render of `page`. It runs before any other background
    /// task. A newer prerender request makes an older one stop without rendering.
    fn schedule_prerender(&mut self, page: u32, viewport: Size, zoom: ZoomMode, format: PixelFormat) {
        self.prerender += 1;
        let token = self.prerender;
        let layout = self.layout;
        self.background.push_front(Box::new(move |w: &mut Worker| {
            if w.prerender != token || w.layout != layout {
                return Step::Done;
            }
            let key = w.render_key(page, viewport, zoom, format);
            if !w.lock_cache().contains(&key) {
                // A failed prerender is not an error for the reader. The
                // page renders again when it is shown.
                let _ = w.render_into_cache(page, viewport, zoom, format);
            }
            Step::Done
        }));
    }

    fn lock_cache(&self) -> std::sync::MutexGuard<'_, PageCache> {
        // A panic while the lock is held leaves the cache usable: its fields are
        // only changed in whole steps. Recover the guard instead of panicking.
        self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Applies a reflowable layout and finds the position in the new layout.
    /// `hyphenation` is the pattern file the layout hyphenates with, or None for none.
    pub fn apply_layout(
        &mut self,
        layout: &TextLayout,
        hyphenation: Option<&'static str>,
        viewport: Size,
        position: Option<Location>,
        percent: f64,
    ) -> std::result::Result<LayoutOutcome, DocError> {
        layout.validate().map_err(DocError::InvalidArgument)?;
        if !self.doc.doc().is_reflowable() {
            return Err(DocError::NotReflowable);
        }
        self.doc.doc_mut().set_layout(&layout.to_doc_layout(viewport))?;
        self.doc.set_cre_options(hyphenation, layout.embedded_styles)?;
        self.layout += 1;
        self.texts.clear();
        let (page, position) = self.restore_position(position, percent)?;
        Ok(LayoutOutcome {
            layout: self.layout,
            page,
            page_count: self.page_count(),
            toc: self.doc.doc().toc(),
            position,
        })
    }

    /// The page and the normalized location for a saved position. An
    /// unresolvable position falls back to the percent.
    pub fn restore_position(
        &mut self,
        position: Option<Location>,
        percent: f64,
    ) -> std::result::Result<(u32, Location), DocError> {
        if let Some(loc) = &position {
            if let Ok(page) = self.doc.doc().resolve(loc) {
                let norm = self.normalize(page, loc)?;
                return Ok((page, norm));
            }
        }
        let page = page_from_percent(percent, self.page_count());
        Ok((page, self.location_of_page(page)?))
    }

    /// The text of a page between two positions. `from` and `to` are the
    /// positions of the range on this page. None means the page start or end.
    /// A position on another page makes the range start or end at this page's
    /// edge, or makes the span empty when the range lies entirely on another page.
    pub fn range_span(
        &mut self,
        page: u32,
        from: Option<&Pos>,
        to: Option<&Pos>,
    ) -> std::result::Result<Span, DocError> {
        if !self.doc.is_cre() {
            return self.fixed_span(page, from, to);
        }
        let count = self.page_count() as u32;
        let top = self.doc.page_xpointer(page)?.unwrap_or_default();
        let next = if page + 1 < count {
            self.doc.page_xpointer(page + 1)?
        } else {
            None
        };
        let start = match from {
            Some(Pos::Xpointer(xp)) => match self.doc.xpointer_page(xp)? {
                Some(p) if p == page => xp.clone(),
                Some(p) if p > page => return Ok(Span::empty()),
                _ => top.clone(),
            },
            _ => top.clone(),
        };
        let end = match to {
            Some(Pos::Xpointer(xp)) => match self.doc.xpointer_page(xp)? {
                Some(p) if p == page => Some(xp.clone()),
                Some(p) if p < page => return Ok(Span::empty()),
                _ => next,
            },
            _ => next,
        };
        let boxes = self
            .doc
            .char_boxes(page, &start, end.as_deref())?
            .unwrap_or_default();
        let text = Arc::new(page_text_from_boxes(&boxes));
        let last = text.chars.len().saturating_sub(1);
        Ok(Span { text, first: 0, last })
    }

    fn fixed_span(
        &mut self,
        page: u32,
        from: Option<&Pos>,
        to: Option<&Pos>,
    ) -> std::result::Result<Span, DocError> {
        let text = self.page_text(page)?;
        let n = text.chars.len();
        if n == 0 {
            return Ok(Span::empty());
        }
        let first = match from {
            Some(Pos::Point { page: p, x, y }) if *p == page => {
                nearest_char(&text, PointF { x: *x, y: *y }).unwrap_or(0)
            }
            Some(Pos::Point { page: p, .. }) if *p > page => return Ok(Span::empty()),
            _ => 0,
        };
        let last = match to {
            Some(Pos::Point { page: p, x, y }) if *p == page => {
                nearest_char(&text, PointF { x: *x, y: *y }).unwrap_or(n - 1)
            }
            Some(Pos::Point { page: p, .. }) if *p < page => return Ok(Span::empty()),
            _ => n - 1,
        };
        Ok(Span { text, first, last })
    }

    /// The page of a position. None when a CREngine xpointer is not in the document.
    pub fn pos_page(&self, pos: &Pos) -> std::result::Result<Option<u32>, DocError> {
        match pos {
            Pos::Point { page, .. } => Ok(Some(*page)),
            Pos::Xpointer(xp) => self.doc.xpointer_page(xp),
        }
    }

    /// The reading-order key of a highlight start or bookmark position.
    pub fn order_key(&mut self, pos: &Pos) -> std::result::Result<OrderKey, DocError> {
        match pos {
            Pos::Point { page, x, y } => Ok(OrderKey {
                page: *page,
                y: *y,
                x: *x,
            }),
            Pos::Xpointer(_) => {
                let Some(page) = self.pos_page(pos)? else {
                    return Ok(OrderKey {
                        page: u32::MAX,
                        y: 0.0,
                        x: 0.0,
                    });
                };
                let span = self.range_span(page, Some(pos), None)?;
                let corner = span.text.chars.get(span.first).map(|c| c.bbox);
                let (x, y) = corner.map_or((0.0, 0.0), |b| (b.x0, b.y0));
                Ok(OrderKey { page, y, x })
            }
        }
    }

    /// The positions of the character range `first..=last` on a page.
    pub fn positions_for(
        &self,
        page: u32,
        text: &PageText,
        first: usize,
        last: usize,
    ) -> std::result::Result<(Pos, Pos), DocError> {
        let center = |i: usize| {
            let b = text.chars[i].bbox;
            PointF {
                x: (b.x0 + b.x1) / 2.0,
                y: (b.y0 + b.y1) / 2.0,
            }
        };
        if self.doc.is_cre() {
            let at = |i: usize| -> std::result::Result<Pos, DocError> {
                match self.doc.xpointer_at(page, center(i))? {
                    Some(xp) => Ok(Pos::Xpointer(xp)),
                    None => Err(DocError::Engine(format!("no text position at character {i}"))),
                }
            };
            let start = at(first)?;
            let Pos::Xpointer(end) = at(last)? else {
                unreachable!();
            };
            let end = next_text_offset(&end)
                .ok_or_else(|| DocError::Engine(format!("xpointer has no text offset: {end}")))?;
            return Ok((start, Pos::Xpointer(end)));
        }
        let point = |i: usize| {
            let c = center(i);
            Pos::Point { page, x: c.x, y: c.y }
        };
        Ok((point(first), point(last)))
    }

    /// The selection that a screen point picks on a page. With no anchor it is
    /// the word under the point. With an anchor (the first and last character of
    /// the current selection) it extends the anchor to the word under the point.
    pub fn select(
        &mut self,
        page: u32,
        viewport: Size,
        zoom: ZoomMode,
        point: kaia_gfx::Point,
        anchor: Option<(usize, usize)>,
    ) -> std::result::Result<Option<RawSelection>, DocError> {
        let transform = self.transform(page, viewport, zoom)?;
        let p = transform.to_page(point);
        let text = self.page_text(page)?;
        let Some(hit) = nearest_char(&text, p) else {
            return Ok(None);
        };
        let (first, last) = match anchor {
            None => match word_span(&text, hit) {
                Some(span) => span,
                None => return Ok(None),
            },
            Some((a0, a1)) if hit >= a0 => {
                let end = word_span(&text, hit).map_or(hit, |(_, e)| e);
                (a0, end.max(a1))
            }
            Some((a0, a1)) => {
                let start = word_span(&text, hit).map_or(hit, |(s, _)| s);
                (start.min(a0), a1)
            }
        };
        let (pos0, pos1) = self.positions_for(page, &text, first, last)?;
        Ok(Some(RawSelection {
            first,
            last,
            text: span_selected_text(&text, first, last),
            pos0,
            pos1,
            page_rects: span_rects(&text, first, last),
            transform,
        }))
    }

    /// The first match of `needle` at or after page `from`, wrapping around the
    /// book, as positions. Matching ignores case. Positions are the first and
    /// last character of the match.
    pub fn find_text(
        &mut self,
        needle: &str,
        from: u32,
    ) -> std::result::Result<Option<(Pos, Pos)>, DocError> {
        let count = self.page_count() as u32;
        if count == 0 {
            return Ok(None);
        }
        let start = from.min(count - 1);
        for i in 0..count {
            let page = (start + i) % count;
            // A whole-book scan reads pages that are not the reader's, so it skips the text cache.
            let text = Arc::new(self.doc.doc().page_text(page)?);
            let flat = Flat::new(&text);
            let Some(range) = flat.find_all(needle).into_iter().next() else {
                continue;
            };
            if let Some((first, last)) = flat.span(&range) {
                return Ok(Some(self.positions_for(page, &text, first, last)?));
            }
        }
        Ok(None)
    }

    /// The text of a highlight, across the pages it covers.
    pub fn annotation_text(&mut self, pos0: &Pos, pos1: &Pos) -> std::result::Result<String, DocError> {
        let (Some(p0), Some(p1)) = (self.pos_page(pos0)?, self.pos_page(pos1)?) else {
            return Ok(String::new());
        };
        let mut parts = Vec::new();
        for page in p0..=p1 {
            let from = (page == p0).then_some(pos0);
            let to = (page == p1).then_some(pos1);
            let span = self.range_span(page, from, to)?;
            if !span.is_empty() {
                parts.push(span_selected_text(&span.text, span.first, span.last));
            }
        }
        Ok(parts.join("\n"))
    }

    /// Screen rectangles of a highlight on one page.
    pub fn highlight_rects(
        &mut self,
        page: u32,
        pos0: &Pos,
        pos1: &Pos,
        transform: &PageTransform,
    ) -> std::result::Result<Vec<kaia_gfx::Rect>, DocError> {
        let (Some(p0), Some(p1)) = (self.pos_page(pos0)?, self.pos_page(pos1)?) else {
            return Ok(Vec::new());
        };
        if page < p0 || page > p1 {
            return Ok(Vec::new());
        }
        let from = (page == p0).then_some(pos0);
        let to = (page == p1).then_some(pos1);
        let span = self.range_span(page, from, to)?;
        if span.is_empty() {
            return Ok(Vec::new());
        }
        Ok(span_rects(&span.text, span.first, span.last)
            .into_iter()
            .map(|r| transform.to_screen(r))
            .collect())
    }
}

fn next_text_offset(xp: &str) -> Option<String> {
    let (node, offset) = xp.rsplit_once('.')?;
    let next = offset.parse::<u32>().ok()?.checked_add(1)?;
    Some(format!("{node}.{next}"))
}

/// Starts a search in the background. Results arrive as events, one chunk per page.
fn start_search(w: &mut Worker, needle: String, token: u64) {
    w.search = token;
    let mut next: u32 = 0;
    w.background.push_back(Box::new(move |w: &mut Worker| {
        if w.search != token {
            w.send(Event::SearchDone {
                token,
                cancelled: true,
            });
            return Step::Done;
        }
        let count = w.page_count() as u32;
        if next >= count {
            w.send(Event::SearchDone {
                token,
                cancelled: false,
            });
            return Step::Done;
        }
        let page = next;
        next += 1;
        let results = match w.doc.doc().page_text(page) {
            Ok(text) => matches_on_page(w, page, &text, &needle),
            // A page that fails to read has no results. The search goes on.
            Err(_) => Vec::new(),
        };
        w.send(Event::SearchChunk { token, page, results });
        Step::Continue
    }));
}

fn matches_on_page(w: &Worker, page: u32, text: &PageText, needle: &str) -> Vec<SearchResult> {
    let flat = Flat::new(text);
    let ranges: Vec<Range<usize>> = flat.find_all(needle);
    if ranges.is_empty() {
        return Vec::new();
    }
    let location = w.location_of_page(page).unwrap_or(Location::Page(page));
    ranges
        .iter()
        .filter_map(|range| {
            let (first, last) = flat.span(range)?;
            let (snippet, match_range) = flat.snippet(range, SNIPPET_CONTEXT);
            Some(SearchResult {
                index: 0,
                page,
                location: location.clone(),
                snippet,
                match_range,
                rects: span_rects(text, first, last),
            })
        })
        .collect()
}

fn run(mut w: Worker, rx: Receiver<Msg>) {
    loop {
        match rx.try_recv() {
            Ok(Msg::Job(job)) => job(&mut w),
            Ok(Msg::Shutdown) | Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {
                if let Some(mut task) = w.background.pop_front() {
                    if let Step::Continue = task(&mut w) {
                        w.background.push_back(task);
                    }
                } else {
                    match rx.recv() {
                        Ok(Msg::Job(job)) => job(&mut w),
                        Ok(Msg::Shutdown) | Err(_) => break,
                    }
                }
            }
        }
    }
}

/// The session's end of the engine thread.
pub(crate) struct Handle {
    tx: Sender<Msg>,
    pub events: Receiver<Event>,
    join: Option<JoinHandle<()>>,
}

impl Handle {
    /// Starts the engine thread and opens the document on it. Returns when the
    /// document is open or has failed to open.
    pub fn spawn(path: PathBuf, engine: DocEngine, cache: Arc<Mutex<PageCache>>) -> Result<(Handle, Facts)> {
        let (tx, rx) = channel::<Msg>();
        let (event_tx, event_rx) = channel::<Event>();
        let (init_tx, init_rx) = channel::<std::result::Result<Facts, DocError>>();
        let thread_path = path.clone();
        let join = std::thread::Builder::new()
            .name("kaia-book-engine".to_string())
            .spawn(move || {
                let doc = match EngineDoc::open(&thread_path, engine) {
                    Ok(doc) => doc,
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };
                let _ = init_tx.send(Ok(facts_of(&doc)));
                let worker = Worker {
                    doc,
                    cache,
                    events: event_tx,
                    layout: 0,
                    texts: VecDeque::new(),
                    background: VecDeque::new(),
                    prerender: 0,
                    search: 0,
                };
                run(worker, rx);
            })
            .map_err(|source| BookError::Io {
                path: path.clone(),
                source,
            })?;
        match init_rx.recv() {
            Ok(Ok(facts)) => Ok((
                Handle {
                    tx,
                    events: event_rx,
                    join: Some(join),
                },
                facts,
            )),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(e.into())
            }
            Err(_) => {
                let _ = join.join();
                Err(BookError::EngineStopped)
            }
        }
    }

    /// Runs `f` on the engine thread and waits for its result.
    pub fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Worker) -> std::result::Result<T, DocError> + Send + 'static,
    ) -> Result<T> {
        let (reply_tx, reply_rx) = channel();
        self.tx
            .send(Msg::Job(Box::new(move |w: &mut Worker| {
                let _ = reply_tx.send(f(w));
            })))
            .map_err(|_| BookError::EngineStopped)?;
        let result = reply_rx.recv().map_err(|_| BookError::EngineStopped)?;
        Ok(result?)
    }

    /// Runs `f` on the engine thread without waiting for it.
    pub fn post(&self, f: impl FnOnce(&mut Worker) + Send + 'static) -> Result<()> {
        self.tx
            .send(Msg::Job(Box::new(f)))
            .map_err(|_| BookError::EngineStopped)
    }

    /// Starts a search on the engine thread. A running search stops at its next step.
    pub fn start_search(&self, needle: String, token: u64) -> Result<()> {
        self.post(move |w| start_search(w, needle, token))
    }

    /// Stops the running search without starting another.
    pub fn stop_search(&self, token: u64) -> Result<()> {
        self.post(move |w| w.search = token)
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
