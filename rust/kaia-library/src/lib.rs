//! Library scanner and SQLite index: books, metadata, covers, collections, search.
//!
//! [`Library`] is the index. It stores one row per book file (keyed by path,
//! identified by KOReader's partial MD5), extracted metadata, two cover
//! thumbnails, collections, reading history and a full-text index.
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
mod query;
mod scan;
mod schema;
mod types;

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

pub use error::{LibraryError, Result};
pub use kaia_id::partial_md5;
pub use types::{
    Book, Collection, CoverImage, CoverSize, Filter, ImportReport, Progress, ScanReport, Sort, SortKey,
    Status,
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
}

impl Library {
    /// Opens the database at `db_path`, creating it and running migrations.
    /// Fails with `SchemaTooNew` when the file was written by a newer build. The
    /// file is then left unchanged.
    pub fn open(db_path: &Path) -> Result<Library> {
        Ok(Library {
            conn: schema::open(db_path)?,
            extract_budget: DEFAULT_EXTRACT_BUDGET,
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
