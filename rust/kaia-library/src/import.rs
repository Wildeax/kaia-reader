//! Imports KOReader's `history.lua` and `collection.lua` into the index.
//!
//! Both files are Lua tables in KOReader's `dump.lua` layout. They are read by
//! `kaia-luadata`, which never runs Lua code. Imports are idempotent: a second
//! run adds nothing it already added.

use std::fs;
use std::path::Path;

use kaia_doc::Format;
use kaia_luadata::{Key, Table, Value};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::error::{LibraryError, Result};
use crate::query::canonical_or;
use crate::types::ImportReport;
use crate::{now_unix, Library};
use kaia_id::partial_md5;

impl Library {
    /// Imports KOReader's `history.lua`: a list of `{ time = <unix seconds>,
    /// file = <path> }`. Each entry adds a history row and moves the book's
    /// last-opened time forward. Entries whose file is missing or unsupported
    /// are counted and skipped.
    pub fn import_koreader_history(&mut self, history_lua: &Path) -> Result<ImportReport> {
        let (table, lossy_values) = read_table(history_lua)?;
        let mut report = ImportReport {
            lossy_values,
            ..Default::default()
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for value in table.values() {
            let Value::Table(entry) = value else {
                report.skipped_invalid += 1;
                continue;
            };
            let (Some(file), Some(time)) = (str_field(entry, "file"), num_field(entry, "time")) else {
                report.skipped_invalid += 1;
                continue;
            };
            let Some(book_id) = ensure_book(&tx, Path::new(file), &mut report)? else {
                continue;
            };
            let opened_at = time as i64;
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM history WHERE book_id = ?1 AND opened_at = ?2)",
                params![book_id, opened_at],
                |r| r.get(0),
            )?;
            if exists {
                report.already_present += 1;
            } else {
                tx.execute(
                    "INSERT INTO history (book_id, opened_at) VALUES (?1, ?2)",
                    params![book_id, opened_at],
                )?;
                report.added += 1;
            }
            tx.execute(
                "UPDATE books SET last_opened_at = MAX(COALESCE(last_opened_at, 0), ?2) WHERE id = ?1",
                params![book_id, opened_at],
            )?;
        }
        tx.commit()?;
        Ok(report)
    }

    /// Imports KOReader's `collection.lua`. Each top-level key is a collection
    /// name. Its numbered entries are `{ file = <path>, order = <n> }`, and
    /// `order` becomes the manual position. The `settings` table is ignored.
    pub fn import_koreader_collections(&mut self, collection_lua: &Path) -> Result<ImportReport> {
        let (table, lossy_values) = read_table(collection_lua)?;
        let mut report = ImportReport {
            lossy_values,
            ..Default::default()
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (key, value) in &table {
            let (Key::Str(name), Value::Table(collection)) = (key, value) else {
                report.skipped_invalid += 1;
                continue;
            };
            if name.trim().is_empty() {
                report.skipped_invalid += 1;
                continue;
            }
            tx.execute(
                "INSERT OR IGNORE INTO collections (name) VALUES (?1)",
                [name.trim()],
            )?;
            let cid: i64 =
                tx.query_row("SELECT id FROM collections WHERE name = ?1", [name.trim()], |r| {
                    r.get(0)
                })?;

            let mut index: i64 = 0;
            for (k, item) in collection {
                if !matches!(k, Key::Int(_)) {
                    continue;
                }
                index += 1;
                let Value::Table(item) = item else {
                    report.skipped_invalid += 1;
                    continue;
                };
                let Some(file) = str_field(item, "file") else {
                    report.skipped_invalid += 1;
                    continue;
                };
                let position = num_field(item, "order").map_or(index, |o| o as i64);
                let Some(book_id) = ensure_book(&tx, Path::new(file), &mut report)? else {
                    continue;
                };
                let n = tx.execute(
                    "INSERT OR IGNORE INTO collection_books (collection_id, book_id, position)
                     VALUES (?1, ?2, ?3)",
                    params![cid, book_id, position],
                )?;
                if n == 1 {
                    report.added += 1;
                } else {
                    report.already_present += 1;
                }
            }
        }
        tx.commit()?;
        Ok(report)
    }
}

fn read_table(path: &Path) -> Result<(Table, usize)> {
    let fail = |message: String| LibraryError::KoreaderData {
        path: path.to_path_buf(),
        message,
    };
    let bytes = fs::read(path).map_err(|e| fail(e.to_string()))?;
    let decoded = String::from_utf8_lossy(&bytes);
    let lossy = matches!(decoded, std::borrow::Cow::Owned(_));
    let mut marker = "\u{e000}".to_string();
    if lossy {
        while decoded.contains(&marker) {
            marker.push('\u{e000}');
        }
    }
    let src = if lossy {
        decode_with_marker(&bytes, &marker)
    } else {
        decoded.into_owned()
    };
    match kaia_luadata::parse(&src) {
        Ok(Value::Table(t)) => {
            if lossy {
                Ok(restore_lossy_values(t, &marker))
            } else {
                Ok((t, 0))
            }
        }
        Ok(_) => Err(fail("expected a Lua table".into())),
        Err(e) => Err(fail(e.to_string())),
    }
}

fn decode_with_marker(mut bytes: &[u8], marker: &str) -> String {
    let mut decoded = String::new();
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(valid) => {
                decoded.push_str(valid);
                break;
            }
            Err(error) => {
                let valid_len = error.valid_up_to();
                decoded.push_str(std::str::from_utf8(&bytes[..valid_len]).expect("valid prefix"));
                decoded.push_str(marker);
                let invalid_len = error.error_len().unwrap_or(bytes.len() - valid_len);
                bytes = &bytes[valid_len + invalid_len..];
            }
        }
    }
    decoded
}

fn restore_lossy_values(table: Table, marker: &str) -> (Table, usize) {
    let mut restored = Table::new();
    let mut affected = 0;
    for (key, value) in table {
        let key = match key {
            Key::Str(s) if s.contains(marker) => {
                affected += 1;
                Key::Str(s.replace(marker, "\u{fffd}"))
            }
            other => other,
        };
        let value = match value {
            Value::Str(s) if s.contains(marker) => {
                affected += 1;
                Value::Str(s.replace(marker, "\u{fffd}"))
            }
            Value::Table(nested) => {
                let (nested, count) = restore_lossy_values(nested, marker);
                affected += count;
                Value::Table(nested)
            }
            other => other,
        };
        restored.insert(key, value);
    }
    (restored, affected)
}

fn str_field<'a>(t: &'a Table, key: &str) -> Option<&'a str> {
    match t.get(&Key::Str(key.to_string())) {
        Some(Value::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn num_field(t: &Table, key: &str) -> Option<f64> {
    match t.get(&Key::Str(key.to_string())) {
        Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 => Some(*n),
        _ => None,
    }
}

/// The id of the book at `path`, adding a row for it if it is new. The row has
/// no metadata yet. The next scan extracts it when its file is under a root.
/// Returns None and counts the reason when the file cannot be indexed.
fn ensure_book(conn: &Connection, path: &Path, report: &mut ImportReport) -> Result<Option<i64>> {
    let real = canonical_or(path);
    let Some(p) = real.to_str() else {
        report.skipped_invalid += 1;
        return Ok(None);
    };
    let meta = match fs::metadata(&real) {
        Ok(m) if m.is_file() => m,
        _ => {
            report.skipped_missing += 1;
            return Ok(None);
        }
    };
    let format = real
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    if Format::from_extension(&format).is_none() {
        report.skipped_unsupported += 1;
        return Ok(None);
    }
    let existing: Option<i64> = conn
        .query_row("SELECT id FROM books WHERE path = ?1", [p], |r| r.get(0))
        .optional()?;
    if let Some(id) = existing {
        return Ok(Some(id));
    }
    let Ok(md5) = partial_md5(&real) else {
        report.skipped_invalid += 1;
        return Ok(None);
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    conn.execute(
        "INSERT INTO books (path, size, mtime, partial_md5, format, added_at, status, extracted, attempts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'new', 0, 0)",
        params![p, meta.len() as i64, mtime, md5, format, now_unix()],
    )?;
    Ok(Some(conn.last_insert_rowid()))
}
