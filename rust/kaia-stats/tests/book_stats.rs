//! `Stats::book` against a fixture: one book, three sessions over two local days.
//!
//! Fixture: the book is "Dune" by "Frank Herbert", 10 pages, keyed by the md5 `dune0001`.
//! Spans are written directly, as the recorder writes them (page, local start, seconds).
//!
//! - Session A, 13 May 08:00: page 1 for 60 s, page 2 for 30 s.
//! - Session B, 13 May 20:00: page 3 for 100 s.
//! - Session C, 14 May 07:00: page 3 for 60 s, then page 4 for 40 s.
//!
//! Hand-computed with the default 120 s limit: total 290 s, read 250 s (page 3 is
//! capped at 120 s of its 160 s), 4 distinct pages, 2 days, 62.5 s per page, 125 s
//! per day, last page 4, so 6 pages left and 375 s left.

mod common;

use common::{add_book, add_span, koreader_db, local, Scratch};
use kaia_stats::{BookEstimate, BookStats, BookTotals, Stats};

const MD5: &str = "dune0001";
const TITLE: &str = "Dune";
const AUTHORS: &str = "Frank Herbert";

fn fixture(dir: &Scratch) -> Stats {
    let path = dir.path("statistics.sqlite3");
    let conn = koreader_db(&path);
    let dune = add_book(&conn, TITLE, Some(AUTHORS), MD5, 10);
    conn.execute_batch(&format!(
        "UPDATE book SET last_open = {} WHERE id = {dune};",
        local("2024-05-14 07:02:00"),
    ))
    .unwrap();
    let spans = [
        (1, "2024-05-13 08:00:00", 60),
        (2, "2024-05-13 08:01:00", 30),
        (3, "2024-05-13 20:00:00", 100),
        (3, "2024-05-14 07:00:00", 60),
        (4, "2024-05-14 07:01:00", 40),
    ];
    for (page, at, seconds) in spans {
        add_span(&conn, dune, page, local(at), seconds, 10);
    }
    drop(conn);
    Stats::open(&path).unwrap()
}

fn expected() -> BookStats {
    BookStats {
        totals: BookTotals {
            id: 1,
            title: TITLE.to_owned(),
            authors: AUTHORS.to_owned(),
            pages: 10,
            last_open: local("2024-05-14 07:02:00"),
            highlights: Some(0),
            notes: Some(0),
            total_seconds: 290,
            read_seconds: 250,
            distinct_pages: 4,
            days_read: 2,
            first_open: Some(local("2024-05-13 08:00:00")),
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

#[test]
fn the_book_query_returns_the_fixture_totals() {
    let dir = Scratch::new();
    let mut stats = fixture(&dir);
    assert_eq!(stats.book(MD5, TITLE, AUTHORS).unwrap(), Some(expected()));
}

#[test]
fn the_time_left_comes_from_the_pages_left_after_the_last_page() {
    let dir = Scratch::new();
    let mut stats = fixture(&dir);
    let now = local("2024-05-14 12:00:00");
    let found = stats.book(MD5, TITLE, AUTHORS).unwrap().unwrap();
    let pages_left = found.totals.pages - found.totals.last_page.unwrap_or(0);
    assert_eq!(pages_left, 6);
    let left = found.estimate.time_left(pages_left, now);
    assert_eq!(left.seconds, 375.0);
    assert_eq!(left.days, Some(3));
    assert_eq!(left.finish_ts, Some(now + 3 * 86400));
}

#[test]
fn a_key_that_matches_no_row_finds_nothing() {
    let dir = Scratch::new();
    let mut stats = fixture(&dir);
    assert_eq!(stats.book("other000", TITLE, AUTHORS).unwrap(), None);
    assert_eq!(stats.book(MD5, "Dune Messiah", AUTHORS).unwrap(), None);
    assert_eq!(stats.book(MD5, TITLE, "N/A").unwrap(), None);
}

#[test]
fn a_row_with_no_reading_has_zero_totals() {
    let dir = Scratch::new();
    let path = dir.path("statistics.sqlite3");
    let conn = koreader_db(&path);
    add_book(&conn, "Emma", Some("Jane Austen"), "emma0001", 300);
    drop(conn);
    let mut stats = Stats::open(&path).unwrap();
    let found = stats.book("emma0001", "Emma", "Jane Austen").unwrap().unwrap();
    assert_eq!(found.totals.total_seconds, 0);
    assert_eq!(found.totals.read_seconds, 0);
    assert_eq!(found.totals.distinct_pages, 0);
    assert_eq!(found.totals.days_read, 0);
    assert_eq!(found.totals.first_open, None);
    assert_eq!(found.totals.last_page, None);
    assert_eq!(found.totals.average_page_seconds, None);
    assert_eq!(found.estimate.seconds_per_day, None);
    assert_eq!(found.estimate.avg_page_seconds, 60.0);
}
