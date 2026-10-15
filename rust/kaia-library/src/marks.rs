//! Favorites and tags.
//!
//! A favorite is a membership of the collection named `favorites`. KOReader keeps
//! its favorites under that name, and the import reads that collection into the
//! library, so a book marked in either app is in the same store. A tag is free
//! text that a book carries. The `tags` and `book_tags` tables hold tags, from
//! schema version 8.

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use crate::error::{LibraryError, Result};
use crate::query::ensure_book;
use crate::types::Tag;
use crate::Library;

/// The name of the favorites collection, as Kaia creates it. KOReader's name is
/// `favorites`. Names match without regard to case, so both are one collection.
pub const FAVORITES: &str = "Favorites";

/// The key of the favorites collection: its name in lower case.
pub(crate) const FAVORITES_KEY: &str = "favorites";

/// The longest tag, in characters after its spaces are trimmed.
pub const MAX_TAG_CHARS: usize = 40;

/// The most tags one book carries.
pub const MAX_TAGS_PER_BOOK: usize = 20;

/// The text with each run of spaces as one space and no spaces at the ends.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The tag text as a user types it, checked against the limits.
fn checked_name(text: &str) -> Result<String> {
    let name = collapse(text);
    if name.is_empty() {
        return Err(LibraryError::TagEmpty);
    }
    if name.chars().count() > MAX_TAG_CHARS {
        return Err(LibraryError::TagTooLong(MAX_TAG_CHARS));
    }
    Ok(name)
}

/// The key a tag is matched by: its collapsed text in lower case. None for blank
/// text, which does not filter.
pub(crate) fn tag_filter_key(text: &str) -> Option<String> {
    let key = collapse(text).to_lowercase();
    (!key.is_empty()).then_some(key)
}

/// True when `collection_id` is the favorites collection.
pub(crate) fn is_favorites_collection(conn: &Connection, collection_id: i64) -> Result<bool> {
    let key: Option<String> = conn
        .query_row(
            "SELECT name_key FROM collections WHERE id = ?1",
            [collection_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(key.as_deref() == Some(FAVORITES_KEY))
}

fn tag_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Tag> {
    Ok(Tag {
        id: r.get(0)?,
        name: r.get(1)?,
        books: r.get::<_, i64>(2)? as u64,
    })
}

impl Library {
    /// True when the book is a favorite.
    pub fn is_favorite(&self, book_id: i64) -> Result<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM collection_books cb
               JOIN collections c ON c.id = cb.collection_id
               WHERE c.name_key = ?1 AND cb.book_id = ?2)",
            params![FAVORITES_KEY, book_id],
            |r| r.get(0),
        )?)
    }

    /// Marks or unmarks a book as a favorite. Marking creates the favorites
    /// collection when it is missing, and a second mark changes nothing. Fails
    /// with `NotFound` when no book has the id.
    pub fn set_favorite(&mut self, book_id: i64, favorite: bool) -> Result<()> {
        if favorite {
            return self.add_to_collection(book_id, FAVORITES);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_book(&tx, book_id)?;
        tx.execute(
            "DELETE FROM collection_books WHERE book_id = ?1
             AND collection_id IN (SELECT id FROM collections WHERE name_key = ?2)",
            params![book_id, FAVORITES_KEY],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The tags of a book, by name. Each tag shows the number of books that carry it.
    pub fn book_tags(&self, book_id: i64) -> Result<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, (SELECT COUNT(*) FROM book_tags x
               JOIN books b ON b.id = x.book_id AND b.missing_since IS NULL
               WHERE x.tag_id = t.id)
             FROM tags t JOIN book_tags bt ON bt.tag_id = t.id
             WHERE bt.book_id = ?1 ORDER BY t.name_key",
        )?;
        let rows = stmt.query_map([book_id], tag_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Every tag that a book in the library carries, by name, with the number of
    /// books that carry it. A tag whose books are all missing is left out.
    pub fn tags(&self) -> Result<Vec<Tag>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.name, COUNT(b.id)
             FROM tags t
             JOIN book_tags bt ON bt.tag_id = t.id
             JOIN books b ON b.id = bt.book_id AND b.missing_since IS NULL
             GROUP BY t.id ORDER BY t.name_key",
        )?;
        let rows = stmt.query_map([], tag_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Gives a book a tag and returns the tag. The text is trimmed, and each run
    /// of spaces becomes one space. A tag matches another without regard to case,
    /// so "Sci-Fi" and "sci-fi" are one tag. The spelling stored first is the one
    /// shown. Adding a tag the book already has changes nothing.
    ///
    /// Fails with `TagEmpty` for blank text, `TagTooLong` for more than
    /// [`MAX_TAG_CHARS`] characters, `TooManyTags` when the book already has
    /// [`MAX_TAGS_PER_BOOK`] tags and this one is new, and `NotFound` for an
    /// unknown book. A failed call changes nothing.
    pub fn add_tag(&mut self, book_id: i64, text: &str) -> Result<Tag> {
        let name = checked_name(text)?;
        let key = name.to_lowercase();
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_book(&tx, book_id)?;
        let existing: Option<(i64, String)> = tx
            .query_row("SELECT id, name FROM tags WHERE name_key = ?1", [&key], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .optional()?;
        let (tag_id, shown) = match existing {
            Some(found) => found,
            None => {
                tx.execute(
                    "INSERT INTO tags (name, name_key) VALUES (?1, ?2)",
                    params![name, key],
                )?;
                (tx.last_insert_rowid(), name)
            }
        };
        let linked: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM book_tags WHERE book_id = ?1 AND tag_id = ?2)",
            params![book_id, tag_id],
            |r| r.get(0),
        )?;
        if !linked {
            let count: i64 = tx.query_row(
                "SELECT COUNT(*) FROM book_tags WHERE book_id = ?1",
                [book_id],
                |r| r.get(0),
            )?;
            if count as usize >= MAX_TAGS_PER_BOOK {
                return Err(LibraryError::TooManyTags(MAX_TAGS_PER_BOOK));
            }
            tx.execute(
                "INSERT INTO book_tags (book_id, tag_id) VALUES (?1, ?2)",
                params![book_id, tag_id],
            )?;
        }
        let books: i64 = tx.query_row(
            "SELECT COUNT(*) FROM book_tags x
               JOIN books b ON b.id = x.book_id AND b.missing_since IS NULL
               WHERE x.tag_id = ?1",
            [tag_id],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(Tag {
            id: tag_id,
            name: shown,
            books: books as u64,
        })
    }

    /// Takes a tag off a book. Returns false when the book did not have it. A tag
    /// that no book carries after the removal is deleted as well.
    pub fn remove_tag(&mut self, book_id: i64, tag_id: i64) -> Result<bool> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let n = tx.execute(
            "DELETE FROM book_tags WHERE book_id = ?1 AND tag_id = ?2",
            params![book_id, tag_id],
        )?;
        tx.execute(
            "DELETE FROM tags WHERE id = ?1
             AND NOT EXISTS (SELECT 1 FROM book_tags WHERE tag_id = ?1)",
            [tag_id],
        )?;
        tx.commit()?;
        Ok(n > 0)
    }
}
