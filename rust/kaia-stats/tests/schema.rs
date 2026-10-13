mod common;

use common::{add_book, koreader_db, legacy_db, local, Scratch};
use kaia_stats::{Stats, DB_SCHEMA_VERSION};
use rusqlite::{Connection, OpenFlags};

/// Every schema object with its SQL, whitespace collapsed, sorted by name.
fn schema_objects(path: &std::path::Path) -> Vec<(String, String, String)> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut stmt = conn
        .prepare("SELECT type, name, COALESCE(sql, '') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name")
        .unwrap();
    stmt.query_map([], |row| {
        let sql: String = row.get(2)?;
        Ok((
            row.get(0)?,
            row.get(1)?,
            sql.split_whitespace().collect::<Vec<_>>().join(" "),
        ))
    })
    .unwrap()
    .map(|row| row.unwrap())
    .collect()
}

fn user_version(path: &std::path::Path) -> i64 {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

#[test]
fn new_database_matches_the_koreader_layout() {
    let dir = Scratch::new();
    let kaia_path = dir.path("kaia.sqlite3");
    let koreader_path = dir.path("koreader.sqlite3");
    drop(Stats::open(&kaia_path).unwrap());
    drop(koreader_db(&koreader_path));

    let kaia = schema_objects(&kaia_path);
    let koreader = schema_objects(&koreader_path);
    assert_eq!(kaia, koreader);
    assert_eq!(user_version(&kaia_path), DB_SCHEMA_VERSION);
    assert_eq!(user_version(&kaia_path), 20221111);
}

#[test]
fn newer_schema_warns_and_opens_without_migrating() {
    let dir = Scratch::new();
    let path = dir.path("stats.sqlite3");
    {
        let conn = koreader_db(&path);
        conn.execute_batch("PRAGMA user_version=20221112;").unwrap();
    }
    drop(Stats::open(&path).unwrap());
    assert_eq!(user_version(&path), 20221112);
    assert!(!dir.path("stats.sqlite3.bkp.20221112-to-20221111").exists());
}

#[test]
fn legacy_schema_is_upgraded_and_backed_up() {
    let dir = Scratch::new();
    let path = dir.path("stats.sqlite3");
    {
        let conn = legacy_db(&path);
        let book = add_book(&conn, "Old", Some("Author"), "md5old", 40);
        conn.execute(
            "INSERT INTO page_stat (id_book, page, start_time, period) VALUES (?1, 4, ?2, 45)",
            rusqlite::params![book, local("2019-02-03 10:00:00")],
        )
        .unwrap();
    }

    let stats = Stats::open(&path).unwrap();
    drop(stats);

    assert_eq!(user_version(&path), 20221111);
    let backup = dir.path("stats.sqlite3.bkp.0-to-20221111");
    assert!(backup.exists(), "backup of the legacy file is kept");
    assert_eq!(user_version(&backup), 0);

    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let row: (i64, i64, i64, i64, i64) = conn
        .query_row(
            "SELECT id_book, page, start_time, duration, total_pages FROM page_stat_data",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
        )
        .unwrap();
    assert_eq!(row, (1, 4, local("2019-02-03 10:00:00"), 45, 40));
    let through_view: i64 = conn
        .query_row("SELECT count(*) FROM page_stat", [], |row| row.get(0))
        .unwrap();
    assert_eq!(through_view, 1);
}

#[test]
fn duplicate_books_merge_when_upgrading_from_20201022() {
    let dir = Scratch::new();
    let path = dir.path("stats.sqlite3");
    {
        let conn = koreader_db(&path);
        // Two rows for one book, one with NULL authors (the form before the unique index).
        conn.execute_batch("DROP INDEX book_title_authors_md5; PRAGMA user_version=20201022;")
            .unwrap();
        let first = add_book(&conn, "Dup", None, "md5dup", 10);
        let second = add_book(&conn, "Dup", None, "md5dup", 10);
        conn.execute(
            "INSERT INTO page_stat_data (id_book, page, start_time, duration, total_pages) VALUES (?1, 1, 1000, 30, 10)",
            [first],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO page_stat_data (id_book, page, start_time, duration, total_pages) VALUES (?1, 2, 2000, 40, 10)",
            [second],
        )
        .unwrap();
    }

    drop(Stats::open(&path).unwrap());

    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let books: Vec<(i64, String)> = conn
        .prepare("SELECT id, authors FROM book")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    assert_eq!(books, vec![(1, String::new())]);
    let spans: Vec<(i64, i64)> = conn
        .prepare("SELECT id_book, duration FROM page_stat_data ORDER BY start_time")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    assert_eq!(spans, vec![(1, 30), (1, 40)]);
    let totals: (i64, i64) = conn
        .query_row(
            "SELECT total_read_pages, total_read_time FROM book WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(totals, (2, 70));
}
