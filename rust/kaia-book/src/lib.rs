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

pub mod annotation;
pub mod cache;
pub mod error;
pub mod hash;
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

pub use annotation::{Annotation, AnnotationId, Drawer, PageAnnotation};
pub use cache::CacheStats;
pub use error::{BookError, Result};
pub use hash::partial_md5;
pub use kaia_doc::{Engine, Link, LinkTarget, Location, Margins, Metadata, PageSize, RectF, TocEntry};
pub use kaia_gfx::{PixelFormat, Pixmap, Point, Rect, Size};
pub use layout::TextLayout;
pub use pos::Pos;
pub use session::{BookEvent, BookSession, OpenOptions, PageInfo, SearchResult, Selection};
pub use sidecar::{ImportReport, SidecarLocation};
pub use state::{DocumentId, ReadingTime};
pub use view::{PageTransform, ZoomMode};
