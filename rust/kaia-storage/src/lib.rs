//! SQLite store for Kaia stickers, their image assets and the word list.
//!
//! The store directory holds `kaia.sqlite3`, `assets/`, `backups/` and
//! `exports/`. See `docs/kaia/specs/STORAGE.md`.

#![forbid(unsafe_code)]

mod assets;
mod error;
mod image;
mod ink;
pub use ink::{InkPoint, InkStroke};
mod store;
mod transfer;
mod words;

pub use assets::PENDING_ASSET_GRACE_SECS;
pub use error::StoreError;
pub use kaia_core::quote::locate as quote_locate;
pub use kaia_core::{
    is_partial_md5, is_sha256, Anchor, AssetRef, DocumentId, Offset, Placement, Sticker, StickerSize,
    StickerState, TextQuote, ValidationError,
};
pub use store::{Migration, Store, DB_FILE, MIGRATIONS};
pub use transfer::ImportReport;
pub use words::{Word, BOOK_TITLE_MAX_CHARS, CONTEXT_MAX_CHARS, WORD_MAX_CHARS};
