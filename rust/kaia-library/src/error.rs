//! Errors from the library index.
//!
//! A book that fails metadata extraction is not an error. The scan records the
//! reason in the book row and returns `Ok`.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// The database was written by a newer build. It is left unchanged.
    #[error("library database schema version {found} is newer than this build supports ({supported})")]
    SchemaTooNew { found: u32, supported: u32 },

    /// No book has this id.
    #[error("no book with id {0}")]
    NotFound(i64),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// A KOReader data file could not be read or parsed.
    #[error("cannot read {}: {message}", path.display())]
    KoreaderData { path: PathBuf, message: String },
}

pub type Result<T> = std::result::Result<T, LibraryError>;
