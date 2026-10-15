//! PDF exports reopened and rendered through the reader's MuPDF backend.

use std::path::{Path, PathBuf};

use kaia_app::surfaces::notes::{
    data::{BookNotes, Location, Note},
    export,
};
use kaia_doc::{Document, Format, MupdfDocument};
use kaia_gfx::{golden::assert_matches_golden, PixelFormat};

/// The bundled fonts directory. MuPDF also loads its fallback faces from
/// FONTDIR, which defaults to `./fonts` and is missing in the test directory,
/// so point it here unless the caller set it.
fn fonts() -> PathBuf {
    let fonts = std::env::var_os("KAIA_RESOURCES")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources"))
        .join("fonts");
    if std::env::var_os("FONTDIR").is_none() {
        std::env::set_var("FONTDIR", &fonts);
    }
    fonts
}

fn fixture(id: &str, title: &str, chapter: &str, quote: &str, note: &str) -> BookNotes {
    BookNotes {
        md5: id.into(),
        title: title.into(),
        author: Some("Ana García".into()),
        library_id: None,
        latest: String::new(),
        ink_counts: Vec::new(),
        notes: vec![Note {
            id: 1,
            sticker: None,
            text: Some(quote.into()),
            note: Some(note.into()),
            chapter: Some(chapter.into()),
            pageno: Some(12),
            location: Location::Page(11),
            datetime: "2026-10-14 09:30:00".into(),
            orphaned: false,
            highlight: true,
            bookmark: false,
        }],
    }
}

fn text(doc: &MupdfDocument) -> String {
    (0..doc.page_count())
        .map(|page| doc.page_text(page as u32).unwrap().text)
        .collect::<Vec<_>>()
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn pdf_books_round_trip_and_first_page_golden() {
    let dir = tempfile::tempdir().unwrap();
    let non_latin = if fonts().join("noto/NotoSansCJKsc-Regular.otf").is_file() {
        "読書の記録"
    } else {
        "Σημείωση"
    };
    let books = vec![
        fixture(
            "english",
            "A reader's notebook",
            "Chapter one",
            "A quiet room and a book.",
            "Return to this passage.",
        ),
        fixture(
            "spanish",
            "Lecturas de otoño",
            "Capítulo dos",
            "El corazón recuerda la canción.",
            "Qué alegría leer aquí.",
        ),
        fixture(
            "cjk",
            "Reading across languages",
            "Chapter three",
            "Keep a record of each reading.",
            non_latin,
        ),
    ];
    let paths = export::export_pdf(dir.path(), &books, &fonts()).unwrap();
    assert_eq!(paths.len(), books.len());
    for (book, path) in books.iter().zip(&paths) {
        assert_eq!(path.parent(), Some(dir.path().join("exports").as_path()));
        let doc = MupdfDocument::open(path, Format::Pdf).unwrap();
        assert!(doc.page_count() >= 1);
        let content = text(&doc);
        for expected in [
            &book.title,
            book.author.as_ref().unwrap(),
            book.notes[0].chapter.as_ref().unwrap(),
            book.notes[0].text.as_ref().unwrap(),
            book.notes[0].note.as_ref().unwrap(),
            "Page 12",
            "2026-10-14 09:30:00",
        ] {
            assert!(content.contains(expected), "missing {expected:?}: {content}");
        }
        let page = doc.render(0, 1.5, PixelFormat::Rgb24).unwrap();
        assert!(page.data().iter().any(|pixel| *pixel < 100));
        if book.md5 == "english" {
            let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/notes-pdf.png");
            assert_matches_golden(&page, &golden, 3);
        }
    }
    assert_eq!(export::export_pdf(dir.path(), &books, &fonts()).unwrap(), paths);
    assert_eq!(std::fs::read_dir(dir.path().join("exports")).unwrap().count(), 3);
}

#[test]
fn pdf_wraps_and_paginates_without_losing_notes() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = fixture(
        "long",
        "Long notebook",
        "Chapter one",
        "A highlight with enough words to wrap across several lines on the page and keep its ending visible.",
        "A note below the highlight.",
    );
    let template = book.notes[0].clone();
    book.notes = (0..65)
        .map(|i| Note {
            chapter: Some(format!("Chapter {}", i / 10 + 1)),
            note: Some(format!("Unique note {i}.")),
            pageno: Some(i + 1),
            ..template.clone()
        })
        .collect();
    let paths = export::export_pdf(dir.path(), std::slice::from_ref(&book), &fonts()).unwrap();
    let doc = MupdfDocument::open(&paths[0], Format::Pdf).unwrap();
    assert!(doc.page_count() > 1);
    let content = text(&doc);
    for note in &book.notes {
        for expected in [
            note.chapter.as_ref().unwrap(),
            note.text.as_ref().unwrap(),
            note.note.as_ref().unwrap(),
        ] {
            assert!(content.contains(expected), "missing {expected:?}");
        }
    }
    for page in 0..doc.page_count() {
        let page_text = doc.page_text(page as u32).unwrap();
        assert_eq!(
            page_text.text.lines().last(),
            Some((page + 1).to_string().as_str())
        );
        for ch in &page_text.chars {
            assert!(
                ch.bbox.x0 >= 35.0 && ch.bbox.x1 <= 385.0,
                "outside page margin: {ch:?}"
            );
            assert!(
                ch.bbox.y0 >= 33.0 && ch.bbox.y1 <= 585.0,
                "outside page margin: {ch:?}"
            );
        }
    }
}

#[test]
fn pdf_escapes_markup_and_preserves_previous_file_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let book = fixture(
        "../escape",
        "<Book & notes>",
        "<Chapter>",
        "<img src='file:///private'> & text",
        "Use < and > literally.",
    );
    let paths = export::export_pdf(dir.path(), std::slice::from_ref(&book), &fonts()).unwrap();
    let original = std::fs::read(&paths[0]).unwrap();
    let doc = MupdfDocument::open(&paths[0], Format::Pdf).unwrap();
    assert!(text(&doc).contains("<img src='file:///private'> & text"));
    assert!(export::export_pdf(dir.path(), &[book], &dir.path().join("missing-fonts")).is_err());
    assert_eq!(std::fs::read(&paths[0]).unwrap(), original);
    assert_eq!(std::fs::read_dir(dir.path().join("exports")).unwrap().count(), 1);
}

#[test]
fn pdf_preserves_long_positions_and_notes_without_quotes() {
    let dir = tempfile::tempdir().unwrap();
    let mut book = fixture(
        "position",
        "Notes without chapters",
        "",
        "",
        "First line.\nSecond line.",
    );
    book.author = None;
    book.notes[0].chapter = None;
    book.notes[0].text = None;
    book.notes[0].pageno = None;
    let position = format!("/body/{}/text.0", "DocFragment[1]/".repeat(30));
    book.notes[0].location = Location::Xpointer(position.clone());
    book.ink_counts.push((7, 4));
    let paths = export::export_pdf(dir.path(), &[book], &fonts()).unwrap();
    let doc = MupdfDocument::open(&paths[0], Format::Pdf).unwrap();
    let content = text(&doc);
    assert!(content.contains("First line. Second line."));
    assert!(content.contains("Page 7: 4 ink strokes"));
    assert!(content.split_whitespace().collect::<String>().contains(&position));
}

#[test]
fn pdf_writer_catches_output_errors_and_releases_its_context() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing/output.pdf");
    let error = kaia_mupdf_sys::write_story_pdf(&path, "<p>Text</p>", "", &fonts()).unwrap_err();
    assert_eq!(error.kind, kaia_mupdf_sys::ErrorKind::Mupdf);
    assert!(!error.message.is_empty());
    assert!(!path.exists());
    let book = fixture("after-error", "After an error", "Chapter", "Highlight", "Note");
    let paths = export::export_pdf(dir.path(), &[book], &fonts()).unwrap();
    let doc = MupdfDocument::open(&paths[0], Format::Pdf).unwrap();
    assert!(text(&doc).contains("After an error"));
}
