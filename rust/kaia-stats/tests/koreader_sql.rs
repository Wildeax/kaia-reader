//! KOReader's own SQL, run unchanged against a database written by Kaia.
//!
//! The statements are copies of the strings in `plugins/statistics.koplugin/main.lua`.
//! `%d` and `%%` are filled in the way Lua's `string.format` does it. Each result must
//! match what Kaia's API returns for the same book and period.

mod common;

use common::{local, Scratch};
use kaia_stats::Stats;
use rusqlite::{params, Connection, OpenFlags};

/// Lua `string.format` for the `%d` and `%%` directives used in the statements below.
fn fmt(sql: &str, args: &[i64]) -> String {
    let mut out = String::new();
    let mut args = args.iter();
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('d') => out.push_str(&args.next().expect("missing argument").to_string()),
            other => panic!("unsupported directive %{other:?}"),
        }
    }
    out
}

/// `STATISTICS_SQL_BOOK_CAPPED_TOTALS_QUERY` (args: max_sec, id_book).
const CAPPED_TOTALS: &str = "
    SELECT count(*),
           sum(durations)
    FROM (
        SELECT min(sum(duration), %d) AS durations
        FROM page_stat
        WHERE id_book = %d
        GROUP BY page
    );
";

/// `STATISTICS_SQL_BOOK_TOTALS_QUERY` (arg: id_book).
const UNCAPPED_TOTALS: &str = "
    SELECT count(DISTINCT page),
           sum(duration)
    FROM   page_stat
    WHERE  id_book = %d;
";

/// `getTodayBookStats` (arg: start of today).
const TODAY_STATS: &str = "
        SELECT count(*),
               sum(sum_duration)
        FROM    (
                     SELECT sum(duration)    AS sum_duration
                     FROM   page_stat
                     WHERE  start_time >= %d
                     GROUP  BY id_book, page
                );
";

/// `sqlDaily` used by `getDatesFromAll` and `getReadingProgressStats` (arg: period begin).
const DAILY: &str = "
            SELECT dates,
                   count(*)             AS pages,
                   sum(sum_duration)    AS durations,
                   start_time
            FROM   (
                        SELECT strftime('%%Y-%%m-%%d', start_time, 'unixepoch', 'localtime') AS dates,
                               sum(duration)                                                 AS sum_duration,
                               start_time
                        FROM   page_stat
                        WHERE  start_time >= %d
                        GROUP  BY id_book, page, dates
                   )
            GROUP  BY dates
            ORDER  BY dates DESC;
";

/// The per-book part of `getBookStat` (arg: id_book).
const BOOK_DAYS: &str = "
    SELECT count(*)
    FROM   (
                SELECT strftime('%%Y-%%m-%%d', start_time, 'unixepoch', 'localtime') AS dates
                FROM   page_stat
                WHERE  id_book = %d
                GROUP  BY dates
           );
";

/// The span part of `getBookStat` (arg: id_book).
const BOOK_SPANS: &str = "
    SELECT sum(duration),
           count(DISTINCT page),
           min(start_time),
           (select max(ps2.page) from page_stat as ps2 where ps2.start_time = max(page_stat.start_time))
    FROM   page_stat
    WHERE  id_book = %d;
";

/// `getTotalStats`, total and per book (arg: id_book, or none for the total).
const TOTAL_ALL: &str = "
        SELECT sum(duration)
        FROM   page_stat;
";
const TOTAL_PER_BOOK: &str = "
            SELECT sum(duration)
            FROM   page_stat
            WHERE  id_book = %d;
";

/// `getReadBookByDay` (binds: start_time offset, month start, month start).
const READ_BOOK_BY_DAY: &str = "
        SELECT
            strftime('%Y-%m-%d', start_time, 'unixepoch', 'localtime') day,
            sum(duration) durations,
            id_book book_id,
            title book_title
        FROM  (
            SELECT start_time-? as start_time, duration, page_stat.id_book, book.title
            FROM page_stat
            JOIN   book ON book.id = page_stat.id_book
            WHERE  start_time BETWEEN strftime('%s', ?, 'utc')
                                  AND strftime('%s', ?, 'utc', '+33 days', 'start of month', '-1 second')
        )
        GROUP  BY
            strftime('%Y-%m-%d', start_time, 'unixepoch', 'localtime'),
            id_book,
            title
        ORDER BY day, durations desc, book_id, book_title;
";

/// `getReadingRatioPerHourByDay` (binds: start_time offset, month start, month start).
const RATIO_PER_HOUR: &str = "
        SELECT
            strftime('%Y-%m-%d', start_time, 'unixepoch', 'localtime') day,
            strftime('%H', start_time, 'unixepoch', 'localtime') hour,
            sum(duration)/3600.0 ratio
        FROM  (
            SELECT
                start_time-? as start_time,
                duration
            FROM page_stat
            WHERE  start_time BETWEEN strftime('%s', ?, 'utc')
                                  AND strftime('%s', ?, 'utc', '+33 days', 'start of month', '-1 second')
        )
        GROUP  BY
            strftime('%Y-%m-%d', start_time, 'unixepoch', 'localtime'),
            strftime('%H', start_time, 'unixepoch', 'localtime')
        ORDER BY day, hour;
";

fn read_only(path: &std::path::Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn query_pair(conn: &Connection, sql: &str) -> (i64, i64) {
    conn.query_row(sql, [], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?.unwrap_or(0)))
    })
    .unwrap()
}

/// Two books with reading over three days, written through the recorder. One span set
/// goes through a suspend, and one book has its page count changed.
fn written_database(dir: &Scratch) -> (std::path::PathBuf, i64, i64) {
    let path = dir.path("stats.sqlite3");
    let mut stats = Stats::open(&path).unwrap();

    let x = stats
        .start_book("xxxx", "Xbook", "Auth", 100, 1, local("2024-05-14 09:00:00"))
        .unwrap();
    stats.page_turn(2, local("2024-05-14 09:01:00")).unwrap();
    stats.page_turn(3, local("2024-05-14 09:02:30")).unwrap();
    stats.page_turn(4, local("2024-05-14 09:10:30")).unwrap();
    stats.end_book(local("2024-05-14 09:12:00")).unwrap();
    stats
        .start_book("xxxx", "Xbook", "Auth", 100, 4, local("2024-05-15 20:00:00"))
        .unwrap();
    stats.page_turn(5, local("2024-05-15 20:00:30")).unwrap();
    stats.end_book(local("2024-05-15 20:05:30")).unwrap();

    let y = stats
        .start_book("yyyy", "Ybook", "Other", 50, 10, local("2024-05-14 18:00:00"))
        .unwrap();
    stats.on_suspend(local("2024-05-14 18:00:20")).unwrap();
    stats.on_resume(local("2024-05-14 18:30:00")).unwrap();
    stats.page_turn(11, local("2024-05-14 18:30:10")).unwrap();
    stats.pages_changed(100).unwrap();
    stats.end_book(local("2024-05-14 18:31:00")).unwrap();
    stats
        .start_book("yyyy", "Ybook", "Other", 100, 11, local("2024-05-15 07:00:00"))
        .unwrap();
    stats.page_turn(12, local("2024-05-15 07:00:40")).unwrap();
    stats.end_book(local("2024-05-15 07:01:40")).unwrap();
    (path, x, y)
}

#[test]
fn koreader_totals_match_kaia_per_book() {
    let dir = Scratch::new();
    let (path, x, y) = written_database(&dir);
    let conn = read_only(&path);
    let mut stats = Stats::open(&path).unwrap();

    for book in [x, y] {
        let capped = query_pair(&conn, &fmt(CAPPED_TOTALS, &[120, book]));
        let estimate = stats.book_estimate(book).unwrap();
        assert_eq!(
            capped,
            (estimate.read_pages, estimate.read_seconds),
            "capped totals of book {book}"
        );

        let uncapped = query_pair(&conn, &fmt(UNCAPPED_TOTALS, &[book]));
        let totals = stats.book_totals(book).unwrap().unwrap();
        assert_eq!(
            uncapped,
            (totals.distinct_pages, totals.total_seconds),
            "uncapped totals of book {book}"
        );

        let days: i64 = conn
            .query_row(&fmt(BOOK_DAYS, &[book]), [], |row| row.get(0))
            .unwrap();
        assert_eq!(days, totals.days_read, "days read of book {book}");

        let (sum, pages, first, last_page): (Option<i64>, i64, Option<i64>, Option<i64>) = conn
            .query_row(&fmt(BOOK_SPANS, &[book]), [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap();
        assert_eq!(sum.unwrap_or(0), totals.total_seconds);
        assert_eq!(pages, totals.distinct_pages);
        assert_eq!(first, totals.first_open);
        assert_eq!(last_page, totals.last_page);
    }

    let total: Option<i64> = conn.query_row(TOTAL_ALL, [], |row| row.get(0)).unwrap();
    let per_book: i64 = books_sum(&conn, &mut stats);
    assert_eq!(total.unwrap_or(0), per_book);
}

/// Sum of KOReader's per-book totals for every book, and a check of each one against `books()`.
fn books_sum(conn: &Connection, stats: &mut Stats) -> i64 {
    let mut sum = 0;
    for summary in stats.books().unwrap() {
        let per_book: Option<i64> = conn
            .query_row(&fmt(TOTAL_PER_BOOK, &[summary.id]), [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            per_book.unwrap_or(0),
            summary.seconds,
            "total of {}",
            summary.title
        );
        sum += per_book.unwrap_or(0);
    }
    sum
}

#[test]
fn koreader_today_and_daily_match_kaia() {
    let dir = Scratch::new();
    let (path, _, _) = written_database(&dir);
    let conn = read_only(&path);
    let mut stats = Stats::open(&path).unwrap();
    let now = local("2024-05-15 12:00:00");

    // getTodayBookStats starts at local midnight; the period begins at midnight of the day.
    let start_today = local("2024-05-15 00:00:00");
    let (pages, seconds): (i64, Option<i64>) = conn
        .query_row(&fmt(TODAY_STATS, &[start_today]), [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    let today = stats.today(now).unwrap();
    assert_eq!((pages, seconds.unwrap_or(0)), (today.pages, today.seconds));
    // 05-15 spans: Ybook pages 11 and 12 (40 s, 60 s), Xbook page 4 (30 s) and page 5 (300 s, capped to 120).
    assert_eq!((today.pages, today.seconds), (4, 250));

    // The last 7 days: `getDatesFromAll(7, "daily_weekday")` starts at midnight of 05-09.
    let mut stmt = conn
        .prepare(&fmt(DAILY, &[local("2024-05-09 00:00:00")]))
        .unwrap();
    let koreader: Vec<(String, i64, i64)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    let kaia: Vec<(String, i64, i64)> = stats
        .daily(now, 7)
        .unwrap()
        .into_iter()
        .filter(|day| day.pages > 0)
        .map(|day| (day.date, day.pages, day.seconds))
        .rev()
        .collect();
    assert!(!kaia.is_empty());
    assert_eq!(koreader, kaia);
}

#[test]
fn koreader_month_queries_match_kaia() {
    let dir = Scratch::new();
    let (path, _, _) = written_database(&dir);
    let conn = read_only(&path);
    let mut stats = Stats::open(&path).unwrap();
    let month = stats.month("2024-05").unwrap();

    // getReadBookByDay: the same days, and each day's book durations add up to the day's total.
    let mut stmt = conn.prepare(READ_BOOK_BY_DAY).unwrap();
    let rows: Vec<(String, i64)> = stmt
        .query_map(params![0, "2024-05-01", "2024-05-01"], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    let mut per_day: Vec<(String, i64)> = Vec::new();
    for (day, seconds) in rows {
        match per_day.last_mut() {
            Some((last, total)) if *last == day => *total += seconds,
            _ => per_day.push((day, seconds)),
        }
    }
    let expected: Vec<(String, i64)> = month.iter().map(|d| (d.date.clone(), d.seconds)).collect();
    assert_eq!(per_day, expected);

    // getReadingRatioPerHourByDay: hourly ratios (hours) add up to the same day totals.
    let mut stmt = conn.prepare(RATIO_PER_HOUR).unwrap();
    let ratios: Vec<(String, f64)> = stmt
        .query_map(params![0, "2024-05-01", "2024-05-01"], |row| {
            Ok((row.get(0)?, row.get(2)?))
        })
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    let mut hourly: Vec<(String, f64)> = Vec::new();
    for (day, ratio) in ratios {
        match hourly.last_mut() {
            Some((last, total)) if *last == day => *total += ratio,
            _ => hourly.push((day, ratio)),
        }
    }
    assert_eq!(hourly.len(), month.len());
    for (ratio_day, day) in hourly.iter().zip(&month) {
        assert_eq!(ratio_day.0, day.date);
        assert!(
            (ratio_day.1 * 3600.0 - day.seconds as f64).abs() < 1e-6,
            "{}",
            day.date
        );
    }
}
