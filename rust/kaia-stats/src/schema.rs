//! Schema creation and upgrades.
//!
//! The SQL strings are copies of the ones in KOReader's
//! `plugins/statistics.koplugin/main.lua` (`createDB` and the `upgradeDBto*`
//! functions). They stay verbatim so both programs write the same layout.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

use crate::{Result, DB_SCHEMA_VERSION};

const BOOK_TABLE: &str = r#"
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
"#;

const BOOK_UNIQUE_INDEX: &str = r#"
        CREATE UNIQUE INDEX IF NOT EXISTS book_title_authors_md5 ON book(title, authors, md5);
"#;

const PAGE_STAT_DATA_SCHEMA: &str = r#"
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
"#;

const PAGE_STAT_DATA_INDEX: &str = r#"
    CREATE INDEX IF NOT EXISTS page_stat_data_start_time ON page_stat_data(start_time);
"#;

const PAGE_STAT_VIEW_SCHEMA: &str = r#"
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

const UPGRADE_20201010_LEGACY: &str = r#"
        -- Start by updating the layout of the old page_stat table
        ALTER TABLE page_stat RENAME COLUMN period TO duration;
        -- We're now using the user_version PRAGMA to keep track of schema version
        DROP TABLE IF EXISTS info;
"#;

const UPGRADE_20201010_MIGRATE: &str = r#"
        -- Migrate page_stat content to page_stat_data, and populate total_pages from book's pages while we're at it.
        -- NOTE: While doing a per-book migration could ensure a potentially more accurate page count,
        --       we need to populate total_pages *now*, or queries against unopened books would return completely bogus values...
        --       We'll just have to hope the current value of the column pages in the book table is not too horribly out of date,
        --       and not too horribly out of phase with the actual page count at the time the data was originally collected...
        INSERT INTO page_stat_data
            SELECT id_book, page, start_time, duration, pages as total_pages FROM page_stat
            JOIN book on book.id = id_book;

        -- Drop old page_stat table
        DROP INDEX IF EXISTS page_stat_id_book;
        DROP TABLE IF EXISTS page_stat;
"#;

const UPGRADE_20221111: &str = r#"
        -- We make the index on book's (title, author, md5) unique in order to sync dbs
        -- First we fill null authors with ''
        UPDATE book SET authors = '' WHERE authors IS NULL;
        -- Secondly, we unify the id_book in page_stat_data entries for duplicate books
        -- to the smallest of each, so as to delete the others.
        UPDATE page_stat_data SET id_book = (
            SELECT map.min_id FROM (
                SELECT id, (
                    SELECT min(id) FROM book b2
                    WHERE (book.title, book.authors, book.md5) = (b2.title, b2.authors, b2.md5)
                ) as min_id
                FROM book WHERE book.id >= min_id
            ) as map WHERE page_stat_data.id_book = map.id
        );
        -- Delete duplicate books and keep the one with smallest id.
        DELETE FROM book WHERE id > (
            SELECT MIN(id) FROM book b2
            WHERE (book.title, book.authors, book.md5) = (b2.title, b2.authors, b2.md5)
        );
        -- Then we recompute the book statistics based on merged books
        UPDATE book SET (total_read_pages, total_read_time) =
        (SELECT count(DISTINCT page),
                sum(duration)
         FROM   page_stat
         WHERE  id_book = book.id);
        -- Finally we update the index to be unique
        DROP INDEX IF EXISTS book_title_authors_md5;
        CREATE UNIQUE INDEX book_title_authors_md5 ON book(title, authors, md5);
"#;

/// Creates the schema of a new statistics database, as KOReader's `createDB` does.
fn create(conn: &mut Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    let tx = conn.transaction()?;
    tx.execute_batch(BOOK_TABLE)?;
    tx.execute_batch(BOOK_UNIQUE_INDEX)?;
    tx.execute_batch(PAGE_STAT_DATA_SCHEMA)?;
    tx.execute_batch(PAGE_STAT_DATA_INDEX)?;
    tx.execute_batch(PAGE_STAT_VIEW_SCHEMA)?;
    tx.execute_batch(&format!("PRAGMA user_version={DB_SCHEMA_VERSION};"))?;
    tx.commit()?;
    Ok(())
}

/// Brings an existing database to `DB_SCHEMA_VERSION`, or creates it when it has no `book` table.
///
/// A newer schema logs a warning and remains open, as KOReader does.
pub(crate) fn prepare(conn: &mut Connection, path: &Path) -> Result<()> {
    let has_book: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'book'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if has_book.is_none() {
        return create(conn);
    }
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > DB_SCHEMA_VERSION {
        eprintln!("warning: statistics schema {version} is newer than supported schema {DB_SCHEMA_VERSION}");
        return Ok(());
    }
    if version < DB_SCHEMA_VERSION {
        backup(conn, path, version)?;
        migrate(conn, version)?;
    }
    Ok(())
}

/// Copies the database file before a migration, as KOReader does. An existing backup is kept.
fn backup(conn: &Connection, path: &Path, version: i64) -> Result<()> {
    // Fold the WAL into the main file so the copy holds every committed write.
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".bkp.{version}-to-{DB_SCHEMA_VERSION}"));
    let target = PathBuf::from(name);
    if !target.exists() {
        std::fs::copy(path, &target)?;
    }
    Ok(())
}

/// Runs the upgrade steps newer than `version`, in order, as KOReader's `checkInitDatabase` does.
fn migrate(conn: &mut Connection, version: i64) -> Result<()> {
    if version < 20201010 {
        let tx = conn.transaction()?;
        tx.execute_batch(UPGRADE_20201010_LEGACY)?;
        tx.execute_batch(PAGE_STAT_DATA_SCHEMA)?;
        tx.execute_batch(UPGRADE_20201010_MIGRATE)?;
        tx.execute_batch(PAGE_STAT_VIEW_SCHEMA)?;
        tx.execute_batch("PRAGMA user_version=20201010;")?;
        tx.commit()?;
    }
    if version < 20201022 {
        let tx = conn.transaction()?;
        tx.execute_batch(PAGE_STAT_DATA_INDEX)?;
        tx.execute_batch("PRAGMA user_version=20201022;")?;
        tx.commit()?;
    }
    if version < 20221111 {
        let tx = conn.transaction()?;
        tx.execute_batch(UPGRADE_20221111)?;
        tx.execute_batch("PRAGMA user_version=20221111;")?;
        tx.commit()?;
    }
    conn.execute_batch("PRAGMA temp_store = 2;")?;
    // Compacting frees the space of the dropped page_stat table. It is optional, so a failure is ignored.
    let _ = conn.execute_batch("VACUUM;");
    Ok(())
}
