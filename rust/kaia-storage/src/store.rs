//! Database open, migrations, and sticker and document records.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kaia_core::{Anchor, DocumentId, Sticker, StickerSize, StickerState, ValidationError};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row, TransactionBehavior};
use uuid::Uuid;

use crate::error::StoreError;

/// Database file name inside the store directory.
pub const DB_FILE: &str = "kaia.sqlite3";
pub(crate) const ASSETS_DIR: &str = "assets";
pub(crate) const BACKUPS_DIR: &str = "backups";
pub(crate) const EXPORTS_DIR: &str = "exports";

const SCHEMA_V1: &str = "
CREATE TABLE documents (
  partial_md5 TEXT PRIMARY KEY,
  file_size   INTEGER NOT NULL,
  title       TEXT
);
CREATE TABLE assets (
  sha256      TEXT PRIMARY KEY,
  mime        TEXT NOT NULL,
  width       INTEGER NOT NULL,
  height      INTEGER NOT NULL,
  bytes       INTEGER NOT NULL,
  source_name TEXT,
  license     TEXT
);
CREATE TABLE stickers (
  id          TEXT PRIMARY KEY,
  doc         TEXT NOT NULL REFERENCES documents(partial_md5),
  anchor_json TEXT NOT NULL,
  asset       TEXT REFERENCES assets(sha256),
  text        TEXT,
  w REAL NOT NULL, h REAL NOT NULL,
  rotation    INTEGER NOT NULL DEFAULT 0,
  state       TEXT NOT NULL CHECK (state IN ('attached','needs_review','orphaned')),
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL
);
CREATE INDEX stickers_doc ON stickers(doc);
CREATE TABLE library_cache (
  path        TEXT PRIMARY KEY,
  partial_md5 TEXT,
  title       TEXT,
  authors     TEXT,
  last_opened INTEGER,
  percent     REAL
);
";

/// One schema step. `version` is the `user_version` after the step runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub version: u32,
    pub sql: &'static str,
}

/// The migrations this build supports, in order.
pub const MIGRATIONS: [Migration; 1] = [Migration {
    version: 1,
    sql: SCHEMA_V1,
}];

/// An open store. One connection, one writer.
pub struct Store {
    dir: PathBuf,
    conn: Connection,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl Store {
    /// Opens or creates the store in `dir`, which is the kaia directory itself.
    pub fn open(dir: &Path) -> Result<Store, StoreError> {
        Self::open_with_migrations(dir, &MIGRATIONS)
    }

    /// Opens the store with an explicit migration list. Tests use this to inject
    /// a failing migration. `migrations` must have strictly increasing versions
    /// starting at 1.
    ///
    /// If the database is newer than the last migration in the list, this returns
    /// `SchemaTooNew` before writing anything. Before running pending migrations on
    /// an existing database, it copies the file to `backups/`.
    pub fn open_with_migrations(dir: &Path, migrations: &[Migration]) -> Result<Store, StoreError> {
        check_migration_list(migrations)?;
        let supported = migrations.last().map_or(0, |m| m.version);

        fs::create_dir_all(dir)?;
        let db_path = dir.join(DB_FILE);
        let existed = db_path.exists();
        let mut conn = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;

        // Read the version before any pragma or write.
        let found: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        if found > supported {
            return Err(StoreError::SchemaTooNew { found, supported });
        }

        conn.pragma_update(None, "journal_mode", "DELETE")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;

        let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > found).collect();
        if existed && !pending.is_empty() {
            backup_database(&conn, dir, found)?;
        }
        for m in pending {
            // user_version is set inside the same transaction, so a failed step
            // rolls back both the schema change and the version bump.
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(m.sql)?;
            tx.pragma_update(None, "user_version", m.version)?;
            tx.commit()?;
        }

        for sub in [ASSETS_DIR, BACKUPS_DIR, EXPORTS_DIR] {
            fs::create_dir_all(dir.join(sub))?;
        }
        Ok(Store {
            dir: dir.to_path_buf(),
            conn,
        })
    }

    /// The kaia directory this store was opened in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    pub(crate) fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    pub(crate) fn assets_dir(&self) -> PathBuf {
        self.dir.join(ASSETS_DIR)
    }

    /// Inserts a sticker when `sticker.id` is `None`, otherwise updates the stored
    /// sticker with that id. Assigns `id`, `created_at` and `updated_at`, and
    /// returns the stored sticker.
    ///
    /// The document is inserted from `sticker.doc` if it is not known yet. For an
    /// existing document, `file_size` and `title` in `sticker.doc` are ignored.
    /// Use `document_seen` to change them.
    pub fn put_sticker(&mut self, sticker: &Sticker) -> Result<Sticker, StoreError> {
        sticker.validate()?;
        let now = now_unix();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        upsert_document(&tx, &sticker.doc)?;
        if let Some(sha) = &sticker.asset {
            if !asset_exists(&tx, sha)? {
                return Err(ValidationError::new("asset", "unknown asset, add the file first").into());
            }
        }
        let id = match &sticker.id {
            None => {
                let id = Uuid::new_v4().hyphenated().to_string();
                insert_sticker_row(&tx, &id, sticker, now, now)?;
                id
            }
            Some(raw_id) => {
                let id = canonical_id(raw_id)?;
                let stored_doc: Option<String> = tx
                    .query_row("SELECT doc FROM stickers WHERE id = ?1", [&id], |r| r.get(0))
                    .optional()?;
                match stored_doc {
                    None => return Err(StoreError::NotFound(format!("sticker {id}"))),
                    Some(doc) if doc != sticker.doc.partial_md5 => {
                        return Err(ValidationError::new(
                            "doc.partial_md5",
                            "a sticker cannot move to another document",
                        )
                        .into());
                    }
                    Some(_) => {}
                }
                tx.execute(
                    "UPDATE stickers SET anchor_json = ?2, asset = ?3, text = ?4, w = ?5, h = ?6,
                        rotation = ?7, state = ?8, updated_at = ?9
                     WHERE id = ?1",
                    params![
                        id,
                        anchor_json(&sticker.anchor)?,
                        sticker.asset,
                        sticker.text,
                        f64::from(sticker.size.w),
                        f64::from(sticker.size.h),
                        i64::from(sticker.rotation),
                        sticker.state.as_str(),
                        now,
                    ],
                )?;
                id
            }
        };
        let stored = read_sticker(&tx, &id)?.ok_or_else(|| StoreError::NotFound(format!("sticker {id}")))?;
        tx.commit()?;
        Ok(stored)
    }

    /// Returns the sticker with this id.
    pub fn get_sticker(&self, id: &str) -> Result<Sticker, StoreError> {
        let id = canonical_id(id).map_err(|_| StoreError::NotFound(format!("sticker {id}")))?;
        read_sticker(&self.conn, &id)?.ok_or_else(|| StoreError::NotFound(format!("sticker {id}")))
    }

    /// Lists the stickers of one document, oldest first. Returns an empty list for
    /// an unknown document.
    pub fn list_stickers(&self, partial_md5: &str) -> Result<Vec<Sticker>, StoreError> {
        if !kaia_core::is_partial_md5(partial_md5) {
            return Err(StoreError::InvalidArg(
                "partial_md5 must be 32 lowercase hex characters".into(),
            ));
        }
        query_stickers(
            &self.conn,
            "WHERE s.doc = ?1 ORDER BY s.created_at, s.id",
            params![partial_md5],
        )
    }

    /// Lists every sticker, grouped by document.
    pub fn list_all(&self) -> Result<Vec<Sticker>, StoreError> {
        query_stickers(&self.conn, "ORDER BY s.doc, s.created_at, s.id", params![])
    }

    /// Deletes a sticker. Assets are not removed here, see `gc`.
    pub fn delete_sticker(&mut self, id: &str) -> Result<(), StoreError> {
        let id = canonical_id(id).map_err(|_| StoreError::NotFound(format!("sticker {id}")))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let removed = tx.execute("DELETE FROM stickers WHERE id = ?1", [&id])?;
        if removed == 0 {
            return Err(StoreError::NotFound(format!("sticker {id}")));
        }
        tx.commit()?;
        Ok(())
    }

    /// Sets the state of a sticker and bumps `updated_at`.
    pub fn set_state(&mut self, id: &str, state: StickerState) -> Result<(), StoreError> {
        let id = canonical_id(id).map_err(|_| StoreError::NotFound(format!("sticker {id}")))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let changed = tx.execute(
            "UPDATE stickers SET state = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, state.as_str(), now_unix()],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound(format!("sticker {id}")));
        }
        tx.commit()?;
        Ok(())
    }

    /// Records that a document was opened. Returns `true` when the stored
    /// `file_size` differs from `file_size`. In that case every `attached` sticker
    /// of the document becomes `needs_review`, in the same transaction.
    ///
    /// A document seen for the first time is stored and returns `false`.
    pub fn document_seen(
        &mut self,
        partial_md5: &str,
        file_size: u64,
        title: Option<&str>,
    ) -> Result<bool, StoreError> {
        let doc = DocumentId {
            partial_md5: partial_md5.to_string(),
            file_size,
            title: title.map(str::to_string),
        };
        doc.validate()?;
        let size = i64::try_from(file_size)
            .map_err(|_| ValidationError::new("doc.file_size", "must fit in a signed 64-bit integer"))?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored: Option<(i64, Option<String>)> = tx
            .query_row(
                "SELECT file_size, title FROM documents WHERE partial_md5 = ?1",
                [partial_md5],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let changed = match stored {
            None => {
                upsert_document(&tx, &doc)?;
                false
            }
            Some((old_size, old_title)) => {
                let changed = old_size != size;
                let title_differs = title.is_some() && title != old_title.as_deref();
                if changed || title_differs {
                    tx.execute(
                        "UPDATE documents SET file_size = ?2, title = COALESCE(?3, title)
                         WHERE partial_md5 = ?1",
                        params![partial_md5, size, title],
                    )?;
                }
                if changed {
                    tx.execute(
                        "UPDATE stickers SET state = 'needs_review', updated_at = ?2
                         WHERE doc = ?1 AND state = 'attached'",
                        params![partial_md5, now_unix()],
                    )?;
                }
                changed
            }
        };
        tx.commit()?;
        Ok(changed)
    }
}

fn check_migration_list(migrations: &[Migration]) -> Result<(), StoreError> {
    let mut prev = 0;
    for m in migrations {
        if m.version <= prev {
            return Err(StoreError::InvalidArg(format!(
                "migration versions must start at 1 and strictly increase, got {} after {prev}",
                m.version
            )));
        }
        prev = m.version;
    }
    Ok(())
}

/// Copies the open database to `backups/kaia-v<from>-<unix time>.sqlite3` with the
/// SQLite backup API. The copy is written under a temp name and renamed when done.
fn backup_database(conn: &Connection, dir: &Path, from: u32) -> Result<PathBuf, StoreError> {
    let backups = dir.join(BACKUPS_DIR);
    fs::create_dir_all(&backups)?;
    let dest = unique_path(&backups, &format!("kaia-v{from}-{}", now_unix()), "sqlite3");
    let tmp = backups.join(format!(
        ".{}.tmp",
        dest.file_name().and_then(|n| n.to_str()).unwrap_or("backup")
    ));
    let copy = (|| -> Result<(), StoreError> {
        let mut out = Connection::open(&tmp)?;
        {
            let backup = rusqlite::backup::Backup::new(conn, &mut out)?;
            backup.run_to_completion(64, Duration::from_millis(0), None)?;
        }
        drop(out);
        File::open(&tmp)?.sync_all()?;
        fs::rename(&tmp, &dest)?;
        Ok(())
    })();
    if let Err(e) = copy {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(dest)
}

/// `<base>.<ext>`, or `<base>-N.<ext>` when that name is taken.
pub(crate) fn unique_path(dir: &Path, base: &str, ext: &str) -> PathBuf {
    let mut n = 0u32;
    loop {
        let name = if n == 0 {
            format!("{base}.{ext}")
        } else {
            format!("{base}-{n}.{ext}")
        };
        let path = dir.join(name);
        if !path.exists() {
            return path;
        }
        n += 1;
    }
}

pub(crate) fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Parses a UUID and returns its lowercase hyphenated form.
pub(crate) fn canonical_id(raw: &str) -> Result<String, StoreError> {
    Uuid::parse_str(raw)
        .map(|u| u.hyphenated().to_string())
        .map_err(|_| StoreError::InvalidArg(format!("sticker id is not a UUID: {raw:?}")))
}

pub(crate) fn anchor_json(anchor: &Anchor) -> Result<String, StoreError> {
    serde_json::to_string(anchor).map_err(|e| StoreError::InvalidArg(e.to_string()))
}

pub(crate) fn upsert_document(conn: &Connection, doc: &DocumentId) -> Result<(), StoreError> {
    let size = i64::try_from(doc.file_size)
        .map_err(|_| ValidationError::new("doc.file_size", "must fit in a signed 64-bit integer"))?;
    conn.execute(
        "INSERT OR IGNORE INTO documents (partial_md5, file_size, title) VALUES (?1, ?2, ?3)",
        params![doc.partial_md5, size, doc.title],
    )?;
    Ok(())
}

pub(crate) fn asset_exists(conn: &Connection, sha256: &str) -> Result<bool, StoreError> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM assets WHERE sha256 = ?1", [sha256], |r| r.get(0))
        .optional()?;
    Ok(found.is_some())
}

/// Inserts one sticker row with the given id and timestamps. The caller checks
/// that the document and asset exist and that the sticker is valid.
pub(crate) fn insert_sticker_row(
    conn: &Connection,
    id: &str,
    s: &Sticker,
    created_at: i64,
    updated_at: i64,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO stickers (id, doc, anchor_json, asset, text, w, h, rotation, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            id,
            s.doc.partial_md5,
            anchor_json(&s.anchor)?,
            s.asset,
            s.text,
            f64::from(s.size.w),
            f64::from(s.size.h),
            i64::from(s.rotation),
            s.state.as_str(),
            created_at,
            updated_at,
        ],
    )?;
    Ok(())
}

const STICKER_SELECT: &str = "SELECT s.id, d.partial_md5, d.file_size, d.title, s.anchor_json, s.asset,
        s.text, s.w, s.h, s.rotation, s.state, s.created_at, s.updated_at
    FROM stickers s JOIN documents d ON d.partial_md5 = s.doc";

fn read_sticker(conn: &Connection, id: &str) -> Result<Option<Sticker>, StoreError> {
    let raw = conn
        .query_row(&format!("{STICKER_SELECT} WHERE s.id = ?1"), [id], raw_sticker)
        .optional()?;
    raw.map(RawSticker::into_sticker).transpose()
}

pub(crate) fn query_stickers<P: rusqlite::Params>(
    conn: &Connection,
    tail: &str,
    params: P,
) -> Result<Vec<Sticker>, StoreError> {
    let mut stmt = conn.prepare(&format!("{STICKER_SELECT} {tail}"))?;
    let rows = stmt
        .query_map(params, raw_sticker)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(RawSticker::into_sticker).collect()
}

/// Column values as stored. Converted to a `Sticker` outside the row callback so
/// JSON and enum errors can be reported as `StoreError`.
struct RawSticker {
    id: String,
    doc_md5: String,
    doc_size: i64,
    doc_title: Option<String>,
    anchor_json: String,
    asset: Option<String>,
    text: Option<String>,
    w: f64,
    h: f64,
    rotation: i64,
    state: String,
    created_at: i64,
    updated_at: i64,
}

fn raw_sticker(row: &Row<'_>) -> rusqlite::Result<RawSticker> {
    Ok(RawSticker {
        id: row.get(0)?,
        doc_md5: row.get(1)?,
        doc_size: row.get(2)?,
        doc_title: row.get(3)?,
        anchor_json: row.get(4)?,
        asset: row.get(5)?,
        text: row.get(6)?,
        w: row.get(7)?,
        h: row.get(8)?,
        rotation: row.get(9)?,
        state: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

impl RawSticker {
    fn into_sticker(self) -> Result<Sticker, StoreError> {
        let corrupt = |col: usize, msg: String| {
            StoreError::Db(rusqlite::Error::FromSqlConversionFailure(
                col,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, msg)),
            ))
        };
        let anchor: Anchor =
            serde_json::from_str(&self.anchor_json).map_err(|e| corrupt(4, format!("anchor_json: {e}")))?;
        let state = StickerState::parse(&self.state)
            .ok_or_else(|| corrupt(10, format!("unknown state {:?}", self.state)))?;
        let file_size = u64::try_from(self.doc_size).map_err(|_| corrupt(2, "negative file_size".into()))?;
        let rotation =
            i16::try_from(self.rotation).map_err(|_| corrupt(9, "rotation out of range".into()))?;
        Ok(Sticker {
            id: Some(self.id),
            doc: DocumentId {
                partial_md5: self.doc_md5,
                file_size,
                title: self.doc_title,
            },
            anchor,
            asset: self.asset,
            text: self.text,
            size: StickerSize {
                w: self.w as f32,
                h: self.h as f32,
            },
            rotation,
            state,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}
