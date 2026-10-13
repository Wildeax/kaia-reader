//! Document trait and engine backends.
//!
//! [`open`] picks a backend for a file and returns it behind the [`Document`]
//! trait. The reader talks only to the trait. Today the only backend is MuPDF,
//! which opens PDF, EPUB, CBZ, XPS, FB2, MOBI and HTML. CREngine follows in
//! phase R5.
//!
//! This crate has no `unsafe` code. The C bindings live in `kaia-mupdf-sys`.

pub mod document;
pub mod error;
pub mod format;
pub mod mupdf;
pub mod types;

use std::path::Path;

pub use document::Document;
pub use error::{DocError, Result};
pub use format::Format;
pub use mupdf::MupdfDocument;
pub use types::{
    Layout, Link, LinkTarget, Location, Margins, Metadata, PageSize, PageText, PointF, Quad, RectF, TextChar,
    TextLine, TocEntry,
};

/// Opens a document. The format comes from the file content when the content
/// identifies it, and from the extension otherwise.
pub fn open(path: &Path) -> Result<Box<dyn Document>> {
    let format = format::detect(path)?;
    let doc = MupdfDocument::open(path, format)?;
    Ok(Box::new(doc))
}
