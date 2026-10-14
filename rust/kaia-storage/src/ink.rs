//! Page ink owned by Kaia. Coordinates and widths are in page units.
use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{DocumentId, Store, StoreError};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InkPoint {
    pub x: f32,
    pub y: f32,
    /// Raw device pressure, retained for future pressure-sensitive brushes.
    pub pressure: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InkStroke {
    pub id: i64,
    /// Page number at creation, starting at zero.
    pub page: u32,
    /// Page text position for reflowable books. None for fixed pages.
    pub locator: Option<String>,
    /// Points are offsets from this text position, or fixed page coordinates.
    pub points: Vec<InkPoint>,
    pub width: f32,
    pub color: String,
}

impl Store {
    pub fn add_ink(&mut self, doc: &DocumentId, stroke: &InkStroke) -> Result<i64, StoreError> {
        doc.validate()?;
        let file_size = i64::try_from(doc.file_size)
            .map_err(|_| StoreError::InvalidArg("book size is too large".into()))?;
        if stroke.points.is_empty()
            || stroke.points.len() > 100_000
            || !stroke.width.is_finite()
            || !(0.0..=1_000_000.0).contains(&stroke.width)
            || stroke.width == 0.0
            || !matches!(stroke.color.as_str(), "black" | "blue" | "green" | "red")
            || stroke
                .locator
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 16_384)
            || stroke.points.iter().any(|p| {
                !p.x.is_finite() || !p.y.is_finite() || p.x.abs() > 1_000_000.0 || p.y.abs() > 1_000_000.0
            })
        {
            return Err(StoreError::InvalidArg("invalid ink stroke".into()));
        }
        let points =
            serde_json::to_string(&stroke.points).map_err(|e| StoreError::InvalidArg(e.to_string()))?;
        let tx = self.conn_mut().transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO documents (partial_md5, file_size, title) VALUES (?1, ?2, ?3)",
            params![doc.partial_md5, file_size, doc.title],
        )?;
        tx.execute("INSERT INTO ink_strokes (doc, page, locator, points_json, width, color) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![doc.partial_md5, stroke.page, stroke.locator, points, stroke.width, stroke.color])?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(id)
    }

    pub fn ink(&self, doc: &str) -> Result<Vec<InkStroke>, StoreError> {
        let mut query = self.conn().prepare(
            "SELECT id, page, locator, points_json, width, color FROM ink_strokes WHERE doc = ?1 ORDER BY id",
        )?;
        let rows = query.query_map([doc], |r| {
            let json: String = r.get(3)?;
            let points = serde_json::from_str(&json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
            })?;
            Ok(InkStroke {
                id: r.get(0)?,
                page: r.get(1)?,
                locator: r.get(2)?,
                points,
                width: r.get(4)?,
                color: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Removes a group of whole strokes in one transaction, scoped to their book.
    pub fn delete_ink(&mut self, doc: &str, ids: &[i64]) -> Result<(), StoreError> {
        let tx = self.conn_mut().transaction()?;
        for id in ids {
            tx.execute(
                "DELETE FROM ink_strokes WHERE doc = ?1 AND id = ?2",
                params![doc, id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Counts by the page number recorded when the ink was written.
    pub fn ink_counts(&self) -> Result<Vec<(DocumentId, u32, u64)>, StoreError> {
        let mut query = self.conn().prepare("SELECT d.partial_md5, d.file_size, d.title, i.page, count(*) FROM ink_strokes i JOIN documents d ON d.partial_md5 = i.doc GROUP BY i.doc, i.page ORDER BY i.doc, i.page")?;
        let counts = query
            .query_map([], |r| {
                Ok((
                    DocumentId {
                        partial_md5: r.get(0)?,
                        file_size: r.get::<_, i64>(1)? as u64,
                        title: r.get(2)?,
                    },
                    r.get(3)?,
                    r.get::<_, i64>(4)? as u64,
                ))
            })?
            .collect::<Result<_, _>>()?;
        Ok(counts)
    }
}
