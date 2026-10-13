mod common;

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use common::*;
use kaia_library::{Filter, Library, Sort, SortKey};

fn setup() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("books");
    fs::create_dir_all(&root).unwrap();
    (tmp, root)
}

/// One thread scans while another reads and writes through its own handle. The
/// reader must never see an error, and every book must end up indexed.
#[test]
fn scan_in_one_thread_and_queries_in_another() {
    let (tmp, root) = setup();
    // Eight copies of two books keep the scan busy for a while.
    for i in 0..4 {
        copy_fixture("2col.pdf", &root, &format!("two-column-{i}.pdf"));
        copy_fixture("leaves.epub", &root, &format!("leaves-{i}.epub"));
    }
    let db = db_path(tmp.path());
    init_engine();
    let mut scanner = Library::open(&db).unwrap();
    let mut reader = Library::open(&db).unwrap();

    let done = Arc::new(AtomicBool::new(false));
    let done_reader = Arc::clone(&done);
    let reader_thread = thread::spawn(move || {
        let mut queries = 0usize;
        let mut max_seen = 0usize;
        while !done_reader.load(Ordering::Relaxed) {
            let books = reader
                .books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
                .expect("query while scanning");
            max_seen = max_seen.max(books.len());
            reader.recent(5).expect("recent while scanning");
            reader.search("two", 5).expect("search while scanning");
            if let Some(first) = books.first() {
                reader.record_open(first.id, 0.5).expect("write while scanning");
            }
            queries += 1;
        }
        (queries, max_seen)
    });

    let cancel = AtomicBool::new(false);
    let report = scanner
        .scan(std::slice::from_ref(&root), |_| {}, &cancel)
        .expect("scan");
    done.store(true, Ordering::Relaxed);
    let (queries, max_seen) = reader_thread.join().expect("reader thread");

    assert_eq!(report.added, 8);
    assert_eq!(report.failed, 0);
    assert!(queries > 0, "the reader ran during the scan");
    assert!(max_seen <= 8);
    let final_count = scanner
        .books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
        .unwrap()
        .len();
    assert_eq!(final_count, 8);
}

/// Two handles on one database, both writing, serialize without losing rows.
#[test]
fn two_writers_do_not_lose_rows() {
    let (tmp, root) = setup();
    fixture_set(&root);
    let db = db_path(tmp.path());
    init_engine();
    let mut first = Library::open(&db).unwrap();
    let cancel = AtomicBool::new(false);
    first.scan(std::slice::from_ref(&root), |_| {}, &cancel).unwrap();

    let mut second = Library::open(&db).unwrap();
    let ids: Vec<i64> = first
        .books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 100)
        .unwrap()
        .iter()
        .map(|b| b.id)
        .collect();
    let t1 = {
        let ids = ids.clone();
        thread::spawn(move || {
            for _ in 0..20 {
                for &id in &ids {
                    first.record_open(id, 0.1).expect("writer one");
                }
            }
            first
        })
    };
    let t2 = thread::spawn(move || {
        for _ in 0..20 {
            for &id in &ids {
                second.add_to_collection(id, "Shared").expect("writer two");
            }
        }
        second
    });
    let first = t1.join().unwrap();
    let second = t2.join().unwrap();

    let coll = second.collections().unwrap();
    assert_eq!(coll.len(), 1);
    assert_eq!(coll[0].books, 4);
    let history: i64 = raw_connection(tmp.path())
        .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(history, 80);
    drop(first);
}
