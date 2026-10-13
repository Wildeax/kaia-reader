//! Reading statistics queries. Each one matches a query in KOReader's statistics plugin.
//!
//! Day boundaries are local midnight, computed by SQLite with the `localtime`
//! modifier. Queries take `&mut self` and write pending spans first (see
//! [`Stats::flush`]), so they include the page being read.

use std::collections::HashMap;

use rusqlite::{params, Connection, OptionalExtension};

use crate::{Error, Result, Stats};

/// Seconds and distinct (book, page) pairs in a period.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Totals {
    pub pages: i64,
    pub seconds: i64,
}

/// Reading in one local calendar day, `date` in `YYYY-MM-DD`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayTotal {
    pub date: String,
    pub pages: i64,
    pub seconds: i64,
}

/// One row of the book list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookSummary {
    pub id: i64,
    pub title: String,
    pub authors: String,
    pub last_open: i64,
    /// Total seconds on the book, as recorded (each stay capped at the max limit when recorded).
    pub seconds: i64,
    pub distinct_pages: i64,
}

/// Statistics for one book, as KOReader's "Current statistics" page shows them.
#[derive(Debug, Clone, PartialEq)]
pub struct BookTotals {
    pub id: i64,
    pub title: String,
    pub authors: String,
    pub pages: i64,
    pub last_open: i64,
    pub highlights: Option<i64>,
    pub notes: Option<i64>,
    /// Total seconds on the book, uncapped.
    pub total_seconds: i64,
    /// Seconds with each page capped at the max limit (`min(sum, max_sec)` per page).
    pub read_seconds: i64,
    pub distinct_pages: i64,
    pub days_read: i64,
    pub first_open: Option<i64>,
    /// Page of the most recent span.
    pub last_page: Option<i64>,
    /// `read_seconds / distinct_pages`, `None` when no page has time.
    pub average_page_seconds: Option<f64>,
}

/// Rates used for time estimates. Built by [`Stats::book_estimate`].
#[derive(Debug, Clone, PartialEq)]
pub struct BookEstimate {
    pub read_pages: i64,
    pub read_seconds: i64,
    pub days_read: i64,
    /// Seconds per page. Without read pages this is half the max limit, as KOReader starts its estimates.
    pub avg_page_seconds: f64,
    /// Seconds read per day that the book was read. `None` without reading time.
    pub seconds_per_day: Option<f64>,
}

/// Time left to read a span of pages, from [`BookEstimate::time_left`].
#[derive(Debug, Clone, PartialEq)]
pub struct TimeLeft {
    /// Seconds at the average rate. `0.0` when no pages are left.
    pub seconds: f64,
    /// Days at the daily rate, rounded up. `None` when the estimate is not valid.
    pub days: Option<i64>,
    /// Unix time of the estimated finish, from `now` plus `days`.
    pub finish_ts: Option<i64>,
}

/// Consecutive days with reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Streaks {
    /// Run that ends today, or yesterday when today has no reading yet.
    pub current: i64,
    pub longest: i64,
}

/// First day of the week, for [`Stats::this_week`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weekday {
    Sunday,
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
}

impl Weekday {
    fn days_from_sunday(self) -> i64 {
        self as i64
    }
}

/// KOReader's `STATISTICS_SQL_BOOK_CAPPED_TOTALS_QUERY`, with the book and max limit as parameters.
const CAPPED_TOTALS_SQL: &str = "SELECT count(*), sum(durations) FROM (
    SELECT min(sum(duration), ?2) AS durations
    FROM page_stat
    WHERE id_book = ?1
    GROUP BY page
)";

/// KOReader's `STATISTICS_SQL_BOOK_TOTALS_QUERY`.
const UNCAPPED_TOTALS_SQL: &str = "SELECT count(DISTINCT page), sum(duration)
    FROM page_stat
    WHERE id_book = ?1";

/// Pages and seconds per day from `start_time` (inclusive) to `until` (exclusive), as KOReader's `sqlDaily`.
const DAY_ROWS_SQL: &str = "SELECT day, count(*), sum(sum_duration) FROM (
    SELECT strftime('%Y-%m-%d', start_time, 'unixepoch', 'localtime') AS day,
           sum(duration) AS sum_duration
    FROM page_stat
    WHERE start_time >= ?1 AND start_time < ?2
    GROUP BY id_book, page, day
)
GROUP BY day";

const BOOK_SPAN_SQL: &str = "SELECT sum(duration), count(DISTINCT page), min(start_time),
       (SELECT max(ps2.page) FROM page_stat AS ps2
        WHERE ps2.id_book = ?1 AND ps2.start_time = max(page_stat.start_time))
FROM page_stat
WHERE id_book = ?1";

const DAYS_READ_SQL: &str = "SELECT count(*) FROM (
    SELECT strftime('%Y-%m-%d', start_time, 'unixepoch', 'localtime') AS day
    FROM page_stat
    WHERE id_book = ?1
    GROUP BY day
)";

const STREAKS_SQL: &str = "WITH days AS (
    SELECT DISTINCT strftime('%Y-%m-%d', start_time, 'unixepoch', 'localtime') AS day
    FROM page_stat
),
numbered AS (
    SELECT day, CAST(julianday(day) AS INTEGER) - row_number() OVER (ORDER BY day) AS grp
    FROM days
),
runs AS (
    SELECT grp, count(*) AS len, max(day) AS last_day
    FROM numbered
    GROUP BY grp
)
SELECT COALESCE(max(len), 0),
       COALESCE((SELECT max(len) FROM runs WHERE last_day IN (?1, ?2)), 0)
FROM runs";

/// Capped read pages and seconds of a book: `(distinct pages, sum of min(page total, max_sec))`.
pub(crate) fn capped_totals(conn: &Connection, book_id: i64, max_sec: i64) -> Result<(i64, i64)> {
    let (pages, seconds) = conn.query_row(CAPPED_TOTALS_SQL, params![book_id, max_sec], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
    })?;
    Ok((pages, seconds.unwrap_or(0)))
}

/// Uncapped totals of a book: `(distinct pages, sum of durations)`.
pub(crate) fn uncapped_totals(conn: &Connection, book_id: i64) -> Result<(i64, i64)> {
    let (pages, seconds) = conn.query_row(UNCAPPED_TOTALS_SQL, params![book_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
    })?;
    Ok((pages, seconds.unwrap_or(0)))
}

/// Accepts `YYYY-MM` with a month from 01 to 12.
/// Average seconds per page from capped totals. Without read pages it is half the max
/// limit, as KOReader starts its estimates.
fn average_or_default(read_pages: i64, read_seconds: i64, max_sec: i64) -> f64 {
    if read_pages > 0 {
        read_seconds as f64 / read_pages as f64
    } else {
        (0.5 * max_sec as f64).floor()
    }
}

fn check_year_month(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    let well_formed = bytes.len() == 7 && bytes[4] == b'-' && digits(0..4) && digits(5..7);
    let month = if well_formed {
        (bytes[5] - b'0') * 10 + (bytes[6] - b'0')
    } else {
        0
    };
    if well_formed && (1..=12).contains(&month) {
        Ok(())
    } else {
        Err(Error::InvalidMonth(value.to_owned()))
    }
}

impl Stats {
    /// Local midnight `days_back` days before `now`, as Unix time.
    fn day_start(&self, now: i64, days_back: i64) -> Result<i64> {
        let start = self.conn.query_row(
            "SELECT CAST(strftime('%s', ?1, 'unixepoch', 'localtime', 'start of day', ?2, 'utc') AS INTEGER)",
            params![now, format!("{:+} days", -days_back)],
            |row| row.get(0),
        )?;
        Ok(start)
    }

    fn totals_since(&self, start: i64) -> Result<Totals> {
        let (pages, seconds) = self.conn.query_row(
            "SELECT count(*), sum(sum_duration) FROM (
                SELECT sum(duration) AS sum_duration
                FROM page_stat
                WHERE start_time >= ?1
                GROUP BY id_book, page
            )",
            params![start],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        )?;
        Ok(Totals {
            pages,
            seconds: seconds.unwrap_or(0),
        })
    }

    /// Day rows from `from` (inclusive) to `until` (exclusive), keyed by local date.
    fn day_rows(&self, from: i64, until: i64) -> Result<HashMap<String, (i64, i64)>> {
        let mut stmt = self.conn.prepare(DAY_ROWS_SQL)?;
        let rows = stmt.query_map(params![from, until], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })?;
        let mut days = HashMap::new();
        for row in rows {
            let (day, pages, seconds) = row?;
            days.insert(day, (pages, seconds.unwrap_or(0)));
        }
        Ok(days)
    }

    /// Time read since local midnight of `now`.
    pub fn today(&mut self, now: i64) -> Result<Totals> {
        self.flush()?;
        let start = self.day_start(now, 0)?;
        self.totals_since(start)
    }

    /// Time read since the most recent `first_day` at local midnight.
    pub fn this_week(&mut self, now: i64, first_day: Weekday) -> Result<Totals> {
        self.flush()?;
        let offset: i64 = self.conn.query_row(
            "SELECT (CAST(strftime('%w', ?1, 'unixepoch', 'localtime') AS INTEGER) - ?2 + 7) % 7",
            params![now, first_day.days_from_sunday()],
            |row| row.get(0),
        )?;
        let start = self.day_start(now, offset)?;
        self.totals_since(start)
    }

    /// Time read since the current reading session started (book opened or device resumed).
    pub fn session(&mut self) -> Result<Totals> {
        self.flush()?;
        match self.session_start {
            Some(start) => self.totals_since(start),
            None => Ok(Totals::default()),
        }
    }

    /// Reading per local day for the last `days` days ending today, oldest first.
    /// Days without reading have zero pages and seconds.
    pub fn daily(&mut self, now: i64, days: i64) -> Result<Vec<DayTotal>> {
        self.flush()?;
        if days <= 0 {
            return Ok(Vec::new());
        }
        let start = self.day_start(now, days - 1)?;
        let labels: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "WITH RECURSIVE n(i) AS (
                    SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < ?2
                )
                SELECT strftime('%Y-%m-%d', ?1, 'unixepoch', 'localtime', printf('-%d days', i))
                FROM n
                ORDER BY i DESC",
            )?;
            let labels = stmt.query_map(params![now, days - 1], |row| row.get(0))?;
            labels.collect::<std::result::Result<_, _>>()?
        };
        let rows = self.day_rows(start, i64::MAX)?;
        Ok(labels
            .into_iter()
            .map(|date| {
                let (pages, seconds) = rows.get(&date).copied().unwrap_or((0, 0));
                DayTotal { date, pages, seconds }
            })
            .collect())
    }

    /// Reading per local day in `year_month` (`YYYY-MM`), oldest first. Days without reading are left out.
    pub fn month(&mut self, year_month: &str) -> Result<Vec<DayTotal>> {
        check_year_month(year_month)?;
        self.flush()?;
        let (from, until): (i64, i64) = self.conn.query_row(
            "SELECT CAST(strftime('%s', ?1 || '-01 00:00:00', 'utc') AS INTEGER),
                    CAST(strftime('%s', ?1 || '-01 00:00:00', '+1 month', 'utc') AS INTEGER)",
            params![year_month],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let mut days: Vec<DayTotal> = self
            .day_rows(from, until)?
            .into_iter()
            .map(|(date, (pages, seconds))| DayTotal { date, pages, seconds })
            .collect();
        days.sort_by(|a, b| a.date.cmp(&b.date));
        Ok(days)
    }

    /// Totals and dates for one book. `None` when the id is unknown.
    pub fn book_totals(&mut self, book_id: i64) -> Result<Option<BookTotals>> {
        self.flush()?;
        let row = self
            .conn
            .query_row(
                "SELECT title, authors, pages, last_open, highlights, notes FROM book WHERE id = ?1",
                params![book_id],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                    ))
                },
            )
            .optional()?;
        let Some((title, authors, pages, last_open, highlights, notes)) = row else {
            return Ok(None);
        };
        let (total_seconds, distinct_pages, first_open, last_page) =
            self.conn.query_row(BOOK_SPAN_SQL, params![book_id], |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })?;
        let days_read: i64 = self
            .conn
            .query_row(DAYS_READ_SQL, params![book_id], |row| row.get(0))?;
        let (read_pages, read_seconds) = capped_totals(&self.conn, book_id, self.max_sec)?;
        let average_page_seconds = (read_pages > 0).then(|| read_seconds as f64 / read_pages as f64);
        Ok(Some(BookTotals {
            id: book_id,
            title: title.unwrap_or_default(),
            authors: authors.unwrap_or_default(),
            pages: pages.unwrap_or(0),
            last_open: last_open.unwrap_or(0),
            highlights,
            notes,
            total_seconds,
            read_seconds,
            distinct_pages,
            days_read,
            first_open,
            last_page,
            average_page_seconds,
        }))
    }

    /// All books, most recently opened first. Books without reading are included with zero time.
    pub fn books(&mut self) -> Result<Vec<BookSummary>> {
        self.flush()?;
        let mut stmt = self.conn.prepare(
            "SELECT book.id, book.title, book.authors, book.last_open,
                    COALESCE(sum(page_stat.duration), 0), count(DISTINCT page_stat.page)
             FROM book
             LEFT JOIN page_stat ON page_stat.id_book = book.id
             GROUP BY book.id
             ORDER BY book.last_open DESC, book.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(BookSummary {
                id: row.get(0)?,
                title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                authors: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                last_open: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                seconds: row.get(4)?,
                distinct_pages: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    /// Average seconds per page for a book, from capped totals. `None` without read pages.
    pub fn average_time_per_page(&mut self, book_id: i64) -> Result<Option<f64>> {
        self.flush()?;
        let (pages, seconds) = capped_totals(&self.conn, book_id, self.max_sec)?;
        Ok((pages > 0).then(|| seconds as f64 / pages as f64))
    }

    /// Rates for estimating the time left in a book. Use [`BookEstimate::time_left`]
    /// with the pages left in the book, or in a chapter.
    pub fn book_estimate(&mut self, book_id: i64) -> Result<BookEstimate> {
        self.flush()?;
        let (read_pages, read_seconds) = capped_totals(&self.conn, book_id, self.max_sec)?;
        let days_read: i64 = self
            .conn
            .query_row(DAYS_READ_SQL, params![book_id], |row| row.get(0))?;
        let seconds_per_day =
            (days_read > 0 && read_seconds > 0).then(|| read_seconds as f64 / days_read as f64);
        Ok(BookEstimate {
            read_pages,
            read_seconds,
            days_read,
            avg_page_seconds: average_or_default(read_pages, read_seconds, self.max_sec),
            seconds_per_day,
        })
    }

    /// Longest run of consecutive reading days, and the run that ends today or yesterday.
    pub fn streaks(&mut self, now: i64) -> Result<Streaks> {
        self.flush()?;
        let (today, yesterday): (String, String) = self.conn.query_row(
            "SELECT strftime('%Y-%m-%d', ?1, 'unixepoch', 'localtime'),
                    strftime('%Y-%m-%d', ?1, 'unixepoch', 'localtime', '-1 day')",
            params![now],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let (current, longest) = self
            .conn
            .query_row(STREAKS_SQL, params![today, yesterday], |row| {
                Ok((row.get::<_, i64>(1)?, row.get::<_, i64>(0)?))
            })?;
        Ok(Streaks { current, longest })
    }
}

impl BookEstimate {
    /// Time to read `pages_left` pages at the average rate, and the finish date at the daily rate.
    pub fn time_left(&self, pages_left: i64, now: i64) -> TimeLeft {
        let seconds = if pages_left > 0 {
            pages_left as f64 * self.avg_page_seconds
        } else {
            0.0
        };
        let days = match self.seconds_per_day {
            Some(rate) if seconds > 0.0 && rate > 0.0 => Some((seconds / rate).ceil() as i64),
            _ => None,
        };
        TimeLeft {
            seconds,
            days,
            finish_ts: days.map(|days| now + days * 86400),
        }
    }
}
