//! Shared fixtures and builders for the storage tests.

#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use kaia_core::{Anchor, DocumentId, Offset, Placement, Sticker, StickerSize, StickerState, TextQuote};
use tempfile::TempDir;

pub const MD5_A: &str = "0123456789abcdef0123456789abcdef";
pub const MD5_B: &str = "fedcba9876543210fedcba9876543210";

/// 1x1 red PNG, 69 bytes. Checked with `sips`.
pub const PNG_RED: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00,
    0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01,
    0x01, 0x00, 0xC9, 0xFE, 0x92, 0xEF, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
    0x82,
];

/// 1x1 blue PNG, 69 bytes. Checked with `sips`.
pub const PNG_BLUE: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00,
    0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xDE, 0x00,
    0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60, 0x60, 0xF8, 0x0F, 0x00, 0x01, 0x03,
    0x01, 0x00, 0x08, 0x89, 0xC2, 0xEC, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60,
    0x82,
];

/// Minimal JPEG: SOI, APP0 (JFIF), SOF0 with height 2 and width 3, EOI.
/// Enough for header parsing. It is not a decodable image.
pub const JPEG_3X2: &[u8] = &[
    0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00,
    0x01, 0x00, 0x00, 0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x02, 0x00, 0x03, 0x01, 0x01, 0x11, 0x00, 0xFF,
    0xD9,
];

/// A temp directory and the kaia store path inside it (the directory is not created).
pub fn temp_store_dir() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().expect("temp dir");
    let dir = tmp.path().join("kaia");
    (tmp, dir)
}

pub fn write_file(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    fs::create_dir_all(dir).expect("create dir");
    let path = dir.join(name);
    fs::write(&path, bytes).expect("write fixture");
    path
}

/// Moves `pending_since` of an asset back by `secs`, as if that much time had passed
/// since its last add. The store must not hold a write open.
pub fn age_pending_asset(dir: &Path, sha256: &str, secs: i64) {
    let conn = rusqlite::Connection::open(dir.join(kaia_storage::DB_FILE)).expect("open db");
    let changed = conn
        .execute(
            "UPDATE assets SET pending_since = pending_since - ?2 WHERE sha256 = ?1",
            rusqlite::params![sha256, secs],
        )
        .expect("age asset");
    assert_eq!(changed, 1, "asset {sha256} must exist and be pending");
}

pub fn sha256_of(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn doc(md5: &str, file_size: u64) -> DocumentId {
    DocumentId {
        partial_md5: md5.to_string(),
        file_size,
        title: Some("Test book".to_string()),
    }
}

/// A reflowable text-only sticker with the given quote.
pub fn reflowable(md5: &str, exact: &str, prefix: &str, suffix: &str) -> Sticker {
    Sticker {
        id: None,
        doc: doc(md5, 1000),
        anchor: Anchor::Reflowable {
            locator: "/body/DocFragment[3]/body/p[2]".to_string(),
            quote: TextQuote {
                exact: exact.to_string(),
                prefix: prefix.to_string(),
                suffix: suffix.to_string(),
            },
            offset: Offset { dx: 0.0, dy: -1.2 },
            placement: Placement::Margin,
        },
        asset: None,
        text: Some("caption".to_string()),
        size: StickerSize { w: 0.1, h: 0.07 },
        rotation: 0,
        state: StickerState::Attached,
        created_at: 0,
        updated_at: 0,
    }
}

/// A fixed-page sticker with an image asset.
pub fn fixed_with_asset(md5: &str, sha: &str) -> Sticker {
    Sticker {
        id: None,
        doc: doc(md5, 1000),
        anchor: Anchor::Fixed {
            page: 12,
            x: 0.25,
            y: 0.5,
        },
        asset: Some(sha.to_string()),
        text: None,
        size: StickerSize { w: 0.2, h: 0.2 },
        rotation: -30,
        state: StickerState::Attached,
        created_at: 0,
        updated_at: 0,
    }
}

/// A book-level text sticker.
pub fn book_text(md5: &str, text: &str) -> Sticker {
    Sticker {
        id: None,
        doc: doc(md5, 1000),
        anchor: Anchor::Book {},
        asset: None,
        text: Some(text.to_string()),
        size: StickerSize { w: 0.3, h: 0.1 },
        rotation: 90,
        state: StickerState::Attached,
        created_at: 0,
        updated_at: 0,
    }
}
