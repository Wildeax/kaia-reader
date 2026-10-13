//! Recording reading time, following KOReader's `ReaderStatistics` rules.
//!
//! A page earns time when the reader leaves it. The time between two page turns
//! counts when it is at least `min_sec`; a longer stay counts `max_sec`. Time
//! between a suspend and the resume is not counted. Spans are written to
//! `page_stat_data` in batches (see [`Stats::flush`]).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection, OptionalExtension};

use crate::query::uncapped_totals;
use crate::{schema, Result, DEFAULT_MAX_READ_SEC, DEFAULT_MIN_READ_SEC};

/// Page turns between automatic flushes, as in KOReader.
const MAX_PAGETURNS_BEFORE_FLUSH: u32 = 50;

/// One visit to a page: when it started and how many seconds count.
/// A span with `duration == 0` is still open (its duration is set when the reader leaves).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    start: i64,
    duration: i64,
}

/// State of the book open in a session. Mirrors the volatile fields of `ReaderStatistics`.
#[derive(Debug)]
struct OpenBook {
    id: i64,
    /// Page count recorded with new spans. Set by `start_book` and `pages_changed`.
    pages: i64,
    curr_page: i64,
    /// Pending spans per page, flushed to `page_stat_data`.
    spans: BTreeMap<i64, Vec<Span>>,
    pageturn_count: u32,
    /// Set between `on_suspend` and `on_resume`.
    paused_since: Option<i64>,
    /// Page the reader was on when a turn happened during a pause.
    paused_page: Option<i64>,
    highlights: i64,
    notes: i64,
}

/// A statistics database and the reading session of the book that is open in it.
///
/// Every method that takes `now` uses that value as the clock. Callers pass the
/// current Unix time. Tests pass fixed times.
#[derive(Debug)]
pub struct Stats {
    pub(crate) conn: Connection,
    path: PathBuf,
    min_sec: i64,
    pub(crate) max_sec: i64,
    /// The last `now` seen. `flush` uses it as the time of the write.
    pub(crate) clock: i64,
    /// Start of the current reading session (book opened or resumed).
    pub(crate) session_start: Option<i64>,
    book: Option<OpenBook>,
}

impl Stats {
    /// Opens the statistics database at `path`, creating it when it does not exist.
    ///
    /// An older schema is backed up next to the file and upgraded. A newer schema
    /// returns [`crate::Error::NewerSchema`] and the file is not changed.
    pub fn open(path: impl AsRef<Path>) -> Result<Stats> {
        let path = path.as_ref().to_path_buf();
        let mut conn = Connection::open(&path)?;
        conn.busy_timeout(Duration::from_secs(5))?;
        schema::prepare(&mut conn, &path)?;
        Ok(Stats {
            conn,
            path,
            min_sec: DEFAULT_MIN_READ_SEC,
            max_sec: DEFAULT_MAX_READ_SEC,
            clock: 0,
            session_start: None,
            book: None,
        })
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Sets the page duration limits in seconds. A stay shorter than `min_sec` does not count.
    /// A longer stay counts `max_sec`.
    pub fn set_read_limits(&mut self, min_sec: i64, max_sec: i64) {
        self.min_sec = min_sec;
        self.max_sec = max_sec;
    }

    /// Sets the highlight and note counts stored with the open book.
    pub fn set_annotation_counts(&mut self, highlights: i64, notes: i64) {
        if let Some(book) = self.book.as_mut() {
            book.highlights = highlights;
            book.notes = notes;
        }
    }

    /// Opens a book at `page`, creating its row when the (title, authors, md5) key is new.
    ///
    /// Any book that is still open is ended first. Returns the book id.
    /// KOReader uses `"N/A"` for missing authors, so pass that to match its rows.
    pub fn start_book(
        &mut self,
        md5: &str,
        title: &str,
        authors: &str,
        pages: i64,
        page: i64,
        now: i64,
    ) -> Result<i64> {
        self.end_book(now)?;
        self.clock = now;
        self.session_start = Some(now);

        let existing: Option<(i64, Option<i64>, Option<i64>)> = self
            .conn
            .query_row(
                "SELECT id, highlights, notes FROM book WHERE title = ?1 AND authors = ?2 AND md5 = ?3",
                params![title, authors, md5],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let (id, highlights, notes) = match existing {
            Some((id, highlights, notes)) => (id, highlights.unwrap_or(0), notes.unwrap_or(0)),
            None => {
                self.conn.execute(
                    "INSERT INTO book (title, authors, notes, last_open, highlights, pages, series, language, md5, \
                     total_read_time, total_read_pages) VALUES (?1, ?2, 0, ?3, 0, ?4, 'N/A', 'N/A', ?5, 0, 0)",
                    params![title, authors, now, pages, md5],
                )?;
                (self.conn.last_insert_rowid(), 0, 0)
            }
        };

        let mut spans = BTreeMap::new();
        spans.insert(
            page,
            vec![Span {
                start: now,
                duration: 0,
            }],
        );
        self.book = Some(OpenBook {
            id,
            pages,
            curr_page: page,
            spans,
            pageturn_count: 0,
            paused_since: None,
            paused_page: None,
            highlights,
            notes,
        });
        Ok(id)
    }

    /// Reports a page turn to `page` at time `now`. Turns to the current page are ignored.
    ///
    /// Every 50 turns the spans are written to the database.
    pub fn page_turn(&mut self, page: i64, now: i64) -> Result<()> {
        self.clock = now;
        self.turn(Some(page), now)
    }

    /// Ends the open book: counts the time on the current page, writes the spans, and closes the book.
    /// Does nothing when no book is open.
    pub fn end_book(&mut self, now: i64) -> Result<()> {
        if self.book.is_none() {
            return Ok(());
        }
        self.clock = now;
        self.turn(None, now)?;
        self.insert_db(None)?;
        self.book = None;
        Ok(())
    }

    /// Reports that the device suspends at `now`. Writes the spans and starts the pause.
    pub fn on_suspend(&mut self, now: i64) -> Result<()> {
        self.clock = now;
        self.insert_db(None)?;
        if let Some(book) = self.book.as_mut() {
            book.paused_since.get_or_insert(now);
        }
        Ok(())
    }

    /// Reports that the device resumes at `now`. Moves the start of the current page
    /// past the pause, so the pause is not counted. A page turn during the pause is applied now.
    pub fn on_resume(&mut self, now: i64) -> Result<()> {
        self.clock = now;
        self.session_start = Some(now);
        let Some(book) = self.book.as_mut() else {
            return Ok(());
        };
        let Some(paused_since) = book.paused_since.take() else {
            return Ok(());
        };
        let pause = now - paused_since;
        let curr_page = book.curr_page;
        if let Some(last) = book.spans.get_mut(&curr_page).and_then(|list| list.last_mut()) {
            last.start += pause;
        }
        let paused_page = book.paused_page.take();
        if let Some(page) = paused_page.filter(|page| *page != curr_page) {
            self.turn(Some(page), now)?;
        }
        Ok(())
    }

    /// Reports a new page count (for example after a font size change).
    ///
    /// Spans recorded so far keep the old count, and the view rescales them to the
    /// new count when queried.
    pub fn pages_changed(&mut self, new_pages: i64) -> Result<()> {
        let Some(book) = self.book.as_ref() else {
            return Ok(());
        };
        if new_pages != book.pages {
            self.insert_db(Some(new_pages))?;
        }
        if let Some(book) = self.book.as_mut() {
            book.pages = new_pages;
        }
        Ok(())
    }

    /// Writes the pending spans of the open book and refreshes its totals.
    /// Does nothing when no book is open.
    pub fn flush(&mut self) -> Result<()> {
        self.insert_db(None)
    }

    /// Applies a page turn (`Some(page)`) or the close of the book (`None`), as KOReader's `onPageUpdate` does.
    fn turn(&mut self, target: Option<i64>, now: i64) -> Result<()> {
        let (min_sec, max_sec) = (self.min_sec, self.max_sec);
        let Some(book) = self.book.as_mut() else {
            return Ok(());
        };
        if book.paused_since.is_some() {
            if let Some(page) = target {
                book.paused_page = Some(page);
            }
            return Ok(());
        }
        if target.is_some() && target == Some(book.curr_page) {
            return Ok(());
        }
        let closing = target.is_none();
        let curr = book.curr_page;
        let pageno = target.unwrap_or(curr);
        book.pageturn_count += 1;

        let prev_start = book
            .spans
            .get(&curr)
            .and_then(|list| list.last())
            .map(|span| span.start);
        let Some(prev_start) = prev_start else {
            // No open span for the page we leave: start a new one on the new page.
            book.spans.insert(
                pageno,
                vec![Span {
                    start: now,
                    duration: 0,
                }],
            );
            book.curr_page = pageno;
            return Ok(());
        };

        let diff = now - prev_start;
        let credited = if diff >= min_sec && diff <= max_sec {
            Some(diff)
        } else if diff > max_sec {
            Some(max_sec)
        } else {
            None
        };
        if let (Some(seconds), Some(last)) = (
            credited,
            book.spans.get_mut(&curr).and_then(|list| list.last_mut()),
        ) {
            last.duration += seconds;
        }
        if closing {
            return Ok(());
        }

        let flush_now = book.pageturn_count >= MAX_PAGETURNS_BEFORE_FLUSH;
        if flush_now {
            book.pageturn_count = 0;
        }
        book.curr_page = pageno;
        book.spans.entry(pageno).or_default().push(Span {
            start: now,
            duration: 0,
        });
        if flush_now {
            self.insert_db(None)?;
        }
        Ok(())
    }

    /// Writes spans with a duration above zero, refreshes the book row and totals,
    /// and keeps the open span of the current page. Mirrors KOReader's `insertDB`.
    fn insert_db(&mut self, updated_pages: Option<i64>) -> Result<()> {
        let now = self.clock;
        let Some(book) = self.book.as_mut() else {
            return Ok(());
        };

        // An open span has duration 0 and is not written. Its start time is kept for the next batch.
        let cur_start = book
            .spans
            .get(&book.curr_page)
            .and_then(|list| list.last())
            .filter(|span| span.duration == 0)
            .map_or(now, |span| span.start);

        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare(
                "INSERT OR IGNORE INTO page_stat_data (id_book, page, start_time, duration, total_pages) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (page, list) in &book.spans {
                for span in list.iter().filter(|span| span.duration > 0) {
                    insert.execute(params![book.id, page, span.start, span.duration, book.pages])?;
                }
            }
        }
        // The page count is written before the totals, so the view rescales with the new count.
        tx.execute(
            "UPDATE book SET pages = ?1 WHERE id = ?2",
            params![updated_pages.unwrap_or(book.pages), book.id],
        )?;
        let (total_pages, total_time) = uncapped_totals(&tx, book.id)?;
        tx.execute(
            "UPDATE book SET last_open = ?1, notes = ?2, highlights = ?3, total_read_time = ?4, \
             total_read_pages = ?5 WHERE id = ?6",
            params![now, book.notes, book.highlights, total_time, total_pages, book.id],
        )?;
        tx.commit()?;

        reset_volatile(book, Some(cur_start));
        Ok(())
    }
}

/// Clears pending spans. The current page gets an open span at `start` when one is given.
fn reset_volatile(book: &mut OpenBook, start: Option<i64>) {
    book.pageturn_count = 0;
    book.spans.clear();
    if let Some(start) = start {
        let curr = book.curr_page;
        book.spans.insert(curr, vec![Span { start, duration: 0 }]);
    }
}
