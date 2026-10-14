//! Hyphenation patterns: the book language picks the pattern, the reader can
//! override it, and CREngine hyphenates long words at line ends with it. The
//! fixtures are EPUBs that the test writes, with long words in a narrow column.
//! The hyphen itself is drawn by CREngine and is not in the page text, so the
//! checks look for words split across lines.

#![cfg(feature = "cre")]

mod common;

use std::path::{Path, PathBuf};

use kaia_book::{BookSession, OpenOptions};
use kaia_doc::cre::CreDocument;
use kaia_doc::{Document, Layout};

/// A narrow column, so the long words reach a line end.
const NARROW_WIDTH: f32 = 420.0;
const HEIGHT: f32 = 1680.0;
const EM: f32 = 24.0;

const SPANISH: &str = "extraordinariamente desafortunadamente incomprensiblemente \
    electroencefalografista responsabilidades inconstitucionalidad";
const FRENCH: &str = "incompréhensiblement inconstitutionnellement extraordinairement \
    responsabilités anticonstitutionnellement";

/// Six paragraphs of the words, each word repeated, so every page has many
/// line ends that could take a hyphen. The first paragraph has the anchor of the table of contents.
fn body(words: &str) -> String {
    let paragraph = std::iter::repeat_n(words, 8).collect::<Vec<_>>().join(" ");
    let mut out = format!("<p id=\"top\">{paragraph}</p>\n");
    for _ in 0..5 {
        out.push_str(&format!("<p>{paragraph}</p>\n"));
    }
    out
}

/// Writes an EPUB whose `dc:language` is `language`.
fn fixture(dir: &Path, name: &str, language: &str, words: &str) -> PathBuf {
    let path = dir.join(name);
    common::epub::write_epub_in(&path, language, &body(words), &[("Text", "top")]);
    path
}

/// The number of words that CREngine splits across lines on the first pages,
/// with `dict` as the pattern. None turns hyphenation off.
///
/// CREngine draws the hyphen at a split, but the page text leaves it out. So a
/// split shows as a line that ends in a letter, followed by a line whose first
/// word finishes the word of `words`. A paragraph end does not join into a word,
/// so it does not count. The last line of each page has no next line.
fn split_word_line_ends(path: &Path, dict: Option<&str>, words: &str) -> usize {
    common::init_engine();
    let mut doc = CreDocument::open(path).expect("open with CREngine");
    doc.set_layout(&Layout::new(NARROW_WIDTH, HEIGHT, EM))
        .expect("apply the narrow layout");
    doc.set_hyphenation(dict).expect("set the pattern");
    let pages = doc.page_count().min(5) as u32;
    (0..pages)
        .map(|page| {
            let text = doc.page_text(page).expect("page text").text;
            let lines: Vec<&str> = text.lines().collect();
            lines
                .windows(2)
                .filter(|pair| {
                    let (line, next) = (pair[0], pair[1]);
                    if line.chars().last().is_none_or(char::is_whitespace) {
                        return false;
                    }
                    let head = line.split_whitespace().last().unwrap_or("");
                    let tail = next.split_whitespace().next().unwrap_or("");
                    let joined = format!("{head}{tail}");
                    words.split_whitespace().any(|word| word == joined)
                })
                .count()
        })
        .sum()
}

/// Opens a book with its own UI language, or none.
fn open_with_ui(path: &Path, data: &tempfile::TempDir, ui: Option<&str>) -> BookSession {
    common::init_engine();
    let mut opts: OpenOptions = common::options(data);
    opts.ui_language = ui.map(str::to_string);
    BookSession::open(path, opts).expect("open book")
}

#[test]
fn the_pattern_files_for_every_language_ship_in_the_engine_data() {
    let hyph = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../base/thirdparty/kpvcrlib/crengine/cr3gui/data/hyph");
    for lang in kaia_book::layout::HYPHENATION_LANGUAGES {
        assert!(
            hyph.join(lang.pattern).is_file(),
            "{} is missing from {}",
            lang.pattern,
            hyph.display()
        );
    }
}

#[test]
fn a_spanish_book_hyphenates_with_the_spanish_pattern() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "spanish.epub", "es", SPANISH);
    let s = common::open(&path, &data);
    assert_eq!(s.hyphenation_pattern(), Some("Spanish.pattern"));
    assert!(
        split_word_line_ends(&path, s.hyphenation_pattern(), SPANISH) > 0,
        "no word split at a line end"
    );
    assert_eq!(
        split_word_line_ends(&path, None, SPANISH),
        0,
        "word split with hyphenation off"
    );
}

#[test]
fn a_french_book_hyphenates_with_the_french_pattern() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "french.epub", "fr", FRENCH);
    let s = common::open(&path, &data);
    assert_eq!(s.hyphenation_pattern(), Some("French.pattern"));
    assert!(
        split_word_line_ends(&path, s.hyphenation_pattern(), FRENCH) > 0,
        "no word split at a line end"
    );
    assert_eq!(
        split_word_line_ends(&path, None, FRENCH),
        0,
        "word split with hyphenation off"
    );
}

#[test]
fn the_reader_override_switches_the_pattern_and_is_saved() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "spanish.epub", "es", SPANISH);
    let mut s = common::open(&path, &data);
    assert_eq!(s.hyphenation_pattern(), Some("Spanish.pattern"));

    let mut layout = s.layout().clone();
    layout.hyphenation_lang = Some("fr".to_string());
    s.set_layout(layout).expect("set the override");
    assert_eq!(s.hyphenation_pattern(), Some("French.pattern"));
    drop(s);

    let s = common::open(&path, &data);
    assert_eq!(s.layout().hyphenation_lang.as_deref(), Some("fr"));
    assert_eq!(s.hyphenation_pattern(), Some("French.pattern"));
}

#[test]
fn an_override_back_to_book_language_uses_the_book_pattern() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "french.epub", "fr", FRENCH);
    let mut s = common::open(&path, &data);
    let mut layout = s.layout().clone();
    layout.hyphenation_lang = Some("de".to_string());
    s.set_layout(layout.clone()).expect("set the override");
    assert_eq!(s.hyphenation_pattern(), Some("German.pattern"));
    layout.hyphenation_lang = None;
    s.set_layout(layout).expect("clear the override");
    assert_eq!(s.hyphenation_pattern(), Some("French.pattern"));
}

#[test]
fn an_unknown_book_language_falls_back_to_the_ui_language_then_english() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "unknown.epub", "tlh", SPANISH);

    let data = tempfile::tempdir().unwrap();
    let s = open_with_ui(&path, &data, Some("fr"));
    assert_eq!(s.hyphenation_pattern(), Some("French.pattern"));

    let data = tempfile::tempdir().unwrap();
    let s = open_with_ui(&path, &data, None);
    assert_eq!(s.hyphenation_pattern(), Some("English_US.pattern"));
}

#[test]
fn a_book_without_a_pattern_language_uses_the_ui_language() {
    let dir = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "japanese.epub", "ja", SPANISH);
    let data = tempfile::tempdir().unwrap();
    let s = open_with_ui(&path, &data, Some("de"));
    assert_eq!(s.hyphenation_pattern(), Some("German.pattern"));
}

#[test]
fn an_unknown_override_falls_back_to_the_book_language() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "spanish.epub", "es", SPANISH);
    let mut s = common::open(&path, &data);
    let mut layout = s.layout().clone();
    layout.hyphenation_lang = Some("xx-unknown".to_string());
    s.set_layout(layout).expect("set the override");
    assert_eq!(s.hyphenation_pattern(), Some("Spanish.pattern"));
}

#[test]
fn hyphenation_off_gives_no_pattern_whatever_the_language() {
    let dir = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let path = fixture(dir.path(), "french.epub", "fr", FRENCH);
    let mut s = common::open(&path, &data);
    let mut layout = s.layout().clone();
    layout.hyphenation = false;
    s.set_layout(layout).expect("turn hyphenation off");
    assert_eq!(s.hyphenation_pattern(), None);
}
