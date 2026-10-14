//! Session imports retain exact bounds and add time without guessed pages.

use kaia_stats::{BookInfo, NickelSession, Stats};

#[test]
fn sessions_are_exact_unique_and_visible_in_totals() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stats.sqlite3");
    let mut stats = Stats::open(&path).unwrap();
    let info = BookInfo {
        md5: "abc",
        display_title: "Book",
        authors: "Author",
        series: "",
        language: "",
        notes: 0,
        highlights: 0,
    };
    let span = NickelSession {
        start: 1767225600,
        end: 1767226200,
        start_percent: 0.1,
        end_percent: 0.2,
        idle_seconds: 60,
    };
    let report = stats.import_nickel(&info, &[span.clone(), span.clone()]).unwrap();
    assert_eq!(report.spans_added, 1);
    assert_eq!(report.spans_skipped, 1);
    assert_eq!(
        stats
            .import_nickel(&info, std::slice::from_ref(&span))
            .unwrap()
            .spans_added,
        0
    );
    let book = stats.books().unwrap().pop().unwrap();
    assert_eq!(book.seconds, 540);
    assert_eq!(book.distinct_pages, 0);
    let totals = stats.book_totals(book.id).unwrap().unwrap();
    assert_eq!(totals.total_seconds, 540);
    assert_eq!(totals.days_read, 1);
    assert_eq!(totals.last_page, None);
    assert_eq!(totals.first_open, Some(span.start));
    assert_eq!(stats.today(span.end).unwrap().seconds, 540);
    let conn = rusqlite::Connection::open(path).unwrap();
    let bounds: (i64, i64, f64, f64) = conn
        .query_row(
            "SELECT start_time,end_time,start_percent,end_percent FROM nickel_sessions",
            [],
            |r| {
                Ok((
                    r.get("start_time")?,
                    r.get("end_time")?,
                    r.get("start_percent")?,
                    r.get("end_percent")?,
                ))
            },
        )
        .unwrap();
    assert_eq!(bounds, (span.start, span.end, 0.1, 0.2));
    let invalid = NickelSession {
        end: span.start - 1,
        ..span
    };
    assert_eq!(stats.import_nickel(&info, &[invalid]).unwrap().spans_skipped, 1);
}
