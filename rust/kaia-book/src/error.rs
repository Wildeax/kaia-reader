//! Errors of the reading session.

use std::path::PathBuf;

use kaia_doc::DocError;

#[derive(Debug, thiserror::Error)]
pub enum BookError {
    /// The document engine failed. The message is the engine's.
    #[error(transparent)]
    Doc(#[from] DocError),
    /// A file could not be read or written.
    #[error("cannot access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The engine thread has stopped. The session cannot be used any more.
    #[error("the book engine has stopped")]
    EngineStopped,
    /// A layout change is running. Page turns and jumps wait until it ends.
    #[error("a layout change is running")]
    LayoutPending,
    /// The caller passed a value the session cannot use.
    #[error("invalid argument: {0}")]
    Invalid(String),
    /// A named annotation does not exist.
    #[error("no annotation with id {0}")]
    NoAnnotation(u64),
    /// This process already has a session for the same book or state file.
    #[error("book is already open: {0}")]
    AlreadyOpen(PathBuf),
    /// The session can read the book but cannot replace its state file.
    #[error("book state is read-only: {0}")]
    ReadOnly(String),
}

pub type Result<T> = std::result::Result<T, BookError>;
