//! Sticker image assets: intake, lookup and garbage collection.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use kaia_core::limits::ASSET_MAX_BYTES;
use kaia_core::{is_sha256, AssetRef};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::StoreError;
use crate::image::{self, ext_for_mime};
use crate::store::{asset_exists, now_unix, Store};

/// Seconds that `gc` keeps an asset after its last add, while no sticker has saved it.
/// A sticker that saves the asset ends the wait at once.
pub const PENDING_ASSET_GRACE_SECS: i64 = 60 * 60;

impl Store {
    /// Adds a PNG or JPEG file to `assets/` and returns its record.
    ///
    /// The type comes from the magic bytes. The size limit is 4 MiB and the
    /// dimension limit is 2048 x 2048, both read from the header. The file is
    /// stored under its SHA-256 through a temp file, fsync and rename. If the same
    /// image is already stored, the existing record is returned and `license` is
    /// not changed.
    ///
    /// Each add makes the asset pending until a sticker saves it or
    /// `PENDING_ASSET_GRACE_SECS` seconds have passed. See `gc`.
    pub fn add_asset_file(&mut self, path: &Path, license: &str) -> Result<AssetRef, StoreError> {
        self.add_asset_inner(path, license, None)
    }

    /// Same as `add_asset_file`, but the file write fails with "no space left on
    /// device" after `fail_after_bytes` bytes. Tests use this to check that a disk
    /// full error leaves no partial file and no row. Not part of the product API.
    #[doc(hidden)]
    pub fn add_asset_file_simulating_disk_full(
        &mut self,
        path: &Path,
        license: &str,
        fail_after_bytes: u64,
    ) -> Result<AssetRef, StoreError> {
        self.add_asset_inner(path, license, Some(fail_after_bytes))
    }

    fn add_asset_inner(
        &mut self,
        path: &Path,
        license: &str,
        fail_after: Option<u64>,
    ) -> Result<AssetRef, StoreError> {
        let bytes = read_bounded(path)?;
        let info = image::inspect(&bytes)?;
        let sha = sha256_hex(&bytes);
        let file_name = format!("{sha}.{}", ext_for_mime(info.mime));
        let final_path = self.assets_dir().join(&file_name);
        let now = now_unix();

        if asset_exists(self.conn(), &sha)? && final_path.is_file() {
            self.conn().execute(
                "UPDATE assets SET pending_since = ?2 WHERE sha256 = ?1",
                params![sha, now],
            )?;
            return self.asset_record(&sha);
        }

        let asset = AssetRef {
            sha256: sha.clone(),
            mime: info.mime.to_string(),
            width: info.width,
            height: info.height,
            bytes: bytes.len() as u64,
            source_name: path.file_name().map(|n| n.to_string_lossy().into_owned()),
            license: license.to_string(),
        };
        asset.validate()?;

        write_atomic(&final_path, &bytes, fail_after)?;

        let tx = self
            .conn_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // If the row exists but its file was missing, the upsert keeps the stored
        // license and sets a new pending time.
        tx.execute(
            "INSERT INTO assets (sha256, mime, width, height, bytes, source_name, license, pending_since)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (sha256) DO UPDATE SET pending_since = excluded.pending_since",
            params![
                asset.sha256,
                asset.mime,
                asset.width,
                asset.height,
                i64::try_from(asset.bytes).unwrap_or(i64::MAX),
                asset.source_name,
                asset.license,
                now,
            ],
        )?;
        tx.commit()?;
        self.asset_record(&sha)
    }

    /// Returns the file path of an asset. Fails with `NotFound` when the asset is
    /// unknown or its file is missing.
    pub fn asset_path(&self, sha256: &str) -> Result<PathBuf, StoreError> {
        if !is_sha256(sha256) {
            return Err(StoreError::InvalidArg(
                "sha256 must be 64 lowercase hex characters".into(),
            ));
        }
        let mime: Option<String> = self
            .conn()
            .query_row("SELECT mime FROM assets WHERE sha256 = ?1", [sha256], |r| {
                r.get(0)
            })
            .optional()?;
        let mime = mime.ok_or_else(|| StoreError::NotFound(format!("asset {sha256}")))?;
        let path = self
            .assets_dir()
            .join(format!("{sha256}.{}", ext_for_mime(&mime)));
        if !path.is_file() {
            return Err(StoreError::NotFound(format!("asset file for {sha256}")));
        }
        Ok(path)
    }

    /// Removes assets that no sticker references, and their files. A pending asset,
    /// one that no sticker has saved since its last add, stays until
    /// `PENDING_ASSET_GRACE_SECS` have passed since that add. If the add time is later
    /// than the clock, for example after the clock moved back, the asset stays until
    /// the clock passes the add time plus the grace period.
    ///
    /// Also removes asset files that have no row, such as leftovers from an
    /// interrupted write. Returns the number of assets removed. Never runs during
    /// ordinary writes.
    pub fn gc(&mut self) -> Result<u32, StoreError> {
        let assets_dir = self.assets_dir();
        let grace_cutoff = now_unix() - PENDING_ASSET_GRACE_SECS;
        let tx = self
            .conn_mut()
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let unreferenced: Vec<(String, String)> = {
            let mut stmt = tx.prepare(
                "SELECT sha256, mime FROM assets
                 WHERE sha256 NOT IN (SELECT asset FROM stickers WHERE asset IS NOT NULL)
                   AND (pending_since IS NULL OR pending_since < ?1)",
            )?;
            let rows = stmt.query_map([grace_cutoff], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (sha, _) in &unreferenced {
            tx.execute("DELETE FROM assets WHERE sha256 = ?1", [sha])?;
        }
        let remaining: HashSet<String> = {
            let mut stmt = tx.prepare("SELECT sha256 FROM assets")?;
            let rows = stmt.query_map([], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<HashSet<String>>>()?
        };
        tx.commit()?;

        let mut removed = 0u32;
        for (sha, mime) in &unreferenced {
            remove_if_exists(&assets_dir.join(format!("{sha}.{}", ext_for_mime(mime))))?;
            removed = removed.saturating_add(1);
        }
        for entry in fs::read_dir(&assets_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                // Temp files from interrupted writes. Not counted as assets.
                remove_if_exists(&entry.path())?;
                continue;
            }
            let sha = name.split('.').next().unwrap_or("");
            if is_sha256(sha) && !remaining.contains(sha) {
                remove_if_exists(&entry.path())?;
                removed = removed.saturating_add(1);
            }
        }
        Ok(removed)
    }

    fn asset_record(&self, sha256: &str) -> Result<AssetRef, StoreError> {
        let rec = self
            .conn()
            .query_row(
                "SELECT sha256, mime, width, height, bytes, source_name, license FROM assets WHERE sha256 = ?1",
                [sha256],
                asset_from_row,
            )
            .optional()?;
        rec.ok_or_else(|| StoreError::NotFound(format!("asset {sha256}")))
    }
}

/// Reads an asset row selected as `sha256, mime, width, height, bytes, source_name, license`.
pub(crate) fn asset_from_row(r: &Row<'_>) -> rusqlite::Result<AssetRef> {
    Ok(AssetRef {
        sha256: r.get(0)?,
        mime: r.get(1)?,
        width: r.get(2)?,
        height: r.get(3)?,
        bytes: u64::try_from(r.get::<_, i64>(4)?).unwrap_or(0),
        source_name: r.get(5)?,
        license: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
    })
}

/// All asset rows, ordered by hash.
pub(crate) fn query_assets(conn: &Connection) -> Result<Vec<AssetRef>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT sha256, mime, width, height, bytes, source_name, license FROM assets ORDER BY sha256",
    )?;
    let rows = stmt
        .query_map([], asset_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Ends the pending state of an asset. Called when a sticker saves a reference to it.
pub(crate) fn clear_pending(conn: &Connection, sha256: &str) -> Result<(), StoreError> {
    conn.execute(
        "UPDATE assets SET pending_since = NULL WHERE sha256 = ?1",
        [sha256],
    )?;
    Ok(())
}

/// Reads a file, refusing anything over the 4 MiB limit. Reads at most one byte
/// past the limit so a file that grows during the read is still caught.
pub(crate) fn read_bounded(path: &Path) -> Result<Vec<u8>, StoreError> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(ASSET_MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > ASSET_MAX_BYTES {
        return Err(StoreError::AssetRejected(format!(
            "file is larger than {ASSET_MAX_BYTES} bytes"
        )));
    }
    Ok(bytes)
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Writes `bytes` to `dest` through a temp file in the same directory: write,
/// fsync, rename, then fsync the directory. On failure the temp file is removed,
/// so `dest` is either absent or complete. `fail_after` makes the write fail with
/// "no space left on device" after that many bytes.
pub(crate) fn write_atomic(dest: &Path, bytes: &[u8], fail_after: Option<u64>) -> io::Result<()> {
    let dir = dest
        .parent()
        .ok_or_else(|| io::Error::other("destination has no parent directory"))?;
    let name = dest.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", Uuid::new_v4().simple()));
    let result = (|| -> io::Result<()> {
        let file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        let mut out = FaultyWriter {
            inner: file,
            remaining: fail_after,
        };
        out.write_all(bytes)?;
        let file = out.inner;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, dest)?;
        File::open(dir)?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Passes writes through until `remaining` bytes have gone out, then fails.
struct FaultyWriter<W> {
    inner: W,
    remaining: Option<u64>,
}

impl<W: Write> Write for FaultyWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let allowed = match self.remaining {
            None => buf.len(),
            Some(0) => return Err(disk_full()),
            Some(n) => usize::try_from(n).unwrap_or(usize::MAX).min(buf.len()),
        };
        let written = self.inner.write(&buf[..allowed])?;
        if let Some(n) = self.remaining.as_mut() {
            *n -= written as u64;
        }
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// ENOSPC. The errno value is 28 on both Linux and macOS.
fn disk_full() -> io::Error {
    io::Error::from_raw_os_error(28)
}

fn remove_if_exists(path: &Path) -> Result<(), StoreError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
