//! Reading captions keep progress visible when a title needs an ellipsis.

use kaia_book::PageInfo;
use kaia_doc::TocEntry;
use kaia_gfx::text::{FontSystem, TextLayout};
use kaia_gfx::{Point, Rect};
use kaia_i18n::{format as tr_format, tr};
use kaia_ui::widget::PaintCx;
use kaia_ui::Theme;

/// Screen-based chapter progress until the document engine supplies logical pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ChapterProgress {
    pub page: u32,
    pub total: u32,
}

impl ChapterProgress {
    pub fn from_toc(info: &PageInfo, toc: &[TocEntry]) -> Option<Self> {
        if info.chapter.as_ref().is_none_or(|title| title.trim().is_empty())
            || info.page as usize >= info.page_count
        {
            return None;
        }
        let start = toc
            .iter()
            .filter_map(|entry| entry.page)
            .filter(|&p| p <= info.page)
            .max()?;
        let end = toc
            .iter()
            .filter_map(|entry| entry.page)
            .filter(|&p| p > info.page)
            .min()
            .unwrap_or(info.page_count as u32)
            .min(info.page_count as u32);
        Some(Self {
            page: info.page - start + 1,
            total: end - start,
        })
    }

    pub fn text(self) -> String {
        tr_format(
            &tr("%1 of %2"),
            &[&self.page.to_string(), &self.total.to_string()],
        )
    }
}

#[derive(Default)]
pub(super) struct Caption {
    pub title: String,
    pub status: String,
}

impl Caption {
    pub fn new(title: impl Into<String>, status: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            status: status.into(),
        }
    }

    pub fn text(&self) -> String {
        match (self.title.is_empty(), self.status.is_empty()) {
            (true, _) => self.status.clone(),
            (_, true) => self.title.clone(),
            _ => format!("{} · {}", self.title, self.status),
        }
    }

    fn layout(&self, fonts: &mut FontSystem, theme: &Theme, width: u32) -> (TextLayout, TextLayout) {
        let style = super::footer::strip_style(theme);
        let available = width.saturating_sub(2 * theme.space.lg);
        let suffix = if self.title.is_empty() || self.status.is_empty() {
            self.status.clone()
        } else {
            format!(" · {}", self.status)
        };
        let status = fonts.layout(&suffix, &style, available, Some(1), true);
        let title_width = available.saturating_sub(status.width);
        let title = if title_width < style.size_px.ceil() as u32 {
            ""
        } else {
            self.title.as_str()
        };
        let title = fonts.layout(title, &style, title_width, Some(1), true);
        (title, status)
    }

    pub fn paint(&self, cx: &mut PaintCx<'_>, rect: Rect) {
        let (title, status) = self.layout(cx.fonts, cx.theme, rect.w);
        let x = rect.x + rect.w.saturating_sub(title.width + status.width) as i32 / 2;
        cx.clipped(rect, |cx| {
            cx.text(
                &title,
                Point::new(x, rect.y + rect.h.saturating_sub(title.height) as i32 / 2),
            );
            cx.text(
                &status,
                Point::new(
                    x + title.width as i32,
                    rect.y + rect.h.saturating_sub(status.height) as i32 / 2,
                ),
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(page: u32, chapter: Option<&str>) -> PageInfo {
        PageInfo {
            page,
            page_count: 20,
            percent: 0.0,
            chapter: chapter.map(str::to_owned),
            pages_left_in_chapter: None,
            location: kaia_book::Location::Page(page),
        }
    }

    fn toc() -> Vec<TocEntry> {
        [(Some(2), 1), (Some(2), 2), (Some(7), 2), (None, 1), (Some(15), 1)]
            .into_iter()
            .map(|(page, level)| TocEntry {
                page,
                level,
                title: "Section".into(),
                xpointer: None,
            })
            .collect()
    }

    #[test]
    fn chapter_counts_use_distinct_boundaries_and_include_the_current_screen() {
        for (page, current, total) in [
            (2, 1, 5),
            (6, 5, 5),
            (7, 1, 8),
            (14, 8, 8),
            (15, 1, 5),
            (19, 5, 5),
        ] {
            assert_eq!(
                ChapterProgress::from_toc(&info(page, Some("Section")), &toc()),
                Some(ChapterProgress { page: current, total })
            );
        }
    }

    #[test]
    fn missing_chapters_and_invalid_pages_have_no_chapter_count() {
        assert_eq!(ChapterProgress::from_toc(&info(0, None), &toc()), None);
        assert_eq!(ChapterProgress::from_toc(&info(3, Some(" ")), &toc()), None);
        assert_eq!(ChapterProgress::from_toc(&info(0, Some("Section")), &[]), None);
        assert_eq!(
            ChapterProgress::from_toc(&info(20, Some("Section")), &toc()),
            None
        );
    }

    #[test]
    fn long_titles_yield_space_to_progress_at_portrait_and_landscape_widths() {
        let mut fonts = FontSystem::load(
            &[concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/fonts/noto")],
            &["NotoSans-Regular"],
        )
        .unwrap();
        for (width, height) in [(1264, 1680), (1680, 1264), (600, 800)] {
            for text_scale in [1.0, 2.0] {
                let mut theme = Theme::new(
                    kaia_ui::theme::Scale {
                        width,
                        height,
                        dpi: Some(300),
                    },
                    false,
                );
                theme.text_scale = text_scale;
                let caption = Caption::new(
                    "A long chapter title that repeats across the entire reading caption. ".repeat(8),
                    "1090 of 1090",
                );
                let (title, status) = caption.layout(&mut fonts, &theme, width);
                assert!(title.truncated);
                assert!(!status.truncated, "the page count must remain complete");
                assert!(title.width + status.width <= width - 2 * theme.space.lg);
                assert_eq!(title.lines.len(), 1);
                assert_eq!(status.lines.len(), 1);
            }
        }
    }

    #[test]
    fn captions_join_only_available_fields() {
        assert_eq!(Caption::new("About", "1 of 2").text(), "About · 1 of 2");
        assert_eq!(Caption::new("", "1 of 2").text(), "1 of 2");
        assert_eq!(Caption::new("About", "").text(), "About");
    }
}
