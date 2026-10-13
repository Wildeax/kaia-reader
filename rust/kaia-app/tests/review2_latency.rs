mod common;
use common::*;
use std::{
    fs,
    time::{Duration, Instant},
};
#[test]
fn probe_library_build_decodes_every_cover() {
    let dir = scratch("large-library-latency");
    let books = library_with_books(&dir);
    let mut app = make_app(&dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    app.command("nav:home", &[]);
    let conn = rusqlite::Connection::open(app.services().library_path()).unwrap();
    let seed: i64 = conn
        .query_row(
            "SELECT book_id FROM covers WHERE small IS NOT NULL LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let original: String = conn
        .query_row("SELECT path FROM books WHERE id=?1", [seed], |r| r.get(0))
        .unwrap();
    conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    for n in 0..600 {
        let path = dir.join(format!("copy-{n}.epub"));
        fs::hard_link(&original, &path).unwrap();
        conn.execute("INSERT INTO books(path,size,mtime,partial_md5,title,format,added_at,extracted) SELECT ?1,size,mtime,partial_md5,title,format,added_at,extracted FROM books WHERE id=?2",rusqlite::params![path.to_string_lossy(),seed]).unwrap();
        let id = conn.last_insert_rowid();
        conn.execute("INSERT INTO covers(book_id,source_w,source_h,small,large,mime) SELECT ?1,source_w,source_h,small,large,mime FROM covers WHERE book_id=?2",[id,seed]).unwrap();
    }
    conn.execute_batch("COMMIT").unwrap();
    let start = Instant::now();
    app.command("nav:library", &[]);
    let elapsed = start.elapsed();
    eprintln!("Building Library with 603 books blocked command for {elapsed:?}");
    assert!(elapsed < Duration::from_millis(100), "{elapsed:?}");
    let start = Instant::now();
    app.key(kaia_input::Key::Power).unwrap();
    eprintln!("Power dispatch after library command: {:?}", start.elapsed());
    assert!(app.sleeping());
    app.key(kaia_input::Key::Power).unwrap();
    app.process(vec![]).unwrap();
    let before = app.backend().display().refreshes().len();
    wait_for(&mut app, "visible covers", |_| !kaia_app::covers::busy());
    let updates = &app.backend().display().refreshes()[before..];
    assert!(updates.iter().all(|r| r.region.h < app.screen().h));
}

#[test]
fn large_book_open_keeps_power_responsive() {
    let dir = scratch("book-open-latency");
    let books = library_with_books(&dir);
    let source = std::env::var_os("KAIA_LATENCY_BOOK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| repo_root().join("test/juliet.epub"));
    fs::copy(source, books.join("latency.epub")).unwrap();
    let mut app = make_app(&dir, vec![books]);
    app.start().unwrap();
    wait_for(&mut app, "scan", |a| !a.scan_running());
    let id = book_id(&app, "latency.epub");
    let start = Instant::now();
    app.command(&format!("open_book:{id}"), &[]);
    let elapsed = start.elapsed();
    eprintln!("Book open command: {elapsed:?}");
    assert!(elapsed < Duration::from_millis(100));
    assert!(app.reader_busy());
    let start = Instant::now();
    app.key(kaia_input::Key::Left).unwrap();
    eprintln!("Input dispatch during book open: {:?}", start.elapsed());
    let start = Instant::now();
    app.key(kaia_input::Key::Power).unwrap();
    eprintln!("Power dispatch during book open: {:?}", start.elapsed());
    assert!(app.sleeping());
    app.key(kaia_input::Key::Power).unwrap();
    wait_reader(&mut app);
}
