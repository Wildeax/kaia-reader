//! Recording rules, driven by a manual clock. Expected values are worked out by hand.

mod common;

use common::{local, span_count, view_pages, Scratch};
use kaia_stats::{BookInfo, Stats, StatsOptions};
use rusqlite::{params, Connection, OpenFlags};

const MD5: &str = "recmd5";

/// Opens a fresh statistics database and starts the test book at `page` at time `at`.
fn start(dir: &Scratch, at: &str, page: i64) -> (Stats, i64) {
    let mut stats = Stats::open(dir.path("stats.sqlite3")).unwrap();
    let id = stats
        .start_book(MD5, "Recorded", "Tester", 10, page, local(at))
        .unwrap();
    (stats, id)
}

fn reader(stats: &Stats) -> Connection {
    Connection::open_with_flags(stats.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

#[test]
fn legacy_mupdf_nul_title_row_is_reused() {
    let dir = Scratch::new();
    let path = dir.path("stats.sqlite3");
    let stats = Stats::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    conn.execute(
        "INSERT INTO book (title, authors, notes, last_open, highlights, pages, series, language, md5, total_read_time, total_read_pages) \
         VALUES (?1, 'Tester', 3, 0, 4, 10, 'N/A', 'N/A', ?2, 0, 0)",
        params!["Recorded\0", MD5],
    ).unwrap();
    drop(stats);

    let mut stats = Stats::open(&path).unwrap();
    let id = stats.start_book(MD5, "Recorded", "Tester", 10, 1, 1_000).unwrap();
    let count: i64 = reader(&stats)
        .query_row("SELECT count(*) FROM book", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(id, 1);
}

#[test]
fn new_book_uses_sidecar_metadata_and_rejects_zero_pages() {
    let dir = Scratch::new();
    let mut stats = Stats::open(dir.path("stats.sqlite3")).unwrap();
    let info = BookInfo {
        md5: "sidecar-md5",
        display_title: "Display",
        authors: "Author",
        series: "Series #2",
        language: "fr",
        notes: 5,
        highlights: 8,
    };
    let id = stats
        .start_book_with_info(info, 20, 1, 1000, false)
        .unwrap()
        .unwrap();
    let row: (String, String, i64, i64, String, String, String) = reader(&stats)
        .query_row(
            "SELECT title, authors, notes, highlights, series, language, md5 FROM book WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        row,
        (
            "Display".into(),
            "Author".into(),
            5,
            8,
            "Series #2".into(),
            "fr".into(),
            "sidecar-md5".into()
        )
    );
    assert!(matches!(
        stats.start_book("zero", "Zero", "N/A", 0, 1, 1001),
        Err(kaia_stats::Error::InvalidPageCount(0))
    ));
}

#[test]
fn freeze_finished_books_and_open_options_are_respected() {
    let dir = Scratch::new();
    let mut stats = Stats::open_with_options(
        dir.path("stats.sqlite3"),
        StatsOptions {
            min_sec: 10,
            max_sec: 30,
            freeze_finished_books: true,
        },
    )
    .unwrap();
    let info = BookInfo {
        md5: "frozen",
        display_title: "Finished",
        authors: "N/A",
        series: "N/A",
        language: "N/A",
        notes: 0,
        highlights: 0,
    };
    assert_eq!(
        stats
            .start_book_with_info(info.clone(), 5, 1, 1000, true)
            .unwrap(),
        None
    );
    assert_eq!(
        reader(&stats)
            .query_row("SELECT count(*) FROM book", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    let id = stats
        .start_book_with_info(info, 5, 1, 1000, false)
        .unwrap()
        .unwrap();
    stats.page_turn(2, 1009).unwrap();
    stats.page_turn(3, 1069).unwrap();
    stats.end_book(1069).unwrap();
    assert_eq!(stats.book_totals(id).unwrap().unwrap().total_seconds, 30);
}

#[test]
fn a_stay_counts_its_full_length_once_the_reader_leaves() {
    let dir = Scratch::new();
    let (mut stats, id) = start(&dir, "2024-05-15 09:00:00", 1);
    stats.page_turn(2, local("2024-05-15 09:01:00")).unwrap();
    stats.flush().unwrap();

    let conn = reader(&stats);
    assert_eq!(view_pages(&conn, id), vec![(1, 60)]);
    let totals = stats.book_totals(id).unwrap().unwrap();
    assert_eq!(
        (totals.total_seconds, totals.read_seconds, totals.distinct_pages),
        (60, 60, 1)
    );
}

#[test]
fn stays_below_the_min_limit_do_not_count_and_the_min_itself_does() {
    let dir = Scratch::new();
    let (mut stats, id) = start(&dir, "2024-05-15 09:00:00", 1);
    let t = local("2024-05-15 09:00:00");
    stats.page_turn(2, t + 60).unwrap(); // page 1: 60 s
    stats.page_turn(3, t + 64).unwrap(); // page 2: 4 s, below the 5 s minimum
    stats.page_turn(4, t + 69).unwrap(); // page 3: 5 s, exactly the minimum, counts
    stats.end_book(t + 70).unwrap(); // page 4: 1 s

    // Page 2 (4 s) and page 4 (1 s) are below the minimum and do not count.
    let totals = stats.book_totals(id).unwrap().unwrap();
    assert_eq!(totals.total_seconds, 60 + 5);
    assert_eq!(totals.distinct_pages, 2);
    let conn = reader(&stats);
    assert_eq!(span_count(&conn, id), 2);
}

#[test]
fn a_long_stay_counts_the_max_limit() {
    let dir = Scratch::new();
    let (mut stats, id) = start(&dir, "2024-05-15 09:00:00", 1);
    let t = local("2024-05-15 09:00:00");
    stats.page_turn(2, t + 300).unwrap(); // 300 s on page 1 counts 120 s
    stats.page_turn(3, t + 420).unwrap(); // exactly 120 s on page 2 counts 120 s
    stats.end_book(t + 420).unwrap(); // zero time on page 3

    let totals = stats.book_totals(id).unwrap().unwrap();
    assert_eq!(totals.total_seconds, 240);
    assert_eq!(totals.read_seconds, 240);
}

#[test]
fn the_read_limits_are_configurable() {
    let dir = Scratch::new();
    let (mut stats, id) = start(&dir, "2024-05-15 09:00:00", 1);
    stats.set_read_limits(10, 30);
    let t = local("2024-05-15 09:00:00");
    stats.page_turn(2, t + 9).unwrap(); // below 10 s: not counted
    stats.page_turn(3, t + 9 + 45).unwrap(); // 45 s: capped to 30 s
    stats.end_book(t + 54).unwrap(); // 9 s: not counted

    assert_eq!(stats.book_totals(id).unwrap().unwrap().total_seconds, 30);
}

#[test]
fn the_cap_applies_per_page_when_a_page_is_read_again() {
    let dir = Scratch::new();
    let (mut stats, id) = start(&dir, "2024-05-15 09:00:00", 1);
    let t = local("2024-05-15 09:00:00");
    stats.page_turn(2, t + 200).unwrap(); // page 1: 120
    stats.page_turn(1, t + 210).unwrap(); // page 2: 10
    stats.end_book(t + 410).unwrap(); // page 1 again: 120

    let totals = stats.book_totals(id).unwrap().unwrap();
    assert_eq!(totals.total_seconds, 250, "uncapped total sums the stored spans");
    assert_eq!(
        totals.read_seconds, 130,
        "capped total takes min(240, 120) for page 1 and 10 for page 2"
    );
    assert_eq!(totals.distinct_pages, 2);
}

#[test]
fn a_suspend_gap_is_not_counted() {
    let dir = Scratch::new();
    let (mut stats, id) = start(&dir, "2024-05-15 09:00:00", 1);
    let t = local("2024-05-15 09:00:00");
    stats.on_suspend(t + 30).unwrap();
    stats.on_resume(t + 1000).unwrap();
    stats.page_turn(2, t + 1040).unwrap();
    stats.flush().unwrap();

    // 30 s before the suspend and 40 s after the resume.
    let conn = reader(&stats);
    assert_eq!(view_pages(&conn, id), vec![(1, 70)]);
}

#[test]
fn a_page_turn_during_a_pause_is_applied_at_resume() {
    let dir = Scratch::new();
    let (mut stats, id) = start(&dir, "2024-05-15 09:00:00", 1);
    let t = local("2024-05-15 09:00:00");
    stats.on_suspend(t + 20).unwrap();
    stats.page_turn(2, t + 500).unwrap(); // ignored while paused, remembered
    stats.on_resume(t + 600).unwrap(); // page 1 gets its 20 s before the suspend, then page 2 starts
    stats.end_book(t + 610).unwrap(); // page 2 gets 10 s

    let totals = stats.book_totals(id).unwrap().unwrap();
    assert_eq!(totals.total_seconds, 30);
    assert_eq!(totals.distinct_pages, 2);
}

#[test]
fn a_resume_without_a_page_turn_keeps_the_page_open() {
    let dir = Scratch::new();
    let (mut stats, id) = start(&dir, "2024-05-15 09:00:00", 1);
    let t = local("2024-05-15 09:00:00");
    stats.on_suspend(t + 10).unwrap();
    stats.on_resume(t + 100).unwrap();
    stats.end_book(t + 115).unwrap(); // 10 s before the suspend and 15 s after the resume

    assert_eq!(stats.book_totals(id).unwrap().unwrap().total_seconds, 25);
}

#[test]
fn a_turn_to_the_same_page_changes_nothing() {
    let dir = Scratch::new();
    let (mut stats, id) = start(&dir, "2024-05-15 09:00:00", 1);
    let t = local("2024-05-15 09:00:00");
    stats.page_turn(1, t + 50).unwrap();
    stats.end_book(t + 100).unwrap();

    assert_eq!(stats.book_totals(id).unwrap().unwrap().total_seconds, 100);
}

#[test]
fn flush_runs_every_fifty_page_turns() {
    let dir = Scratch::new();
    let (mut stats, _id) = start(&dir, "2024-05-15 09:00:00", 1);
    let t = local("2024-05-15 09:00:00");
    for turn in 1..=50 {
        stats.page_turn(turn + 1, t + 6 * turn).unwrap();
    }
    // Without any explicit flush, the 50th turn wrote the 50 counted spans.
    let conn = reader(&stats);
    let written: i64 = conn
        .query_row("SELECT count(*) FROM page_stat_data", [], |row| row.get(0))
        .unwrap();
    assert_eq!(written, 50);
}

#[test]
fn reopening_a_book_reuses_its_row_and_adds_to_its_totals() {
    let dir = Scratch::new();
    let (mut stats, first) = start(&dir, "2024-05-15 09:00:00", 1);
    let t = local("2024-05-15 09:00:00");
    stats.end_book(t + 30).unwrap();
    let second = stats
        .start_book(MD5, "Recorded", "Tester", 10, 1, t + 3600)
        .unwrap();
    stats.end_book(t + 3640).unwrap();

    assert_eq!(first, second);
    assert_eq!(stats.book_totals(first).unwrap().unwrap().total_seconds, 70);
}

#[test]
fn a_new_page_count_rescales_spans_recorded_before_it() {
    let dir = Scratch::new();
    let (mut stats, id) = start(&dir, "2024-05-15 09:00:00", 3);
    let t = local("2024-05-15 09:00:00");
    stats.page_turn(4, t + 100).unwrap(); // page 3 of 10: 100 s, recorded with total_pages 10
    stats.pages_changed(20).unwrap(); // the book now has 20 pages
    stats.flush().unwrap();

    // Stored row keeps the old count.
    let conn = reader(&stats);
    let stored: (i64, i64) = conn
        .query_row(
            "SELECT duration, total_pages FROM page_stat_data WHERE id_book = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(stored, (100, 10));
    // Through the view, page 3 of 10 covers pages 5 and 6 of 20, and the 100 s is split between them.
    assert_eq!(view_pages(&conn, id), vec![(5, 50), (6, 50)]);
    let stored_pages: i64 = conn
        .query_row("SELECT pages FROM book WHERE id = ?1", params![id], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(stored_pages, 20);
}

#[test]
fn session_time_restarts_on_resume() {
    let dir = Scratch::new();
    let (mut stats, _id) = start(&dir, "2024-05-15 09:00:00", 1);
    let t = local("2024-05-15 09:00:00");
    stats.page_turn(2, t + 60).unwrap();
    assert_eq!(stats.session().unwrap().seconds, 60);
    // A span counts in a session when it starts inside it. The page open at the resume started before it.
    stats.on_resume(t + 5000).unwrap();
    stats.page_turn(3, t + 5030).unwrap();
    stats.page_turn(4, t + 5060).unwrap();
    assert_eq!(stats.session().unwrap().seconds, 30);
}
