//! The reading statistics of one book: the time read, the pages read, the days
//! read, the first and last read dates, the average time per page and the time left.
//! The figures come from the statistics database (`statistics.sqlite3`), keyed as
//! KOReader keys its book rows. The page is a sheet that the book details open.

use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_library::Book;
use kaia_stats::{BookStats, Stats};
use kaia_ui::components::{Column, Divider, Expanded, Label, Padding, Row};
use kaia_ui::controls::ButtonStyle;
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, Tone, WidgetPod};

use crate::services::Services;
use crate::surfaces::common::display_title;
use crate::surfaces::library::views;
use crate::surfaces::settings::facts;

/// One row of the page: a key for tests, the label and the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Figure {
    pub key: &'static str,
    pub label: String,
    pub value: String,
}

/// The statistics of `book` at Unix time `now`. None when the statistics database
/// does not exist or cannot be read, or when it has no row for the book.
///
/// The row is found by the same key the reader writes: the sidecar's partial MD5,
/// the display title and the authors, with `N/A` when the book has none.
pub fn load(services: &Services, book: &Book) -> Option<BookStats> {
    let path = facts::stats_path(&services.paths.data_dir);
    if !path.is_file() {
        return None;
    }
    let mut stats = Stats::open(&path).ok()?;
    let title = display_title(book);
    let authors = book.authors.as_deref().unwrap_or("N/A");
    stats.book(&book.partial_md5, &title, authors).ok().flatten()
}

/// Whether the book has any reading: a page with time, or time on a page.
pub fn has_reading(stats: &BookStats) -> bool {
    stats.totals.distinct_pages > 0 || stats.totals.total_seconds > 0
}

/// The rows of the page for `stats` at Unix time `now`, in order.
///
/// Time read counts each page at most the longest stay that counts, as KOReader's
/// "Time spent reading" does. The time left is the pages after the last page read,
/// at the average time per page.
pub fn figures(stats: &BookStats, now: i64) -> Vec<Figure> {
    let t = &stats.totals;
    let row = |key: &'static str, label: String, value: String| Figure { key, label, value };

    let pages = if t.pages > 0 {
        let percent = (t.distinct_pages as f64 * 100.0 / t.pages as f64).round() as i64;
        tr_format(
            &tr("%1 of %2 (%3%)"),
            &[
                &t.distinct_pages.to_string(),
                &t.pages.to_string(),
                &percent.to_string(),
            ],
        )
    } else {
        t.distinct_pages.to_string()
    };
    let days = tr_format(
        &tr_n("%1 day", "%1 days", t.days_read),
        &[&t.days_read.to_string()],
    );
    let first = t.first_open.map_or_else(na, date);
    let last = if t.last_open > 0 { date(t.last_open) } else { na() };
    let average = t.average_page_seconds.map_or_else(na, per_page);
    let left = time_left(stats, now);

    vec![
        row("book-stats-time", tr("Time read"), clock(t.read_seconds as f64)),
        row("book-stats-pages", tr("Pages read"), pages),
        row("book-stats-days", tr("Days read"), days),
        row("book-stats-first", tr("First read"), first),
        row("book-stats-last", tr("Last read"), last),
        row("book-stats-average", tr("Average time per page"), average),
        row("book-stats-left", tr("Estimated time left"), left),
    ]
}

/// The time left in the book, from the pages after the last page read.
fn time_left(stats: &BookStats, now: i64) -> String {
    let t = &stats.totals;
    if t.pages <= 0 {
        return na();
    }
    let pages_left = (t.pages - t.last_page.unwrap_or(0)).max(0);
    if pages_left == 0 {
        return tr("Last page of the book");
    }
    clock(stats.estimate.time_left(pages_left, now).seconds)
}

/// A duration as the footer writes time left: whole minutes under an hour, and
/// hours with one decimal from an hour, such as `1.5 h`.
pub fn clock(seconds: f64) -> String {
    let minutes = (seconds.max(0.0) / 60.0).round() as u64;
    if minutes < 60 {
        tr_format(&tr("%1 min"), &[&minutes.to_string()])
    } else {
        tr_format(&tr("%1 h"), &[&format!("{:.1}", minutes as f64 / 60.0)])
    }
}

/// Seconds per page. Under a minute the value is seconds, since whole minutes
/// would show `0 min`.
fn per_page(seconds: f64) -> String {
    if seconds < 60.0 {
        tr_format(&tr("%1 s"), &[&(seconds.max(0.0).round() as u64).to_string()])
    } else {
        clock(seconds)
    }
}

/// A Unix time as the local calendar date, `YYYY-MM-DD`.
fn date(ts: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_opt(ts, 0).single() {
        Some(when) => when.format("%Y-%m-%d").to_string(),
        None => na(),
    }
}

fn na() -> String {
    tr("N/A")
}

/// One figure as a row: the label on the left, the value on the right, and a
/// hairline under it. The rows are shorter than the Settings list rows, so the
/// seven figures and the Back button fit the sheet.
fn figure_row(t: &Theme, figure: Figure) -> WidgetPod {
    let line = Row::new(vec![
        WidgetPod::new(Expanded::new(
            WidgetPod::new(Label::new(figure.label, t.font.body.clone(), Tone::Ink)),
            1,
        )),
        WidgetPod::new(Label::new(figure.value, t.font.body.clone(), Tone::InkSecondary)),
    ])
    .gap(t.space.sm)
    .shrink();
    let body = WidgetPod::new(Padding::new(
        WidgetPod::new(line),
        Insets::symmetric(0, t.space.xs),
    ));
    WidgetPod::new(Column::new(vec![body, WidgetPod::new(Divider::new(Tone::Divider))]).shrink())
        .with_key(figure.key)
}

/// The statistics page: the book title, then the figures, or the empty notice when
/// the book has no reading. `back` is the action that returns to the details.
pub fn page(t: &Theme, title: &str, stats: Option<&BookStats>, now: i64, back: &str) -> WidgetPod {
    let mut children = vec![WidgetPod::new(Label::new(
        title.to_string(),
        t.font.book_title.clone(),
        Tone::Ink,
    ))];
    match stats.filter(|s| has_reading(s)) {
        Some(stats) => {
            // One child for the list, so the sheet's gap sits once above it.
            let rows = figures(stats, now)
                .into_iter()
                .map(|figure| figure_row(t, figure))
                .collect();
            children.push(WidgetPod::new(Column::new(rows).shrink()));
        }
        None => {
            let notice = views::body_text(t, &tr("No reading recorded yet."));
            children.push(notice.with_key("book-stats-empty"));
        }
    }
    children.push(views::button(ButtonStyle::Secondary, &tr("Back"), back).with_key("book-stats-back"));
    views::sheet(t, &tr("Reading statistics"), children)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_stats::{BookEstimate, BookTotals};

    /// The totals of the fixture in `kaia-stats/tests/book_stats.rs`.
    fn fixture() -> BookStats {
        BookStats {
            totals: BookTotals {
                id: 1,
                title: "Dune".into(),
                authors: "Frank Herbert".into(),
                pages: 10,
                last_open: 1_715_000_000,
                highlights: Some(0),
                notes: Some(0),
                total_seconds: 290,
                read_seconds: 250,
                distinct_pages: 4,
                days_read: 2,
                first_open: Some(1_715_000_000 - 86_400),
                last_page: Some(4),
                average_page_seconds: Some(62.5),
            },
            estimate: BookEstimate {
                read_pages: 4,
                read_seconds: 250,
                days_read: 2,
                avg_page_seconds: 62.5,
                seconds_per_day: Some(125.0),
            },
        }
    }

    fn value(figures: &[Figure], key: &str) -> String {
        figures
            .iter()
            .find(|f| f.key == key)
            .unwrap_or_else(|| panic!("no row {key}"))
            .value
            .clone()
    }

    #[test]
    fn durations_show_minutes_under_an_hour_and_hours_with_one_decimal() {
        assert_eq!(clock(0.0), "0 min");
        assert_eq!(clock(250.0), "4 min");
        assert_eq!(clock(59.0 * 60.0), "59 min");
        assert_eq!(clock(90.0 * 60.0), "1.5 h");
        assert_eq!(clock(3900.0), "1.1 h");
    }

    #[test]
    fn a_page_under_a_minute_shows_seconds() {
        assert_eq!(per_page(62.5), "1 min");
        assert_eq!(per_page(45.4), "45 s");
        assert_eq!(per_page(120.0), "2 min");
    }

    #[test]
    fn the_figures_of_the_fixture() {
        let stats = fixture();
        let now = stats.totals.last_open;
        let figures = figures(&stats, now);
        let keys: Vec<&str> = figures.iter().map(|f| f.key).collect();
        assert_eq!(
            keys,
            [
                "book-stats-time",
                "book-stats-pages",
                "book-stats-days",
                "book-stats-first",
                "book-stats-last",
                "book-stats-average",
                "book-stats-left",
            ]
        );
        assert_eq!(value(&figures, "book-stats-time"), "4 min");
        assert_eq!(value(&figures, "book-stats-pages"), "4 of 10 (40%)");
        assert_eq!(value(&figures, "book-stats-days"), "2 days");
        assert_eq!(value(&figures, "book-stats-average"), "1 min");
        // Pages 5 to 10 are left: six pages at 62.5 s.
        assert_eq!(value(&figures, "book-stats-left"), "6 min");
        assert_eq!(value(&figures, "book-stats-first").len(), 10);
        assert_eq!(value(&figures, "book-stats-last").len(), 10);
    }

    #[test]
    fn the_time_left_is_the_last_page_when_the_book_is_read_to_the_end() {
        let mut stats = fixture();
        stats.totals.last_page = Some(10);
        assert_eq!(
            value(&figures(&stats, 0), "book-stats-left"),
            "Last page of the book"
        );
    }

    #[test]
    fn a_book_without_a_page_count_has_no_time_left() {
        let mut stats = fixture();
        stats.totals.pages = 0;
        assert_eq!(value(&figures(&stats, 0), "book-stats-left"), "N/A");
    }

    #[test]
    fn a_book_with_no_time_is_not_reading() {
        let mut stats = fixture();
        assert!(has_reading(&stats));
        stats.totals.distinct_pages = 0;
        stats.totals.total_seconds = 0;
        assert!(!has_reading(&stats));
    }
}
