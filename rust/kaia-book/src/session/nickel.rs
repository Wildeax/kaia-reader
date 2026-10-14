//! Kobo records written to the same state as reader edits.

use super::*;

impl BookSession {
    /// Imports progress once per Kobo content id without changing the position.
    pub fn import_nickel_progress(&mut self, id: &str, percent: f64, status: &str) -> Result<bool> {
        let key = format!("content:{id}");
        if self.state.nickel_imports.contains(&key) {
            return Ok(false);
        }
        // TODO Q33: import ChapterIDBookmarked, ParagraphBookmarked and
        // BookmarkWordOffset only after their meaning is checked on a device.
        self.state.percent = percent.clamp(0.0, 1.0);
        self.state.status = Some(status.into());
        self.finish_nickel(key)?;
        Ok(true)
    }

    /// Finds the first text match and stores a highlight with KOReader field names.
    /// A missing match remains eligible for a later import.
    pub fn import_nickel_highlight(
        &mut self,
        id: &str,
        text: &str,
        note: Option<&str>,
        datetime: &str,
    ) -> Result<bool> {
        self.ensure_idle()?;
        let key = format!("bookmark:{id}");
        if self.state.nickel_imports.contains(&key) || text.trim().is_empty() {
            return Ok(false);
        }
        let needle = text.to_owned();
        let Some((pos0, pos1)) = self.engine.call(move |w| w.find_text(&needle, 0))? else {
            return Ok(false);
        };
        let id = self.state.assign_id();
        self.state.annotations.push(Annotation {
            id,
            datetime: datetime.into(),
            datetime_updated: None,
            drawer: Some("lighten".into()),
            color: Some("yellow".into()),
            text: Some(text.into()),
            text_edited: None,
            note: clean_note(note),
            note_format: None,
            chapter: None,
            pageno: None,
            pageref: None,
            pboxes: None,
            ext: None,
            page: pos_location(&pos0),
            pos0: Some(pos0),
            pos1: Some(pos1),
            orphaned: false,
        });
        self.finish_nickel(key)?;
        Ok(true)
    }

    /// Imports the reader's rating and review once per content id.
    pub fn import_nickel_rating(
        &mut self,
        id: &str,
        rating: Option<u8>,
        review: Option<&str>,
    ) -> Result<bool> {
        let key = format!("rating:{id}");
        if self.state.nickel_imports.contains(&key) {
            return Ok(false);
        }
        self.state.rating = rating.filter(|r| *r <= 5).or(self.state.rating);
        if let Some(review) = clean_note(review) {
            self.state.summary_note = Some(review);
        }
        self.finish_nickel(key)?;
        Ok(true)
    }

    /// Adds imported session time once, without inventing a reading position.
    pub fn import_nickel_time(&mut self, id: &str, seconds: f64) -> Result<bool> {
        let key = format!("session:{id}");
        if self.state.nickel_imports.contains(&key) {
            return Ok(false);
        }
        if !seconds.is_finite() || seconds < 0.0 {
            return Ok(false);
        }
        self.state.nickel_reading_seconds += seconds;
        self.finish_nickel(key)?;
        Ok(true)
    }

    fn finish_nickel(&mut self, key: String) -> Result<()> {
        self.state.nickel_imports.push(key);
        self.dirty = true;
        self.save()
    }
}
