//! The chapter caption is centred by default. Optional title and device status
//! retain the two-group layout chosen in reading settings.

use kaia_book::{PageInfo, Size};
use kaia_events::Settings;
use kaia_i18n::{format as tr_format, tr};
use kaia_ui::Theme;

use super::caption::{Caption, ChapterProgress};
use super::footer::{Footer, Status};
use crate::surfaces::settings::keys;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    Title,
    Chapter,
    Clock,
    Battery,
}

impl Item {
    pub const ALL: [Self; 4] = [Self::Title, Self::Chapter, Self::Clock, Self::Battery];

    pub fn id(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Chapter => "chapter",
            Self::Clock => "clock",
            Self::Battery => "battery",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.id() == id)
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Title => keys::READING_HEADER_TITLE,
            Self::Chapter => keys::READING_HEADER_CHAPTER,
            Self::Clock => keys::READING_HEADER_CLOCK,
            Self::Battery => keys::READING_HEADER_BATTERY,
        }
    }

    pub fn label(self) -> String {
        match self {
            Self::Title => tr("Book title"),
            Self::Chapter => tr("Chapter title"),
            Self::Clock => tr("Clock"),
            Self::Battery => tr("Battery"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    on: [bool; 4],
}

impl Default for Header {
    fn default() -> Self {
        Self {
            on: [false, true, false, false],
        }
    }
}

impl Header {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            on: Item::ALL.map(|item| keys::bool_or(settings, item.key(), item == Item::Chapter)),
        }
    }

    pub fn is_on(&self, item: Item) -> bool {
        self.on[item as usize]
    }

    pub fn set(&mut self, item: Item, on: bool) {
        self.on[item as usize] = on;
    }

    pub fn height(&self, theme: &Theme) -> u32 {
        if self.on.iter().any(|on| *on) {
            super::footer::strip_height(theme)
        } else {
            0
        }
    }

    /// Reserve both strips before the book lays out or fits a fixed page.
    pub fn viewport(&self, footer: &Footer, screen: Size, theme: &Theme) -> Size {
        Size::new(
            screen.w,
            screen
                .h
                .saturating_sub(self.height(theme) + footer.height(theme))
                .max(1),
        )
    }

    pub(super) fn chapter_only(&self) -> bool {
        self.on == [false, true, false, false]
    }

    pub(super) fn chapter_caption(info: &PageInfo, progress: Option<ChapterProgress>) -> Caption {
        Caption::new(
            info.chapter.clone().unwrap_or_default(),
            progress.map(ChapterProgress::text).unwrap_or_default(),
        )
    }

    /// `PageInfo::chapter` is the session's TOC entry for the current page.
    pub(super) fn text(
        &self,
        title: &str,
        info: &PageInfo,
        status: &Status,
        progress: Option<ChapterProgress>,
    ) -> (String, String) {
        let left = if self.is_on(Item::Title) { title } else { "" }.to_string();
        let mut right = Vec::new();
        if self.is_on(Item::Chapter) && info.chapter.as_ref().is_some_and(|s| !s.is_empty()) {
            right.push(Self::chapter_caption(info, progress).text());
        }
        if self.is_on(Item::Clock) && !status.clock.is_empty() {
            right.push(status.clock.clone());
        }
        if self.is_on(Item::Battery) {
            if let Some(level) = status.battery {
                right.push(tr_format(&tr("Battery %1%"), &[&level.to_string()]));
            }
        }
        (left, right.join(" | "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_defaults_on_and_items_join_only_available_text() {
        let mut header = Header::default();
        let mut info = PageInfo {
            page: 0,
            page_count: 10,
            percent: 0.1,
            chapter: Some("First chapter".into()),
            pages_left_in_chapter: Some(2),
            location: kaia_book::Location::Page(0),
        };
        let status = Status {
            clock: "09:41".into(),
            battery: Some(64),
        };
        assert_eq!(
            header.text("Book", &info, &status, None),
            (String::new(), "First chapter".into())
        );
        for item in Item::ALL {
            assert_eq!(Item::from_id(item.id()), Some(item));
            header.set(item, true);
        }
        assert_eq!(
            header.text("Book", &info, &status, None),
            ("Book".into(), "First chapter | 09:41 | Battery 64%".into())
        );
        info.chapter = None;
        assert_eq!(
            header.text("Book", &info, &Status::default(), None),
            ("Book".into(), String::new())
        );
        assert_eq!(Item::from_id("unknown"), None);
    }
}
