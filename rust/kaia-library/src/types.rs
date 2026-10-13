//! Public data types: books, queries, scan reports.

use std::path::{Path, PathBuf};

/// Reading status. The names match KOReader's `summary.status`. A book with no
/// status is `New`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Status {
    #[default]
    New,
    Reading,
    Abandoned,
    Complete,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::New => "new",
            Status::Reading => "reading",
            Status::Abandoned => "abandoned",
            Status::Complete => "complete",
        }
    }

    pub fn parse(s: &str) -> Option<Status> {
        match s {
            "new" => Some(Status::New),
            "reading" => Some(Status::Reading),
            "abandoned" => Some(Status::Abandoned),
            "complete" => Some(Status::Complete),
            _ => None,
        }
    }
}

/// One indexed book.
///
/// `title` is the metadata title, or the file name without its extension when
/// the engine gives none, so the app can always show a title. `authors` is the
/// engine's author string as written. `series`, `series_index`, `language` and
/// `description` stay empty until the document layer exposes them.
#[derive(Debug, Clone, PartialEq)]
pub struct Book {
    pub id: i64,
    pub path: PathBuf,
    pub size: u64,
    /// Modification time in unix seconds.
    pub mtime: i64,
    /// KOReader's partial MD5, 32 lowercase hex characters.
    pub partial_md5: String,
    pub title: Option<String>,
    pub authors: Option<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub language: Option<String>,
    pub description: Option<String>,
    pub pages: Option<u32>,
    /// Lowercase file extension, such as `epub`.
    pub format: String,
    /// Unix seconds when the book first entered the index.
    pub added_at: i64,
    /// Unix seconds of the last `record_open`, or None.
    pub last_opened_at: Option<i64>,
    /// Reading progress from 0.0 to 1.0.
    pub percent: f64,
    pub status: Status,
    /// True when metadata extraction ran to completion.
    pub extracted: bool,
    /// Why extraction failed. None when it succeeded or has not run yet.
    pub failure: Option<String>,
}

/// Sort keys for [`Library::books`](crate::Library::books).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    /// Title, case-insensitive for ASCII.
    Title,
    Author,
    /// When the book entered the index.
    Added,
    /// Last `record_open`. Books never opened sort last in either direction.
    LastRead,
    Progress,
    /// Series name, series index, then title.
    Series,
    /// Manual order inside the collection named by `Filter::collection`. Falls
    /// back to title when no collection is set.
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sort {
    pub key: SortKey,
    pub descending: bool,
}

impl Sort {
    pub const fn new(key: SortKey, descending: bool) -> Self {
        Self { key, descending }
    }
}

/// Narrows a book list. Unset fields do not filter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    pub status: Option<Status>,
    /// Lowercase extension without the dot.
    pub format: Option<String>,
    /// Collection id from [`Collection::id`].
    pub collection: Option<i64>,
    /// Books in this folder and its subfolders.
    pub folder: Option<PathBuf>,
}

/// A stored cover thumbnail: the encoded image and its MIME type.
///
/// Covers are JPEG (quality 85, color kept). Schema version 2 re-encodes the
/// PNG covers of version 1, so `mime` is `image/jpeg` for every cover that
/// [`Library::open`](crate::Library::open) returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverImage {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

/// Size of a stored cover thumbnail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverSize {
    /// Fits in a 160 x 160 box.
    Small,
    /// Fits in a 480 x 480 box.
    Large,
}

/// A named collection with its book count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub id: i64,
    pub name: String,
    pub books: u64,
}

/// Progress of a scan. `done` books out of `total` have been checked.
#[derive(Debug, Clone, Copy)]
pub struct Progress<'a> {
    pub done: usize,
    pub total: usize,
    pub path: &'a Path,
}

/// What a scan did. Counts are per book file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanReport {
    /// Book files found under the roots.
    pub files_seen: usize,
    /// New rows.
    pub added: usize,
    /// Known paths whose size, mtime or content changed.
    pub changed: usize,
    /// Rows whose path changed because the same content was found elsewhere.
    pub moved: usize,
    /// Rows skipped because size and mtime match and extraction is settled.
    pub unchanged: usize,
    /// Files whose content could not be read. Their rows are kept.
    pub unreadable: usize,
    /// Calls into the document engine. Zero on an unchanged rescan.
    pub extracted: usize,
    /// Books whose extraction failed in this scan.
    pub failed: usize,
    /// Rows purged after more than 30 days missing from populated scans.
    pub removed: usize,
    /// True when the cancel flag stopped the scan. Nothing is removed then.
    pub cancelled: bool,
    pub elapsed: std::time::Duration,
}

/// What an import from a KOReader file did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// New history entries or collection memberships.
    pub added: usize,
    /// Entries that already existed. Running an import twice adds nothing.
    pub already_present: usize,
    /// Entries whose file does not exist.
    pub skipped_missing: usize,
    /// Entries whose file type is not a supported book.
    pub skipped_unsupported: usize,
    /// Entries without the expected fields.
    pub skipped_invalid: usize,
    /// String values that contained bytes replaced during UTF-8 decoding.
    pub lossy_values: usize,
}
