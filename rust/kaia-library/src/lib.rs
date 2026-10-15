//! Library scanner and SQLite index: books, metadata, covers, collections, search.
//!
//! [`Library`] is the index. It stores one row per book file (keyed by path,
//! identified by KOReader's partial MD5), extracted metadata, two cover
//! thumbnails, collections (favorites are one of them), tags, reading history and
//! a full-text index.
//!
//! # Threads
//!
//! `Library` is `Send` but not `Sync`. Give each thread its own handle, opened
//! on the same database path. SQLite serializes writes, and the busy timeout
//! waits for a lock held by another handle. [`Library::scan`] reads files and
//! runs the document engine, so call it from a worker thread and not from the
//! UI thread. Document objects are created and dropped inside `scan`, so they
//! never cross threads.
//!
//! # Schema
//!
//! The schema version is `PRAGMA user_version`. See `schema.rs` for the tables.
//! Metadata fields that `kaia-doc` does not expose yet (language, description)
//! are NULL. Series comes from the EPUB package.

mod error;
mod extract;
mod import;
mod marks;
mod query;
mod scan;
mod schema;
pub mod storage;
mod types;

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

pub use error::{LibraryError, Result};
pub use kaia_id::partial_md5;
pub use marks::{FAVORITES, MAX_TAGS_PER_BOOK, MAX_TAG_CHARS};
pub use types::{
    Book, Collection, CoverImage, CoverSize, Filter, ImportReport, Progress, ScanReport, Sort, SortKey,
    Status, Tag,
};

/// Time allowed for extracting one book. Passed to [`Library::set_extract_budget`]
/// to change it.
pub const DEFAULT_EXTRACT_BUDGET: Duration = Duration::from_secs(10);

/// Distinct searches that [`Library::record_search`] keeps.
pub const RECENT_SEARCHES: usize = 10;

/// A handle on the library database.
pub struct Library {
    pub(crate) conn: Connection,
    pub(crate) extract_budget: Duration,
    clock: Box<dyn Fn() -> i64 + Send>,
}

impl Library {
    /// Opens the database at `db_path`, creating it and running migrations.
    /// Fails with `SchemaTooNew` when the file was written by a newer build. The
    /// file is then left unchanged.
    pub fn open(db_path: &Path) -> Result<Library> {
        Ok(Library {
            conn: schema::open(db_path)?,
            extract_budget: DEFAULT_EXTRACT_BUDGET,
            clock: Box::new(now_unix),
        })
    }

    /// Sets the time allowed for extracting one book. A book that takes longer
    /// is recorded with a failure. Its row keeps the file name as its title.
    pub fn set_extract_budget(&mut self, budget: Duration) {
        self.extract_budget = budget;
    }

    pub fn extract_budget(&self) -> Duration {
        self.extract_budget
    }

    /// Replaces the clock that stamps rows with Unix seconds. The stamps are the
    /// time a book is found missing, the time a book is added and the time it is
    /// opened. The default reads the system clock. Tests use this to move time.
    pub fn set_clock(&mut self, clock: impl Fn() -> i64 + Send + 'static) {
        self.clock = Box::new(clock);
    }

    /// The current time in Unix seconds, from the clock in use.
    pub(crate) fn now(&self) -> i64 {
        (self.clock)()
    }
}

pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Library>();
    }
}

mod nickel;
