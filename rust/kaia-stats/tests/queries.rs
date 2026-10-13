//! Query results against hand-computed values.
//!
//! Fixture: now is Wednesday 2024-05-15 12:00 local. Spans are inserted directly
//! (page, local start, seconds). The max limit is the default 120 s.

mod common;

use common::{add_book, add_span, koreader_db, local, Scratch};
use kaia_stats::{DayTotal, Stats, Streaks, Totals, Weekday};

struct Fixture {
    stats: Stats,
    alpha: i64,
    beta: i64,
    gamma: i64,
}

fn fixture(dir: &Scratch) -> Fixture {
    let path = dir.path("stats.sqlite3");
    let conn = koreader_db(&path);
    let alpha = add_book(&conn, "Alpha", Some("X"), "aaaa", 100);
    let beta = add_book(&conn, "Beta", Some("Y"), "bbbb", 50);
    let gamma = add_book(&conn, "Gamma", Some("Z"), "cccc", 10);
    conn.execute_batch(&format!(
        "UPDATE book SET last_open = {} WHERE id = {alpha};
         UPDATE book SET last_open = {} WHERE id = {beta};
         UPDATE book SET last_open = {}, highlights = 2, notes = 1 WHERE id = {gamma};",
        local("2024-05-15 10:00:00"),
        local("2024-05-14 20:00:00"),
        local("2024-01-01 00:00:00"),
    ))
    .unwrap();
    let spans = [
        (alpha, 1, "2024-05-15 09:00:00", 60, 100),
        (alpha, 2, "2024-05-15 09:01:00", 30, 100),
        (alpha, 2, "2024-05-15 10:00:00", 200, 100),
        (beta, 3, "2024-05-14 20:00:00", 50, 50),
        (alpha, 5, "2024-05-13 08:00:00", 40, 100),
        (alpha, 6, "2024-05-10 08:00:00", 80, 100),
        (beta, 1, "2024-04-30 12:00:00", 25, 50),
        (alpha, 4, "2024-05-12 23:59:50", 20, 100),
    ];
    for (book, page, at, seconds, total_pages) in spans {
        add_span(&conn, book, page, local(at), seconds, total_pages);
    }
    drop(conn);
    let stats = Stats::open(&path).unwrap();
    Fixture {
        stats,
        alpha,
        beta,
        gamma,
    }
}

fn day(date: &str, pages: i64, seconds: i64) -> DayTotal {
    DayTotal {
        date: date.to_owned(),
        pages,
        seconds,
    }
}

#[test]
fn today_counts_pages_and_seconds_since_local_midnight() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    // Page 1 (60 s) and page 2 (30 + 200 s) since midnight on 05-15.
    assert_eq!(
        f.stats.today(local("2024-05-15 12:00:00")).unwrap(),
        Totals {
            pages: 2,
            seconds: 290
        }
    );
}

#[test]
fn this_week_starts_on_the_chosen_weekday() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    let now = local("2024-05-15 12:00:00");
    // Monday 05-13: pages 5 (40 s), 3 of Beta (50 s), 1 (60 s) and 2 (230 s).
    assert_eq!(
        f.stats.this_week(now, Weekday::Monday).unwrap(),
        Totals {
            pages: 4,
            seconds: 380
        }
    );
    // Sunday 05-12: the same plus page 4, which starts at 23:59:50 on 05-12.
    assert_eq!(
        f.stats.this_week(now, Weekday::Sunday).unwrap(),
        Totals {
            pages: 5,
            seconds: 400
        }
    );
}

#[test]
fn daily_lists_the_last_days_oldest_first_with_zero_days() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    let days = f.stats.daily(local("2024-05-15 12:00:00"), 7).unwrap();
    assert_eq!(
        days,
        vec![
            day("2024-05-09", 0, 0),
            day("2024-05-10", 1, 80),
            day("2024-05-11", 0, 0),
            day("2024-05-12", 1, 20),
            day("2024-05-13", 1, 40),
            day("2024-05-14", 1, 50),
            day("2024-05-15", 2, 290),
        ]
    );
}

#[test]
fn month_lists_the_days_with_reading() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    assert_eq!(
        f.stats.month("2024-05").unwrap(),
        vec![
            day("2024-05-10", 1, 80),
            day("2024-05-12", 1, 20),
            day("2024-05-13", 1, 40),
            day("2024-05-14", 1, 50),
            day("2024-05-15", 2, 290),
        ]
    );
    assert_eq!(f.stats.month("2024-04").unwrap(), vec![day("2024-04-30", 1, 25)]);
    assert!(f.stats.month("2024-06").unwrap().is_empty());
}

#[test]
fn month_rejects_a_malformed_month() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    for bad in ["2024-5", "2024/05", "2024-13", "2024-00", "abcd-ef"] {
        assert!(f.stats.month(bad).is_err(), "{bad} should be refused");
    }
}

#[test]
fn books_are_listed_most_recent_first_with_their_totals() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    let books = f.stats.books().unwrap();
    let summary: Vec<(&str, i64, i64, i64)> = books
        .iter()
        .map(|b| (b.title.as_str(), b.seconds, b.distinct_pages, b.last_open))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("Alpha", 430, 5, local("2024-05-15 10:00:00")),
            ("Beta", 75, 2, local("2024-05-14 20:00:00")),
            ("Gamma", 0, 0, local("2024-01-01 00:00:00")),
        ]
    );
}

#[test]
fn book_totals_use_capped_and_uncapped_sums() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    let alpha = f.stats.book_totals(f.alpha).unwrap().unwrap();
    assert_eq!(alpha.title, "Alpha");
    assert_eq!(alpha.authors, "X");
    assert_eq!(alpha.pages, 100);
    // Uncapped: 60 + 30 + 200 + 40 + 80 + 20.
    assert_eq!(alpha.total_seconds, 430);
    // Capped per page: 60 + min(230, 120) + 20 + 40 + 80.
    assert_eq!(alpha.read_seconds, 320);
    assert_eq!(alpha.distinct_pages, 5);
    // Days 05-10, 05-12, 05-13, 05-15.
    assert_eq!(alpha.days_read, 4);
    assert_eq!(alpha.first_open, Some(local("2024-05-10 08:00:00")));
    // The latest span is page 2 at 05-15 10:00.
    assert_eq!(alpha.last_page, Some(2));
    assert_eq!(alpha.average_page_seconds, Some(64.0));
    assert_eq!((alpha.highlights, alpha.notes), (Some(0), Some(0)));

    let beta = f.stats.book_totals(f.beta).unwrap().unwrap();
    assert_eq!(beta.total_seconds, 75);
    assert_eq!(beta.read_seconds, 75);
    assert_eq!(beta.days_read, 2);
    assert_eq!(beta.first_open, Some(local("2024-04-30 12:00:00")));
    assert_eq!(beta.last_page, Some(3));
    assert_eq!(beta.average_page_seconds, Some(37.5));
}

#[test]
fn a_book_without_reading_has_no_averages() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    let gamma = f.stats.book_totals(f.gamma).unwrap().unwrap();
    assert_eq!(
        (
            gamma.total_seconds,
            gamma.read_seconds,
            gamma.distinct_pages,
            gamma.days_read
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(
        (gamma.first_open, gamma.last_page, gamma.average_page_seconds),
        (None, None, None)
    );
    assert_eq!((gamma.highlights, gamma.notes), (Some(2), Some(1)));
    assert!(f.stats.book_totals(999).unwrap().is_none());
}

#[test]
fn average_time_per_page_is_capped_seconds_over_pages() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    assert_eq!(f.stats.average_time_per_page(f.alpha).unwrap(), Some(64.0));
    assert_eq!(f.stats.average_time_per_page(f.gamma).unwrap(), None);
}

#[test]
fn time_left_uses_the_average_page_time_and_the_daily_rate() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    let now = local("2024-05-15 12:00:00");
    let estimate = f.stats.book_estimate(f.alpha).unwrap();
    assert_eq!(estimate.read_pages, 5);
    assert_eq!(estimate.read_seconds, 320);
    assert_eq!(estimate.days_read, 4);
    assert_eq!(estimate.avg_page_seconds, 64.0);
    assert_eq!(estimate.seconds_per_day, Some(80.0));

    // Book at page 10 of 100: 90 pages left, 90 * 64 = 5760 s, 5760 / 80 = 72 days.
    let left = estimate.time_left(90, now);
    assert_eq!(left.seconds, 5760.0);
    assert_eq!(left.days, Some(72));
    assert_eq!(left.finish_ts, Some(now + 72 * 86400));

    // Chapter of 3 pages left in the same book: 192 s, 3 days at the daily rate.
    let chapter = estimate.time_left(3, now);
    assert_eq!(chapter.seconds, 192.0);
    assert_eq!(chapter.days, Some(3));

    let finished = estimate.time_left(0, now);
    assert_eq!(
        (finished.seconds, finished.days, finished.finish_ts),
        (0.0, None, None)
    );
}

#[test]
fn a_book_without_reading_estimates_from_half_the_max_limit() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    let estimate = f.stats.book_estimate(f.gamma).unwrap();
    assert_eq!(estimate.avg_page_seconds, 60.0);
    assert_eq!(estimate.seconds_per_day, None);
    let left = estimate.time_left(10, local("2024-05-15 12:00:00"));
    assert_eq!((left.seconds, left.days, left.finish_ts), (600.0, None, None));
}

#[test]
fn streaks_count_consecutive_reading_days() {
    let dir = Scratch::new();
    let mut f = fixture(&dir);
    // Reading days: 04-30, 05-10, 05-12, 05-13, 05-14, 05-15.
    assert_eq!(
        f.stats.streaks(local("2024-05-15 12:00:00")).unwrap(),
        Streaks {
            current: 4,
            longest: 4
        }
    );
    // Today has no reading yet: the run that ends yesterday still counts.
    assert_eq!(
        f.stats.streaks(local("2024-05-16 12:00:00")).unwrap(),
        Streaks {
            current: 4,
            longest: 4
        }
    );
    // Two days without reading: the streak is broken.
    assert_eq!(
        f.stats.streaks(local("2024-05-17 12:00:00")).unwrap(),
        Streaks {
            current: 0,
            longest: 4
        }
    );
    // The run that ends yesterday is the single day 05-10.
    assert_eq!(
        f.stats.streaks(local("2024-05-11 12:00:00")).unwrap(),
        Streaks {
            current: 1,
            longest: 4
        }
    );
}

#[test]
fn an_empty_database_has_no_totals() {
    let dir = Scratch::new();
    let path = dir.path("empty.sqlite3");
    let mut stats = Stats::open(&path).unwrap();
    let now = local("2024-05-15 12:00:00");
    assert_eq!(stats.today(now).unwrap(), Totals::default());
    assert_eq!(stats.session().unwrap(), Totals::default());
    assert_eq!(stats.streaks(now).unwrap(), Streaks::default());
    assert!(stats.books().unwrap().is_empty());
    assert_eq!(stats.daily(now, 3).unwrap().len(), 3);
}
