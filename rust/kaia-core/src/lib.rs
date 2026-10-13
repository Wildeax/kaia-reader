//! Kaia annotation types, validation and quote re-anchoring.
//!
//! This crate does no I/O and does not use SQLite. The JSON shapes follow
//! `docs/kaia/specs/STORAGE.md`.

#![forbid(unsafe_code)]

mod anchor;
mod asset;
mod document;
mod error;
pub mod quote;
mod sticker;

pub use anchor::{Anchor, Offset, TextQuote};
pub use asset::{is_sha256, AssetRef};
pub use document::{is_partial_md5, DocumentId};
pub use error::ValidationError;
pub use sticker::{Sticker, StickerSize, StickerState};

/// Limits from `STORAGE.md`. Validation and the store both use these.
pub mod limits {
    /// Maximum length of `quote.exact`, in chars.
    pub const QUOTE_EXACT_MAX_CHARS: usize = 1000;
    /// Maximum length of `quote.prefix` and `quote.suffix`, in chars.
    pub const QUOTE_CONTEXT_MAX_CHARS: usize = 64;
    /// Absolute bound for `offset.dx` and `offset.dy`.
    pub const OFFSET_ABS_MAX: f32 = 50.0;
    /// Maximum length of sticker text, in chars.
    pub const TEXT_MAX_CHARS: usize = 2000;
    /// Absolute bound for sticker rotation, in degrees.
    pub const ROTATION_ABS_MAX: i16 = 180;
    /// Maximum encoded asset size, in bytes (4 MiB).
    pub const ASSET_MAX_BYTES: u64 = 4 * 1024 * 1024;
    /// Maximum asset width and height, in pixels.
    pub const ASSET_MAX_DIM: u32 = 2048;
}
