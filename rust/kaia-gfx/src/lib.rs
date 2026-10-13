//! Pixmaps, geometry, drawing, text and images for the Kaia UI.
//!
//! The core types in this file are shared by every Kaia crate that draws:
//! `kaia-ui`, `kaia-display`, `kaia-doc` and the surfaces in `kaia-app`.
//! Change them only with the orchestrator's agreement.

pub mod geom;
pub mod pixmap;

pub use geom::{Point, Rect, Size};
pub use pixmap::{Color, PixelFormat, Pixmap};
