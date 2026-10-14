//! Reads and small writes: book lists, recent books, search, collections,
//! reading progress, status and covers.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row, TransactionBehavior};

use crate::error::{LibraryError, Result};
use crate::scan::{book_file, mtime_secs};
use crate::types::{Book, Collection, CoverImage, CoverSize, Filter, Sort, SortKey, Status};
use crate::{now_unix, Library};

const COLS: [&str; 19] = [
    "id",
    "path",
    "size",
    "mtime",
    "partial_md5",
    "title",
    "authors",
    "series",
    "series_index",
    "language",
    "description",
    "pages",
    "format",
    "added_at",
    "last_opened_at",
    "percent",
    "status",
    "extracted",
    "failure",
];

/// Id of the newest history row of a book. Opens in the same second are ordered
/// by it, so the last open always comes first.
const LAST_OPEN_SEQ: &str = "(SELECT MAX(h.id) FROM history h WHERE h.book_id = books.id)";

/// The book columns, each with `prefix` in front.
fn cols(prefix: &str) -> String {
    COLS.iter()
        .map(|c| format!("{prefix}{c}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn book_from_row(r: &Row<'_>) -> rusqlite::Result<Book> {
    let status: String = r.get(16)?;
    Ok(Book {
        id: r.get(0)?,
        path: PathBuf::from(r.get::<_, String>(1)?),
        size: r.get::<_, i64>(2)? as u64,
        mtime: r.get(3)?,
        partial_md5: r.get(4)?,
        title: r.get(5)?,
        authors: r.get(6)?,
        series: r.get(7)?,
        series_index: r.get(8)?,
        language: r.get(9)?,
        description: r.get(10)?,
        pages: r.get::<_, Option<i64>>(11)?.map(|p| p as u32),
        format: r.get(12)?,
        added_at: r.get(13)?,
        last_opened_at: r.get(14)?,
        percent: r.get(15)?,
        status: Status::parse(&status).unwrap_or_default(),
        extracted: r.get::<_, i64>(17)? != 0,
        failure: r.get(18)?,
    })
}

impl Library {
    /// The book with this id.
    pub fn book(&self, id: i64) -> Result<Option<Book>> {
        let sql = format!(
            "SELECT {} FROM books WHERE id = ?1 AND missing_since IS NULL",
            cols("")
        );
        Ok(self.conn.query_row(&sql, [id], book_from_row).optional()?)
    }

    /// The book stored at `path`. A relative or symlinked path is resolved first.
    pub fn book_by_path(&self, path: &Path) -> Result<Option<Book>> {
        let real = canonical_or(path);
        let Some(p) = real.to_str() else {
            return Ok(None);
        };
        let sql = format!(
            "SELECT {} FROM books WHERE path = ?1 AND missing_since IS NULL",
            cols("")
        );
        Ok(self.conn.query_row(&sql, [p], book_from_row).optional()?)
    }

    /// Books in the order `sort` gives, after `filter`, from `offset`, at most `limit`.
    pub fn books(&self, sort: Sort, filter: &Filter, offset: usize, limit: usize) -> Result<Vec<Book>> {
        let mut sql = format!("SELECT {} FROM books WHERE missing_since IS NULL", cols(""));
        let mut args: Vec<Value> = Vec::new();

        if let Some(status) = filter.status {
            sql.push_str(" AND status = ?");
            args.push(Value::Text(status.as_str().to_string()));
        }
        if let Some(format) = &filter.format {
            sql.push_str(" AND format = ?");
            args.push(Value::Text(format.to_ascii_lowercase()));
        }
        if let Some(cid) = filter.collection {
            sql.push_str(" AND id IN (SELECT book_id FROM collection_books WHERE collection_id = ?)");
            args.push(Value::Integer(cid));
        }
        if let Some(folder) = &filter.folder {
            let (lo, hi) = folder_range(folder)?;
            sql.push_str(" AND path >= ? AND path < ?");
            args.push(Value::Text(lo));
            args.push(Value::Text(hi));
        }
        if let Some(series) = &filter.series {
            sql.push_str(" AND series IS NOT NULL AND series COLLATE NOCASE = ?");
            args.push(Value::Text(series.trim().to_string()));
        }

        let dir = if sort.descending { "DESC" } else { "ASC" };
        let order = match sort.key {
            SortKey::Title => format!("COALESCE(title, path) COLLATE NOCASE {dir}, id"),
            SortKey::Author => format!(
                "COALESCE(authors, '') COLLATE NOCASE {dir}, COALESCE(title, path) COLLATE NOCASE, id"
            ),
            // Ties follow the same direction, so the book indexed last comes first
            // when the list is newest first.
            SortKey::Added => format!("added_at {dir}, id {dir}"),
            SortKey::LastRead => {
                format!("last_opened_at IS NULL, last_opened_at {dir}, {LAST_OPEN_SEQ} {dir}, id")
            }
            SortKey::Progress => format!("percent {dir}, COALESCE(title, path) COLLATE NOCASE, id"),
            SortKey::Series => format!(
                "series IS NULL, series COLLATE NOCASE {dir}, series_index IS NULL, series_index {dir}, COALESCE(title, path) COLLATE NOCASE {dir}, id"
            ),
            SortKey::Manual => match filter.collection {
                Some(cid) => {
                    args.push(Value::Integer(cid));
                    format!(
                        "(SELECT position FROM collection_books WHERE collection_id = ? AND book_id = books.id) {dir}, id"
                    )
                }
                None => format!("COALESCE(title, path) COLLATE NOCASE {dir}, id"),
            },
        };
        sql.push_str(&format!(" ORDER BY {order} LIMIT ? OFFSET ?"));
        args.push(Value::Integer(limit as i64));
        args.push(Value::Integer(offset as i64));

        self.query_books(&sql, params_from_iter(args))
    }

    /// The most recently opened books, newest first.
    pub fn recent(&self, limit: usize) -> Result<Vec<Book>> {
        let sql = format!(
            "SELECT {} FROM books WHERE last_opened_at IS NOT NULL AND missing_since IS NULL
             ORDER BY last_opened_at DESC, {LAST_OPEN_SEQ} DESC, id DESC LIMIT ?1",
            cols("")
        );
        self.query_books(&sql, [limit as i64])
    }

    /// Full-text search over title, authors, series and description. Each word
    /// matches as a prefix, and all words must match. Results are ranked by
    /// relevance. A query with no letters or digits returns nothing.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Book>> {
        let Some(expr) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT {} FROM book_fts JOIN books ON books.id = book_fts.rowid
             WHERE book_fts MATCH ?1 AND books.missing_since IS NULL ORDER BY book_fts.rank LIMIT ?2",
            cols("books.")
        );
        self.query_books(&sql, params![expr, limit as i64])
    }

    /// The book that follows `book_id` in its series: the book with the smallest
    /// series number above the book's own, and then the first by title. Books
    /// with no number do not count. None when the book has no series number, or
    /// when no other book in the library has a higher one.
    pub fn next_in_series(&self, book_id: i64) -> Result<Option<Book>> {
        let sql = format!(
            "SELECT {} FROM books
             JOIN (SELECT series, series_index FROM books WHERE id = ?1) AS cur
             WHERE cur.series_index IS NOT NULL
               AND books.series IS NOT NULL
               AND books.series COLLATE NOCASE = cur.series COLLATE NOCASE
               AND books.series_index > cur.series_index
               AND books.id != ?1
               AND books.missing_since IS NULL
             ORDER BY books.series_index, COALESCE(books.title, books.path) COLLATE NOCASE, books.id
             LIMIT 1",
            cols("books.")
        );
        Ok(self.conn.query_row(&sql, [book_id], book_from_row).optional()?)
    }

    /// Records a search. The text is trimmed. A search that matches an earlier
    /// one ignoring case replaces it, so each search is kept once and moves to the
    /// top. Only the newest [`RECENT_SEARCHES`](crate::RECENT_SEARCHES) are kept.
    /// Blank text is not recorded.
    pub fn record_search(&mut self, query: &str) -> Result<()> {
        let text = query.trim();
        if text.is_empty() {
            return Ok(());
        }
        let key = text.to_lowercase();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM recent_searches WHERE key = ?1", [&key])?;
        tx.execute(
            "INSERT INTO recent_searches (query, key) VALUES (?1, ?2)",
            params![text, key],
        )?;
        tx.execute(
            "DELETE FROM recent_searches WHERE id NOT IN
             (SELECT id FROM recent_searches ORDER BY id DESC LIMIT ?1)",
            [crate::RECENT_SEARCHES as i64],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The recent searches, newest first, at most `limit` of them.
    pub fn recent_searches(&self, limit: usize) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT query FROM recent_searches ORDER BY id DESC LIMIT ?1")?;
        let rows = stmt.query_map([limit as i64], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Removes every recent search. Books and their state do not change.
    pub fn clear_recent_searches(&mut self) -> Result<()> {
        self.conn.execute("DELETE FROM recent_searches", [])?;
        Ok(())
    }

    /// All collections by name, with the number of books in each.
    pub fn collections(&self) -> Result<Vec<Collection>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.name, COUNT(b.id)
             FROM collections c LEFT JOIN collection_books cb ON cb.collection_id = c.id
             LEFT JOIN books b ON b.id = cb.book_id AND b.missing_since IS NULL
             GROUP BY c.id ORDER BY c.position, c.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Collection {
                id: r.get(0)?,
                name: r.get(1)?,
                books: r.get::<_, i64>(2)? as u64,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Creates an empty collection and returns its id, name and book count.
    pub fn create_collection(&mut self, name: &str) -> Result<Collection> {
        let name = name.trim();
        let key = collection_key(name)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if collection_id_by_key(&tx, &key)?.is_some() {
            return Err(LibraryError::CollectionNameExists(name.to_string()));
        }
        let position: i64 = tx.query_row(
            "SELECT COALESCE(MAX(position), -1) + 1 FROM collections",
            [],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO collections (name, position, name_key) VALUES (?1, ?2, ?3)",
            params![name, position, key],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(Collection {
            id,
            name: name.to_string(),
            books: 0,
        })
    }

    /// Renames a collection. Names are unique without regard to case.
    pub fn rename_collection(&mut self, collection_id: i64, name: &str) -> Result<()> {
        let name = name.trim();
        let key = collection_key(name)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM collections WHERE id = ?1)",
            [collection_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(LibraryError::CollectionNotFound(collection_id));
        }
        if collection_id_by_key(&tx, &key)?.is_some_and(|id| id != collection_id) {
            return Err(LibraryError::CollectionNameExists(name.to_string()));
        }
        tx.execute(
            "UPDATE collections SET name = ?2, name_key = ?3 WHERE id = ?1",
            params![collection_id, name, key],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Deletes a collection and its book memberships. Returns false if absent.
    pub fn delete_collection(&mut self, collection_id: i64) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM collections WHERE id = ?1", [collection_id])?
            > 0)
    }

    /// Moves a collection to `position`, where zero is the first position.
    pub fn reorder_collection(&mut self, collection_id: i64, position: usize) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut ids = {
            let mut stmt = tx.prepare("SELECT id FROM collections ORDER BY position, id")?;
            let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let Some(index) = ids.iter().position(|id| *id == collection_id) else {
            return Err(LibraryError::CollectionNotFound(collection_id));
        };
        let id = ids.remove(index);
        ids.insert(position.min(ids.len()), id);
        for (index, id) in ids.into_iter().enumerate() {
            tx.execute(
                "UPDATE collections SET position = ?2 WHERE id = ?1",
                params![id, index as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Points the book with `book_id` at `to`, a file that already holds the
    /// same book. The id stays, so the history, reading progress, status, cover
    /// and collections stay with the book. The file at `to` must match the
    /// stored row: the same size, partial MD5 and format. Nothing changes when a
    /// check fails.
    ///
    /// A row at `to` that a scan has marked missing is removed with its history
    /// and collections, because the file now at `to` replaces it. A row at `to`
    /// that is still live is an error. The book file is not touched, so the
    /// caller writes `to` before it calls this.
    pub fn move_book(&mut self, book_id: i64, to: &Path) -> Result<Book> {
        let real = fs::canonicalize(to)?;
        let (meta, format) = book_file(&real)?;
        let md5 = kaia_id::partial_md5(&real)?;
        let path = real
            .to_str()
            .ok_or_else(|| LibraryError::InvalidArgument("the path is not UTF-8".into()))?;

        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let stored: (i64, String, String) = tx
            .query_row(
                "SELECT size, partial_md5, format FROM books WHERE id = ?1",
                [book_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?
            .ok_or(LibraryError::NotFound(book_id))?;
        if stored != (meta.len() as i64, md5, format) {
            return Err(LibraryError::InvalidArgument(format!(
                "the file at {} does not match book {book_id} in the index",
                real.display()
            )));
        }
        tx.execute(
            "DELETE FROM books WHERE path = ?1 AND id != ?2 AND missing_since IS NOT NULL",
            params![path, book_id],
        )?;
        let taken: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM books WHERE path = ?1 AND id != ?2)",
            params![path, book_id],
            |r| r.get(0),
        )?;
        if taken {
            return Err(LibraryError::InvalidArgument(format!(
                "another book is indexed at {}",
                real.display()
            )));
        }
        tx.execute(
            "UPDATE books SET path = ?2, size = ?3, mtime = ?4, missing_since = NULL WHERE id = ?1",
            params![book_id, path, meta.len() as i64, mtime_secs(&meta)],
        )?;
        let book = tx.query_row(
            &format!("SELECT {} FROM books WHERE id = ?1", cols("")),
            [book_id],
            book_from_row,
        )?;
        tx.commit()?;
        Ok(book)
    }

    /// Deletes a book row and its indexed covers, collections and history.
    /// Returns false when the id was not present. The book file is untouched.
    pub fn remove_book(&mut self, book_id: i64) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM books WHERE id = ?1", [book_id])? > 0)
    }

    /// Removes a book from the reading history. Deletes its history rows and
    /// clears its last-opened time. Progress and status do not change, because
    /// the book's own state holds them. The book file is untouched.
    pub fn remove_from_history(&mut self, book_id: i64) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_book(&tx, book_id)?;
        tx.execute("DELETE FROM history WHERE book_id = ?1", [book_id])?;
        tx.execute("UPDATE books SET last_opened_at = NULL WHERE id = ?1", [book_id])?;
        tx.commit()?;
        Ok(())
    }

    /// Removes every book from the reading history. Deletes all history rows and
    /// clears the last-opened time of every book. Progress and status do not
    /// change. Book files are untouched.
    pub fn clear_history(&mut self) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("DELETE FROM history", [])?;
        tx.execute("UPDATE books SET last_opened_at = NULL", [])?;
        tx.commit()?;
        Ok(())
    }

    /// Adds a book to a collection and creates the collection if needed. The
    /// book goes last in the manual order. Adding it twice changes nothing.
    pub fn add_to_collection(&mut self, book_id: i64, name: &str) -> Result<()> {
        let name = name.trim();
        let key = collection_key(name)?;
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_book(&tx, book_id)?;
        let cid = match collection_id_by_key(&tx, &key)? {
            Some(id) => id,
            None => {
                let position: i64 = tx.query_row(
                    "SELECT COALESCE(MAX(position), -1) + 1 FROM collections",
                    [],
                    |r| r.get(0),
                )?;
                tx.execute(
                    "INSERT INTO collections (name, position, name_key) VALUES (?1, ?2, ?3)",
                    params![name, position, key],
                )?;
                tx.last_insert_rowid()
            }
        };
        tx.execute(
            "INSERT OR IGNORE INTO collection_books (collection_id, book_id, position)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1 FROM collection_books WHERE collection_id = ?1))",
            params![cid, book_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Removes a book from a collection. Returns false when it was not there. The
    /// collection itself stays, even when it becomes empty.
    pub fn remove_from_collection(&mut self, book_id: i64, name: &str) -> Result<bool> {
        let key = collection_key(name.trim())?;
        let n = self.conn.execute(
            "DELETE FROM collection_books WHERE book_id = ?1
             AND collection_id = (SELECT id FROM collections WHERE name_key = ?2)",
            params![book_id, key],
        )?;
        Ok(n > 0)
    }

    /// Records that a book was opened at `percent` (0.0 to 1.0). Adds a history
    /// entry, sets the last-opened time and the progress, and moves a `new` book
    /// to `reading`. Values outside 0 to 1 are clamped.
    pub fn record_open(&mut self, book_id: i64, percent: f64) -> Result<()> {
        if !percent.is_finite() {
            return Err(LibraryError::InvalidArgument(
                "percent is not a finite number".into(),
            ));
        }
        let percent = percent.clamp(0.0, 1.0);
        let now = now_unix();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_book(&tx, book_id)?;
        tx.execute(
            "INSERT INTO history (book_id, opened_at) VALUES (?1, ?2)",
            params![book_id, now],
        )?;
        tx.execute(
            "UPDATE books SET last_opened_at = ?2, percent = ?3,
               status = CASE WHEN status = 'new' THEN 'reading' ELSE status END
             WHERE id = ?1",
            params![book_id, now, percent],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Sets the reading status of a book. A book set to `New` also leaves the
    /// reading history, as [`Library::set_status_many`] describes.
    pub fn set_status(&mut self, book_id: i64, status: Status) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if set_status_in(&tx, book_id, status)? == 0 {
            return Err(LibraryError::NotFound(book_id));
        }
        tx.commit()?;
        Ok(())
    }

    /// Sets the reading status of many books in one transaction and returns how
    /// many books changed. An id with no book is skipped, so a selection that a
    /// scan has since changed still applies to the other books.
    ///
    /// A book set to `New` (unread) also leaves the reading history and loses its
    /// last-opened time, so it no longer shows under Recent or Continue. Its
    /// progress stays, because the reading position is kept.
    pub fn set_status_many(&mut self, book_ids: &[i64], status: Status) -> Result<usize> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut changed = 0;
        for &id in book_ids {
            changed += set_status_in(&tx, id, status)?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// The JPEG cover of a book at `size`. None when the book has no cover, when
    /// extraction failed, or when the stored MIME type is unknown.
    pub fn cover(&self, book_id: i64, size: CoverSize) -> Result<Option<CoverImage>> {
        let col = match size {
            CoverSize::Small => "small",
            CoverSize::Large => "large",
        };
        let sql = format!("SELECT {col}, mime FROM covers WHERE book_id = ?1");
        let row: Option<(Option<Vec<u8>>, String)> = self
            .conn
            .query_row(&sql, [book_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()?;
        Ok(row.and_then(|(bytes, mime)| {
            let bytes = bytes?;
            let mime = match mime.as_str() {
                "image/jpeg" => "image/jpeg",
                "image/png" => "image/png",
                _ => return None,
            };
            Some(CoverImage { bytes, mime })
        }))
    }

    fn query_books<P: rusqlite::Params>(&self, sql: &str, params: P) -> Result<Vec<Book>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params, book_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

/// `path` resolved through the file system. Keeps the path unchanged when it
/// does not exist.
pub(crate) fn canonical_or(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The half-open range of stored paths under `folder`, including subfolders.
/// `"dir/"` is the lower bound and `"dir0"` the upper one, since `0` follows `/`
/// in byte order.
fn folder_range(folder: &Path) -> Result<(String, String)> {
    let real = canonical_or(folder);
    let s = real
        .to_str()
        .ok_or_else(|| LibraryError::InvalidArgument("folder path is not UTF-8".into()))?;
    let s = s.trim_end_matches('/');
    Ok((format!("{s}/"), format!("{s}0")))
}

/// Turns user input into an FTS5 expression. Each run of letters or digits
/// becomes a quoted prefix term, so the input cannot form FTS syntax. Combining
/// marks count as part of a word, as they do for the unicode61 tokenizer.
pub(crate) fn fts_query(input: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split(|c: char| !is_word_char(c))
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || ('\u{0300}'..='\u{036F}').contains(&c)
}

/// Sets the status of one book on `conn`, which is normally a transaction.
/// Returns the rows changed: 0 when no book has the id. See
/// [`Library::set_status_many`] for what `New` clears.
fn set_status_in(conn: &Connection, book_id: i64, status: Status) -> Result<usize> {
    let n = conn.execute(
        "UPDATE books SET status = ?2 WHERE id = ?1",
        params![book_id, status.as_str()],
    )?;
    if n > 0 && status == Status::New {
        conn.execute("DELETE FROM history WHERE book_id = ?1", [book_id])?;
        conn.execute("UPDATE books SET last_opened_at = NULL WHERE id = ?1", [book_id])?;
    }
    Ok(n)
}

/// Fails with `NotFound` when no book has this id.
fn ensure_book(conn: &Connection, book_id: i64) -> Result<()> {
    let found: Option<i64> = conn
        .query_row("SELECT 1 FROM books WHERE id = ?1", [book_id], |r| r.get(0))
        .optional()?;
    match found {
        Some(_) => Ok(()),
        None => Err(LibraryError::NotFound(book_id)),
    }
}

pub(crate) fn collection_key(name: &str) -> Result<String> {
    if name.is_empty() {
        return Err(LibraryError::InvalidArgument("collection name is empty".into()));
    }
    Ok(name.to_lowercase())
}

pub(crate) fn collection_id_by_key(conn: &Connection, key: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT id FROM collections WHERE name_key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_quotes_each_word_as_a_prefix() {
        assert_eq!(fts_query("dun ta").as_deref(), Some("\"dun\"* \"ta\"*"));
        assert_eq!(
            fts_query("  O'Brien-Smith ").as_deref(),
            Some("\"O\"* \"Brien\"* \"Smith\"*")
        );
        assert_eq!(fts_query("Ünïcödé").as_deref(), Some("\"Ünïcödé\"*"));
        assert_eq!(fts_query("\" OR * ").as_deref(), Some("\"OR\"*"));
        assert_eq!(fts_query(" -- ** "), None);
        assert_eq!(fts_query(""), None);
    }

    #[test]
    fn folder_range_matches_only_the_folder_and_below() {
        // The folder does not exist, so the path is used as written.
        let (lo, hi) = folder_range(Path::new("/books/sci")).unwrap();
        assert_eq!((lo.as_str(), hi.as_str()), ("/books/sci/", "/books/sci0"));
        let inside = |p: &str| p >= lo.as_str() && p < hi.as_str();
        assert!(inside("/books/sci/a.epub"));
        assert!(inside("/books/sci/deep/b.epub"));
        // A sibling whose name extends the folder name is outside the range.
        assert!(!inside("/books/sci0/a.epub"));
        assert!(!inside("/books/sci-fi/a.epub"));
    }
}
