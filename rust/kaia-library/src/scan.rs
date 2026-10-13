//! Library scan. It walks the roots, sorts each book file into unchanged, moved,
//! changed or new, extracts metadata only where needed, and removes rows whose
//! files are gone from roots that were scanned completely.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, UNIX_EPOCH};

use kaia_doc::Format;
use rusqlite::{params, OptionalExtension, Row, TransactionBehavior};

use crate::error::Result;
use crate::extract::{extract_guarded, Cover, Extracted, COVER_MIME};
use crate::types::{Progress, ScanReport};
use crate::{now_unix, Library};
use kaia_id::partial_md5;

/// Extraction attempts before a book is marked failed. A book whose extraction
/// keeps dying (a crash or a kill during extraction) is not retried after this
/// many attempts, until its file changes.
pub(crate) const MAX_ATTEMPTS: u32 = 3;

/// Deepest folder the walk enters.
const MAX_DEPTH: usize = 32;

#[derive(Debug)]
struct Found {
    path: PathBuf,
    size: u64,
    mtime: i64,
    format: String,
}

/// The row fields a scan needs.
#[derive(Debug, Clone)]
struct Stored {
    id: i64,
    path: String,
    size: u64,
    mtime: i64,
    md5: String,
    extracted: bool,
    failure: Option<String>,
    attempts: u32,
}

impl Stored {
    /// Settled rows are not extracted again unless the file changes.
    fn settled(&self) -> bool {
        self.extracted || self.failure.is_some()
    }
}

impl Library {
    /// Scans the roots and brings the index up to date.
    ///
    /// Call this from a worker thread. It reads files and runs the document
    /// engine, which can take seconds per book. `progress` runs on this thread
    /// before each book. Setting `cancel` stops the scan after the current
    /// book. A cancelled scan removes nothing.
    ///
    /// A root that is missing or cannot be read completely is not used for
    /// removal, so its books stay in the index.
    pub fn scan<F>(&mut self, roots: &[PathBuf], mut progress: F, cancel: &AtomicBool) -> Result<ScanReport>
    where
        F: FnMut(Progress<'_>),
    {
        let started = Instant::now();
        let mut report = ScanReport::default();
        let mut found: BTreeMap<PathBuf, Found> = BTreeMap::new();
        let mut complete_roots = Vec::new();
        for root in roots {
            let Ok(dir) = fs::canonicalize(root) else {
                continue;
            };
            if !dir.is_dir() {
                continue;
            }
            if walk_dir(&dir, 0, &mut found, cancel) {
                complete_roots.push(dir);
            }
        }
        report.files_seen = found.len();

        let total = found.len();
        let mut seen = HashSet::new();
        for (done, f) in found.values().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            progress(Progress {
                done,
                total,
                path: &f.path,
            });
            self.index_one(f, &mut seen, &mut report)?;
        }

        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
        } else {
            self.remove_vanished(&complete_roots, &seen, &mut report)?;
        }
        report.elapsed = started.elapsed();
        Ok(report)
    }

    /// Decides what to do with one file and does it. Each book is committed on
    /// its own, so a scan that is cancelled or killed keeps the books it finished.
    fn index_one(&mut self, f: &Found, seen: &mut HashSet<i64>, report: &mut ScanReport) -> Result<()> {
        let stored = self.stored_by_path(&f.path)?;
        if let Some(s) = &stored {
            if s.size == f.size && s.mtime == f.mtime && s.settled() {
                seen.insert(s.id);
                report.unchanged += 1;
                return Ok(());
            }
        }

        let md5 = match partial_md5(&f.path) {
            Ok(m) => m,
            Err(_) => {
                report.unreadable += 1;
                if let Some(s) = &stored {
                    seen.insert(s.id);
                }
                return Ok(());
            }
        };

        let (id, prev_attempts) = match stored {
            Some(s) => {
                let same = s.size == f.size && s.mtime == f.mtime && s.md5 == md5;
                if !same {
                    report.changed += 1;
                }
                self.conn.execute(
                    "UPDATE books SET size = ?1, mtime = ?2, partial_md5 = ?3, format = ?4 WHERE id = ?5",
                    params![f.size as i64, f.mtime, md5, f.format, s.id],
                )?;
                // A changed file starts over. An unchanged file that never
                // settled keeps its attempt count.
                (s.id, if same { s.attempts } else { 0 })
            }
            None => match self.find_moved(&md5, f.size)? {
                Some(m) => {
                    let path = f.path.to_str().unwrap_or_default();
                    self.conn.execute(
                        "UPDATE books SET path = ?1, size = ?2, mtime = ?3 WHERE id = ?4",
                        params![path, f.size as i64, f.mtime, m.id],
                    )?;
                    report.moved += 1;
                    if m.settled() {
                        seen.insert(m.id);
                        return Ok(());
                    }
                    (m.id, m.attempts)
                }
                None => {
                    self.conn.execute(
                        "INSERT INTO books (path, size, mtime, partial_md5, format, added_at, status, extracted, attempts)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'new', 0, 0)",
                        params![
                            f.path.to_str().unwrap_or_default(),
                            f.size as i64,
                            f.mtime,
                            md5,
                            f.format,
                            now_unix()
                        ],
                    )?;
                    report.added += 1;
                    (self.conn.last_insert_rowid(), 0)
                }
            },
        };
        seen.insert(id);

        if prev_attempts >= MAX_ATTEMPTS {
            self.conn.execute(
                "UPDATE books SET failure = ?1, extracted = 0, attempts = ?2 WHERE id = ?3",
                params![
                    format!("extraction was interrupted {MAX_ATTEMPTS} times"),
                    prev_attempts,
                    id
                ],
            )?;
            report.failed += 1;
            return Ok(());
        }

        // Crash guard: the attempt is committed before the engine runs. If the
        // process dies inside the engine, the count is already higher next time.
        self.conn.execute(
            "UPDATE books SET attempts = ?1, extracted = 0, failure = NULL WHERE id = ?2",
            params![prev_attempts + 1, id],
        )?;
        report.extracted += 1;
        let outcome = extract_guarded(&f.path, self.extract_budget);

        let stem = file_stem(&f.path);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        match outcome {
            Ok(ex) => {
                write_extracted(&tx, id, &stem, &ex)?;
            }
            Err(message) => {
                report.failed += 1;
                tx.execute(
                    "UPDATE books SET title = ?1, authors = NULL, series = NULL, series_index = NULL,
                       language = NULL, description = NULL, pages = NULL, failure = ?2, extracted = 0
                     WHERE id = ?3",
                    params![stem, message, id],
                )?;
                tx.execute("DELETE FROM covers WHERE book_id = ?1", [id])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes rows under a fully scanned root whose files were not seen.
    fn remove_vanished(
        &mut self,
        complete_roots: &[PathBuf],
        seen: &HashSet<i64>,
        report: &mut ScanReport,
    ) -> Result<()> {
        if complete_roots.is_empty() {
            return Ok(());
        }
        let rows: Vec<(i64, String)> = {
            let mut stmt = self.conn.prepare("SELECT id, path FROM books")?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (id, path) in rows {
            if seen.contains(&id) {
                continue;
            }
            let p = Path::new(&path);
            if complete_roots.iter().any(|root| p.starts_with(root)) {
                tx.execute("DELETE FROM books WHERE id = ?1", [id])?;
                report.removed += 1;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn stored_by_path(&self, path: &Path) -> Result<Option<Stored>> {
        let Some(p) = path.to_str() else {
            return Ok(None);
        };
        Ok(self
            .conn
            .query_row(
                "SELECT id, path, size, mtime, partial_md5, extracted, failure, attempts
                 FROM books WHERE path = ?1",
                [p],
                stored_from_row,
            )
            .optional()?)
    }

    /// A row with the same content and size whose file is no longer at its
    /// stored path.
    fn find_moved(&self, md5: &str, size: u64) -> Result<Option<Stored>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, size, mtime, partial_md5, extracted, failure, attempts
             FROM books WHERE partial_md5 = ?1 AND size = ?2 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![md5, size as i64], stored_from_row)?;
        for row in rows {
            let s = row?;
            if !Path::new(&s.path).exists() {
                return Ok(Some(s));
            }
        }
        Ok(None)
    }
}

fn stored_from_row(r: &Row<'_>) -> rusqlite::Result<Stored> {
    Ok(Stored {
        id: r.get(0)?,
        path: r.get(1)?,
        size: r.get::<_, i64>(2)? as u64,
        mtime: r.get(3)?,
        md5: r.get(4)?,
        extracted: r.get::<_, i64>(5)? != 0,
        failure: r.get(6)?,
        attempts: r.get::<_, i64>(7)? as u32,
    })
}

/// Writes the metadata and cover of a successful extraction. Engine fields that
/// the document layer does not expose stay NULL.
fn write_extracted(tx: &rusqlite::Transaction<'_>, id: i64, stem: &str, ex: &Extracted) -> Result<()> {
    tx.execute(
        "UPDATE books SET title = ?1, authors = ?2, pages = ?3, extracted = 1, failure = NULL, attempts = 0
         WHERE id = ?4",
        params![ex.title.as_deref().unwrap_or(stem), ex.authors, ex.pages, id],
    )?;
    write_cover(tx, id, ex.cover.as_ref())
}

fn write_cover(tx: &rusqlite::Transaction<'_>, id: i64, cover: Option<&Cover>) -> Result<()> {
    let (w, h, small, large) = match cover {
        Some(c) => (c.source_w, c.source_h, Some(&c.small), Some(&c.large)),
        None => (0, 0, None, None),
    };
    tx.execute(
        "INSERT INTO covers (book_id, source_w, source_h, small, large, mime) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(book_id) DO UPDATE SET
           source_w = excluded.source_w, source_h = excluded.source_h,
           small = excluded.small, large = excluded.large, mime = excluded.mime",
        params![id, w, h, small, large, COVER_MIME],
    )?;
    Ok(())
}

/// Adds `path` if its name has a book extension. Symlinked files are stored
/// under their real path, so every path in the index is canonical.
fn add_file(path: &Path, size: u64, mtime: i64, found: &mut BTreeMap<PathBuf, Found>) {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return;
    };
    let ext = ext.to_ascii_lowercase();
    if Format::from_extension(&ext).is_none() || path.to_str().is_none() {
        return;
    }
    found.insert(
        path.to_path_buf(),
        Found {
            path: path.to_path_buf(),
            size,
            mtime,
            format: ext,
        },
    );
}

/// Adds the book files under `dir`. Returns false when any part of the tree
/// could not be read or the walk was cancelled, so the caller knows the root
/// is not complete.
fn walk_dir(dir: &Path, depth: usize, found: &mut BTreeMap<PathBuf, Found>, cancel: &AtomicBool) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    let mut complete = true;
    for entry in entries {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let Ok(entry) = entry else {
            complete = false;
            continue;
        };
        // Hidden names and macOS "._" resource forks are skipped, as in KOReader.
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            complete = false;
            continue;
        };
        if file_type.is_symlink() {
            // Symlinked files count. Symlinked folders are not followed, so a
            // loop cannot hang the scan.
            if let Ok(meta) = fs::metadata(&path) {
                if meta.is_file() {
                    if let Ok(real) = fs::canonicalize(&path) {
                        add_file(&real, meta.len(), mtime_secs(&meta), found);
                    }
                }
            }
        } else if file_type.is_dir() {
            if !walk_dir(&path, depth + 1, found, cancel) {
                complete = false;
            }
        } else if file_type.is_file() {
            match entry.metadata() {
                Ok(meta) => add_file(&path, meta.len(), mtime_secs(&meta), found),
                Err(_) => complete = false,
            }
        }
    }
    complete
}

fn mtime_secs(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64)
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}
