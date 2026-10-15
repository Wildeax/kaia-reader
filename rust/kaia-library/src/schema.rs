//! Database open, pragmas and migrations. Follows the rules of `kaia-storage`:
//! `user_version` holds the schema version, each migration runs in one
//! transaction that also bumps the version, and an existing database is copied
//! to `backups/` before any migration runs.

use std::fs;
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, Transaction, TransactionBehavior};

use crate::error::{LibraryError, Result};
use crate::extract::reencode_png;
use crate::query::register_sql_functions;

pub(crate) const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// One schema step. `version` is the `user_version` after the step runs.
pub(crate) struct Migration {
    pub version: u32,
    pub sql: &'static str,
    /// Work that SQL cannot express. It runs after `sql`, inside the same
    /// transaction, so a failure rolls back both.
    pub step: Option<fn(&Transaction<'_>) -> Result<()>>,
}

const SCHEMA_V1: &str = "
CREATE TABLE books (
  id             INTEGER PRIMARY KEY,
  path           TEXT NOT NULL UNIQUE,
  size           INTEGER NOT NULL,
  mtime          INTEGER NOT NULL,
  partial_md5    TEXT NOT NULL,
  title          TEXT,
  authors        TEXT,
  series         TEXT,
  series_index   REAL,
  language       TEXT,
  description    TEXT,
  pages          INTEGER,
  format         TEXT NOT NULL,
  added_at       INTEGER NOT NULL,
  last_opened_at INTEGER,
  percent        REAL NOT NULL DEFAULT 0 CHECK (percent >= 0 AND percent <= 1),
  status         TEXT NOT NULL DEFAULT 'new'
                 CHECK (status IN ('new', 'reading', 'abandoned', 'complete')),
  extracted      INTEGER NOT NULL DEFAULT 0 CHECK (extracted IN (0, 1)),
  failure        TEXT,
  attempts       INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX books_content ON books(partial_md5, size);
CREATE INDEX books_last_opened ON books(last_opened_at);

CREATE TABLE covers (
  book_id  INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
  source_w INTEGER NOT NULL,
  source_h INTEGER NOT NULL,
  small    BLOB,
  large    BLOB
);

CREATE TABLE collections (
  id   INTEGER PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
);
CREATE TABLE collection_books (
  collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
  book_id       INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  position      INTEGER NOT NULL,
  PRIMARY KEY (collection_id, book_id)
);
CREATE INDEX collection_books_book ON collection_books(book_id);

CREATE TABLE history (
  id        INTEGER PRIMARY KEY,
  book_id   INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  opened_at INTEGER NOT NULL
);
CREATE INDEX history_book_time ON history(book_id, opened_at);

CREATE VIRTUAL TABLE book_fts USING fts5(
  title, authors, series, description,
  content='books', content_rowid='id',
  tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER books_fts_insert AFTER INSERT ON books BEGIN
  INSERT INTO book_fts(rowid, title, authors, series, description)
  VALUES (new.id, new.title, new.authors, new.series, new.description);
END;
CREATE TRIGGER books_fts_delete AFTER DELETE ON books BEGIN
  INSERT INTO book_fts(book_fts, rowid, title, authors, series, description)
  VALUES ('delete', old.id, old.title, old.authors, old.series, old.description);
END;
CREATE TRIGGER books_fts_update AFTER UPDATE OF title, authors, series, description ON books BEGIN
  INSERT INTO book_fts(book_fts, rowid, title, authors, series, description)
  VALUES ('delete', old.id, old.title, old.authors, old.series, old.description);
  INSERT INTO book_fts(rowid, title, authors, series, description)
  VALUES (new.id, new.title, new.authors, new.series, new.description);
END;
";

/// Version 2: covers record their MIME type, and the existing PNG covers are
/// re-encoded as JPEG by the step below. New rows state their MIME type, so the
/// default only applies to rows written before this version.
const SCHEMA_V2: &str = "
ALTER TABLE covers ADD COLUMN mime TEXT NOT NULL DEFAULT 'image/png';
";

/// Missing books keep their related rows. A scan never deletes them. Covers lost
/// in version 2 need extraction.
const SCHEMA_V3: &str = "
ALTER TABLE books ADD COLUMN missing_since INTEGER;
CREATE INDEX books_missing_since ON books(missing_since);
UPDATE books SET extracted = 0, attempts = 0, failure = NULL
WHERE id IN (SELECT book_id FROM covers
             WHERE (small IS NULL OR large IS NULL) AND (source_w > 0 OR source_h > 0));
";

/// Version 4 adds collection ordering and a normalized name key. The step
/// below merges names that differ only by case before it adds the unique key.
const SCHEMA_V4: &str = "
ALTER TABLE collections ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
ALTER TABLE collections ADD COLUMN name_key TEXT NOT NULL DEFAULT '';
";

/// Version 5 changes the cover size and source. This step queues every extracted
/// book, so the next scan of its file stores the larger cover. A re-extraction
/// that fails keeps the stored title, metadata and cover (see `scan.rs`). Books
/// whose file is gone keep their rows and covers. No column changes.
const SCHEMA_V5: &str = "
UPDATE books SET extracted = 0, attempts = 0 WHERE extracted = 1;
";

/// Version 6 adds the recent searches. Each distinct search is one row, under
/// its lower-case key. The newest search has the highest id.
const SCHEMA_V6: &str = "
CREATE TABLE recent_searches (
  id    INTEGER PRIMARY KEY,
  query TEXT NOT NULL,
  key   TEXT NOT NULL UNIQUE
);
";

/// Version 7 queues the extracted books that have a series but no number. An
/// EPUB 3 collection can give its number in a `group-position` refinement, which
/// earlier builds did not read. The next scan extracts these books again.
const SCHEMA_V7: &str = "
UPDATE books SET extracted = 0, attempts = 0
WHERE extracted = 1 AND series IS NOT NULL AND series_index IS NULL;
";

/// Version 8 adds tags. A tag is free text that a book carries. `tags` holds each
/// distinct tag once, under its lower-case key, and `book_tags` links books to
/// tags. Favorites need no change: a favorite is a membership of the collection
/// named `favorites`. Existing rows stay as they are.
const SCHEMA_V8: &str = "
CREATE TABLE tags (
  id       INTEGER PRIMARY KEY,
  name     TEXT NOT NULL,
  name_key TEXT NOT NULL UNIQUE
);
CREATE TABLE book_tags (
  book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
  tag_id  INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (book_id, tag_id)
);
CREATE INDEX book_tags_tag ON book_tags(tag_id);
";

pub(crate) const MIGRATIONS: [Migration; 8] = [
    Migration {
        version: 1,
        sql: SCHEMA_V1,
        step: None,
    },
    Migration {
        version: 2,
        sql: SCHEMA_V2,
        step: Some(reencode_covers),
    },
    Migration {
        version: 3,
        sql: SCHEMA_V3,
        step: None,
    },
    Migration {
        version: 4,
        sql: SCHEMA_V4,
        step: Some(migrate_collections),
    },
    Migration {
        version: 5,
        sql: SCHEMA_V5,
        step: None,
    },
    Migration {
        version: 6,
        sql: SCHEMA_V6,
        step: None,
    },
    Migration {
        version: 7,
        sql: SCHEMA_V7,
        step: None,
    },
    Migration {
        version: 8,
        sql: SCHEMA_V8,
        step: None,
    },
];

fn migrate_collections(tx: &Transaction<'_>) -> Result<()> {
    let mut stmt = tx.prepare("SELECT id, name FROM collections ORDER BY name COLLATE NOCASE, id")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let rows: Vec<(i64, String)> = rows.collect::<rusqlite::Result<_>>()?;
    drop(stmt);

    let mut keys = std::collections::HashMap::<String, i64>::new();
    let mut ordered = Vec::new();
    for (id, name) in rows {
        let key = name.to_lowercase();
        if let Some(&keep_id) = keys.get(&key) {
            tx.execute(
                "INSERT OR IGNORE INTO collection_books (collection_id, book_id, position)
                 SELECT ?1, book_id, position FROM collection_books WHERE collection_id = ?2",
                params![keep_id, id],
            )?;
            tx.execute("DELETE FROM collections WHERE id = ?1", [id])?;
        } else {
            keys.insert(key.clone(), id);
            ordered.push((id, key));
        }
    }

    for (position, (id, key)) in ordered.into_iter().enumerate() {
        tx.execute(
            "UPDATE collections SET position = ?2, name_key = ?3 WHERE id = ?1",
            params![id, position as i64, key],
        )?;
    }
    tx.execute_batch("CREATE UNIQUE INDEX collections_name_key ON collections(name_key);")?;
    Ok(())
}

/// Re-encodes version 1 PNG covers as JPEG at the same pixel size. One book is
/// handled at a time, so memory use stays at one cover. A cover that does not
/// decode loses its bytes (NULL) and the book has no cover until it is
/// extracted again.
fn reencode_covers(tx: &Transaction<'_>) -> Result<()> {
    let ids: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT book_id FROM covers WHERE mime = 'image/png'")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        let collected: rusqlite::Result<Vec<i64>> = rows.collect();
        collected?
    };
    for id in ids {
        let (small, large): (Option<Vec<u8>>, Option<Vec<u8>>) =
            tx.query_row("SELECT small, large FROM covers WHERE book_id = ?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let small = small.and_then(|b| reencode_png(&b));
        let large = large.and_then(|b| reencode_png(&b));
        tx.execute(
            "UPDATE covers SET small = ?2, large = ?3, mime = 'image/jpeg' WHERE book_id = ?1",
            params![id, small, large],
        )?;
    }
    Ok(())
}

/// Opens the database at `path` with the built-in migrations.
pub(crate) fn open(path: &Path) -> Result<Connection> {
    open_with_migrations(path, &MIGRATIONS)
}

/// Opens the database with an explicit migration list. Tests use this to inject a
/// failing migration. Versions must start at 1 and increase.
pub(crate) fn open_with_migrations(path: &Path, migrations: &[Migration]) -> Result<Connection> {
    let supported = migrations.last().map_or(0, |m| m.version);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let existed = path.exists();
    let mut conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(BUSY_TIMEOUT)?;
    register_sql_functions(&conn)?;

    // Read the version before any write, so a newer file is never touched.
    let found: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if found > supported {
        return Err(LibraryError::SchemaTooNew { found, supported });
    }

    conn.pragma_update(None, "journal_mode", "DELETE")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;

    let pending: Vec<&Migration> = migrations.iter().filter(|m| m.version > found).collect();
    if existed && !pending.is_empty() {
        backup_database(&conn, path, found)?;
    }
    for m in pending {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(m.sql)?;
        if let Some(step) = m.step {
            step(&tx)?;
        }
        tx.pragma_update(None, "user_version", m.version)?;
        tx.commit()?;
    }
    Ok(conn)
}

/// Copies the database to `<dir>/backups/library-v<from>-<unix time>.sqlite3`.
fn backup_database(conn: &Connection, db_path: &Path, from: u32) -> Result<()> {
    let dir = db_path.parent().unwrap_or(Path::new(".")).join("backups");
    fs::create_dir_all(&dir)?;
    let dest = dir.join(format!("library-v{from}-{}.sqlite3", crate::now_unix()));
    let mut out = Connection::open(&dest)?;
    let backup = rusqlite::backup::Backup::new(conn, &mut out)?;
    backup.run_to_completion(64, Duration::ZERO, None)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CoverSize, Library};
    use kaia_gfx::image::{decode_rgb, DecodeLimits};
    use kaia_gfx::{PixelFormat, Pixmap};

    const BROKEN_V2: &str = "CREATE TABLE nope (";

    const V1_ONLY: [Migration; 1] = [Migration {
        version: 1,
        sql: SCHEMA_V1,
        step: None,
    }];

    /// A pixmap with a color gradient, so the JPEG round trip can be checked
    /// for color as well as size.
    fn gradient(w: u32, h: u32) -> Pixmap {
        let mut pm = Pixmap::new(w, h, PixelFormat::Rgb24);
        for y in 0..h {
            for x in 0..w {
                let r = (x * 255 / w.max(1)) as u8;
                let g = (y * 255 / h.max(1)) as u8;
                pm.put(x as i32, y as i32, kaia_gfx::Color::rgb(r, g, 128));
            }
        }
        pm
    }

    #[test]
    fn fresh_database_is_at_latest_version() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("library.sqlite3")).unwrap();
        let v: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 8);
        let fts: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'book_fts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts, 1, "FTS5 table exists");
    }

    #[test]
    fn newer_schema_is_refused_without_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", 99).unwrap();
        }
        let before = fs::read(&path).unwrap();
        let err = open(&path).unwrap_err();
        assert!(matches!(
            err,
            LibraryError::SchemaTooNew {
                found: 99,
                supported: 8
            }
        ));
        assert_eq!(fs::read(&path).unwrap(), before, "file is unchanged");
    }

    #[test]
    fn failed_migration_rolls_back_and_keeps_a_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite3");
        open_with_migrations(&path, &V1_ONLY).unwrap();

        let bad = [
            Migration {
                version: 1,
                sql: SCHEMA_V1,
                step: None,
            },
            Migration {
                version: 2,
                sql: BROKEN_V2,
                step: None,
            },
        ];
        assert!(open_with_migrations(&path, &bad).is_err());

        let conn = Connection::open(&path).unwrap();
        let v: u32 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1, "version unchanged after failed migration");
        let backups: Vec<_> = fs::read_dir(dir.path().join("backups")).unwrap().collect();
        assert_eq!(backups.len(), 1, "one backup was taken before the migration");
    }

    #[test]
    fn failing_step_rolls_back_the_sql_too() {
        fn fail(_: &Transaction<'_>) -> Result<()> {
            Err(LibraryError::InvalidArgument("step failed".into()))
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite3");
        open_with_migrations(&path, &V1_ONLY).unwrap();
        let bad = [
            Migration {
                version: 1,
                sql: SCHEMA_V1,
                step: None,
            },
            Migration {
                version: 2,
                sql: SCHEMA_V2,
                step: Some(fail),
            },
        ];
        assert!(open_with_migrations(&path, &bad).is_err());
        let conn = Connection::open(&path).unwrap();
        let has_mime: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('covers') WHERE name = 'mime'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(has_mime, 0, "the column added by the SQL step is rolled back");
    }

    /// A version 1 database with PNG covers, opened by this build, comes out at
    /// version 5 with JPEG covers of the same pixel size.
    #[test]
    fn version_one_png_covers_are_reencoded_to_jpeg() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite3");
        let large_png = kaia_gfx::png::encode_pixmap(&gradient(320, 480)).unwrap();
        let small_png = kaia_gfx::png::encode_pixmap(&gradient(107, 160)).unwrap();
        {
            let conn = open_with_migrations(&path, &V1_ONLY).unwrap();
            conn.execute(
                "INSERT INTO books (id, path, size, mtime, partial_md5, format, added_at)
                 VALUES (1, '/books/a.epub', 10, 1, 'abcdef', 'epub', 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO covers (book_id, source_w, source_h, small, large) VALUES (1, 600, 900, ?1, ?2)",
                params![small_png, large_png],
            )
            .unwrap();
            // A cover that does not decode is dropped by the migration.
            conn.execute(
                "INSERT INTO books (id, path, size, mtime, partial_md5, format, added_at, extracted)
                 VALUES (2, '/books/b.epub', 10, 1, 'bcdef0', 'epub', 1, 1)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO covers (book_id, source_w, source_h, small, large) VALUES (2, 1, 1, ?1, ?1)",
                params![b"not an image".to_vec()],
            )
            .unwrap();
        }

        let lib = Library::open(&path).unwrap();
        let v: u32 = Connection::open(&path)
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 8);
        assert_eq!(
            fs::read_dir(dir.path().join("backups")).unwrap().count(),
            1,
            "backup taken"
        );

        let large = lib.cover(1, CoverSize::Large).unwrap().expect("large cover kept");
        assert_eq!(large.mime, "image/jpeg");
        assert_eq!(&large.bytes[..3], &[0xFF, 0xD8, 0xFF]);
        let decoded = decode_rgb(&large.bytes, DecodeLimits::default()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (320, 480), "pixel size kept");
        // Color survives: the blue channel stays near the source value of 128.
        let px = decoded.get(160, 240).unwrap();
        assert!(px.r.abs_diff(128) < 12 && px.g.abs_diff(128) < 12 && px.b.abs_diff(128) < 12);
        let small = lib.cover(1, CoverSize::Small).unwrap().expect("small cover kept");
        assert_eq!(small.mime, "image/jpeg");
        let decoded = decode_rgb(&small.bytes, DecodeLimits::default()).unwrap();
        assert_eq!((decoded.width(), decoded.height()), (107, 160));

        assert!(
            lib.cover(2, CoverSize::Large).unwrap().is_none(),
            "undecodable cover dropped"
        );
        assert!(
            !lib.book(2).unwrap().unwrap().extracted,
            "book is queued for extraction"
        );
    }

    /// A version 7 database, as the previous build left it, with a favorites
    /// collection of KOReader's name. It opens at version 8, keeps the favorite,
    /// takes a backup, and accepts tags.
    #[test]
    fn version_seven_database_migrates_and_keeps_its_favorites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite3");
        {
            let conn = open_with_migrations(&path, &MIGRATIONS[..7]).unwrap();
            conn.execute(
                "INSERT INTO books (id, path, size, mtime, partial_md5, title, format, added_at)
                 VALUES (3, '/books/a.epub', 12, 3, 'hash', 'Title', 'epub', 4)",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO collections (id, name, position, name_key) VALUES (5, 'favorites', 0, 'favorites')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO collection_books (collection_id, book_id, position) VALUES (5, 3, 1)",
                [],
            )
            .unwrap();
        }

        let mut lib = Library::open(&path).unwrap();
        assert!(lib.is_favorite(3).unwrap(), "the KOReader favorite is kept");
        assert_eq!(lib.collections().unwrap()[0].name, "favorites");
        assert!(lib.tags().unwrap().is_empty());
        lib.add_tag(3, "after upgrade").unwrap();
        assert_eq!(lib.book_tags(3).unwrap()[0].name, "after upgrade");
        let v: u32 = Connection::open(&path)
            .unwrap()
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 8);
        assert_eq!(
            fs::read_dir(dir.path().join("backups")).unwrap().count(),
            1,
            "a backup of version 7 is taken"
        );
    }

    #[test]
    fn version_three_upgrade_keeps_books_and_merges_case_only_collections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite3");
        {
            let conn = open_with_migrations(&path, &MIGRATIONS[..3]).unwrap();
            conn.execute(
                "INSERT INTO books (id, path, size, mtime, partial_md5, title, series,
                   series_index, format, added_at, last_opened_at, percent, status)
                 VALUES (7, '/books/a.epub', 12, 3, 'hash', 'Title', 'Saga', 2,
                   'epub', 4, 5, 0.4, 'reading')",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO collections (id, name) VALUES (10, 'Books')", [])
                .unwrap();
            conn.execute("INSERT INTO collections (id, name) VALUES (11, 'books')", [])
                .unwrap();
            conn.execute(
                "INSERT INTO collection_books (collection_id, book_id, position)
                 VALUES (10, 7, 3), (11, 7, 1)",
                [],
            )
            .unwrap();
            conn.execute("INSERT INTO history (book_id, opened_at) VALUES (7, 5)", [])
                .unwrap();
        }

        let lib = Library::open(&path).unwrap();
        let book = lib.book(7).unwrap().unwrap();
        assert_eq!(book.title.as_deref(), Some("Title"));
        assert_eq!(book.series.as_deref(), Some("Saga"));
        assert_eq!(book.series_index, Some(2.0));
        assert_eq!(book.last_opened_at, Some(5));
        assert_eq!(book.percent, 0.4);
        let collections = lib.collections().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].name, "Books");
        assert_eq!(collections[0].books, 1);
        let raw = Connection::open(&path).unwrap();
        let history: i64 = raw
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(history, 1);
        let version: u32 = raw
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 8);
    }
}
