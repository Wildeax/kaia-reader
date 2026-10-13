//! Errors for document access.

use std::fmt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum DocError {
    /// The file could not be read.
    Io { path: PathBuf, source: std::io::Error },
    /// The file is not a format the document engines support.
    UnknownFormat(PathBuf),
    /// The engine rejected the file or an operation. The message comes from MuPDF.
    Engine(String),
    /// The page number is not in the document.
    PageOutOfRange { page: u32, count: usize },
    /// A reflowable-only operation was used on a fixed-layout document.
    NotReflowable,
    /// An argument has a value the engine cannot use.
    InvalidArgument(String),
    /// A location could not be resolved to a page.
    UnresolvedLocation(String),
}

pub type Result<T> = std::result::Result<T, DocError>;

impl fmt::Display for DocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocError::Io { path, source } => write!(f, "cannot read {}: {}", path.display(), source),
            DocError::UnknownFormat(path) => write!(f, "unsupported document: {}", path.display()),
            DocError::Engine(msg) => write!(f, "document engine error: {msg}"),
            DocError::PageOutOfRange { page, count } => {
                write!(f, "page {page} is outside the document ({count} pages)")
            }
            DocError::NotReflowable => write!(f, "the document has a fixed layout"),
            DocError::InvalidArgument(msg) => write!(f, "invalid argument: {msg}"),
            DocError::UnresolvedLocation(loc) => write!(f, "cannot resolve location {loc}"),
        }
    }
}

impl std::error::Error for DocError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DocError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl From<kaia_mupdf_sys::Error> for DocError {
    fn from(e: kaia_mupdf_sys::Error) -> Self {
        use kaia_mupdf_sys::ErrorKind;
        match e.kind {
            ErrorKind::Argument => DocError::InvalidArgument(e.message),
            ErrorKind::PageRange => DocError::Engine(e.message),
            ErrorKind::OutOfMemory => DocError::Engine("out of memory".to_string()),
            ErrorKind::Mupdf => DocError::Engine(e.message),
        }
    }
}
