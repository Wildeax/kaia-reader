mod common;

use std::fs;
use std::sync::Barrier;

use common::*;
use kaia_library::CoverSize;

#[test]
fn concurrent_pdf_and_epub_extractions() {
    init_engine();
    for _ in 0..3 {
        let ready = Barrier::new(16);
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| {
                    let tmp = tempfile::tempdir().unwrap();
                    let root = tmp.path().join("books");
                    fs::create_dir(&root).unwrap();
                    let epub = copy_fixture("leaves.epub", &root, "book.epub");
                    let pdf = copy_fixture("2col.pdf", &root, "book.pdf");
                    let mut lib = open_library(tmp.path());
                    ready.wait();
                    let report = scan(&mut lib, &[root]);
                    assert_eq!(report.extracted, 2);
                    assert_eq!(report.failed, 0);
                    for path in [epub, pdf] {
                        let book = lib.book_by_path(&path).unwrap().unwrap();
                        assert!(book.extracted);
                        assert!(book.title.as_deref().is_some_and(|s| !s.is_empty()));
                        assert!(book.pages.unwrap() > 0);
                        for size in [CoverSize::Small, CoverSize::Large] {
                            let cover = lib.cover(book.id, size).unwrap().unwrap();
                            let (width, height) = jpeg_size(&cover.bytes);
                            assert!(width > 0 && height > 0);
                        }
                    }
                });
            }
        });
    }
}
