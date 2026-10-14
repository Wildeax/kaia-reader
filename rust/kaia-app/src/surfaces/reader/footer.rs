//! The Footer sheet turns items on or off. The page and the overlay show those
//! items in one line. Each choice is a settings key, so it stays across books.
//!
//! The time left follows Nickel's format: whole minutes under an hour, and hours
//! with one decimal from an hour. The pace comes from the reading statistics of the
//! book. With no statistics, it is one minute per remaining page.

use kaia_book::PageInfo;
use kaia_events::Settings;
use kaia_i18n::{format as tr_format, tr};

use crate::surfaces::settings::keys;

/// One footer item, in the order the sheet lists them and the line shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item {
    /// `Page 3 of 200`.
    Page,
    /// The reading percent, such as `42%`.
    Percent,
    /// The time left in the chapter, or in the book before the first chapter.
    TimeChapter,
    /// The time left in the book.
    TimeBook,
    /// The local time as `HH:MM`.
    Clock,
    /// The battery level.
    Battery,
}

impl Item {
    /// Every item, in the order of the sheet and the line. `Footer` indexes by this order.
    pub const ALL: [Item; 6] = [
        Item::Page,
        Item::Percent,
        Item::TimeChapter,
        Item::TimeBook,
        Item::Clock,
        Item::Battery,
    ];

    /// The name in reader action ids, such as `page` in `reader:footer:page`.
    pub fn id(self) -> &'static str {
        match self {
            Item::Page => "page",
            Item::Percent => "percent",
            Item::TimeChapter => "time_chapter",
            Item::TimeBook => "time_book",
            Item::Clock => "clock",
            Item::Battery => "battery",
        }
    }

    /// The item with the action id `id`.
    pub fn from_id(id: &str) -> Option<Item> {
        Item::ALL.into_iter().find(|item| item.id() == id)
    }

    /// The settings key that stores whether the item is on.
    pub fn key(self) -> &'static str {
        match self {
            Item::Page => keys::READING_FOOTER_PAGE,
            Item::Percent => keys::READING_FOOTER_PERCENT,
            Item::TimeChapter => keys::READING_FOOTER_TIME_CHAPTER,
            Item::TimeBook => keys::READING_FOOTER_TIME_BOOK,
            Item::Clock => keys::READING_FOOTER_CLOCK,
            Item::Battery => keys::READING_FOOTER_BATTERY,
        }
    }

    /// Whether the item is on before the reader saves a choice. Nickel defaults
    /// to page counts, as recorded in docs/kaia/nickel/PARITY.md.
    pub fn default_on(self) -> bool {
        self == Item::Page
    }

    /// The name of the item on the Footer sheet.
    pub fn label(self) -> String {
        match self {
            Item::Page => tr("Page number"),
            Item::Percent => tr("Percent"),
            Item::TimeChapter => tr("Time left in chapter"),
            Item::TimeBook => tr("Time left in book"),
            Item::Clock => tr("Clock"),
            Item::Battery => tr("Battery"),
        }
    }
}

/// Which footer items are on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Footer {
    /// One flag per item, in the order of [`Item::ALL`].
    on: [bool; 6],
}

impl Default for Footer {
    fn default() -> Self {
        Self {
            on: Item::ALL.map(Item::default_on),
        }
    }
}

impl Footer {
    /// The choices that `settings` holds. A missing key takes the item's default.
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            on: Item::ALL.map(|item| keys::bool_or(settings, item.key(), item.default_on())),
        }
    }

    pub fn is_visible(&self) -> bool {
        self.on.iter().any(|on| *on)
    }

    /// One caption line plus `space.sm`, with the space split above and below.
    /// At the Libra Colour theme size this is 40 + 16 = 56 pixels.
    pub fn height(&self, theme: &kaia_ui::Theme) -> u32 {
        if self.is_visible() {
            strip_height(theme)
        } else {
            0
        }
    }

    pub fn is_on(&self, item: Item) -> bool {
        self.on[index(item)]
    }

    pub fn set(&mut self, item: Item, on: bool) {
        self.on[index(item)] = on;
    }
}

/// One caption line and the space above and below it, shared by both strips.
pub(super) fn strip_height(theme: &kaia_ui::Theme) -> u32 {
    let style = theme.style(&theme.font.caption, kaia_ui::Tone::InkSecondary);
    style.line_height.ceil() as u32 + theme.space.sm
}

fn index(item: Item) -> usize {
    Item::ALL
        .iter()
        .position(|&i| i == item)
        .expect("every item is in Item::ALL")
}

/// What the footer reads from the device when the page redraws or its bars open.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// The local time as `HH:MM`. Empty before the first read.
    pub clock: String,
    /// The battery level in percent, when the device reports one.
    pub battery: Option<u8>,
}

impl Status {
    /// The clock now, with the battery level that the device reports.
    pub fn now(battery: Option<u8>) -> Self {
        Self {
            clock: chrono::Local::now().format("%H:%M").to_string(),
            battery,
        }
    }
}

/// The footer line for a page: the items that are on, in order, with a middle dot
/// between them. Empty when no item is on, or when the book has no pages and the
/// line needs the page.
pub fn line(footer: &Footer, info: &PageInfo, seconds_per_page: f64, status: &Status) -> String {
    joined_line(footer, info, seconds_per_page, status, " · ")
}

/// The page uses KOReader's default item separator from `readerfooter.lua`.
pub fn page_line(footer: &Footer, info: &PageInfo, seconds_per_page: f64, status: &Status) -> String {
    joined_line(footer, info, seconds_per_page, status, " | ")
}

fn joined_line(
    footer: &Footer,
    info: &PageInfo,
    seconds_per_page: f64,
    status: &Status,
    separator: &str,
) -> String {
    let in_chapter = info.chapter.is_some() && info.pages_left_in_chapter.is_some();
    let parts: Vec<String> = Item::ALL
        .into_iter()
        .filter(|&item| footer.is_on(item))
        // Before the first chapter, the chapter item already shows the time left in
        // the book. The book item would repeat it.
        .filter(|&item| !(item == Item::TimeBook && footer.is_on(Item::TimeChapter) && !in_chapter))
        .filter_map(|item| text(item, info, seconds_per_page, status))
        .collect();
    parts.join(separator)
}

/// The text of one item, or None when the item has nothing to show.
fn text(item: Item, info: &PageInfo, seconds_per_page: f64, status: &Status) -> Option<String> {
    match item {
        Item::Clock => (!status.clock.is_empty()).then(|| status.clock.clone()),
        Item::Battery => status
            .battery
            .map(|level| tr_format(&tr("Battery %1%"), &[&level.to_string()])),
        _ if info.page_count == 0 => None,
        Item::Page => Some(tr_format(
            &tr("Page %1 of %2"),
            &[&(info.page + 1).to_string(), &info.page_count.to_string()],
        )),
        Item::Percent => {
            let percent = (info.percent * 100.0).round() as u32;
            Some(tr_format(&tr("%1%"), &[&percent.to_string()]))
        }
        Item::TimeChapter => Some(match (info.chapter.is_some(), info.pages_left_in_chapter) {
            (true, Some(pages)) => left(u64::from(pages), seconds_per_page, Place::Chapter),
            _ => time_book(info, seconds_per_page),
        }),
        Item::TimeBook => Some(time_book(info, seconds_per_page)),
    }
}

fn time_book(info: &PageInfo, seconds_per_page: f64) -> String {
    let pages = info.page_count.saturating_sub(info.page as usize + 1) as u64;
    left(pages, seconds_per_page, Place::Book)
}

/// Where the time is left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Chapter,
    Book,
}

/// The time to read `pages` pages at `seconds_per_page`, such as `12 min left in chapter`.
fn left(pages: u64, seconds_per_page: f64, place: Place) -> String {
    if pages == 0 {
        return match place {
            Place::Chapter => tr("Last page of the chapter"),
            Place::Book => tr("Last page of the book"),
        };
    }
    let minutes = (pages as f64 * seconds_per_page / 60.0).round().max(1.0) as u64;
    match (place, minutes < 60) {
        (Place::Chapter, true) => tr_format(&tr("%1 min left in chapter"), &[&minutes.to_string()]),
        (Place::Chapter, false) => tr_format(&tr("%1 h left in chapter"), &[&hours(minutes)]),
        (Place::Book, true) => tr_format(&tr("%1 min left in book"), &[&minutes.to_string()]),
        (Place::Book, false) => tr_format(&tr("%1 h left in book"), &[&hours(minutes)]),
    }
}

/// Hours with one decimal, such as `1.5` for 90 minutes.
fn hours(minutes: u64) -> String {
    format!("{:.1}", minutes as f64 / 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_book::Location;

    fn info(page: u32, count: usize) -> PageInfo {
        PageInfo {
            page,
            page_count: count,
            percent: f64::from(page + 1) / count as f64,
            chapter: None,
            pages_left_in_chapter: None,
            location: Location::Page(page),
        }
    }

    fn in_chapter(page: u32, count: usize, left: u32) -> PageInfo {
        PageInfo {
            chapter: Some("One".into()),
            pages_left_in_chapter: Some(left),
            ..info(page, count)
        }
    }

    #[test]
    fn minutes_under_an_hour_and_one_decimal_of_hours_from_an_hour() {
        assert_eq!(left(12, 60.0, Place::Chapter), "12 min left in chapter");
        assert_eq!(left(59, 60.0, Place::Chapter), "59 min left in chapter");
        assert_eq!(left(60, 60.0, Place::Chapter), "1.0 h left in chapter");
        assert_eq!(left(90, 60.0, Place::Book), "1.5 h left in book");
        assert_eq!(left(125, 60.0, Place::Book), "2.1 h left in book");
    }

    #[test]
    fn the_last_page_says_so_and_a_page_is_at_least_a_minute() {
        assert_eq!(left(0, 60.0, Place::Chapter), "Last page of the chapter");
        assert_eq!(left(0, 60.0, Place::Book), "Last page of the book");
        assert_eq!(left(1, 5.0, Place::Book), "1 min left in book");
    }

    #[test]
    fn the_pace_turns_pages_into_minutes() {
        // With no statistics the pace is one minute per page, so 40 pages are 40 minutes.
        assert_eq!(left(40, 60.0, Place::Book), "40 min left in book");
        // A faster reader with statistics at 30 seconds a page reads the same 40 pages in 20.
        assert_eq!(left(40, 30.0, Place::Book), "20 min left in book");
    }

    #[test]
    fn the_default_footer_shows_the_page_count() {
        let footer = Footer::default();
        let line_text = line(&footer, &in_chapter(2, 100, 10), 60.0, &Status::default());
        assert_eq!(line_text, "Page 3 of 100");
    }

    #[test]
    fn items_join_in_order_with_a_middle_dot() {
        let mut footer = Footer::default();
        footer.set(Item::Page, true);
        footer.set(Item::Percent, true);
        footer.set(Item::TimeChapter, true);
        footer.set(Item::Battery, true);
        let status = Status {
            clock: "09:41".into(),
            battery: Some(64),
        };
        let line_text = line(&footer, &in_chapter(2, 100, 10), 60.0, &status);
        assert_eq!(
            line_text,
            "Page 3 of 100 · 3% · 10 min left in chapter · Battery 64%"
        );
    }

    #[test]
    fn the_clock_shows_only_when_it_has_a_time() {
        let mut footer = Footer::default();
        footer.set(Item::Page, false);
        footer.set(Item::Clock, true);
        // The clock is empty until the reader reads it, so the line is empty too.
        assert_eq!(line(&footer, &info(0, 10), 60.0, &Status::default()), "");
        let status = Status {
            clock: "09:41".into(),
            battery: None,
        };
        assert_eq!(line(&footer, &info(0, 10), 60.0, &status), "09:41");
    }

    #[test]
    fn time_book_is_not_repeated_before_the_first_chapter() {
        let mut footer = Footer::default();
        footer.set(Item::TimeBook, true);
        footer.set(Item::Page, false);
        footer.set(Item::TimeChapter, true);
        // Before the first chapter the chapter item already shows the book time.
        assert_eq!(
            line(&footer, &info(0, 11), 60.0, &Status::default()),
            "10 min left in book"
        );
        // In a chapter the book item adds its own time.
        assert_eq!(
            line(&footer, &in_chapter(2, 100, 10), 60.0, &Status::default()),
            "10 min left in chapter · 1.6 h left in book"
        );
    }

    #[test]
    fn items_and_keys_follow_the_sheet_order() {
        assert_eq!(
            Item::ALL.map(Item::id),
            ["page", "percent", "time_chapter", "time_book", "clock", "battery"]
        );
        for (i, item) in Item::ALL.iter().enumerate() {
            assert_eq!(index(*item), i);
            assert_eq!(Item::from_id(item.id()), Some(*item));
        }
        assert_eq!(Item::from_id("nope"), None);
        assert_eq!(Item::Page.key(), "reading.footer_page");
        assert_eq!(Item::Battery.key(), "reading.footer_battery");
    }

    #[test]
    fn a_book_without_pages_shows_no_page_items() {
        let mut footer = Footer::default();
        footer.set(Item::Page, true);
        assert_eq!(line(&footer, &info(0, 0), 60.0, &Status::default()), "");
    }
}
