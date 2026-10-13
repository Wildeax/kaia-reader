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

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, UNIX_EPOCH};

use kaia_doc::{DocError, Engine, Link, LinkTarget, Location, Metadata, RectF, TocEntry};
use kaia_events::{Settings, Table, Value};
use kaia_gfx::{PixelFormat, Pixmap, Point, Rect, Size};

use crate::annotation::{Annotation, AnnotationId, Drawer, PageAnnotation};
use crate::cache::{CacheStats, PageCache, RenderKey};
use crate::engine::{page_from_percent, Event, Facts, Handle, OrderKey, RawSelection, Worker};
use crate::error::{BookError, Result};
use crate::layout::TextLayout;
use crate::pos::Pos;
use crate::sidecar::{map_sidecar, read_first, sidecar_candidates, ImportReport, SidecarLocation};
use crate::state::{
    read_state, write_state, BookState, DocumentId, ReadingTime, StateLoad, MAX_READING_ENTRIES,
};
use crate::time::now_datetime;
use crate::view::{PageTransform, ZoomMode};
use kaia_id::partial_md5;

/// A visit to a page longer than this is not counted. The device may have slept.
const MAX_VISIT_SECONDS: f64 = 30.0 * 60.0;
/// Default bound of the rendered page cache.
const DEFAULT_CACHE_BYTES: usize = 32 * 1024 * 1024;
const POSITION_SAVE_INTERVAL: Duration = Duration::from_secs(5);

static OPEN_BOOKS: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

struct SessionLease {
    book_key: PathBuf,
    state_key: PathBuf,
    lock_path: PathBuf,
    lock: Option<File>,
}

impl SessionLease {
    fn acquire(book: &Path, state: &Path) -> Result<Self> {
        let canonical = fs::canonicalize(book).map_err(|source| BookError::Io {
            path: book.to_path_buf(),
            source,
        })?;
        let state_key = state.to_path_buf();
        let book_key = PathBuf::from(format!(
            "{}::{}",
            state.parent().unwrap_or(Path::new(".")).display(),
            canonical.display()
        ));
        let registry = OPEN_BOOKS.get_or_init(|| Mutex::new(HashSet::new()));
        let mut open = registry.lock().unwrap_or_else(|poison| poison.into_inner());
        if open.contains(&state_key) || open.contains(&book_key) {
            return Err(BookError::AlreadyOpen(book.to_path_buf()));
        }
        open.insert(state_key.clone());
        open.insert(book_key.clone());
        drop(open);
        let lock_path = state.with_extension("lock");
        let lock = fs::create_dir_all(state.parent().unwrap_or(Path::new(".")))
            .and_then(|_| {
                File::options()
                    .read(true)
                    .write(true)
                    .create_new(true)
                    .open(&lock_path)
                    .or_else(|error| {
                        if error.kind() == std::io::ErrorKind::AlreadyExists {
                            File::options().read(true).write(true).open(&lock_path)
                        } else {
                            Err(error)
                        }
                    })
            })
            .ok()
            .and_then(|file| file.try_lock().ok().map(|_| file));
        Ok(Self {
            book_key,
            state_key,
            lock_path,
            lock,
        })
    }

    fn writable(&self) -> bool {
        self.lock.is_some()
    }
}

impl Drop for SessionLease {
    fn drop(&mut self) {
        if self.lock.is_some() {
            let _ = fs::remove_file(&self.lock_path);
        }
        let registry = OPEN_BOOKS.get_or_init(|| Mutex::new(HashSet::new()));
        let mut open = registry.lock().unwrap_or_else(|poison| poison.into_inner());
        open.remove(&self.state_key);
        open.remove(&self.book_key);
    }
}

/// Why Kaia could not use the main state file for ordinary writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateIssue {
    /// A newer Kaia wrote this state. The session is read-only.
    NewerVersion(i64),
    /// The main state cannot parse. New edits go to a recovered file.
    Corrupt,
    /// Another process holds the book's advisory lock. The session is read-only.
    Locked,
}

/// A changed book reused state from its previous partial MD5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    pub old_md5: String,
    pub new_md5: String,
    /// Annotations need new positions in the changed file.
    pub annotations_need_reanchoring: usize,
}

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
    /// Where to look for KOReader sidecars, in priority order. The newest
    /// valid file is imported.
    pub sidecars: Vec<SidecarLocation>,
    /// Bound of the rendered page cache, in bytes of pixel data.
    pub cache_bytes: usize,
    /// Pixel format of rendered pages. Gray8 suits E Ink.
    pub pixel_format: PixelFormat,
}

impl OpenOptions {
    /// Options with the defaults: automatic engine choice, a 1264 x 1680 viewport,
    /// KOReader's default text layout and sidecar locations, a 32 MiB page
    /// cache and Gray8 pages.
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        let data_dir = data_dir.into();
        Self {
            data_dir: data_dir.clone(),
            engine: Engine::Auto,
            viewport: Size { w: 1264, h: 1680 },
            default_layout: TextLayout::default(),
            sidecars: vec![
                SidecarLocation::BesideBook,
                SidecarLocation::DocSettingsDir(data_dir.join("docsettings")),
                SidecarLocation::HashDocSettingsDir(data_dir.join("hashdocsettings")),
                SidecarLocation::HistoryDir(data_dir.join("history")),
            ],
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
    /// The selected text, with line breaks retained.
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
    _lease: SessionLease,
    read_only: bool,
    state_issue: Option<StateIssue>,
    migration: Option<MigrationReport>,
    save_error: Option<String>,
    saved_position: Option<Location>,
    last_position_save: Instant,
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
    /// that file is missing, the newest valid KOReader sidecar in
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
        let mut state_path = opts
            .data_dir
            .join("kaia")
            .join("books")
            .join(format!("{md5}.json"));
        let cache = Arc::new(Mutex::new(PageCache::new(opts.cache_bytes)));
        let (engine, facts) = Handle::spawn(path.clone(), opts.engine, cache.clone())?;
        let lease = SessionLease::acquire(&path, &state_path)?;
        let mut read_only = !lease.writable();
        let mut state_issue = read_only.then_some(StateIssue::Locked);

        let mut migration = None;
        let mut pending_migration_old = None;
        let mut import = None;
        let mut load = read_state(&state_path).map_err(|source| BookError::Io {
            path: state_path.clone(),
            source,
        })?;
        if matches!(load, StateLoad::Missing) {
            if let Some(old_md5) = indexed_md5(&opts.data_dir, &path).map_err(|source| BookError::Io {
                path: path_index_file(&opts.data_dir),
                source,
            })? {
                if old_md5 != md5 {
                    let old_path = state_path.with_file_name(format!("{old_md5}.json"));
                    match read_state(&old_path).map_err(|source| BookError::Io {
                        path: old_path.clone(),
                        source,
                    })? {
                        StateLoad::Loaded(mut old) | StateLoad::Backup(mut old) => {
                            old.doc = identity.clone();
                            old.file_change_pending = true;
                            for annotation in &mut old.annotations {
                                annotation.orphaned = true;
                            }
                            migration = Some(MigrationReport {
                                old_md5,
                                new_md5: md5.clone(),
                                annotations_need_reanchoring: old.annotations.len(),
                            });
                            pending_migration_old = Some(old_path);
                            load = StateLoad::Loaded(old);
                        }
                        _ => {}
                    }
                }
            }
        }
        let mut recovery_pending = false;
        let mut state = match load {
            StateLoad::Loaded(state) | StateLoad::Backup(state) => state,
            StateLoad::Newer(version) => {
                read_only = true;
                state_issue = Some(StateIssue::NewerVersion(version));
                BookState::new(identity.clone(), opts.default_layout.clone())
            }
            StateLoad::Corrupt(backup) => {
                state_issue = Some(StateIssue::Corrupt);
                recovery_pending = true;
                let recovery = recovery_path(&state_path);
                if let StateLoad::Loaded(existing) =
                    read_state(&recovery).map_err(|source| BookError::Io {
                        path: recovery.clone(),
                        source,
                    })?
                {
                    state_path = recovery;
                    existing
                } else {
                    state_path = recovery;
                    backup.unwrap_or_else(|| BookState::new(identity.clone(), opts.default_layout.clone()))
                }
            }
            StateLoad::Missing => {
                let candidates = sidecar_candidates(&path, &md5, &opts.sidecars);
                match read_first(&candidates) {
                    Some((root, source, lossy)) => {
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
                        let mut report = imported.report;
                        report.lossy = lossy;
                        import = Some(report);
                        st
                    }
                    None => BookState::new(identity.clone(), opts.default_layout.clone()),
                }
            }
        };
        if state.doc.file_size != file_size {
            state.file_change_pending = true;
        }
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
        let saved_position = state.position.clone();
        let stale = state.file_change_pending;
        let mut session = BookSession {
            path,
            engine,
            cache,
            facts,
            state,
            state_path,
            _lease: lease,
            read_only,
            state_issue,
            migration,
            save_error: None,
            saved_position,
            last_position_save: Instant::now(),
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
            if session.read_only {
                session.save_error =
                    Some(BookError::ReadOnly(format!("{:?}", session.state_issue)).to_string());
            } else if !recovery_pending {
                if let Err(error) = session.save() {
                    session.save_error = Some(error.to_string());
                }
            }
        }
        if let Some(old_path) = pending_migration_old {
            if !session.read_only {
                match session.save() {
                    Ok(()) => {
                        let migrated = old_path.with_extension("json.migrated");
                        if !migrated.exists() {
                            if let Err(error) = fs::rename(&old_path, &migrated) {
                                session.save_error = Some(error.to_string());
                            }
                        }
                    }
                    Err(error) => session.save_error = Some(error.to_string()),
                }
            }
        }
        if !session.read_only {
            if let Err(error) = update_path_index(&opts.data_dir, &session.path, &md5) {
                session.save_error.get_or_insert_with(|| error.to_string());
            }
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

    /// Clears the file-change warning after the reader has reviewed the book.
    pub fn acknowledge_file_change(&mut self) -> Result<()> {
        self.state.file_change_pending = false;
        self.dirty = true;
        self.save()?;
        self.stale = false;
        Ok(())
    }

    /// Reports a newer or damaged state file, or a lock held by another process.
    pub fn state_issue(&self) -> Option<&StateIssue> {
        self.state_issue.as_ref()
    }

    /// Reports state moved from a previous partial MD5 after the book changed.
    pub fn migration_report(&self) -> Option<&MigrationReport> {
        self.migration.as_ref()
    }

    /// An automatic save error during sidecar import, migration or index update.
    pub fn save_error(&self) -> Option<&str> {
        self.save_error.as_deref()
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
            text_edited: None,
            note: clean_note(note),
            note_format: None,
            chapter: self.chapter_title(selection.page),
            pageno: Some(selection.page + 1),
            pageref: None,
            pboxes: None,
            ext: None,
            page: page_loc,
            pos0: Some(selection.pos0.clone()),
            pos1: Some(selection.pos1.clone()),
            orphaned: false,
        };
        self.state.annotations.push(annotation);
        self.sort_annotations()?;
        self.dirty = true;
        self.save()?;
        Ok(id)
    }

    /// Sets or clears the note of an annotation.
    pub fn edit_note(&mut self, id: AnnotationId, note: Option<&str>) -> Result<()> {
        let now = now_datetime();
        let a = self.annotation_mut(id)?;
        a.note = clean_note(note);
        a.datetime_updated = Some(now);
        self.dirty = true;
        self.save()
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
        self.save()
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
        self.save()
    }

    /// One annotation as stored. Its `pageno` and `chapter` are as of the last listing.
    pub fn annotation(&self, id: AnnotationId) -> Option<Annotation> {
        self.state.annotations.iter().find(|a| a.id == id).cloned()
    }

    /// Highlights, notes and bookmarks in reading order. `pageno` and
    /// `chapter` are computed for the current layout.
    pub fn annotations(&mut self) -> Result<Vec<Annotation>> {
        self.ensure_idle()?;
        self.sort_annotations()?;
        let mut items = self.state.annotations.clone();
        for a in items.iter_mut() {
            if let Some(k) = self.order.keys.get(&a.id).copied() {
                if k.page != u32::MAX {
                    a.pageno = Some(k.page + 1);
                    a.chapter = self.chapter_title(k.page);
                    if a.is_bookmark() {
                        a.text = a.chapter.as_ref().map(|name| format!("in {name}"));
                    }
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
        self.save()?;
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
            self.save()?;
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
            text: chapter.as_ref().map(|name| format!("in {name}")),
            text_edited: None,
            note: None,
            note_format: None,
            chapter,
            pageno: Some(page + 1),
            pageref: None,
            pboxes: None,
            ext: None,
            page: location,
            pos0: None,
            pos1: None,
            orphaned: false,
        });
        self.sort_annotations()?;
        self.dirty = true;
        self.save()?;
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
    pub fn set_status(&mut self, status: Option<&str>) -> Result<()> {
        self.state.status = status.map(str::to_string).filter(|s| !s.is_empty());
        self.dirty = true;
        self.save()
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
        self.save()
    }

    /// The time spent on pages, one entry per visit, oldest first.
    pub fn reading_time(&self) -> &[ReadingTime] {
        &self.state.reading_time
    }

    /// True when a change is not yet saved.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Writes the Kaia state file atomically. The app calls this on suspend.
    /// Dropping the session also saves when dirty.
    pub fn save(&mut self) -> Result<()> {
        if self.read_only {
            return Err(BookError::ReadOnly(format!("{:?}", self.state_issue)));
        }
        if !self.layout_pending {
            self.sort_annotations()?;
        }
        write_state(&self.state_path, &self.state).map_err(|source| BookError::Io {
            path: self.state_path.clone(),
            source,
        })?;
        self.dirty = false;
        self.saved_position = self.state.position.clone();
        self.last_position_save = Instant::now();
        Ok(())
    }

    /// Call after each page turn. Saves a changed position once five seconds
    /// have passed since its last save. Returns true when it wrote the state.
    pub fn maybe_save(&mut self, now: Instant) -> Result<bool> {
        if self.state.position == self.saved_position
            || now
                .checked_duration_since(self.last_position_save)
                .unwrap_or_default()
                < POSITION_SAVE_INTERVAL
        {
            return Ok(false);
        }
        self.save()?;
        self.last_position_save = now;
        Ok(true)
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
        chapter_title_from_toc(&self.facts.toc, page)
    }

    fn sort_annotations(&mut self) -> Result<()> {
        self.ensure_order_keys()?;
        for a in &mut self.state.annotations {
            if let Some(k) = self.order.keys.get(&a.id) {
                if k.page != u32::MAX {
                    a.pageno = Some(k.page + 1);
                    a.chapter = chapter_title_from_toc(&self.facts.toc, k.page);
                    if a.is_bookmark() {
                        a.text = a.chapter.as_ref().map(|name| format!("in {name}"));
                    }
                }
            }
        }
        self.state
            .annotations
            .sort_by(|a, b| annotation_order(a, b, &self.order.keys));
        Ok(())
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

fn chapter_title_from_toc(toc: &[TocEntry], page: u32) -> Option<String> {
    toc.iter()
        .find(|t| t.page == Some(page))
        .or_else(|| toc.iter().rev().find(|t| t.page.is_some_and(|p| p < page)))
        .map(|t| t.title.trim().to_string())
        .filter(|s| !s.is_empty())
}

impl Drop for BookSession {
    fn drop(&mut self) {
        if self.dirty {
            let _ = self.save();
        }
    }
}

fn recovery_path(main: &Path) -> PathBuf {
    let stem = main.file_stem().unwrap_or_default().to_string_lossy();
    let dir = main.parent().unwrap_or(Path::new("."));
    let first = dir.join(format!("{stem}.recovered.json"));
    if !first.exists() || matches!(read_state(&first), Ok(StateLoad::Loaded(_))) {
        return first;
    }
    for number in 1.. {
        let candidate = dir.join(format!("{stem}.recovered.{number}.json"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn path_index_file(data_dir: &Path) -> PathBuf {
    data_dir.join("kaia/books/paths.json")
}

fn indexed_md5(data_dir: &Path, book: &Path) -> std::io::Result<Option<String>> {
    let key = fs::canonicalize(book)?.to_string_lossy().into_owned();
    let index_path = path_index_file(data_dir);
    let mut old = index_path.as_os_str().to_owned();
    old.push(".old");
    if !index_path.exists() && !Path::new(&old).exists() {
        return Ok(None);
    }
    let lock = open_index_lock(&index_path).ok();
    if let Some(lock) = &lock {
        lock.lock()?;
    }
    let settings = read_index(&index_path)?;
    Ok(settings
        .read(key.as_str())
        .get("md5")
        .as_str()
        .map(str::to_string))
}

fn update_path_index(data_dir: &Path, book: &Path, md5: &str) -> std::io::Result<()> {
    let index_path = path_index_file(data_dir);
    fs::create_dir_all(index_path.parent().unwrap_or(Path::new(".")))?;
    let lock = open_index_lock(&index_path)?;
    lock.lock()?;
    let mut settings = read_index(&index_path)?;
    let canonical = fs::canonicalize(book)?;
    let metadata = fs::metadata(book)?;
    let mtime_ns = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut entry = Table::new();
    entry.insert("md5".into(), Value::from(md5));
    entry.insert("size".into(), Value::from(metadata.len() as f64));
    entry.insert("mtime_ns".into(), Value::from(mtime_ns.to_string()));
    settings.save(canonical.to_string_lossy().as_ref(), Value::Table(entry));
    settings.flush_book(&index_path)
}

fn open_index_lock(index_path: &Path) -> std::io::Result<File> {
    File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(index_path.with_extension("lock"))
}

fn read_index(index_path: &Path) -> std::io::Result<Settings> {
    match Settings::read_exact(index_path) {
        Ok(Some(settings)) => Ok(settings),
        Ok(None) | Err(_) => {
            let mut old = index_path.as_os_str().to_owned();
            old.push(".old");
            match Settings::read_exact(Path::new(&old)) {
                Ok(Some(settings)) => Ok(settings),
                Ok(None) => Settings::read_exact(index_path).map(|settings| settings.unwrap_or_default()),
                Err(error) => Err(error),
            }
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

#[cfg(all(test, unix))]
mod fault_tests {
    use super::*;

    #[test]
    fn sidecar_import_survives_disk_full_state_write() {
        let work = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let book = work.path().join("paper.pdf");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/paper.pdf"),
            &book,
        )
        .unwrap();
        let sidecar = work.path().join("paper.sdr");
        fs::create_dir_all(&sidecar).unwrap();
        fs::write(
            sidecar.join("metadata.pdf.lua"),
            b"return { [\"last_page\"] = 1 }",
        )
        .unwrap();
        if std::env::var_os("FONTDIR").is_none() {
            std::env::set_var("FONTDIR", env!("KAIA_BOOK_FONT_DIR"));
        }
        crate::state::fail_next_write_with_disk_full();
        let session = BookSession::open(&book, OpenOptions::new(data.path())).unwrap();
        assert!(session.import_report().is_some());
        assert!(session.is_dirty());
        assert!(session.save_error().is_some());
        assert!(!session.state_path().exists());
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

fn annotation_order(
    a: &Annotation,
    b: &Annotation,
    keys: &HashMap<AnnotationId, OrderKey>,
) -> std::cmp::Ordering {
    let ka = keys.get(&a.id);
    let kb = keys.get(&b.id);
    let page = match (ka, kb) {
        (Some(x), Some(y)) => x.page.cmp(&y.page),
        _ => std::cmp::Ordering::Equal,
    };
    page.then_with(|| b.is_bookmark().cmp(&a.is_bookmark()))
        .then_with(|| match (&a.page, &b.page) {
            (Location::Engine(x), Location::Engine(y)) => {
                cmp_xp(x, y).then_with(|| match (&a.pos1, &b.pos1) {
                    (Some(Pos::Xpointer(x)), Some(Pos::Xpointer(y))) => cmp_xp(x, y),
                    _ => std::cmp::Ordering::Equal,
                })
            }
            _ => fixed_annotation_order(a, b).then_with(|| match (ka, kb) {
                (Some(x), Some(y)) => cmp_keys(x, y),
                _ => std::cmp::Ordering::Equal,
            }),
        })
        .then(a.id.cmp(&b.id))
}

fn fixed_annotation_order(a: &Annotation, b: &Annotation) -> std::cmp::Ordering {
    let range = |a: &Annotation| match (&a.pos0, &a.pos1) {
        (
            Some(Pos::Point {
                page: p0,
                x: x0,
                y: y0,
            }),
            Some(Pos::Point {
                page: p1,
                x: x1,
                y: y1,
            }),
        ) => {
            let first = (*p0, *y0, *x0);
            let last = (*p1, *y1, *x1);
            Some(if cmp_point(first, last).is_le() {
                (first, last)
            } else {
                (last, first)
            })
        }
        _ => None,
    };
    match (range(a), range(b)) {
        (Some((a0, a1)), Some((b0, b1))) => cmp_point(a0, b0).then_with(|| cmp_point(a1, b1)),
        _ => std::cmp::Ordering::Equal,
    }
}

fn cmp_point(a: (u32, f32, f32), b: (u32, f32, f32)) -> std::cmp::Ordering {
    a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.total_cmp(&b.2))
}

fn cmp_xp(a: &str, b: &str) -> std::cmp::Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let start_i = i;
            let start_j = j;
            while i < a.len() && a[i].is_ascii_digit() {
                i += 1;
            }
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let left = a[start_i..i].iter().fold(0u64, |n, d| {
                n.saturating_mul(10).saturating_add((d - b'0') as u64)
            });
            let right = b[start_j..j].iter().fold(0u64, |n, d| {
                n.saturating_mul(10).saturating_add((d - b'0') as u64)
            });
            if left != right {
                return left.cmp(&right);
            }
        } else {
            if a[i] != b[j] {
                return a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
    }
    a.len().cmp(&b.len())
}
