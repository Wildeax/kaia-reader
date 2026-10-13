//! Merging a KOReader `statistics.sqlite3` into Kaia's database.
//!
//! The source is opened read-only and is never written. Books match on
//! (title, authors, md5), as KOReader matches them. Spans are copied with
//! `INSERT OR IGNORE` on (book, page, start_time), so importing the same file twice
//! adds nothing.

use std::path::Path;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::query::uncapped_totals;
use crate::{Error, Result, Stats, DB_SCHEMA_VERSION};

/// What an import changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Books in the source that were not in the target.
    pub books_added: i64,
    /// Books in the source that matched a book in the target.
    pub books_matched: i64,
    /// Spans copied into the target.
    pub spans_added: i64,
    /// Spans already in the target (same book, page and start time).
    pub spans_skipped: i64,
}

/// A book row read from the source.
struct SourceBook {
    id: i64,
    title: String,
    authors: String,
    md5: String,
    notes: Option<i64>,
    last_open: Option<i64>,
    highlights: Option<i64>,
    pages: Option<i64>,
    series: Option<String>,
    language: Option<String>,
}

impl Stats {
    /// Merges the KOReader statistics database at `source` into this one.
    ///
    /// The open book is ended first, so its pending spans are written. The source
    /// must use schema 20201010 or later. Older files need one start in KOReader
    /// to upgrade.
    pub fn import_koreader(&mut self, source: impl AsRef<Path>) -> Result<ImportReport> {
        let source = source.as_ref();
        if is_same_file(source, self.path()) {
            return Err(Error::SameDatabase(source.to_path_buf()));
        }
        let src = Connection::open_with_flags(
            source,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        src.pragma_update(None, "query_only", true)?;
        let version: i64 = src.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if !(20201010..=DB_SCHEMA_VERSION).contains(&version) {
            return Err(Error::UnsupportedSource { found: version });
        }

        let clock = self.clock;
        self.end_book(clock)?;
        let tx = self.conn.transaction()?;
        let report = merge(&tx, &src)?;
        tx.commit()?;
        Ok(report)
    }
}

/// Copies books and spans from `src` into `tx`, then recomputes totals of the touched books.
fn merge(tx: &rusqlite::Transaction<'_>, src: &Connection) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    let mut touched = Vec::new();

    let books: Vec<SourceBook> = {
        let mut stmt = src.prepare(
            "SELECT id, COALESCE(title, ''), COALESCE(authors, ''), COALESCE(md5, ''), notes, last_open,
                    highlights, pages, series, language
             FROM book
             WHERE EXISTS (SELECT 1 FROM page_stat_data WHERE page_stat_data.id_book = book.id)
             ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SourceBook {
                id: row.get(0)?,
                title: row.get(1)?,
                authors: row.get(2)?,
                md5: row.get(3)?,
                notes: row.get(4)?,
                last_open: row.get(5)?,
                highlights: row.get(6)?,
                pages: row.get(7)?,
                series: row.get(8)?,
                language: row.get(9)?,
            })
        })?;
        rows.collect::<std::result::Result<_, _>>()?
    };

    let mut find =
        tx.prepare("SELECT id, last_open FROM book WHERE title = ?1 AND authors = ?2 AND md5 = ?3")?;
    let mut src_spans =
        src.prepare("SELECT page, start_time, duration, total_pages FROM page_stat_data WHERE id_book = ?1")?;

    for book in &books {
        let existing: Option<(i64, Option<i64>)> = find
            .query_row(params![book.title, book.authors, book.md5], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()?;
        let target_id = match existing {
            Some((id, target_last_open)) => {
                report.books_matched += 1;
                tx.execute(
                    "UPDATE book SET last_open = max(COALESCE(last_open, 0), COALESCE(?1, 0)),
                                     notes = max(COALESCE(notes, 0), COALESCE(?2, 0)),
                                     highlights = max(COALESCE(highlights, 0), COALESCE(?3, 0))
                     WHERE id = ?4",
                    params![book.last_open, book.notes, book.highlights, id],
                )?;
                // The copy opened most recently sets the page count, series and language.
                if book.last_open.unwrap_or(0) > target_last_open.unwrap_or(0) {
                    tx.execute(
                        "UPDATE book SET pages = ?1, series = ?2, language = ?3 WHERE id = ?4",
                        params![book.pages, book.series, book.language, id],
                    )?;
                }
                id
            }
            None => {
                report.books_added += 1;
                tx.execute(
                    "INSERT INTO book (title, authors, notes, last_open, highlights, pages, series, language, md5,
                                       total_read_time, total_read_pages)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 0, 0)",
                    params![
                        book.title,
                        book.authors,
                        book.notes,
                        book.last_open,
                        book.highlights,
                        book.pages,
                        book.series,
                        book.language,
                        book.md5
                    ],
                )?;
                tx.last_insert_rowid()
            }
        };

        let mut rows = src_spans.query(params![book.id])?;
        while let Some(row) = rows.next()? {
            let inserted = tx.execute(
                "INSERT OR IGNORE INTO page_stat_data (id_book, page, start_time, duration, total_pages)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    target_id,
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?
                ],
            )?;
            if inserted == 1 {
                report.spans_added += 1;
            } else {
                report.spans_skipped += 1;
            }
        }
        touched.push(target_id);
    }

    // Totals are recomputed after every page count is final, because the view rescales with it.
    touched.sort_unstable();
    touched.dedup();
    for id in touched {
        let (pages, seconds) = uncapped_totals(tx, id)?;
        tx.execute(
            "UPDATE book SET total_read_pages = ?1, total_read_time = ?2 WHERE id = ?3",
            params![pages, seconds, id],
        )?;
    }
    Ok(report)
}

/// True when both paths name the same existing file.
fn is_same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
