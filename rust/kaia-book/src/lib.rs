//! Reading sessions: document, positions, page cache, selection, highlights,
//! bookmarks and search.
//!
//! A [`BookSession`] is the handle that the Reader surface uses. It owns one
//! engine thread. That thread owns the document, because MuPDF and CREngine
//! handles are not `Send`. Calls from the UI thread either read state that the
//! session keeps, or send a job to the engine thread and wait for it. Layout
//! changes and search run without blocking the UI thread. See
//! [`session`] for the threading rules.
//!
//! Per-book state (position, layout, annotations, bookmarks, reading time,
//! status and rating) lives in a Kaia file keyed by the partial MD5 of the
//! book. KOReader's sidecar files are read for import and never written.
//! See [`state`] and [`sidecar`].
//!
//! The app calls [`BookSession::maybe_save`] after each page turn, passing the
//! current [`std::time::Instant`], and [`BookSession::save`] on suspend. The
//! session saves annotation, bookmark, status and rating edits before their
//! methods return. The app should reuse one session per book. It should show
//! [`BookSession::state_issue`], [`BookSession::migration_report`] and
//! [`BookSession::save_error`] when present. After the reader reviews a changed
//! file, the app calls [`BookSession::acknowledge_file_change`].

pub mod annotation;
pub mod cache;
pub mod error;

pub mod layout;
pub mod pos;
pub mod session;
pub mod sidecar;
pub mod state;
pub mod text;
pub mod view;

mod doc;
mod engine;
mod time;

pub use annotation::{
    highlight_rgb, Annotation, AnnotationId, Drawer, PageAnnotation, HIGHLIGHT_COLORS, HIGHLIGHT_DRAWERS,
};
pub use cache::CacheStats;
pub use error::{BookError, Result};
pub use kaia_doc::{Engine, Link, LinkTarget, Location, Margins, Metadata, PageSize, RectF, TocEntry};
pub use kaia_gfx::{PixelFormat, Pixmap, Point, Rect, Size};
pub use kaia_id::partial_md5;
pub use layout::TextLayout;
pub use pos::Pos;
pub use session::{
    BookEvent, BookSession, MigrationReport, OpenOptions, PageInfo, SearchResult, Selection, StateIssue,
};
pub use sidecar::{ImportReport, SidecarLocation};
pub use state::{DocumentId, ReadingTime};
pub use time::local_date;
pub use view::{PageTransform, ZoomMode};
