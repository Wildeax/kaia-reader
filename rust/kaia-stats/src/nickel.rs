//! Kobo sessions retain their timestamps and percentages without guessed pages.

use crate::{BookInfo, ImportReport, Result, Stats};
use rusqlite::{params, OptionalExtension};

/// One completed Kobo reading session. Percentages are fractions in 0..=1.
#[derive(Debug, Clone, PartialEq)]
pub struct NickelSession {
    pub start: i64,
    pub end: i64,
    pub start_percent: f64,
    pub end_percent: f64,
    pub idle_seconds: i64,
}

impl NickelSession {
    pub fn valid(&self) -> bool {
        self.start >= 0
            && self.end > self.start
            && self.idle_seconds >= 0
            && self.idle_seconds <= self.end - self.start
            && (0.0..=1.0).contains(&self.start_percent)
            && (0.0..=1.0).contains(&self.end_percent)
    }

    /// Stable source identity within one book.
    pub fn key(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.start, self.end, self.start_percent, self.end_percent
        )
    }
}

const TABLE_SQL: &str = "CREATE TABLE IF NOT EXISTS nickel_sessions (
    id_book INTEGER NOT NULL REFERENCES book(id),
    source_key TEXT NOT NULL,
    start_time INTEGER NOT NULL,
    end_time INTEGER NOT NULL,
    start_percent REAL NOT NULL,
    end_percent REAL NOT NULL,
    idle_seconds INTEGER NOT NULL,
    UNIQUE(id_book, source_key)
);";

pub(crate) fn prepare(conn: &rusqlite::Connection) -> Result<()> {
    let imported = conn
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type='table' AND name='nickel_sessions'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some();
    let sessions = if imported {
        " UNION ALL SELECT id_book, NULL, start_time, end_time - start_time - idle_seconds FROM nickel_sessions"
    } else {
        ""
    };
    conn.execute_batch(&format!("CREATE TEMP VIEW kaia_read_time AS SELECT id_book, page, start_time, duration FROM page_stat{sessions}"))?;
    Ok(())
}

impl Stats {
    /// Imports each book, start, end and percent pair once. Invalid rows are skipped.
    pub fn import_nickel(&mut self, book: &BookInfo<'_>, sessions: &[NickelSession]) -> Result<ImportReport> {
        self.conn.execute_batch(TABLE_SQL)?;
        self.conn
            .execute_batch("DROP VIEW IF EXISTS temp.kaia_read_time")?;
        prepare(&self.conn)?;
        self.flush()?;
        let tx = self.conn.transaction()?;
        let mut report = ImportReport::default();
        for session in sessions {
            if !session.valid() {
                report.spans_skipped += 1;
                continue;
            }
            let added = tx.execute(
                "INSERT OR IGNORE INTO book (title, authors, md5, pages, last_open)
                VALUES (?1, ?2, ?3, 0, ?4)",
                params![book.display_title, book.authors, book.md5, session.start],
            )?;
            report.books_added += added as i64;
            let id: i64 = tx.query_row(
                "SELECT id FROM book WHERE title=?1 AND authors=?2 AND md5=?3",
                params![book.display_title, book.authors, book.md5],
                |r| r.get("id"),
            )?;
            let added = tx.execute(
                "INSERT OR IGNORE INTO nickel_sessions
                (id_book, source_key, start_time, end_time, start_percent, end_percent, idle_seconds)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    id,
                    session.key(),
                    session.start,
                    session.end,
                    session.start_percent,
                    session.end_percent,
                    session.idle_seconds
                ],
            )?;
            report.spans_added += added as i64;
            report.spans_skipped += (added == 0) as i64;
            tx.execute(
                "UPDATE book SET last_open=max(COALESCE(last_open,0),?2),
                total_read_time=(SELECT COALESCE(sum(duration),0) FROM kaia_read_time WHERE id_book=?1)
                WHERE id=?1",
                params![id, session.start],
            )?;
        }
        tx.commit()?;
        Ok(report)
    }
}
