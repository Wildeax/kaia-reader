//! The document trait that the reader and the library use. Backends implement it.

use kaia_gfx::{PixelFormat, Pixmap, Rect, Size};

use crate::error::Result;
use crate::types::{Layout, Link, Location, Metadata, PageSize, PageText, Quad, TocEntry};

/// An open document. Page numbers are 0-based.
///
/// Methods that return a plain value (`page_count`, `toc`, `metadata`,
/// `is_reflowable`, `cover`) do not return an error. When the engine fails
/// inside them, they return the empty value: 0 pages, no entries, no metadata,
/// false, or None. The other methods return `Result`.
pub trait Document {
    /// Pages at the current layout.
    fn page_count(&self) -> usize;

    /// Size of a page in points.
    fn page_size(&self, page: u32) -> Result<PageSize>;

    /// Renders a whole page. `scale` 1.0 is 72 dpi. The pixmap is white where
    /// the page has no ink.
    fn render(&self, page: u32, scale: f32, format: PixelFormat) -> Result<Pixmap>;

    /// Renders part of a page. `clip` is in scaled device pixels, measured from
    /// the top left of the whole rendered page.
    fn render_clip(&self, page: u32, scale: f32, format: PixelFormat, clip: Rect) -> Result<Pixmap>;

    /// Table of contents, flattened depth first.
    fn toc(&self) -> Vec<TocEntry>;

    fn metadata(&self) -> Metadata;

    /// Text and character boxes of a page.
    fn page_text(&self, page: u32) -> Result<PageText>;

    /// Hit quads for `needle` on a page. An empty needle has no hits.
    fn search(&self, page: u32, needle: &str) -> Result<Vec<Quad>>;

    fn links(&self, page: u32) -> Result<Vec<Link>>;

    fn is_reflowable(&self) -> bool;

    /// Applies a reflowable layout. Fails with `NotReflowable` on fixed-layout
    /// documents.
    fn set_layout(&mut self, layout: &Layout) -> Result<()>;

    /// Renders the first page scaled to fit `max_size`, in color. None when the
    /// document has no pages or rendering fails.
    fn cover(&self, max_size: Size) -> Option<Pixmap>;

    /// Resolves a location to a page number.
    fn resolve(&self, location: &Location) -> Result<u32>;
}
