//! The word list: words saved from dictionary lookups, with the book they came
//! from and the sentence around them. Schema version 3. See `STORAGE.md`.

use rusqlite::{params, OptionalExtension};

use crate::error::StoreError;
use crate::store::Store;

/// Longest word kept, in characters. A longer selection is a phrase, not a word.
pub const WORD_MAX_CHARS: usize = 100;
/// Longest book title kept. A longer title is cut.
pub const BOOK_TITLE_MAX_CHARS: usize = 300;
/// Longest context kept. A longer context is cut.
pub const CONTEXT_MAX_CHARS: usize = 1000;

/// One saved word.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Word {
    pub id: i64,
    /// The word as it was first saved. Matching ignores case.
    pub word: String,
    /// The title of the book the word was last saved from.
    pub book_title: Option<String>,
    /// The sentence around the word, when the reader had it.
    pub context: Option<String>,
    /// Unix seconds when the word was last saved.
    pub created_at: i64,
    /// Kobo dictionary suffix, when the source supplied one.
    pub dict_suffix: Option<String>,
}

impl Store {
    /// Imports a saved word with its context and suffix. The first spelling wins.
    pub fn import_word(&mut self, word: &Word) -> Result<bool, StoreError> {
        if word.word.trim().is_empty() || word.word.chars().count() > WORD_MAX_CHARS {
            return Ok(false);
        }
        Ok(self.conn().execute(
            "INSERT OR IGNORE INTO words (word, book_title, context, created_at, dict_suffix) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![word.word, clipped(word.book_title.as_deref(), BOOK_TITLE_MAX_CHARS), clipped(word.context.as_deref(), CONTEXT_MAX_CHARS), word.created_at, clipped(word.dict_suffix.as_deref(), 100)],
        )? == 1)
    }

    /// Imports a Kobo word only when its case-insensitive spelling is new.
    pub fn import_nickel_word(
        &mut self,
        word: &str,
        book_title: Option<&str>,
        suffix: Option<&str>,
        created_at: i64,
    ) -> Result<bool, StoreError> {
        let word = word.trim();
        if word.is_empty() || word.chars().count() > WORD_MAX_CHARS {
            return Ok(false);
        }
        Ok(self.conn().execute(
            "INSERT OR IGNORE INTO words (word, book_title, created_at, dict_suffix) VALUES (?1, ?2, ?3, ?4)",
            params![
                word,
                clipped(book_title, BOOK_TITLE_MAX_CHARS),
                created_at,
                clipped(suffix, 100)
            ],
        )? == 1)
    }

    /// Saves a word, or saves it again. A word already in the list keeps its id
    /// and takes the new book, context and time. Returns the id.
    ///
    /// Fails with `InvalidArg` for an empty word or one over `WORD_MAX_CHARS`.
    pub fn add_word(
        &mut self,
        word: &str,
        book_title: Option<&str>,
        context: Option<&str>,
        created_at: i64,
    ) -> Result<i64, StoreError> {
        let word = word.trim();
        if word.is_empty() {
            return Err(StoreError::InvalidArg("the word is empty".into()));
        }
        if word.chars().count() > WORD_MAX_CHARS {
            return Err(StoreError::InvalidArg(format!(
                "the word has more than {WORD_MAX_CHARS} characters"
            )));
        }
        let book_title = clipped(book_title, BOOK_TITLE_MAX_CHARS);
        let context = clipped(context, CONTEXT_MAX_CHARS);
        // One statement is one transaction, so the row is written or not at all.
        let id = self.conn().query_row(
            "INSERT INTO words (word, book_title, context, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(word) DO UPDATE SET
               book_title = excluded.book_title,
               context = excluded.context,
               created_at = excluded.created_at
             RETURNING id",
            params![word, book_title, context, created_at],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    /// Every saved word, newest first.
    pub fn list_words(&self) -> Result<Vec<Word>, StoreError> {
        let mut stmt = self.conn().prepare(
            "SELECT id, word, book_title, context, created_at, dict_suffix FROM words
             ORDER BY created_at DESC, id DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Word {
                id: row.get(0)?,
                word: row.get(1)?,
                book_title: row.get(2)?,
                context: row.get(3)?,
                created_at: row.get(4)?,
                dict_suffix: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StoreError::from)
    }

    /// One saved word by id, or None.
    pub fn get_word(&self, id: i64) -> Result<Option<Word>, StoreError> {
        self.conn()
            .query_row(
                "SELECT id, word, book_title, context, created_at, dict_suffix FROM words WHERE id = ?1",
                params![id],
                |row| {
                    Ok(Word {
                        id: row.get(0)?,
                        word: row.get(1)?,
                        book_title: row.get(2)?,
                        context: row.get(3)?,
                        created_at: row.get(4)?,
                        dict_suffix: row.get(5)?,
                    })
                },
            )
            .optional()
            .map_err(StoreError::from)
    }

    /// Removes a saved word. Fails with `NotFound` when no word has the id.
    pub fn delete_word(&mut self, id: i64) -> Result<(), StoreError> {
        let removed = self
            .conn()
            .execute("DELETE FROM words WHERE id = ?1", params![id])?;
        if removed == 0 {
            return Err(StoreError::NotFound(format!("word {id}")));
        }
        Ok(())
    }
}

/// The text cut to `max` characters, with empty text read as no text.
fn clipped(text: Option<&str>, max: usize) -> Option<String> {
    let text = text?.trim();
    if text.is_empty() {
        return None;
    }
    Some(text.chars().take(max).collect())
}
