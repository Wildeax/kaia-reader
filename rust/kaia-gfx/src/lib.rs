//! Pixmaps, geometry, drawing, text and images for the Kaia UI.
//!
//! The core types in this file are shared by every Kaia crate that draws:
//! `kaia-ui`, `kaia-display`, `kaia-doc` and the surfaces in `kaia-app`.
//! Change them only with the orchestrator's agreement.

pub mod dither;
pub mod draw;
pub mod geom;
pub mod golden;
pub mod image;
pub mod pixmap;
pub mod png;
pub mod rgba;
pub mod text;

pub use geom::{Point, Rect, Size};
pub use pixmap::{Color, PixelFormat, Pixmap};
pub use rgba::AlphaPixmap;
