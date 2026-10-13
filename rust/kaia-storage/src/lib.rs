//! SQLite store for Kaia stickers and their image assets.
//!
//! The store directory holds `kaia.sqlite3`, `assets/`, `backups/` and
//! `exports/`. See `docs/kaia/specs/STORAGE.md`.

#![forbid(unsafe_code)]

mod assets;
mod error;
mod image;
mod store;
mod transfer;

pub use assets::PENDING_ASSET_GRACE_SECS;
pub use error::StoreError;
pub use kaia_core::{
    is_partial_md5, is_sha256, Anchor, AssetRef, DocumentId, Offset, Sticker, StickerSize, StickerState,
    TextQuote, ValidationError,
};
pub use store::{Migration, Store, DB_FILE, MIGRATIONS};
pub use transfer::ImportReport;
