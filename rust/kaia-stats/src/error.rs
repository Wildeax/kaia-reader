//! Errors returned by the statistics store.

use std::path::PathBuf;

/// Result type used by this crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Failures when opening, writing, importing or querying the statistics database.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A SQLite call failed.
    #[error("statistics database: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// A file operation failed (backup copy).
    #[error("statistics file: {0}")]
    Io(#[from] std::io::Error),

    /// The database was written by a newer schema than this crate knows.
    #[error("statistics schema {found} is newer than supported schema {supported}")]
    NewerSchema { found: i64, supported: i64 },

    /// A book cannot be recorded without a positive page count.
    #[error("statistics page count must be greater than zero, got {0}")]
    InvalidPageCount(i64),

    /// The compatibility start method cannot create a frozen finished book.
    #[error("finished book is frozen by statistics settings")]
    FrozenFinishedBook,

    /// An import source has a schema this crate cannot read.
    #[error("cannot import statistics schema {found}: open the database once in KOReader to upgrade it")]
    UnsupportedSource { found: i64 },

    /// The import source and the target are the same file.
    #[error("cannot import a statistics database into itself: {0}")]
    SameDatabase(PathBuf),

    /// A month argument was not in `YYYY-MM` form.
    #[error("month must be YYYY-MM, got {0:?}")]
    InvalidMonth(String),
}
