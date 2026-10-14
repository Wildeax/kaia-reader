//! Storage groups and deletion of indexed files.
//!
//! The index has no source column. Calibre paths come from its metadata cache
//! in the configured download folder. Those paths take precedence over the
//! OPDS folder rule. Other files in that folder count as OPDS, and files outside
//! it count as Other. This describes current locations, not download history.
//! Manually copied books in the download folder also count as OPDS. Moving a
//! book or changing that folder can change its group.

use std::path::{Path, PathBuf};

use kaia_platform::storage::BookFile;
use rusqlite::{params, TransactionBehavior};

use crate::{Library, LibraryError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Calibre,
    Opds,
    Other,
}

impl Source {
    pub const ALL: [Self; 3] = [Self::Calibre, Self::Opds, Self::Other];

    pub fn key(self) -> &'static str {
        match self {
            Self::Calibre => "calibre",
            Self::Opds => "opds",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sources {
    pub calibre_books: Vec<PathBuf>,
    pub downloads: PathBuf,
}

impl Sources {
    pub fn source(&self, path: &Path) -> Source {
        if self.calibre_books.iter().any(|book| book == path) {
            Source::Calibre
        } else if path.starts_with(&self.downloads) {
            Source::Opds
        } else {
            Source::Other
        }
    }
}

/// The exact index entry offered for deletion. A changed row must be confirmed again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageBook {
    pub id: i64,
    pub path: PathBuf,
    pub bytes: u64,
    pub partial_md5: String,
}

#[derive(Debug, Clone)]
pub struct StorageGroup {
    pub source: Source,
    pub books: Vec<StorageBook>,
    pub bytes: u64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct DeleteReport {
    pub deleted: usize,
    pub failed: usize,
    pub skipped_open: usize,
}

impl Library {
    /// Counts indexed sizes, excluding rows the scanner marked missing.
    /// The returned book list also fixes the scope of a later confirmation.
    pub fn storage_groups(&self, sources: &Sources) -> Result<Vec<StorageGroup>> {
        let mut groups: Vec<_> = Source::ALL
            .into_iter()
            .map(|source| StorageGroup {
                source,
                books: Vec::new(),
                bytes: 0,
            })
            .collect();
        let mut stmt = self.conn.prepare(
            "SELECT id, path, size, partial_md5 FROM books WHERE missing_since IS NULL ORDER BY id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(StorageBook {
                id: row.get(0)?,
                path: PathBuf::from(row.get::<_, String>(1)?),
                bytes: row.get::<_, i64>(2)?.max(0) as u64,
                partial_md5: row.get(3)?,
            })
        })?;
        for row in rows {
            let book = row?;
            let group = groups
                .iter_mut()
                .find(|g| g.source == sources.source(&book.path))
                .unwrap();
            group.bytes = group.bytes.saturating_add(book.bytes);
            group.books.push(book);
        }
        Ok(groups)
    }

    /// Deletes only the confirmed rows that still belong to this source.
    /// The caller checks its live reader sessions immediately before each file.
    pub fn delete_storage_group(
        &mut self,
        group: &StorageGroup,
        sources: &Sources,
        roots: &[PathBuf],
        mut is_open: impl FnMut(&StorageBook) -> Result<bool>,
    ) -> DeleteReport {
        let mut report = DeleteReport::default();
        for book in &group.books {
            match is_open(book) {
                Ok(true) => report.skipped_open += 1,
                Err(_) => report.failed += 1,
                Ok(false) => {
                    if sources.source(&book.path) != group.source
                        || self.delete_indexed_file(book, roots).is_err()
                    {
                        report.failed += 1;
                    } else {
                        report.deleted += 1;
                    }
                }
            }
        }
        report
    }

    /// Shared by delete-one and delete-all. Only a regular indexed file beneath
    /// an allowed root can be removed. Hidden paths and symlinks are refused.
    /// Reading state and annotations live outside this database and stay intact.
    pub fn delete_indexed_file(&mut self, expected: &StorageBook, roots: &[PathBuf]) -> Result<()> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let same: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM books WHERE id = ?1 AND path = ?2 AND size = ?3 AND partial_md5 = ?4 AND missing_since IS NULL)",
            params![expected.id, expected.path.to_string_lossy(), expected.bytes as i64, expected.partial_md5],
            |row| row.get(0),
        )?;
        if !same {
            return Err(LibraryError::InvalidArgument(
                "book changed after confirmation".into(),
            ));
        }
        let mut file = roots
            .iter()
            .find_map(|root| BookFile::open(root, &expected.path).ok())
            .ok_or_else(|| {
                LibraryError::InvalidArgument(
                    "book is outside the allowed folders or has an unsafe path".into(),
                )
            })?;
        if file.file.metadata()?.len() != expected.bytes
            || kaia_id::partial_md5_file(&mut file.file)? != expected.partial_md5
        {
            return Err(LibraryError::InvalidArgument(
                "book file changed after indexing".into(),
            ));
        }
        // Hold the database write lock across removal so scans cannot change the row.
        tx.execute("DELETE FROM books WHERE id = ?1", [expected.id])?;
        file.remove()?;
        tx.commit()?;
        Ok(())
    }
}
