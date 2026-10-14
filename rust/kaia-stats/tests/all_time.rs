//! All-time totals cover every recorded stay, not only the recent days.

use kaia_stats::Stats;

#[test]
fn all_time_adds_every_book_and_old_reading() {
    let dir = tempfile::tempdir().unwrap();
    let mut stats = Stats::open(dir.path().join("statistics.sqlite3")).unwrap();
    let now = 1_700_000_000;
    let long_ago = now - 2 * 365 * 24 * 3600;

    assert_eq!(stats.all_time().unwrap().seconds, 0, "no reading yet");

    stats
        .start_book("aaaa", "Old", "Author", 40, 1, long_ago)
        .unwrap();
    stats.page_turn(2, long_ago + 300).unwrap();
    stats.end_book(long_ago + 600).unwrap();

    stats
        .start_book("bbbb", "New", "Author", 40, 1, now - 900)
        .unwrap();
    stats.page_turn(2, now - 600).unwrap();
    stats.end_book(now).unwrap();

    let all = stats.all_time().unwrap();
    let books: i64 = stats.books().unwrap().iter().map(|b| b.seconds).sum();
    assert_eq!(all.seconds, books);
    assert!(all.seconds > 0);
    assert!(
        stats.today(now).unwrap().seconds < all.seconds,
        "the two years before today count in the total only"
    );
}
