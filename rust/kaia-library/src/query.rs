//! Reads and small writes: book lists, recent books, search, collections,
//! reading progress, status and covers.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet};
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

/// Books that one suggestion query reads at most.
const SUGGEST_CANDIDATES: usize = 200;

/// A suggestion for the search field: a title, an author name or a series name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    /// The text as the book shows it, with each run of spaces as one space.
    pub text: String,
    /// `"title"`, `"author"` or `"series"`.
    pub kind: &'static str,
}

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

    /// Suggestions for the search field. Each one is a title, an author name or a
    /// series name, as the books show it. A text matches when it starts with the
    /// query, or when each word of the query starts a word of the text. Matching
    /// ignores ASCII case. Only the title, authors and series count, so a word
    /// that appears only in the description suggests nothing.
    ///
    /// Prefix matches come first, then the other matches. Within each group the
    /// text of the book opened most recently comes first, then the text in
    /// alphabetical order. Each text appears once for each kind. At most `limit`
    /// come back. A query with no letters or digits returns nothing.
    ///
    /// The index matches accents, but this check does not: "ecl" finds
    /// "Éclair" in [`Library::search`], and no suggestion shows it.
    ///
    /// One query reads at most 200 books, the best of them by prefix match and
    /// then by recency. Each book is a row of the full-text index, so the cost
    /// grows with the books that match the words, not with the library.
    pub fn suggestions(&self, query: &str, limit: usize) -> Result<Vec<Suggestion>> {
        let words = suggestion_words(query);
        if words.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let phrase = normalize(query);
        let expr = words
            .iter()
            .map(|w| format!("{{title authors series}} : \"{w}\"*"))
            .collect::<Vec<_>>()
            .join(" ");
        let sql = format!(
            "SELECT {} FROM book_fts JOIN books ON books.id = book_fts.rowid
             WHERE book_fts MATCH ?1 AND books.missing_since IS NULL
             ORDER BY (substr(lower(COALESCE(books.title, '')), 1, length(?2)) = ?2
                       OR substr(lower(COALESCE(books.authors, '')), 1, length(?2)) = ?2
                       OR substr(lower(COALESCE(books.series, '')), 1, length(?2)) = ?2) DESC,
                      books.last_opened_at DESC, books.id
             LIMIT ?3",
            cols("books.")
        );
        let books = self.query_books(&sql, params![expr, phrase, SUGGEST_CANDIDATES as i64])?;
        Ok(rank_suggestions(&books, &phrase, &words, limit))
    }

    /// The books whose title is `title`, ignoring ASCII case, and without the
    /// missing ones. The title must equal the stored one apart from case. At most
    /// two come back, so a caller can tell one book from several.
    pub fn books_titled(&self, title: &str) -> Result<Vec<Book>> {
        let sql = format!(
            "SELECT {} FROM books WHERE title = ?1 COLLATE NOCASE AND missing_since IS NULL
             ORDER BY id LIMIT 2",
            cols("")
        );
        self.query_books(&sql, [title])
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

    /// Splits an author field into names. The engine joins the authors of an EPUB
    /// with a line feed, so the field is split on line feeds only. A name is
    /// trimmed, and an empty one is dropped. A comma stays inside a name, so
    /// "Shakespeare, William" is one author. A PDF field that joins names with
    /// "and" or a semicolon is one author too.
    pub fn split_authors(field: &str) -> Vec<&str> {
        field
            .split('\n')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect()
    }

    /// The authors of an author field on one line, with the names joined by ", ".
    /// Show a book's authors with this, not with the stored field, which keeps
    /// the line feeds. An empty field gives an empty string.
    pub fn display_authors(field: &str) -> String {
        Self::split_authors(field).join(", ")
    }

    /// The authors of `books` and the number of books each one has, in the order
    /// the Authors view shows them. A book with two authors counts once for each,
    /// so the counts can add up to more than the number of books. Names match
    /// without regard to case and to runs of spaces. Each author has the smallest
    /// spelling among its variants, so the result does not depend on the order of
    /// `books`.
    ///
    /// Authors sort by name, ignoring case and a leading "The ", so "The Bard"
    /// sorts under B. A name sorts as written: the surname does not move to the
    /// front. Names that sort the same come in case-insensitive order.
    ///
    /// The caller passes the books it shows, so the counts follow its rules for
    /// hidden books. The result is a list of (name, books).
    pub fn author_index(books: &[Book]) -> Vec<(String, usize)> {
        let mut map: BTreeMap<String, (String, usize)> = BTreeMap::new();
        for book in books {
            let Some(field) = book.authors.as_deref() else {
                continue;
            };
            let mut seen = BTreeSet::new();
            for name in Self::split_authors(field) {
                let key = author_key(name);
                if !seen.insert(key.clone()) {
                    continue;
                }
                let entry = map.entry(key).or_insert_with(|| (name.to_string(), 0));
                if name < entry.0.as_str() {
                    entry.0 = name.to_string();
                }
                entry.1 += 1;
            }
        }
        let mut authors: Vec<(String, usize)> = map.into_values().collect();
        authors.sort_by_cached_key(|(name, _)| (sort_name(name), author_key(name)));
        authors
    }

    /// The books of `books` by `name`, in the order the Authors view shows them:
    /// by series, then by number in the series, then by title. A book with no
    /// series comes after the books with one, as in [`SortKey::Series`]. A book
    /// with two authors belongs to each of them. The name matches as
    /// [`Library::author_index`] matches names.
    pub fn books_by_author(books: &[Book], name: &str) -> Vec<Book> {
        let key = author_key(name);
        let mut found: Vec<Book> = books
            .iter()
            .filter(|book| {
                book.authors.as_deref().is_some_and(|field| {
                    Self::split_authors(field)
                        .into_iter()
                        .any(|author| author_key(author) == key)
                })
            })
            .cloned()
            .collect();
        found.sort_by(series_order);
        found
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

/// How well a text matches a suggestion query. A prefix match sorts first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Match {
    /// The text starts with the query.
    Prefix,
    /// Each word of the query starts a word of the text.
    Words,
}

/// A candidate suggestion. The fields are in sort order: match, recency,
/// alphabetical key, kind, then the text as shown.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Ranked {
    tier: Match,
    /// The last time the book was opened, newest first. A book never opened
    /// sorts after every opened book.
    opened: Reverse<Option<i64>>,
    /// The text ignoring ASCII case. Equal keys are one suggestion per kind.
    key: String,
    kind: &'static str,
    text: String,
}

/// The suggestions for `phrase` and `words` among `books`, best first, at most
/// `limit`. `phrase` and `words` come from [`normalize`] and [`suggestion_words`].
fn rank_suggestions(books: &[Book], phrase: &str, words: &[String], limit: usize) -> Vec<Suggestion> {
    let mut ranked = Vec::new();
    for book in books {
        let mut fields: Vec<(&'static str, &str)> = Vec::new();
        if let Some(title) = book.title.as_deref() {
            fields.push(("title", title));
        }
        if let Some(authors) = book.authors.as_deref() {
            fields.extend(
                Library::split_authors(authors)
                    .into_iter()
                    .map(|name| ("author", name)),
            );
        }
        if let Some(series) = book.series.as_deref() {
            fields.push(("series", series));
        }
        for (kind, field) in fields {
            let Some(tier) = match_tier(field, phrase, words) else {
                continue;
            };
            ranked.push(Ranked {
                tier,
                opened: Reverse(book.last_opened_at),
                key: normalize(field),
                kind,
                text: collapse(field),
            });
        }
    }
    ranked.sort();
    let mut seen = BTreeSet::new();
    ranked
        .into_iter()
        .filter(|r| seen.insert((r.kind, r.key.clone())))
        .take(limit)
        .map(|r| Suggestion {
            text: r.text,
            kind: r.kind,
        })
        .collect()
}

/// How `field` matches the query: a prefix of it, or every word of the query at
/// the start of a word in it. None when neither holds.
fn match_tier(field: &str, phrase: &str, words: &[String]) -> Option<Match> {
    let text = normalize(field);
    if text.starts_with(phrase) {
        return Some(Match::Prefix);
    }
    let text_words: Vec<&str> = text
        .split(|c: char| !is_word_char(c))
        .filter(|w| !w.is_empty())
        .collect();
    words
        .iter()
        .all(|w| text_words.iter().any(|t| t.starts_with(w.as_str())))
        .then_some(Match::Words)
}

/// The words of a suggestion query in lower case. Each run of letters or digits
/// is one word.
fn suggestion_words(input: &str) -> Vec<String> {
    normalize(input)
        .split(|c: char| !is_word_char(c))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// The text with each run of white space as one space, and the ends trimmed.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// [`collapse`] with ASCII letters in lower case. Other letters keep their case.
fn normalize(text: &str) -> String {
    collapse(text).to_ascii_lowercase()
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

/// The key that matches author names and series names: lower case, with each run
/// of spaces as one space.
fn author_key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The key that sorts an author: [`author_key`] without a leading "the ".
fn sort_name(name: &str) -> String {
    let key = author_key(name);
    match key.strip_prefix("the ") {
        Some(rest) if !rest.is_empty() => rest.to_string(),
        _ => key,
    }
}

/// The key that sorts a title. The title is the file path when it has none.
fn title_key(book: &Book) -> String {
    match book.title.as_deref() {
        Some(title) => author_key(title),
        None => author_key(&book.path.to_string_lossy()),
    }
}

/// Series, then number in the series, then title, then id. A book with no series
/// comes last, and so does a book with no number in its series.
fn series_order(a: &Book, b: &Book) -> Ordering {
    let series = |book: &Book| (book.series.is_none(), book.series.as_deref().map(author_key));
    series(a)
        .cmp(&series(b))
        .then_with(|| a.series_index.is_none().cmp(&b.series_index.is_none()))
        .then_with(|| {
            a.series_index
                .unwrap_or(0.0)
                .total_cmp(&b.series_index.unwrap_or(0.0))
        })
        .then_with(|| title_key(a).cmp(&title_key(b)))
        .then_with(|| a.id.cmp(&b.id))
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

    fn book(id: i64, title: &str, authors: Option<&str>, series: Option<(&str, f64)>) -> Book {
        Book {
            id,
            path: PathBuf::from(format!("/books/{id}.epub")),
            size: 0,
            mtime: 0,
            partial_md5: String::new(),
            title: Some(title.to_string()),
            authors: authors.map(str::to_string),
            series: series.map(|(name, _)| name.to_string()),
            series_index: series.map(|(_, index)| index),
            language: None,
            description: None,
            pages: None,
            format: "epub".to_string(),
            added_at: 0,
            last_opened_at: None,
            percent: 0.0,
            status: Status::New,
            extracted: true,
            failure: None,
        }
    }

    #[test]
    fn split_authors_splits_on_line_feeds_and_keeps_commas() {
        assert_eq!(
            Library::split_authors("Ada Lovelace\nCharles Babbage "),
            vec!["Ada Lovelace", "Charles Babbage"]
        );
        assert_eq!(
            Library::split_authors("Shakespeare, William"),
            vec!["Shakespeare, William"]
        );
        assert_eq!(Library::split_authors("Ann and Bo"), vec!["Ann and Bo"]);
        assert!(Library::split_authors(" \n\r\n ").is_empty());
    }

    #[test]
    fn display_authors_joins_names_with_a_comma() {
        assert_eq!(
            Library::display_authors("Ada Lovelace\nCharles Babbage "),
            "Ada Lovelace, Charles Babbage"
        );
        assert_eq!(Library::display_authors("Walt Whitman"), "Walt Whitman");
        assert_eq!(Library::display_authors(" \n "), "");
    }

    #[test]
    fn author_index_counts_a_book_once_per_author() {
        let books = [
            book(1, "One", Some("Ada\nBabbage"), None),
            book(2, "Two", Some("ada"), None),
            book(3, "Three", Some("Ada\nAda"), None),
            book(4, "Four", None, None),
        ];
        assert_eq!(
            Library::author_index(&books),
            vec![("Ada".to_string(), 3), ("Babbage".to_string(), 1)]
        );
    }

    #[test]
    fn author_index_sorts_ignoring_case_and_a_leading_the() {
        let books = [
            book(1, "One", Some("Zola"), None),
            book(2, "Two", Some("the Bard"), None),
            book(3, "Three", Some("Bronte"), None),
            book(4, "Four", Some("austen"), None),
        ];
        let names: Vec<String> = Library::author_index(&books)
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["austen", "the Bard", "Bronte", "Zola"]);
    }

    #[test]
    fn author_index_shows_the_smallest_spelling_in_any_order() {
        let a = [
            book(1, "One", Some("ursula k. le guin"), None),
            book(2, "Two", Some("Ursula K. Le Guin"), None),
        ];
        let b = [a[1].clone(), a[0].clone()];
        assert_eq!(
            Library::author_index(&a),
            vec![("Ursula K. Le Guin".to_string(), 2)]
        );
        assert_eq!(Library::author_index(&a), Library::author_index(&b));
    }

    #[test]
    fn books_by_author_sorts_by_series_then_title_and_matches_each_author() {
        let books = [
            book(1, "Zeta", Some("Ada"), None),
            book(2, "Gamma", Some("Ada\nBabbage"), Some(("Verse", 10.0))),
            book(3, "Beta", Some("Ada"), Some(("Verse", 2.0))),
            book(4, "Delta", Some("Ada"), None),
            book(5, "Alpha", Some("Babbage"), Some(("Verse", 1.0))),
            book(6, "Eta", Some("Ada"), Some(("Verse", 2.0))),
        ];
        let ids: Vec<i64> = Library::books_by_author(&books, "ADA")
            .iter()
            .map(|b| b.id)
            .collect();
        // Verse 2 by title (Beta, Eta), then Verse 10 (Gamma), then the books with
        // no series, by title (Delta, Zeta).
        assert_eq!(ids, [3, 6, 2, 4, 1]);
        let babbage: Vec<i64> = Library::books_by_author(&books, "Babbage")
            .iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(babbage, [5, 2]);
        assert!(Library::books_by_author(&books, "  ").is_empty());
    }
}
