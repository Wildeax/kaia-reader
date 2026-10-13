//! Errors from opening StarDict files and folders.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Errors from opening a dictionary or a dictionary folder.
///
/// Errors from parsing carry the path of the file that failed. The path is empty
/// until the caller attaches it with [`Error::at`].
#[derive(Debug)]
pub enum Error {
    /// A file or folder could not be read.
    Io { path: PathBuf, source: io::Error },
    /// A file exists but does not follow the StarDict format.
    Format { path: PathBuf, reason: String },
    /// A file the dictionary needs is not on disk.
    Missing { path: PathBuf },
}

impl Error {
    pub(crate) fn format(reason: impl Into<String>) -> Self {
        Error::Format {
            path: PathBuf::new(),
            reason: reason.into(),
        }
    }

    /// Attaches the file path to an error that does not have one yet.
    pub(crate) fn at(self, path: &Path) -> Self {
        match self {
            Error::Io { source, .. } => Error::Io {
                path: path.to_path_buf(),
                source,
            },
            Error::Format { reason, .. } => Error::Format {
                path: path.to_path_buf(),
                reason,
            },
            Error::Missing { .. } => Error::Missing {
                path: path.to_path_buf(),
            },
        }
    }
}

impl From<io::Error> for Error {
    fn from(source: io::Error) -> Self {
        Error::Io {
            path: PathBuf::new(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Error::Format { path, reason } if path.as_os_str().is_empty() => write!(f, "{reason}"),
            Error::Format { path, reason } => write!(f, "{}: {reason}", path.display()),
            Error::Missing { path } => write!(f, "{}: file is missing", path.display()),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
