//! Fixtures for the integration tests.
//!
//! The SQL below is a second copy of the schema from KOReader's
//! `plugins/statistics.koplugin/main.lua`. The tests compare it against what
//! `Stats::open` creates, so a drift between the two copies fails a test.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

/// `ReaderStatistics:createDB` in KOReader, without the journal mode line.
pub const KOREADER_CREATE_SQL: &str = r#"
        -- book
        CREATE TABLE IF NOT EXISTS book
            (
                id integer PRIMARY KEY autoincrement,
                title text,
                authors text,
                notes      integer,
                last_open  integer,
                highlights integer,
                pages      integer,
                series text,
                language text,
                md5 text,
                total_read_time  integer,
                total_read_pages integer
            );
        CREATE UNIQUE INDEX IF NOT EXISTS book_title_authors_md5 ON book(title, authors, md5);
    CREATE TABLE IF NOT EXISTS page_stat_data
        (
            id_book     integer,
            page        integer NOT NULL DEFAULT 0,
            start_time  integer NOT NULL DEFAULT 0,
            duration    integer NOT NULL DEFAULT 0,
            total_pages integer NOT NULL DEFAULT 0,
            UNIQUE (id_book, page, start_time),
            FOREIGN KEY(id_book) REFERENCES book(id)
        );
    CREATE INDEX IF NOT EXISTS page_stat_data_start_time ON page_stat_data(start_time);
    -- Create the numbers table, used as a source of extra rows when scaling pages in the page_stat view
    CREATE TABLE IF NOT EXISTS numbers
        (
            number INTEGER PRIMARY KEY
        );
    WITH RECURSIVE counter AS
        (
            SELECT 1 as N UNION ALL
            SELECT N + 1 FROM counter WHERE N < 1000
        )
        INSERT INTO numbers SELECT N AS number FROM counter;

    -- Create the page_stat view
    -- This view rescales data from the page_stat_data table to the current number of book pages
    -- c.f., https://github.com/koreader/koreader/pull/6761#issuecomment-705660154
    CREATE VIEW IF NOT EXISTS page_stat AS
        SELECT id_book, first_page + idx - 1 AS page, start_time, duration / (last_page - first_page + 1) AS duration
        FROM (
            SELECT id_book, page, total_pages, pages, start_time, duration,
                -- First page_number for this page after rescaling single row
                ((page - 1) * pages) / total_pages + 1 AS first_page,
                -- Last page_number for this page after rescaling single row
                max(((page - 1) * pages) / total_pages + 1, (page * pages) / total_pages) AS last_page,
                idx
            FROM page_stat_data
            JOIN book ON book.id = id_book
            -- Duplicate rows for multiple pages as needed (as a result of rescaling)
            JOIN (SELECT number as idx FROM numbers) AS N ON idx <= (last_page - first_page + 1)
        );
"#;

/// Schema before KOReader's 2020-10-10 change: a `period` column and an `info` table.
const LEGACY_SQL: &str = r#"
    CREATE TABLE book
        (
            id integer PRIMARY KEY autoincrement,
            title text,
            authors text,
            notes      integer,
            last_open  integer,
            highlights integer,
            pages      integer,
            series text,
            language text,
            md5 text,
            total_read_time  integer,
            total_read_pages integer
        );
    CREATE TABLE page_stat (id_book integer, page integer, start_time integer, period integer);
    CREATE TABLE info (version integer);
"#;

/// A statistics database created the way KOReader creates it, at schema 20221111.
pub fn koreader_db(path: &Path) -> Connection {
    let conn = Connection::open(path).expect("open fixture db");
    conn.execute_batch(KOREADER_CREATE_SQL)
        .expect("create koreader schema");
    conn.execute_batch("PRAGMA user_version=20221111;")
        .expect("set version");
    conn
}

/// A database at schema version 0 with the legacy `page_stat` table.
pub fn legacy_db(path: &Path) -> Connection {
    let conn = Connection::open(path).expect("open legacy db");
    conn.execute_batch(LEGACY_SQL).expect("create legacy schema");
    conn
}

/// Inserts a book row the way KOReader's `addBookStatToDB` does. Returns its id.
pub fn add_book(conn: &Connection, title: &str, authors: Option<&str>, md5: &str, pages: i64) -> i64 {
    conn.execute(
        "INSERT INTO book (title, authors, notes, last_open, highlights, pages, series, language, md5, \
         total_read_time, total_read_pages) VALUES (?1, ?2, 0, 0, 0, ?3, 'N/A', 'N/A', ?4, 0, 0)",
        params![title, authors, pages, md5],
    )
    .expect("insert book");
    conn.last_insert_rowid()
}

/// Inserts one span into `page_stat_data`, bypassing the recorder.
pub fn add_span(conn: &Connection, book: i64, page: i64, start: i64, duration: i64, total_pages: i64) {
    conn.execute(
        "INSERT INTO page_stat_data (id_book, page, start_time, duration, total_pages) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![book, page, start, duration, total_pages],
    )
    .expect("insert span");
}

/// Unix time of a local wall-clock time, `YYYY-MM-DD HH:MM:SS`, in the process time zone.
/// Computed by SQLite, so the expected values follow the same local time rules as the code.
pub fn local(text: &str) -> i64 {
    let conn = Connection::open_in_memory().expect("memory db");
    conn.query_row(
        "SELECT CAST(strftime('%s', ?1, 'utc') AS INTEGER)",
        params![text],
        |row| row.get(0),
    )
    .expect("local time")
}

/// A scratch directory that is removed when the value is dropped.
pub struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    pub fn new() -> Scratch {
        Scratch {
            dir: tempfile::tempdir().expect("temp dir"),
        }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }
}

/// Reads every row of `page_stat` for one book as `(page, duration)`, through the view.
pub fn view_pages(conn: &Connection, book: i64) -> Vec<(i64, i64)> {
    let mut stmt = conn
        .prepare("SELECT page, duration FROM page_stat WHERE id_book = ?1 ORDER BY page, duration")
        .expect("prepare");
    stmt.query_map(params![book], |row| Ok((row.get(0)?, row.get(1)?)))
        .expect("query")
        .map(|row| row.expect("row"))
        .collect()
}

/// Number of rows in `page_stat_data` for one book.
pub fn span_count(conn: &Connection, book: i64) -> i64 {
    conn.query_row(
        "SELECT count(*) FROM page_stat_data WHERE id_book = ?1",
        params![book],
        |row| row.get(0),
    )
    .expect("count")
}
