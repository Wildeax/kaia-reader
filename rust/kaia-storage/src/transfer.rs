//! Export and import of stickers and assets as a directory with `stickers.json`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kaia_core::{AssetRef, DocumentId, Sticker, ValidationError};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::assets::{clear_pending, read_bounded, sha256_hex, write_atomic};
use crate::error::StoreError;
use crate::image::{self, ext_for_mime};
use crate::store::{
    asset_exists, canonical_id, insert_sticker_row, now_unix, query_stickers, upsert_document, Store,
    ASSETS_DIR, EXPORTS_DIR,
};

const FORMAT: &str = "kaia-stickers";
const FORMAT_VERSION: u32 = 1;
const STICKERS_FILE: &str = "stickers.json";

/// Counts from an import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub stickers_added: u32,
    pub stickers_skipped: u32,
    pub assets_added: u32,
}

/// The `stickers.json` layout from `STORAGE.md`. Unknown top-level fields are ignored.
#[derive(Debug, Serialize, Deserialize)]
struct ExportFile {
    format: String,
    version: u32,
    exported_at: i64,
    app_version: String,
    #[serde(default)]
    documents: Vec<DocumentId>,
    #[serde(default)]
    assets: Vec<AssetRef>,
    #[serde(default)]
    stickers: Vec<Sticker>,
}

impl Store {
    /// Writes `exports/kaia-export-<unix time>/` with `stickers.json` and an
    /// `assets/` copy. Returns that directory. The snapshot is read in one
    /// transaction.
    pub fn export(&mut self) -> Result<PathBuf, StoreError> {
        let now = now_unix();
        let exports = self.dir().join(EXPORTS_DIR);
        let out = unique_dir(&exports, &format!("kaia-export-{now}"))?;
        fs::create_dir_all(out.join(ASSETS_DIR))?;

        let tx = self.conn_mut().transaction()?;
        let documents = query_documents(&tx)?;
        let assets = crate::assets::query_assets(&tx)?;
        let stickers = query_stickers(&tx, "ORDER BY s.doc, s.created_at, s.id", params![])?;
        tx.commit()?;

        for asset in &assets {
            let src = self.asset_path(&asset.sha256)?;
            let bytes = fs::read(&src)?;
            let name = format!("{}.{}", asset.sha256, ext_for_mime(&asset.mime));
            write_atomic(&out.join(ASSETS_DIR).join(name), &bytes, None)?;
        }

        let file = ExportFile {
            format: FORMAT.into(),
            version: FORMAT_VERSION,
            exported_at: now,
            app_version: format!("kaia-{}", env!("CARGO_PKG_VERSION")),
            documents,
            assets,
            stickers,
        };
        let json = serde_json::to_vec_pretty(&file).map_err(|e| StoreError::InvalidArg(e.to_string()))?;
        write_atomic(&out.join(STICKERS_FILE), &json, None)?;
        Ok(out)
    }

    /// Imports a directory written by `export`.
    ///
    /// The whole file is validated before anything is written. Assets are copied
    /// first, each checked against its hash and header. Then the rows go in one
    /// transaction. Stickers whose id already exists are skipped. An unknown
    /// `version` is rejected.
    pub fn import(&mut self, dir: &Path) -> Result<ImportReport, StoreError> {
        let raw = fs::read(dir.join(STICKERS_FILE))?;
        let file: ExportFile =
            serde_json::from_slice(&raw).map_err(|e| ValidationError::new("stickers.json", e.to_string()))?;
        if file.format != FORMAT {
            return Err(ValidationError::new("format", format!("expected {FORMAT:?}")).into());
        }
        if file.version != FORMAT_VERSION {
            return Err(ValidationError::new(
                "version",
                format!(
                    "unsupported export version {}, this build reads {FORMAT_VERSION}",
                    file.version
                ),
            )
            .into());
        }
        for doc in &file.documents {
            doc.validate()?;
        }
        for asset in &file.assets {
            asset.validate()?;
        }
        let mut ids = Vec::with_capacity(file.stickers.len());
        for sticker in &file.stickers {
            sticker.validate()?;
            let raw_id = sticker
                .id
                .as_deref()
                .ok_or_else(|| ValidationError::new("id", "required in export"))?;
            ids.push(canonical_id(raw_id)?);
        }

        let mut assets_added = 0u32;
        let mut new_assets = Vec::new();
        for asset in &file.assets {
            if asset_exists(self.conn(), &asset.sha256)? {
                continue;
            }
            let name = format!("{}.{}", asset.sha256, ext_for_mime(&asset.mime));
            let bytes = read_bounded(&dir.join(ASSETS_DIR).join(&name))?;
            if sha256_hex(&bytes) != asset.sha256 {
                return Err(StoreError::AssetRejected(format!(
                    "{name} does not match its hash"
                )));
            }
            let info = image::inspect(&bytes)?;
            if info.mime != asset.mime || info.width != asset.width || info.height != asset.height {
                return Err(StoreError::AssetRejected(format!(
                    "{name} does not match its record"
                )));
            }
            write_atomic(&self.assets_dir().join(&name), &bytes, None)?;
            new_assets.push(asset);
            assets_added += 1;
        }

        let tx = self
            .conn_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for doc in &file.documents {
            upsert_document(&tx, doc)?;
        }
        for asset in &new_assets {
            tx.execute(
                "INSERT OR IGNORE INTO assets (sha256, mime, width, height, bytes, source_name, license)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    asset.sha256,
                    asset.mime,
                    asset.width,
                    asset.height,
                    i64::try_from(asset.bytes).unwrap_or(i64::MAX),
                    asset.source_name,
                    asset.license,
                ],
            )?;
        }

        let mut report = ImportReport {
            stickers_added: 0,
            stickers_skipped: 0,
            assets_added,
        };
        for (sticker, id) in file.stickers.iter().zip(&ids) {
            if sticker_exists(&tx, id)? {
                report.stickers_skipped += 1;
                continue;
            }
            if !document_exists(&tx, &sticker.doc.partial_md5)? {
                return Err(ValidationError::new("doc.partial_md5", "unknown document").into());
            }
            if let Some(sha) = &sticker.asset {
                if !asset_exists(&tx, sha)? {
                    return Err(ValidationError::new("asset", "unknown asset").into());
                }
                clear_pending(&tx, sha)?;
            }
            insert_sticker_row(&tx, id, sticker, sticker.created_at, sticker.updated_at)?;
            report.stickers_added += 1;
        }
        tx.commit()?;
        Ok(report)
    }
}

fn query_documents(conn: &Connection) -> Result<Vec<DocumentId>, StoreError> {
    let mut stmt =
        conn.prepare("SELECT partial_md5, file_size, title FROM documents ORDER BY partial_md5")?;
    let rows = stmt.query_map([], |r| {
        Ok(DocumentId {
            partial_md5: r.get(0)?,
            file_size: u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
            title: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn sticker_exists(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM stickers WHERE id = ?1", [id], |r| r.get(0))
        .optional()?;
    Ok(found.is_some())
}

fn document_exists(conn: &Connection, md5: &str) -> Result<bool, StoreError> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM documents WHERE partial_md5 = ?1", [md5], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(found.is_some())
}

/// Creates `<base>`, or `<base>-N` when taken. Creating the directory is the check,
/// so two exports in the same second cannot share a directory.
fn unique_dir(parent: &Path, base: &str) -> Result<PathBuf, StoreError> {
    let mut n = 0u32;
    loop {
        let name = if n == 0 {
            base.to_string()
        } else {
            format!("{base}-{n}")
        };
        let path = parent.join(name);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e.into()),
        }
    }
}
