//! Kobo collections merge by name and membership without replacing the index.

use crate::{Filter, Library, Result, Sort, SortKey, Status};

impl Library {
    /// Copies the index into a staging database through SQLite's backup API.
    pub fn stage_nickel(&self, path: &std::path::Path) -> Result<()> {
        let mut target = rusqlite::Connection::open(path)?;
        rusqlite::backup::Backup::new(&self.conn, &mut target)?.run_to_completion(
            64,
            std::time::Duration::ZERO,
            None,
        )?;
        Ok(())
    }

    /// Replaces the index after the app has excluded scans and closed other writers.
    pub fn publish_nickel(&mut self, staged: &std::path::Path, target: &std::path::Path) -> Result<()> {
        let placeholder = rusqlite::Connection::open_in_memory()?;
        drop(std::mem::replace(&mut self.conn, placeholder));
        let moved = std::fs::rename(staged, target);
        self.conn = crate::schema::open(target)?;
        moved?;
        Ok(())
    }

    /// Merges a staged shelf. Returns true when the collection or its members changed.
    pub fn import_nickel_collection(&mut self, name: &str, books: &[i64]) -> Result<bool> {
        let existing = self
            .collections()?
            .into_iter()
            .find(|c| c.name.to_lowercase() == name.trim().to_lowercase());
        let mut changed = existing.is_none();
        let collection = match existing {
            Some(c) => c,
            None => self.create_collection(name)?,
        };
        let members = self.books(
            Sort::new(SortKey::Title, false),
            &Filter {
                collection: Some(collection.id),
                ..Filter::default()
            },
            0,
            usize::MAX,
        )?;
        for id in books {
            if !members.iter().any(|b| b.id == *id) {
                self.add_to_collection(*id, name)?;
                changed = true;
            }
        }
        Ok(changed)
    }

    /// Refreshes derived progress without adding a reading-history visit.
    pub fn import_nickel_progress(&mut self, id: i64, percent: f64, status: Status) -> Result<()> {
        self.conn.execute(
            "UPDATE books SET percent=?2, status=?3 WHERE id=?1",
            rusqlite::params![id, percent, status.as_str()],
        )?;
        Ok(())
    }
}
