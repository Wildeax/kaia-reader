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

    /// A collection with this name already exists, ignoring case.
    #[error("collection name already exists: {0}")]
    CollectionNameExists(String),

    /// No collection has this id.
    #[error("no collection with id {0}")]
    CollectionNotFound(i64),

    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// A tag is empty after its spaces are trimmed.
    #[error("a tag cannot be empty")]
    TagEmpty,

    /// A tag has more characters than the limit, which is the number given.
    #[error("a tag can have at most {0} characters")]
    TagTooLong(usize),

    /// The book has the number of tags given, so it takes no new one.
    #[error("a book can have at most {0} tags")]
    TooManyTags(usize),

    /// A KOReader data file could not be read or parsed.
    #[error("cannot read {}: {message}", path.display())]
    KoreaderData { path: PathBuf, message: String },
}

pub type Result<T> = std::result::Result<T, LibraryError>;
