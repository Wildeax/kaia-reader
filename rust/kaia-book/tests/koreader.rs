//! KOReader compatibility: partial MD5, sidecar import (read only), and the
//! Kaia state that follows an import.

mod common;

use std::fs;
use std::path::Path;

use kaia_book::*;

/// The sidecar checksum that KOReader wrote for `84.epub` in the developer's library.
const SIDECAR_84_MD5: &str = "378df3409d69b6994febc92492dbb19e";

fn file_bytes(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<(String, Vec<u8>)> = fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn partial_md5_matches_koreader_and_the_library_crate() {
    for name in ["juliet.epub", "paper.pdf", "tall.pdf"] {
        let p = common::repo_file(name);
        assert_eq!(
            kaia_book::partial_md5(&p).unwrap(),
            kaia_library::partial_md5(&p).unwrap(),
            "{name}"
        );
    }
    let Some(lib) = common::library_dir() else {
        eprintln!("skipped: ~/kaia-test-library is missing");
        return;
    };
    let book = lib.join("84.epub");
    if book.exists() {
        assert_eq!(kaia_book::partial_md5(&book).unwrap(), SIDECAR_84_MD5);
    }
}

#[test]
fn koreader_end_exclusive_xpointer_extracts_librarian() {
    let Some(lib) = common::library_dir() else {
        eprintln!("skipped: ~/kaia-test-library is missing");
        return;
    };
    let source = lib.join("2701.epub");
    if !source.exists() {
        eprintln!("skipped: 2701.epub is missing");
        return;
    }
    let work = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let book = common::copy_into(&source, work.path());
    let sidecar = work.path().join("2701.sdr");
    fs::create_dir(&sidecar).unwrap();
    fs::write(
        sidecar.join("metadata.epub.lua"),
        r#"return { ["annotations"] = {
        [1] = { ["datetime"] = "2026-10-12 07:45:00", ["drawer"] = "lighten",
            ["page"] = "/body/DocFragment[2]/body/div[3]/h2[2]/text().34",
            ["pos0"] = "/body/DocFragment[2]/body/div[3]/h2[2]/text().34",
            ["pos1"] = "/body/DocFragment[2]/body/div[3]/h2[2]/text().43",
            ["text"] = "Librarian" }
    } }"#,
    )
    .unwrap();
    common::init_engine();
    let mut reader = BookSession::open(&book, OpenOptions::new(data.path())).unwrap();
    let annotation = reader.annotations().unwrap().remove(0);
    assert_eq!(reader.annotation_text(annotation.id).unwrap(), "Librarian");
}

#[test]
fn imports_a_koreader_sidecar_from_a_copy_and_leaves_the_original_alone() {
    let Some(lib) = common::library_dir() else {
        eprintln!("skipped: ~/kaia-test-library is missing");
        return;
    };
    let book_src = lib.join("84.epub");
    let sdr_src = lib.join("84.sdr");
    if !book_src.exists() || !sdr_src.exists() {
        eprintln!("skipped: 84.epub or 84.sdr is missing");
        return;
    }
    let before = file_bytes(&sdr_src);
    let work = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let book = common::copy_book_with_sidecar(&book_src, work.path());

    common::init_engine();
    let mut opts = OpenOptions::new(data.path());
    opts.sidecars = vec![SidecarLocation::BesideBook];
    let mut s = BookSession::open(&book, opts.clone()).expect("open with sidecar");
    let report = s.import_report().expect("sidecar was imported").clone();
    assert_eq!(
        report.source,
        work.path().join("84.sdr").join("metadata.epub.lua")
    );
    assert!(report.annotations >= 2, "84.sdr has several annotations");
    assert!(report.position, "84.sdr has last_xpointer");
    assert!(report.unresolved <= report.annotations);

    // Layout keys from KOReader: copt_font_size 22, copt_h_page_margins {10, 10}.
    assert_eq!(s.layout().font_size, 22.0);
    assert_eq!(s.layout().margins.left, 10.0);
    assert_eq!(s.layout().margins.right, 10.0);
    assert_eq!(s.layout().font_family.as_deref(), Some("Noto Serif"));

    let all = s.annotations().unwrap();
    assert_eq!(all.len(), report.annotations);
    assert!(
        all.iter().all(|a| a.datetime.len() == 19),
        "KOReader datetimes are kept"
    );
    let orphans = all.iter().filter(|a| a.orphaned).count();
    assert_eq!(
        orphans, report.unresolved,
        "unresolved positions are marked orphaned"
    );
    assert!(s.page_count() > 1);
    s.save().unwrap();
    drop(s);

    // The Kaia state takes over from the sidecar. The sidecar is no longer read.
    fs::remove_dir_all(work.path().join("84.sdr")).unwrap();
    let s2 = BookSession::open(&book, opts).expect("reopen without sidecar");
    assert!(s2.import_report().is_none());
    assert_eq!(s2.layout().font_size, 22.0);

    // KOReader's files stay as they were.
    assert_eq!(file_bytes(&sdr_src), before, "the library sidecar is unchanged");
}

#[test]
fn sidecar_in_the_hash_location_is_found() {
    let Some(lib) = common::library_dir() else {
        eprintln!("skipped: ~/kaia-test-library is missing");
        return;
    };
    let book_src = lib.join("84.epub");
    let sdr_src = lib.join("84.sdr").join("metadata.epub.lua");
    if !book_src.exists() || !sdr_src.exists() {
        eprintln!("skipped: 84 files are missing");
        return;
    }
    let work = tempfile::tempdir().unwrap();
    let hash_root = work.path().join("hashdocsettings");
    let md5 = kaia_book::partial_md5(&book_src).unwrap();
    let dir = hash_root.join(&md5[..2]).join(format!("{md5}.sdr"));
    fs::create_dir_all(&dir).unwrap();
    fs::copy(&sdr_src, dir.join("metadata.epub.lua")).unwrap();
    let book = common::copy_into(&book_src, work.path());

    let data = tempfile::tempdir().unwrap();
    common::init_engine();
    let mut opts = OpenOptions::new(data.path());
    opts.sidecars = vec![SidecarLocation::HashDocSettingsDir(hash_root)];
    let s = BookSession::open(&book, opts).expect("open");
    assert!(s.import_report().is_some(), "found under the hash directory");
}

/// A PDF with a KOReader sidecar next to it. The sidecar holds one point
/// highlight (fits a PDF) and one xpointer entry (does not).
fn pdf_with_sidecar(work: &Path) -> std::path::PathBuf {
    let book = common::copy_into(&common::repo_file("paper.pdf"), work);
    let sdr = work.join("paper.sdr");
    fs::create_dir_all(&sdr).unwrap();
    let lua = r#"return {
        ["annotations"] = {
            [1] = { ["chapter"] = "Abstract", ["color"] = "yellow", ["datetime"] = "2026-10-01 10:00:00",
                    ["drawer"] = "lighten", ["page"] = 1, ["note"] = "start",
                    ["pos0"] = { ["page"] = 1, ["x"] = 72.0, ["y"] = 60.0, ["zoom"] = 1.0, ["rotation"] = 0 },
                    ["pos1"] = { ["page"] = 1, ["x"] = 200.0, ["y"] = 60.0, ["zoom"] = 1.0, ["rotation"] = 0 },
                    ["text"] = "Statistica" },
            [2] = { ["datetime"] = "2026-10-02 10:00:00", ["page"] = "/body/DocFragment[2]/body/div/p[1].0" },
        },
        ["last_page"] = 3,
        ["percent_finished"] = 0.1,
        ["zoom_mode"] = "width",
        ["summary"] = { ["status"] = "reading", ["rating"] = 3 },
    }"#;
    fs::write(sdr.join("metadata.pdf.lua"), lua).unwrap();
    book
}

#[test]
fn pdf_sidecar_gives_page_zoom_and_point_highlights() {
    let work = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let book = pdf_with_sidecar(work.path());
    common::init_engine();
    let mut opts = OpenOptions::new(data.path());
    opts.sidecars = vec![SidecarLocation::BesideBook];
    let mut s = BookSession::open(&book, opts).expect("open");
    let report = s.import_report().expect("imported").clone();
    assert_eq!(report.annotations, 1);
    assert_eq!(report.skipped, 1, "the xpointer entry does not fit a PDF");
    assert_eq!(report.status.as_deref(), Some("reading"));
    assert_eq!(report.rating, Some(3));
    assert_eq!(s.current().page, 2, "last_page 3 is page index 2");
    assert_eq!(s.zoom(), ZoomMode::FitWidth);
    assert_eq!(s.status(), Some("reading"));
    assert_eq!(s.rating(), Some(3));
    let all = s.annotations().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].note.as_deref(), Some("start"));
    assert_eq!(all[0].page, Location::Page(0));
    assert!(matches!(all[0].pos0, Some(Pos::Point { page: 0, .. })));
    // The range runs in reading order from the nearest character to the start point
    // to the nearest one to the end point.
    assert_eq!(
        s.annotation_text(all[0].id).unwrap(),
        "Statistica Sinica 20 (2010), 000"
    );
}

#[test]
fn stale_state_is_flagged_when_the_file_grows() {
    let work = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let book = common::copy_into(&common::repo_file("paper.pdf"), work.path());
    {
        let mut s = common::open(&book, &data);
        s.save().unwrap();
    }
    assert!(!common::open(&book, &data).state_is_stale());
    let mut bytes = fs::read(&book).unwrap();
    bytes.extend_from_slice(b"\n% appended after the copy was made\n");
    fs::write(&book, bytes).unwrap();
    let s = common::open(&book, &data);
    assert!(s.state_is_stale(), "same partial MD5, different size");
}

#[test]
fn unreadable_state_file_is_preserved_and_the_book_still_opens() {
    let work = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let book = common::copy_into(&common::repo_file("paper.pdf"), work.path());
    let md5 = kaia_book::partial_md5(&book).unwrap();
    let dir = data.path().join("kaia").join("books");
    fs::create_dir_all(&dir).unwrap();
    let state = dir.join(format!("{md5}.json"));
    fs::write(&state, b"{ this is not json").unwrap();
    let s = common::open(&book, &data);
    assert_eq!(s.current().page, 0);
    assert_eq!(fs::read(&state).unwrap(), b"{ this is not json");
    assert!(!dir.join(format!("{md5}.recovered.json")).exists());
}
