//! The reading session: the public API of the crate.
//!
//! # Threading
//!
//! A [`BookSession`] is `Send` but not `Sync`. Its methods take `&mut self`
//! where they change the session, so one thread drives it. The document lives on
//! an engine thread, because MuPDF and CREngine handles are not `Send`.
//!
//! - Cheap reads (`current`, `toc`, `status`, `cached_current`) use state the
//!   session keeps, and never wait for the engine.
//! - Short engine calls (page turns, selection, annotation boxes, bookmark
//!   toggles) wait for the engine. The engine answers between background
//!   steps, so a call waits for at most one page render or one search page.
//! - Layout changes run on the engine thread without blocking the caller:
//!   [`BookSession::set_layout`] returns at once, and [`BookSession::poll`]
//!   reports [`BookEvent::LayoutApplied`]. Navigation returns
//!   [`BookError::LayoutPending`] until then.
//! - Search runs on the engine thread one page per step. Results arrive through
//!   `poll`. [`BookSession::cancel_search`] stops it at the next step.
//! - Prerendering runs on the engine thread after a render, as a background
//!   step. A later [`BookSession::render_current`] for the next page is a
//!   cache hit.
//!
//! Only [`BookSession::open`] waits for the document to load.

use std::collections::HashMap;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use kaia_doc::{DocError, Engine, Link, LinkTarget, Location, Metadata, RectF, TocEntry};
use kaia_gfx::{PixelFormat, Pixmap, Point, Rect, Size};

use crate::annotation::{Annotation, AnnotationId, Drawer, PageAnnotation};
use crate::cache::{CacheStats, PageCache, RenderKey};
use crate::engine::{page_from_percent, Event, Facts, Handle, OrderKey, RawSelection, Worker};
use crate::error::{BookError, Result};
use crate::hash::partial_md5;
use crate::layout::TextLayout;
use crate::pos::Pos;
use crate::sidecar::{map_sidecar, read_first, sidecar_candidates, ImportReport, SidecarLocation};
use crate::state::{read_state, write_state, BookState, DocumentId, ReadingTime, MAX_READING_ENTRIES};
use crate::time::now_datetime;
use crate::view::{PageTransform, ZoomMode};

/// A visit to a page longer than this is not counted. The device may have slept.
const MAX_VISIT_SECONDS: f64 = 30.0 * 60.0;
/// Default bound of the rendered page cache.
const DEFAULT_CACHE_BYTES: usize = 32 * 1024 * 1024;

/// Options for [`BookSession::open`].
#[derive(Debug, Clone)]
pub struct OpenOptions {
    /// Kaia's data directory. The state file goes in `<data_dir>/kaia/books/`.
    pub data_dir: PathBuf,
    /// Which engine opens the file. See [`Engine`].
    pub engine: Engine,
    /// The viewport of a reflowable layout, in pixels. Books with no saved layout use it.
    pub viewport: Size,
    /// The layout for a book with no saved state and no sidecar.
    pub default_layout: TextLayout,
    /// Where to look for KOReader sidecars, in order. The first one that
    /// parses is imported.
    pub sidecars: Vec<SidecarLocation>,
    /// Bound of the rendered page cache, in bytes of pixel data.
    pub cache_bytes: usize,
    /// Pixel format of rendered pages. Gray8 suits E Ink.
    pub pixel_format: PixelFormat,
}

impl OpenOptions {
    /// Options with the defaults: automatic engine choice, a 1264 x 1680 viewport,
    /// KOReader's default text layout, a sidecar next to the book, a 32 MiB
    /// page cache and Gray8 pages.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            engine: Engine::Auto,
            viewport: Size { w: 1264, h: 1680 },
            default_layout: TextLayout::default(),
            sidecars: vec![SidecarLocation::BesideBook],
            cache_bytes: DEFAULT_CACHE_BYTES,
            pixel_format: PixelFormat::Gray8,
        }
    }
}

/// Where the reader is, with the figures the footer and the overlay show.
#[derive(Debug, Clone, PartialEq)]
pub struct PageInfo {
    /// The page, 0-based.
    pub page: u32,
    pub page_count: usize,
    /// Reading progress: the page shown, as a fraction of the book.
    pub percent: f64,
    /// The title of the table of contents entry that holds the page.
    pub chapter: Option<String>,
    /// Pages after this one before the next table of contents entry. None when the book has no pages in its table of contents.
    pub pages_left_in_chapter: Option<u32>,
    /// The location to store for this page.
    pub location: Location,
}

/// A text selection on the current page.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub page: u32,
    /// The selected text. Line breaks become spaces.
    pub text: String,
    /// The start of the selection, in the form that annotations store.
    pub pos0: Pos,
    /// The end of the selection.
    pub pos1: Pos,
    /// One box per line, in page points.
    pub page_rects: Vec<RectF>,
    /// The same boxes in screen pixels for the viewport they were computed for.
    pub screen_rects: Vec<Rect>,
}

/// One search hit.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    /// Position in the result list, from 0.
    pub index: usize,
    /// The page, 0-based.
    pub page: u32,
    /// Where a jump to the hit goes.
    pub location: Location,
    /// The hit with context, from the same line and nearby text.
    pub snippet: String,
    /// The hit inside `snippet`, in characters.
    pub match_range: Range<usize>,
    /// Boxes of the hit on its page, in page points.
    pub rects: Vec<RectF>,
}

/// Something that happened on the engine thread, reported by [`BookSession::poll`].
#[derive(Debug, Clone, PartialEq)]
pub enum BookEvent {
    /// A layout change finished. The session is at the reading position in the new layout.
    LayoutApplied { page_count: usize },
    /// A layout change failed. The engine may have applied part of it. Set the
    /// layout again to retry.
    LayoutFailed { message: String },
    /// A page requested with [`BookSession::request_render`] is in the cache.
    PageRendered { page: u32 },
    /// A search page finished. `found` is the number of results so far.
    SearchProgress { page: u32, found: usize },
    /// The search ended, either at the last page or by cancellation.
    SearchFinished { cancelled: bool },
}

struct SelectionState {
    /// The first and last character of the word the selection started on.
    anchor: (usize, usize),
}

#[derive(Default)]
struct SearchState {
    token: u64,
    results: Vec<SearchResult>,
    running: bool,
    cursor: Option<usize>,
}

#[derive(Default)]
struct OrderCache {
    layout: u64,
    keys: HashMap<AnnotationId, OrderKey>,
}

/// An open book: its document, its Kaia state, and a reader's view of it.
pub struct BookSession {
    path: PathBuf,
    engine: Handle,
    cache: Arc<Mutex<PageCache>>,
    facts: Facts,
    state: BookState,
    state_path: PathBuf,
    dirty: bool,
    stale: bool,
    import: Option<ImportReport>,
    page: u32,
    page_count: usize,
    layout_gen: u64,
    layout_pending: bool,
    viewport: Size,
    format: PixelFormat,
    selection: Option<SelectionState>,
    search: SearchState,
    link_history: Vec<Location>,
    order: OrderCache,
    visit_started: Instant,
    out: Vec<BookEvent>,
}

impl BookSession {
    /// Opens a book. The document is loaded on the engine thread. Kaia's
    /// state is read from `<data_dir>/kaia/books/<partial MD5>.json`. When
    /// that file is missing or unreadable, the first KOReader sidecar in
    /// `opts.sidecars` is imported. KOReader's files are read and never written.
    /// A saved layout that the engine rejects gives way to the default layout.
    ///
    /// Fails with a document error when the engine cannot read the file, and
    /// with an I/O error when the file cannot be read.
    pub fn open(path: impl AsRef<Path>, opts: OpenOptions) -> Result<BookSession> {
        let path = path.as_ref().to_path_buf();
        let io = |source: std::io::Error| BookError::Io {
            path: path.clone(),
            source,
        };
        let md5 = partial_md5(&path).map_err(io)?;
        let file_size = fs::metadata(&path).map_err(io)?.len();
        let identity = DocumentId {
            partial_md5: md5.clone(),
            file_size,
        };
        let cache = Arc::new(Mutex::new(PageCache::new(opts.cache_bytes)));
        let (engine, facts) = Handle::spawn(path.clone(), opts.engine, cache.clone())?;
        let state_path = opts
            .data_dir
            .join("kaia")
            .join("books")
            .join(format!("{md5}.json"));

        let mut stale = false;
        let mut import = None;
        let mut state = match read_state(&state_path) {
            Ok(Some(state)) => {
                stale = state.doc.file_size != file_size;
                state
            }
            other => {
                if other.is_err() {
                    // Keep the unreadable file for inspection. The next save writes a new one.
                    let _ = fs::rename(&state_path, state_path.with_extension("json.unreadable"));
                }
                let candidates = sidecar_candidates(&path, &md5, &opts.sidecars);
                match read_first(&candidates) {
                    Some((root, source)) => {
                        let imported = map_sidecar(&root, !facts.reflowable, &source);
                        let layout = imported
                            .layout
                            .clone()
                            .unwrap_or_else(|| opts.default_layout.clone());
                        let mut st = BookState::new(identity.clone(), layout);
                        st.position = imported.position.clone();
                        st.percent = imported.percent.unwrap_or(0.0);
                        st.zoom = imported.zoom.unwrap_or_default();
                        st.status = imported.status.clone();
                        st.rating = imported.rating;
                        st.summary_note = imported.summary_note.clone();
                        st.imported_from = Some(source.display().to_string());
                        for mut a in imported.annotations {
                            a.id = st.assign_id();
                            st.annotations.push(a);
                        }
                        import = Some(imported.report);
                        st
                    }
                    None => BookState::new(identity.clone(), opts.default_layout.clone()),
                }
            }
        };
        state.doc = identity;
        if state.title.is_none() {
            state.title = facts.metadata.title.clone();
        }
        if state.layout.validate().is_err() {
            state.layout = opts.default_layout.clone();
        }

        let reflowable = facts.reflowable;
        let viewport = opts.viewport;
        let restored = match engine.call(restore_job(
            reflowable,
            state.layout.clone(),
            viewport,
            state.position.clone(),
            state.percent,
        )) {
            Ok(restored) => restored,
            // A saved layout that the engine rejects (a font it lacks, say)
            // gives way to the default layout, so the book still opens.
            Err(_) if reflowable && state.layout != opts.default_layout => {
                state.layout = opts.default_layout.clone();
                engine.call(restore_job(
                    reflowable,
                    state.layout.clone(),
                    viewport,
                    state.position.clone(),
                    state.percent,
                ))?
            }
            Err(e) => return Err(e),
        };
        let (page, position, page_count, layout_gen) = restored;
        state.position = Some(position);
        state.percent = fraction(page, page_count);
        if !facts.cre && facts.reflowable {
            // MuPDF point positions depend on the layout. Mark them when the
            // layout differs from the one they were made in.
            let key = layout_key(&state.layout, viewport);
            if state.layout_key.as_deref() != Some(key.as_str()) {
                for a in state.annotations.iter_mut() {
                    a.orphaned = true;
                }
                state.layout_key = Some(key);
            }
        }

        if let Some(report) = import.as_mut() {
            let items: Vec<(AnnotationId, Option<Pos>, Location)> = state
                .annotations
                .iter()
                .map(|a| (a.id, a.pos0.clone(), a.page.clone()))
                .collect();
            let bad: Vec<AnnotationId> = engine.call(move |w| {
                let mut bad = Vec::new();
                for (id, pos0, page) in items {
                    let mut ok = w.doc.doc().resolve(&page).is_ok();
                    if let Some(p) = &pos0 {
                        ok &= w.pos_page(p)?.is_some();
                    }
                    if !ok {
                        bad.push(id);
                    }
                }
                Ok(bad)
            })?;
            for a in state.annotations.iter_mut() {
                if bad.contains(&a.id) {
                    a.orphaned = true;
                }
            }
            report.unresolved = bad.len();
        }

        let imported = import.is_some();
        let mut session = BookSession {
            path,
            engine,
            cache,
            facts,
            state,
            state_path,
            dirty: false,
            stale,
            import,
            page,
            page_count,
            layout_gen,
            layout_pending: false,
            viewport,
            format: opts.pixel_format,
            selection: None,
            search: SearchState::default(),
            link_history: Vec::new(),
            order: OrderCache::default(),
            visit_started: Instant::now(),
            out: Vec::new(),
        };
        if imported {
            session.dirty = true;
            session.save()?;
        }
        Ok(session)
    }

    /// The book file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The identity of the book file: partial MD5 and size.
    pub fn identity(&self) -> &DocumentId {
        &self.state.doc
    }

    /// True when the file size differs from the size saved in the state. The
    /// file changed, and its saved positions may not fit it.
    pub fn state_is_stale(&self) -> bool {
        self.stale
    }

    /// Where the Kaia state file is, whether or not it exists yet.
    pub fn state_path(&self) -> &Path {
        &self.state_path
    }

    /// What the sidecar import brought in. None when the state was not imported.
    pub fn import_report(&self) -> Option<&ImportReport> {
        self.import.as_ref()
    }

    /// True for reflowable formats, which relayout on a font or size change.
    pub fn is_reflowable(&self) -> bool {
        self.facts.reflowable
    }

    /// True when CREngine opened the book. Its positions are xpointers.
    pub fn uses_cre(&self) -> bool {
        self.facts.cre
    }

    /// Pages at the current layout.
    pub fn page_count(&self) -> usize {
        self.page_count
    }

    /// The table of contents, flattened in reading order.
    pub fn toc(&self) -> &[TocEntry] {
        &self.facts.toc
    }

    /// The title and author the engine read from the file.
    pub fn metadata(&self) -> &Metadata {
        &self.facts.metadata
    }

    /// Where the reader is, and the figures for the footer.
    pub fn current(&self) -> PageInfo {
        PageInfo {
            page: self.page,
            page_count: self.page_count,
            percent: self.state.percent,
            chapter: self.chapter_title(self.page),
            pages_left_in_chapter: self.pages_left(self.page),
            location: self.state.position.clone().unwrap_or(Location::Page(self.page)),
        }
    }

    /// Moves to a location. Fails with [`BookError::LayoutPending`] during a layout change.
    pub fn go_to(&mut self, location: Location) -> Result<()> {
        self.ensure_idle()?;
        let target = location.clone();
        let (page, normalized) = self.engine.call(move |w| {
            let page = w.doc.doc().resolve(&target)?;
            let normalized = w.normalize(page, &target)?;
            Ok((page, normalized))
        })?;
        self.arrive(page, normalized);
        Ok(())
    }

    /// Moves to the next page. Returns false at the last page.
    pub fn next_page(&mut self) -> Result<bool> {
        self.ensure_idle()?;
        if self.page as usize + 1 >= self.page_count {
            return Ok(false);
        }
        self.go_to(Location::Page(self.page + 1))?;
        Ok(true)
    }

    /// Moves to the previous page. Returns false at the first page.
    pub fn prev_page(&mut self) -> Result<bool> {
        self.ensure_idle()?;
        if self.page == 0 {
            return Ok(false);
        }
        self.go_to(Location::Page(self.page - 1))?;
        Ok(true)
    }

    /// Moves to a fraction of the book, from 0.0 to 1.0. The page is the one
    /// whose `(page + 1) / count` is nearest, as KOReader's percent works.
    pub fn go_to_percent(&mut self, percent: f64) -> Result<()> {
        if !(percent.is_finite() && (0.0..=1.0).contains(&percent)) {
            return Err(BookError::Invalid(format!("percent {percent} is outside 0 to 1")));
        }
        let page = page_from_percent(percent, self.page_count);
        self.go_to(Location::Page(page))
    }

    /// Moves to the page of a table of contents entry. Returns false when the entry has no page.
    pub fn go_to_toc_entry(&mut self, index: usize) -> Result<bool> {
        match self.facts.toc.get(index).and_then(|t| t.page) {
            Some(page) => {
                self.go_to(Location::Page(page))?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Links on the current page.
    pub fn links(&mut self) -> Result<Vec<Link>> {
        self.ensure_idle()?;
        let page = self.page;
        self.engine.call(move |w| w.doc.doc().links(page))
    }

    /// The link under a screen point, in a viewport.
    pub fn link_at(&mut self, point: Point, viewport: Size) -> Result<Option<Link>> {
        self.ensure_idle()?;
        let (page, zoom) = (self.page, self.state.zoom);
        self.engine.call(move |w| {
            let t = w.transform(page, viewport, zoom)?;
            let p = t.to_page(point);
            let links = w.doc.doc().links(page)?;
            Ok(links.into_iter().find(|l| {
                let b = l.bbox;
                p.x >= b.x0 && p.x <= b.x1 && p.y >= b.y0 && p.y <= b.y1
            }))
        })
    }

    /// Follows a link. A page link moves there and remembers the place it
    /// came from, so [`BookSession::back`] returns. Returns false for links the
    /// session does not follow (external URIs).
    pub fn follow_link(&mut self, link: &Link) -> Result<bool> {
        match &link.target {
            LinkTarget::Page(page) => {
                self.ensure_idle()?;
                let here = self.state.position.clone().unwrap_or(Location::Page(self.page));
                self.go_to(Location::Page(*page))?;
                self.link_history.push(here);
                Ok(true)
            }
            LinkTarget::Uri(_) => Ok(false),
        }
    }

    /// Returns to the place before the last followed link. Returns false when there is none.
    pub fn back(&mut self) -> Result<bool> {
        self.ensure_idle()?;
        let Some(place) = self.link_history.pop() else {
            return Ok(false);
        };
        if let Err(e) = self.go_to(place.clone()) {
            self.link_history.push(place);
            return Err(e);
        }
        Ok(true)
    }

    /// The reflowable layout.
    pub fn layout(&self) -> &TextLayout {
        &self.state.layout
    }

    /// Changes the reflowable layout. The layout runs on the engine thread. The
    /// reader keeps its place: an xpointer stays put, and the page number changes.
    /// Fails for fixed-layout books. Call [`BookSession::poll`] to learn when it is done.
    pub fn set_layout(&mut self, layout: TextLayout) -> Result<()> {
        if !self.facts.reflowable {
            return Err(BookError::Doc(DocError::NotReflowable));
        }
        layout.validate().map_err(BookError::Invalid)?;
        self.state.layout = layout;
        self.dirty = true;
        self.post_layout()
    }

    /// Sets the viewport of a reflowable book. The layout takes the viewport's
    /// size. A change posts a relayout. Fixed-layout books ignore the viewport
    /// for layout and use it when they fit pages.
    pub fn set_viewport(&mut self, viewport: Size) -> Result<()> {
        if viewport.w == 0 || viewport.h == 0 {
            return Err(BookError::Invalid("viewport has no area".to_string()));
        }
        if !self.facts.reflowable || viewport == self.viewport {
            self.viewport = viewport;
            return Ok(());
        }
        self.viewport = viewport;
        self.post_layout()
    }

    /// The zoom of a fixed-layout book.
    pub fn zoom(&self) -> ZoomMode {
        self.state.zoom
    }

    /// Sets the zoom of a fixed-layout book. Reflowable books ignore it.
    pub fn set_zoom(&mut self, zoom: ZoomMode) -> Result<()> {
        if let ZoomMode::Manual { percent } = zoom {
            if !(10..=800).contains(&percent) {
                return Err(BookError::Invalid(format!(
                    "zoom {percent}% is outside 10 to 800"
                )));
            }
        }
        self.state.zoom = zoom;
        self.dirty = true;
        Ok(())
    }

    /// The transform between the current page and a viewport.
    pub fn page_transform(&mut self, viewport: Size) -> Result<PageTransform> {
        self.ensure_idle()?;
        let (page, zoom) = (self.page, self.state.zoom);
        self.engine.call(move |w| w.transform(page, viewport, zoom))
    }

    /// The current page as an image. It waits for the engine when the page is not
    /// cached, and then prerenders the next page. A reflowable page has the size
    /// of the layout, which [`BookSession::set_viewport`] sets, so the viewport
    /// only matters for fixed-layout books.
    pub fn render_current(&mut self, viewport: Size) -> Result<Arc<Pixmap>> {
        self.ensure_idle()?;
        let key = self.render_key(viewport);
        if let Some(hit) = self.lock_cache().get(&key) {
            return Ok(hit);
        }
        let (page, zoom, format) = (self.page, self.state.zoom, self.format);
        self.engine
            .call(move |w| w.render_and_prefetch(page, viewport, zoom, format))
    }

    /// The current page if it is cached. Never waits for the engine. Returns
    /// None during a layout change.
    pub fn cached_current(&mut self, viewport: Size) -> Option<Arc<Pixmap>> {
        self.drain();
        if self.layout_pending {
            return None;
        }
        let key = self.render_key(viewport);
        self.lock_cache().get(&key)
    }

    /// Asks the engine to render the current page. Returns at once. A
    /// [`BookEvent::PageRendered`] follows, and the page is then cached.
    pub fn request_render(&mut self, viewport: Size) -> Result<()> {
        self.ensure_idle()?;
        let (page, zoom, format) = (self.page, self.state.zoom, self.format);
        self.engine.post(move |w| {
            let _ = w.render_and_prefetch(page, viewport, zoom, format);
            w.send(Event::Rendered { page });
        })
    }

    /// Counters of the rendered page cache.
    pub fn cache_stats(&self) -> CacheStats {
        self.lock_cache().stats()
    }

    /// The events that happened since the last call. Does not wait.
    pub fn poll(&mut self) -> Vec<BookEvent> {
        self.drain();
        std::mem::take(&mut self.out)
    }

    /// Waits up to `timeout` for the next event, then returns the events.
    pub fn poll_timeout(&mut self, timeout: Duration) -> Vec<BookEvent> {
        if self.out.is_empty() {
            if let Ok(event) = self.engine.events.recv_timeout(timeout) {
                self.handle_event(event);
            }
        }
        self.poll()
    }

    /// Selects the word under a screen point. Returns None when the point is
    /// not on a word. Starts a new selection, and replaces the old one.
    pub fn select_word_at(&mut self, point: Point, viewport: Size) -> Result<Option<Selection>> {
        self.ensure_idle()?;
        let (page, zoom) = (self.page, self.state.zoom);
        let raw = self
            .engine
            .call(move |w| w.select(page, viewport, zoom, point, None))?;
        Ok(self.take_selection(raw, None))
    }

    /// Extends the selection to the word under a screen point. The selection
    /// keeps its first word, so dragging back shrinks it. Without a selection,
    /// this selects the word under the point.
    pub fn extend_selection(&mut self, point: Point, viewport: Size) -> Result<Option<Selection>> {
        self.ensure_idle()?;
        let Some(current) = self.selection.as_ref() else {
            return self.select_word_at(point, viewport);
        };
        let anchor = current.anchor;
        let (page, zoom) = (self.page, self.state.zoom);
        let raw = self
            .engine
            .call(move |w| w.select(page, viewport, zoom, point, Some(anchor)))?;
        Ok(self.take_selection(raw, Some(anchor)))
    }

    /// Drops the selection.
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Adds a highlight for a selection, with a style, a color and an
    /// optional note. Returns the new annotation's id.
    pub fn add_highlight(
        &mut self,
        selection: &Selection,
        drawer: Drawer,
        color: &str,
        note: Option<&str>,
    ) -> Result<AnnotationId> {
        self.ensure_idle()?;
        if selection.text.trim().is_empty() {
            return Err(BookError::Invalid("the selection has no text".to_string()));
        }
        if color.trim().is_empty() {
            return Err(BookError::Invalid("the color has no name".to_string()));
        }
        let id = self.state.assign_id();
        let page_loc = pos_location(&selection.pos0);
        let annotation = Annotation {
            id,
            datetime: now_datetime(),
            datetime_updated: None,
            drawer: Some(drawer.as_str().to_string()),
            color: Some(color.to_string()),
            text: Some(selection.text.clone()),
            note: clean_note(note),
            chapter: self.chapter_title(selection.page),
            pageno: Some(selection.page + 1),
            page: page_loc,
            pos0: Some(selection.pos0.clone()),
            pos1: Some(selection.pos1.clone()),
            orphaned: false,
        };
        self.state.annotations.push(annotation);
        self.dirty = true;
        Ok(id)
    }

    /// Sets or clears the note of an annotation.
    pub fn edit_note(&mut self, id: AnnotationId, note: Option<&str>) -> Result<()> {
        let now = now_datetime();
        let a = self.annotation_mut(id)?;
        a.note = clean_note(note);
        a.datetime_updated = Some(now);
        self.dirty = true;
        Ok(())
    }

    /// Changes the style and color of a highlight. Fails for a bookmark.
    pub fn set_highlight_style(&mut self, id: AnnotationId, drawer: Drawer, color: &str) -> Result<()> {
        let now = now_datetime();
        let a = self.annotation_mut(id)?;
        if a.is_bookmark() {
            return Err(BookError::Invalid("a bookmark has no style".to_string()));
        }
        a.drawer = Some(drawer.as_str().to_string());
        a.color = Some(color.to_string());
        a.datetime_updated = Some(now);
        self.dirty = true;
        Ok(())
    }

    /// Removes an annotation. Its text stays in the book.
    pub fn delete_annotation(&mut self, id: AnnotationId) -> Result<()> {
        let before = self.state.annotations.len();
        self.state.annotations.retain(|a| a.id != id);
        if self.state.annotations.len() == before {
            return Err(BookError::NoAnnotation(id));
        }
        self.order.keys.remove(&id);
        self.dirty = true;
        Ok(())
    }

    /// One annotation as stored. Its `pageno` and `chapter` are as of the last listing.
    pub fn annotation(&self, id: AnnotationId) -> Option<Annotation> {
        self.state.annotations.iter().find(|a| a.id == id).cloned()
    }

    /// Highlights, notes and bookmarks in reading order. `pageno` and
    /// `chapter` are computed for the current layout.
    pub fn annotations(&mut self) -> Result<Vec<Annotation>> {
        self.ensure_idle()?;
        self.ensure_order_keys()?;
        let mut items = self.state.annotations.clone();
        items.sort_by(|a, b| {
            let ka = self.order.keys.get(&a.id);
            let kb = self.order.keys.get(&b.id);
            match (ka, kb) {
                (Some(x), Some(y)) => cmp_keys(x, y).then(a.id.cmp(&b.id)),
                _ => a.id.cmp(&b.id),
            }
        });
        for a in items.iter_mut() {
            if let Some(k) = self.order.keys.get(&a.id).copied() {
                if k.page != u32::MAX {
                    a.pageno = Some(k.page + 1);
                    a.chapter = self.chapter_title(k.page);
                }
            }
        }
        Ok(items)
    }

    /// The text of a highlight, read from the book at its current layout.
    /// Empty for a bookmark.
    pub fn annotation_text(&mut self, id: AnnotationId) -> Result<String> {
        self.ensure_idle()?;
        let Some(a) = self.annotation(id) else {
            return Err(BookError::NoAnnotation(id));
        };
        match (a.pos0, a.pos1) {
            (Some(p0), Some(p1)) => Ok(self.engine.call(move |w| w.annotation_text(&p0, &p1))?),
            _ => Ok(String::new()),
        }
    }

    /// The annotations that touch the current page, in reading order, with
    /// their boxes in a viewport. Boxes come from the current layout, so they
    /// follow the text after a relayout.
    pub fn annotations_on_page(&mut self, viewport: Size) -> Result<Vec<PageAnnotation>> {
        let all = self.annotations()?;
        let page = self.page;
        let zoom = self.state.zoom;
        let spans: Vec<(AnnotationId, Option<(Pos, Pos)>)> = all
            .iter()
            .map(|a| {
                let span = match (&a.pos0, &a.pos1) {
                    (Some(p0), Some(p1)) if !a.orphaned => Some((p0.clone(), p1.clone())),
                    _ => None,
                };
                (a.id, span)
            })
            .collect();
        let rects: HashMap<AnnotationId, Vec<Rect>> = self.engine.call(move |w| {
            let t = w.transform(page, viewport, zoom)?;
            let mut out = HashMap::new();
            for (id, span) in spans {
                let rects = match span {
                    Some((p0, p1)) => w.highlight_rects(page, &p0, &p1, &t)?,
                    None => Vec::new(),
                };
                out.insert(id, rects);
            }
            Ok(out)
        })?;
        let mut out = Vec::new();
        for a in all {
            let rects = rects.get(&a.id).cloned().unwrap_or_default();
            let starts_here = self.key_page(a.id) == Some(page);
            if starts_here || !rects.is_empty() {
                out.push(PageAnnotation { annotation: a, rects });
            }
        }
        Ok(out)
    }

    /// Re-anchors an annotation by its highlighted text, searching from its
    /// last known page and wrapping around the book. Use it on annotations
    /// marked orphaned. Returns false when the text is not found, and when the
    /// annotation has no text. The search reads pages on the engine thread, so
    /// on a large book the call takes a while.
    pub fn repair_annotation(&mut self, id: AnnotationId) -> Result<bool> {
        self.ensure_idle()?;
        let Some(a) = self.annotation(id) else {
            return Err(BookError::NoAnnotation(id));
        };
        let Some(needle) = a.text.clone().filter(|t| !t.trim().is_empty()) else {
            return Ok(false);
        };
        let from = a.pageno.map_or(self.page, |n| n.saturating_sub(1));
        let found = self.engine.call(move |w| w.find_text(&needle, from))?;
        let Some((pos0, pos1)) = found else {
            return Ok(false);
        };
        let now = now_datetime();
        let target = self.annotation_mut(id)?;
        target.page = pos_location(&pos0);
        target.pos0 = Some(pos0);
        target.pos1 = Some(pos1);
        target.orphaned = false;
        target.datetime_updated = Some(now);
        self.dirty = true;
        self.order.keys.remove(&id);
        Ok(true)
    }

    /// True when the current page has a bookmark.
    pub fn is_bookmarked(&mut self) -> Result<bool> {
        self.ensure_order_keys()?;
        let page = self.page;
        Ok(self
            .state
            .annotations
            .iter()
            .any(|a| a.is_bookmark() && self.key_page(a.id) == Some(page)))
    }

    /// Adds a bookmark on the current page, or removes the one there. Returns
    /// true when a bookmark was added.
    pub fn toggle_bookmark(&mut self) -> Result<bool> {
        self.ensure_idle()?;
        self.ensure_order_keys()?;
        let page = self.page;
        let existing = self
            .state
            .annotations
            .iter()
            .position(|a| a.is_bookmark() && self.key_page(a.id) == Some(page));
        if let Some(index) = existing {
            self.state.annotations.remove(index);
            self.dirty = true;
            return Ok(false);
        }
        let location = self.state.position.clone().unwrap_or(Location::Page(page));
        let id = self.state.assign_id();
        let chapter = self.chapter_title(page);
        self.state.annotations.push(Annotation {
            id,
            datetime: now_datetime(),
            datetime_updated: None,
            drawer: None,
            color: None,
            text: None,
            note: None,
            chapter,
            pageno: Some(page + 1),
            page: location,
            pos0: None,
            pos1: None,
            orphaned: false,
        });
        self.dirty = true;
        Ok(true)
    }

    /// The bookmarks in reading order.
    pub fn bookmarks(&mut self) -> Result<Vec<Annotation>> {
        Ok(self
            .annotations()?
            .into_iter()
            .filter(|a| a.is_bookmark())
            .collect())
    }

    /// Starts a search of the whole book. It replaces a running search. Hits
    /// arrive through [`BookSession::poll`], in page order.
    pub fn start_search(&mut self, needle: &str) -> Result<()> {
        let needle = needle.trim();
        if needle.is_empty() {
            return Err(BookError::Invalid("the search text is empty".to_string()));
        }
        self.drain();
        self.search.token += 1;
        self.search.results.clear();
        self.search.cursor = None;
        self.search.running = true;
        self.engine.start_search(needle.to_string(), self.search.token)
    }

    /// Stops the running search. Hits found so far stay in the list.
    pub fn cancel_search(&mut self) -> Result<()> {
        self.drain();
        if !self.search.running {
            return Ok(());
        }
        self.search.token += 1;
        self.search.running = false;
        // Events from the stopped search carry an old token and are dropped, so
        // the stop is reported here.
        self.out.push(BookEvent::SearchFinished { cancelled: true });
        self.engine.stop_search(self.search.token)
    }

    /// True while a search is still reading pages.
    pub fn search_running(&self) -> bool {
        self.search.running
    }

    /// The hits found so far, in page order.
    pub fn search_results(&self) -> &[SearchResult] {
        &self.search.results
    }

    /// Goes to the next hit after the current one (or after the current page,
    /// when no hit was visited). Returns None after the last hit.
    pub fn next_result(&mut self) -> Result<Option<SearchResult>> {
        self.drain();
        let n = self.search.results.len();
        let next = match self.search.cursor {
            Some(i) => i + 1,
            None => self
                .search
                .results
                .iter()
                .position(|r| r.page >= self.page)
                .unwrap_or(n),
        };
        if next >= n {
            return Ok(None);
        }
        let hit = self.search.results[next].clone();
        self.go_to(hit.location.clone())?;
        self.search.cursor = Some(next);
        Ok(Some(hit))
    }

    /// Goes to the previous hit. Returns None before the first hit.
    pub fn prev_result(&mut self) -> Result<Option<SearchResult>> {
        self.drain();
        let prev = match self.search.cursor {
            Some(0) => return Ok(None),
            Some(i) => i - 1,
            None => match self.search.results.iter().rposition(|r| r.page < self.page) {
                Some(i) => i,
                None => return Ok(None),
            },
        };
        let hit = self.search.results[prev].clone();
        self.go_to(hit.location.clone())?;
        self.search.cursor = Some(prev);
        Ok(Some(hit))
    }

    /// The reading status: `reading`, `complete`, `abandoned` or a value of KOReader's own.
    pub fn status(&self) -> Option<&str> {
        self.state.status.as_deref()
    }

    /// Sets the reading status. None clears it.
    pub fn set_status(&mut self, status: Option<&str>) {
        self.state.status = status.map(str::to_string).filter(|s| !s.is_empty());
        self.dirty = true;
    }

    /// The rating, 1 to 5.
    pub fn rating(&self) -> Option<u8> {
        self.state.rating
    }

    /// Sets the rating from 0 to 5. Zero clears it.
    pub fn set_rating(&mut self, rating: u8) -> Result<()> {
        if rating > 5 {
            return Err(BookError::Invalid(format!("rating {rating} is outside 0 to 5")));
        }
        self.state.rating = (rating > 0).then_some(rating);
        self.dirty = true;
        Ok(())
    }

    /// The time spent on pages, one entry per visit, oldest first.
    pub fn reading_time(&self) -> &[ReadingTime] {
        &self.state.reading_time
    }

    /// True when a change is not yet saved.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Writes the Kaia state file, atomically. Callers save on a debounce
    /// after position changes, and on close. Dropping the session saves when dirty.
    pub fn save(&mut self) -> Result<()> {
        write_state(&self.state_path, &self.state).map_err(|source| BookError::Io {
            path: self.state_path.clone(),
            source,
        })?;
        self.dirty = false;
        Ok(())
    }

    // ----- internals -----

    fn drain(&mut self) {
        while let Ok(event) = self.engine.events.try_recv() {
            self.handle_event(event);
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Layout(Ok(outcome)) => {
                self.layout_pending = false;
                self.layout_gen = outcome.layout;
                self.page = outcome.page;
                self.page_count = outcome.page_count;
                self.state.position = Some(outcome.position);
                self.state.percent = fraction(self.page, self.page_count);
                self.selection = None;
                if !self.facts.cre && self.facts.reflowable {
                    for a in self.state.annotations.iter_mut() {
                        a.orphaned = true;
                    }
                    self.state.layout_key = Some(layout_key(&self.state.layout, self.viewport));
                }
                self.dirty = true;
                self.out.push(BookEvent::LayoutApplied {
                    page_count: self.page_count,
                });
            }
            Event::Layout(Err(message)) => {
                self.layout_pending = false;
                self.out.push(BookEvent::LayoutFailed { message });
            }
            Event::Rendered { page } => self.out.push(BookEvent::PageRendered { page }),
            Event::SearchChunk { token, page, results } => {
                if token != self.search.token {
                    return;
                }
                for mut hit in results {
                    hit.index = self.search.results.len();
                    self.search.results.push(hit);
                }
                self.out.push(BookEvent::SearchProgress {
                    page,
                    found: self.search.results.len(),
                });
            }
            Event::SearchDone { token, cancelled } => {
                if token != self.search.token {
                    return;
                }
                self.search.running = false;
                self.out.push(BookEvent::SearchFinished { cancelled });
            }
        }
    }

    /// Fails while a layout change runs. Position and text then come from one layout.
    fn ensure_idle(&mut self) -> Result<()> {
        self.drain();
        if self.layout_pending {
            return Err(BookError::LayoutPending);
        }
        Ok(())
    }

    fn post_layout(&mut self) -> Result<()> {
        let layout = self.state.layout.clone();
        let viewport = self.viewport;
        let position = self.state.position.clone();
        let percent = self.state.percent;
        self.layout_pending = true;
        self.engine.post(move |w| {
            let result = w
                .apply_layout(&layout, viewport, position, percent)
                .map_err(|e| e.to_string());
            w.send(Event::Layout(result));
        })
    }

    fn render_key(&self, viewport: Size) -> RenderKey {
        RenderKey {
            layout: self.layout_gen,
            page: self.page,
            viewport,
            zoom: self.state.zoom,
            format: self.format,
        }
    }

    fn lock_cache(&self) -> MutexGuard<'_, PageCache> {
        // The cache is only changed in whole steps, so a poisoned lock holds valid data.
        self.cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Records the visit that ends, then moves the session to `page`.
    fn arrive(&mut self, page: u32, location: Location) {
        self.record_visit();
        if self.selection.is_some() {
            self.selection = None;
        }
        self.page = page;
        self.state.position = Some(location);
        self.state.percent = fraction(page, self.page_count);
        self.dirty = true;
        self.visit_started = Instant::now();
    }

    fn record_visit(&mut self) {
        let seconds = self.visit_started.elapsed().as_secs_f64();
        if seconds <= 0.0 || seconds > MAX_VISIT_SECONDS {
            return;
        }
        self.state.reading_time.push(ReadingTime {
            datetime: now_datetime(),
            page: self.page + 1,
            seconds: (seconds * 1000.0).round() / 1000.0,
        });
        if self.state.reading_time.len() > MAX_READING_ENTRIES {
            let excess = self.state.reading_time.len() - MAX_READING_ENTRIES;
            self.state.reading_time.drain(..excess);
        }
        self.dirty = true;
    }

    fn chapter_title(&self, page: u32) -> Option<String> {
        self.facts
            .toc
            .iter()
            .filter_map(|t| t.page.filter(|p| *p <= page).map(|p| (p, t)))
            .max_by_key(|(p, _)| *p)
            .map(|(_, t)| t.title.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn pages_left(&self, page: u32) -> Option<u32> {
        if self.facts.toc.iter().all(|t| t.page.is_none()) {
            return None;
        }
        let next = self
            .facts
            .toc
            .iter()
            .filter_map(|t| t.page)
            .filter(|p| *p > page)
            .min();
        Some(match next {
            Some(n) => n - page - 1,
            None => (self.page_count as u32).saturating_sub(page + 1),
        })
    }

    fn annotation_mut(&mut self, id: AnnotationId) -> Result<&mut Annotation> {
        self.state
            .annotations
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or(BookError::NoAnnotation(id))
    }

    fn key_page(&self, id: AnnotationId) -> Option<u32> {
        self.order
            .keys
            .get(&id)
            .map(|k| k.page)
            .filter(|p| *p != u32::MAX)
    }

    /// Computes reading-order keys for annotations that do not have one at the
    /// current layout. One engine call covers all of them.
    fn ensure_order_keys(&mut self) -> Result<()> {
        if self.order.layout != self.layout_gen {
            self.order.layout = self.layout_gen;
            self.order.keys.clear();
        }
        let missing: Vec<(AnnotationId, Pos)> = self
            .state
            .annotations
            .iter()
            .filter(|a| !self.order.keys.contains_key(&a.id))
            .map(|a| {
                let pos = a.pos0.clone().unwrap_or_else(|| location_pos(&a.page));
                (a.id, pos)
            })
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let keys = self.engine.call(move |w| {
            let mut out = Vec::with_capacity(missing.len());
            for (id, pos) in missing {
                out.push((id, w.order_key(&pos)?));
            }
            Ok(out)
        })?;
        self.order.keys.extend(keys);
        Ok(())
    }

    fn take_selection(
        &mut self,
        raw: Option<RawSelection>,
        anchor: Option<(usize, usize)>,
    ) -> Option<Selection> {
        let raw = raw?;
        let anchor = anchor.unwrap_or((raw.first, raw.last));
        self.selection = Some(SelectionState { anchor });
        let screen_rects = raw
            .page_rects
            .iter()
            .map(|r| raw.transform.to_screen(*r))
            .collect();
        Some(Selection {
            page: self.page,
            text: raw.text,
            pos0: raw.pos0,
            pos1: raw.pos1,
            page_rects: raw.page_rects,
            screen_rects,
        })
    }
}

impl Drop for BookSession {
    fn drop(&mut self) {
        if self.dirty {
            let _ = self.save();
        }
    }
}

/// The job that restores the reading position on the engine thread. For a
/// reflowable book it applies the layout first.
fn restore_job(
    reflowable: bool,
    layout: TextLayout,
    viewport: Size,
    position: Option<Location>,
    percent: f64,
) -> impl FnOnce(&mut Worker) -> std::result::Result<(u32, Location, usize, u64), DocError> + Send + 'static {
    move |w: &mut Worker| {
        if reflowable {
            let out = w.apply_layout(&layout, viewport, position, percent)?;
            Ok((out.page, out.position, out.page_count, out.layout))
        } else {
            let (page, loc) = w.restore_position(position, percent)?;
            Ok((page, loc, w.page_count(), w.layout))
        }
    }
}

/// The key of a layout: its settings and the viewport. Positions made in one
/// key may not fit another when the engine lays text out by its own rules.
fn layout_key(layout: &TextLayout, viewport: Size) -> String {
    format!("{layout:?}|{}x{}", viewport.w, viewport.h)
}

/// The fraction of the book that a page reaches, as KOReader's percent does.
fn fraction(page: u32, count: usize) -> f64 {
    if count == 0 {
        return 0.0;
    }
    ((page as f64 + 1.0) / count as f64).min(1.0)
}

/// The location that names the place of a position.
fn pos_location(pos: &Pos) -> Location {
    match pos {
        Pos::Xpointer(xp) => Location::Engine(xp.clone()),
        Pos::Point { page, .. } => Location::Page(*page),
    }
}

/// The position of a location, for a bookmark that has no range.
fn location_pos(loc: &Location) -> Pos {
    match loc {
        Location::Engine(xp) => Pos::Xpointer(xp.clone()),
        Location::Page(p) => Pos::Point {
            page: *p,
            x: 0.0,
            y: 0.0,
        },
    }
}

fn clean_note(note: Option<&str>) -> Option<String> {
    note.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string)
}

fn cmp_keys(a: &OrderKey, b: &OrderKey) -> std::cmp::Ordering {
    a.page
        .cmp(&b.page)
        .then(a.y.total_cmp(&b.y))
        .then(a.x.total_cmp(&b.x))
}
