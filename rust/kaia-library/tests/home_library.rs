//! Scans `~/kaia-test-library` when the developer has it. Skips otherwise. Run
//! with `--nocapture` to see the timings.

mod common;

use std::path::PathBuf;
use std::time::Instant;

use common::*;
use kaia_library::{Filter, Sort, SortKey};

fn home_library() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("HOME")?).join("kaia-test-library");
    dir.is_dir().then_some(dir)
}

#[test]
fn scans_the_home_test_library() {
    let Some(dir) = home_library() else {
        eprintln!("skipped: ~/kaia-test-library is missing");
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let mut lib = open_library(tmp.path());

    let t = Instant::now();
    let cold = scan(&mut lib, std::slice::from_ref(&dir));
    let cold_time = t.elapsed();

    let t = Instant::now();
    let warm = scan(&mut lib, std::slice::from_ref(&dir));
    let warm_time = t.elapsed();

    let books = lib
        .books(Sort::new(SortKey::Title, false), &Filter::default(), 0, 1000)
        .unwrap();
    for b in &books {
        eprintln!(
            "  {:<28} pages {:>5}  title {:?}  failure {:?}",
            b.path.file_name().unwrap().to_string_lossy(),
            b.pages.map_or(String::from("-"), |p| p.to_string()),
            b.title.as_deref().unwrap_or(""),
            b.failure
        );
    }
    let raw = raw_connection(tmp.path());
    let (covers, small_avg, large_avg, small_total, large_total): (i64, f64, f64, i64, i64) = raw
        .query_row(
            "SELECT COUNT(small), COALESCE(AVG(length(small)), 0), COALESCE(AVG(length(large)), 0),
                    COALESCE(SUM(length(small)), 0), COALESCE(SUM(length(large)), 0)
             FROM covers",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    let mimes: Vec<String> = raw
        .prepare("SELECT DISTINCT mime FROM covers")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|m| m.unwrap())
        .collect();
    eprintln!(
        "kaia-library covers: {covers} books with a cover, mime {mimes:?}, \
         average small {:.1} KiB, average large {:.1} KiB (totals {} KiB and {} KiB)",
        small_avg / 1024.0,
        large_avg / 1024.0,
        small_total / 1024,
        large_total / 1024
    );
    let db_bytes = std::fs::metadata(db_path(tmp.path())).unwrap().len();
    eprintln!(
        "kaia-library timing: files {}, cold scan {:.2?} (extracted {}, failed {}), \
         rescan {:.2?} (extracted {}, unchanged {}), db {} KiB",
        cold.files_seen,
        cold_time,
        cold.extracted,
        cold.failed,
        warm_time,
        warm.extracted,
        warm.unchanged,
        db_bytes / 1024
    );
    assert_eq!(warm.extracted, 0, "an unchanged library is not extracted again");
    assert_eq!(books.len(), cold.files_seen);
}
