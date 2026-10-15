//! Reads the annotations of every book from the per-book state files, without
//! opening a document.
//!
//! Format dependency: each file is `<data>/kaia/books/<partial md5>.json`, a
//! `kaia_events::Settings` file written by `kaia-book`. The format name and
//! version come from `kaia_book::state::{STATE_FORMAT, STATE_VERSION}`. The
//! annotation keys (`kaia_id`, `datetime`, `drawer`, `color`, `text`, `note`,
//! `chapter`, `pageno`, `page`, `orphaned`) follow KOReader's names. The reader
//! in `kaia-book` is crate-private, so this module parses the table itself.
//! A file that fails the check is skipped and never rewritten.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use kaia_book::state::{STATE_FORMAT, STATE_VERSION};
use kaia_events::{Settings, Value};
use kaia_i18n::{format as tr_format, tr};
use kaia_library::Book;

/// One highlight, note or bookmark, read from a state file.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    /// The annotation id within its book (`kaia_id`). 0 when the file has none.
    pub id: u64,
    /// UUID in the shared sticker store, when this entry is a sticker.
    pub sticker: Option<String>,
    /// The highlighted text. None for a bookmark or a note with no quote.
    pub text: Option<String>,
    /// The user's note text.
    pub note: Option<String>,
    pub chapter: Option<String>,
    /// Page number, 1-based, when the annotation carries one.
    pub pageno: Option<u32>,
    /// The position: an xpointer, or a 0-based page for fixed-layout books.
    pub location: Location,
    /// Creation time, `YYYY-MM-DD HH:MM:SS`.
    pub datetime: String,
    /// True when the position no longer resolves in the document.
    pub orphaned: bool,
    /// True for a highlight: the entry has a drawer style.
    pub highlight: bool,
    /// True for a bookmark: the entry has no drawer style.
    pub bookmark: bool,
}

/// Where an annotation points in the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Xpointer(String),
    /// 0-based page.
    Page(u32),
}

/// One book with at least one annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct BookNotes {
    /// Partial MD5 of the book file. Names the state file.
    pub md5: String,
    pub title: String,
    pub author: Option<String>,
    /// The library row of this book, when the library has the file.
    pub library_id: Option<i64>,
    /// Newest annotation time, `YYYY-MM-DD HH:MM:SS`. Used for the newest-first order.
    pub latest: String,
    /// The annotations in reading order.
    pub notes: Vec<Note>,
    /// Stroke counts by their original page number, starting at one.
    pub ink_counts: Vec<(u32, u64)>,
}

/// The filter the Notes surface applies to the lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    Highlights,
    Notes,
    Bookmarks,
    Stickers,
}

impl Filter {
    /// Filters in the order the chips show them.
    pub const ALL: [Filter; 5] = [
        Filter::All,
        Filter::Highlights,
        Filter::Notes,
        Filter::Bookmarks,
        Filter::Stickers,
    ];

    /// The name used in action ids: `notes:filter:<name>`.
    pub fn name(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Highlights => "highlights",
            Filter::Notes => "notes",
            Filter::Bookmarks => "bookmarks",
            Filter::Stickers => "stickers",
        }
    }

    pub fn parse(name: &str) -> Option<Filter> {
        Filter::ALL.into_iter().find(|f| f.name() == name)
    }

    /// The label of the chip.
    pub fn label(self) -> String {
        match self {
            Filter::All => tr("All"),
            Filter::Highlights => tr("Highlights"),
            Filter::Notes => tr("Notes"),
            Filter::Bookmarks => tr("Bookmarks"),
            Filter::Stickers => tr("Stickers"),
        }
    }

    pub fn matches(self, note: &Note) -> bool {
        match self {
            Filter::All => true,
            Filter::Highlights => note.highlight,
            Filter::Notes => {
                note.sticker.is_none() && note.note.as_deref().is_some_and(|n| !n.trim().is_empty())
            }
            Filter::Bookmarks => note.bookmark,
            Filter::Stickers => note.sticker.is_some(),
        }
    }
}

impl Note {
    /// The entry in the list: the quote for a highlight, the note for a note,
    /// and the chapter or page for a bookmark.
    pub fn primary(&self) -> String {
        if self.sticker.is_some() {
            if let Some(text) = self.note.as_deref().filter(|t| !t.trim().is_empty()) {
                return text.into();
            }
        }
        if let Some(text) = self.text.as_deref().filter(|t| !t.trim().is_empty()) {
            return text.to_string();
        }
        if let Some(note) = self.note.as_deref().filter(|n| !n.trim().is_empty()) {
            return note.to_string();
        }
        if self.sticker.is_some() {
            return tr("Sticker");
        }
        match &self.chapter {
            Some(chapter) if !chapter.is_empty() => tr_format(&tr("Bookmark in %1"), &[chapter.as_str()]),
            _ => tr("Bookmark"),
        }
    }

    /// The line under the entry: the kind, the page and the note text.
    pub fn secondary(&self) -> String {
        let kind = if self.sticker.is_some() {
            tr("Sticker")
        } else if self.note.as_deref() == Some("Shared to Notes") {
            tr("Shared to Notes")
        } else {
            match (self.highlight, self.bookmark) {
                (true, _) => tr("Highlight"),
                (false, true) => tr("Bookmark"),
                (false, false) => tr("Note"),
            }
        };
        let mut parts = vec![kind];
        if let Some(page) = self.pageno {
            parts.push(tr_format(&tr("page %1"), &[&page.to_string()]));
        }
        if let Some(chapter) = self.chapter.as_deref().filter(|c| !c.is_empty()) {
            parts.push(chapter.to_string());
        }
        if self.highlight {
            if let Some(note) = self.note.as_deref().filter(|n| !n.trim().is_empty()) {
                parts.push(tr_format(&tr("note: %1"), &[note]));
            }
        }
        if self.orphaned {
            parts.push(tr("needs repair"));
        }
        parts.join(" · ")
    }
}

/// The per-book state folder: `<data>/kaia/books`.
pub fn books_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("kaia").join("books")
}

/// Reads every state file under `data_dir` and keeps the books that have
/// annotations. `library` maps a partial MD5 to the library row of that book.
/// Books come newest first. Each book's notes are in reading order.
pub fn load(data_dir: &Path, library: &HashMap<String, Book>) -> Vec<BookNotes> {
    load_with_empty(data_dir, library)
        .into_iter()
        .filter(|b| !b.notes.is_empty() || !b.ink_counts.is_empty())
        .collect()
}

/// Includes empty books so an export can replace notes that the user removed.
pub fn load_with_empty(data_dir: &Path, library: &HashMap<String, Book>) -> Vec<BookNotes> {
    let mut books: Vec<BookNotes> = fs::read_dir(books_dir(data_dir))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| read_book(&path, library))
        .collect();
    if data_dir.join("kaia/kaia.sqlite3").exists() {
        if let Ok(store) = kaia_storage::Store::open(&data_dir.join("kaia")) {
            if let Ok(counts) = store.ink_counts() {
                for (doc, page, count) in counts {
                    let index = books
                        .iter()
                        .position(|b| b.md5 == doc.partial_md5)
                        .unwrap_or_else(|| {
                            let row = library.get(&doc.partial_md5);
                            books.push(BookNotes {
                                title: row
                                    .and_then(|b| b.title.clone())
                                    .or(doc.title)
                                    .unwrap_or_else(|| doc.partial_md5.clone()),
                                md5: doc.partial_md5,
                                author: row.and_then(|b| b.authors.clone()),
                                library_id: row.map(|b| b.id),
                                latest: String::new(),
                                notes: Vec::new(),
                                ink_counts: Vec::new(),
                            });
                            books.len() - 1
                        });
                    books[index].ink_counts.push((page + 1, count));
                }
            }
            if let Ok(stickers) = store.list_all() {
                for sticker in stickers {
                    let md5 = &sticker.doc.partial_md5;
                    let index = books.iter().position(|b| &b.md5 == md5).unwrap_or_else(|| {
                        let row = library.get(md5);
                        books.push(BookNotes {
                            md5: md5.clone(),
                            title: row
                                .and_then(|b| b.title.clone())
                                .or(sticker.doc.title.clone())
                                .unwrap_or_else(|| md5.clone()),
                            author: row.and_then(|b| b.authors.clone()),
                            library_id: row.map(|b| b.id),
                            latest: String::new(),
                            notes: Vec::new(),
                            ink_counts: Vec::new(),
                        });
                        books.len() - 1
                    });
                    let (location, text, pageno) = match &sticker.anchor {
                        kaia_storage::Anchor::Reflowable { locator, quote, .. } => (
                            Location::Xpointer(locator.clone()),
                            Some(quote.exact.clone()),
                            None,
                        ),
                        kaia_storage::Anchor::Fixed { page, .. } => {
                            (Location::Page(page.saturating_sub(1)), None, Some(*page))
                        }
                        kaia_storage::Anchor::Book {} => (Location::Page(0), None, None),
                    };
                    let datetime = super::export::timestamp(sticker.created_at);
                    let book = &mut books[index];
                    book.latest = book
                        .latest
                        .clone()
                        .max(super::export::timestamp(sticker.updated_at));
                    book.notes.push(Note {
                        id: 0,
                        sticker: sticker.id,
                        text,
                        note: sticker.text,
                        chapter: None,
                        pageno,
                        location,
                        datetime,
                        orphaned: sticker.state != kaia_storage::StickerState::Attached,
                        highlight: false,
                        bookmark: false,
                    });
                }
            }
        }
    }
    for book in &mut books {
        book.notes.sort_by(reading_order);
    }
    books.sort_by(|a, b| b.latest.cmp(&a.latest).then_with(|| a.title.cmp(&b.title)));
    books
}

/// Reads one state file. None when the file is not a readable Kaia state file.
pub fn read_book(path: &Path, library: &HashMap<String, Book>) -> Option<BookNotes> {
    let settings = Settings::open(path);
    let root = Value::Table(settings.table().clone());
    if root.get("format").as_str() != Some(STATE_FORMAT) {
        return None;
    }
    if root.get("version").as_i64()? > STATE_VERSION {
        return None;
    }
    let doc = root.get("doc");
    let md5 = doc.get("partial_md5").as_str()?.to_string();
    let library_book = library.get(&md5);
    let title = library_book
        .and_then(|b| b.title.clone())
        .filter(|t| !t.trim().is_empty())
        .or_else(|| doc.get("title").as_str().map(str::to_string))
        .unwrap_or_else(|| md5.clone());

    let mut notes: Vec<Note> = match root.get("annotations") {
        Value::Table(list) => list.values().filter_map(parse_note).collect(),
        _ => Vec::new(),
    };
    notes.sort_by(reading_order);
    let latest = notes.iter().map(|n| n.datetime.clone()).max().unwrap_or_default();
    Some(BookNotes {
        md5,
        title,
        author: library_book.and_then(|b| b.authors.clone()),
        library_id: library_book.map(|b| b.id),
        latest,
        notes,
        ink_counts: Vec::new(),
    })
}

/// Reads one annotation table. None when it has no usable position.
fn parse_note(v: &Value) -> Option<Note> {
    let location = match v.get("page") {
        Value::Str(xp) if !xp.is_empty() => Location::Xpointer(xp.clone()),
        Value::Number(n) if *n >= 1.0 && n.fract() == 0.0 => Location::Page(*n as u32 - 1),
        _ => return None,
    };
    let text_of = |key: &str| v.get(key).as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let drawer = text_of("drawer");
    Some(Note {
        sticker: None,
        id: v
            .get("kaia_id")
            .as_i64()
            .filter(|n| *n > 0)
            .map_or(0, |n| n as u64),
        text: text_of("text"),
        note: text_of("note"),
        chapter: text_of("chapter"),
        pageno: v.get("pageno").as_i64().filter(|n| *n > 0).map(|n| n as u32),
        location,
        datetime: text_of("datetime").unwrap_or_default(),
        orphaned: v.get("orphaned").as_bool().unwrap_or(false),
        highlight: drawer.is_some(),
        bookmark: drawer.is_none(),
    })
}

/// Reading order: by page first, then by xpointer in natural order (so
/// `DocFragment[2]` comes before `DocFragment[10]`), then by creation time.
fn reading_order(a: &Note, b: &Note) -> Ordering {
    let page = |n: &Note| match (&n.location, n.pageno) {
        (Location::Page(p), _) => Some(*p),
        (_, Some(p)) => Some(p.saturating_sub(1)),
        _ => None,
    };
    page(a)
        .unwrap_or(u32::MAX)
        .cmp(&page(b).unwrap_or(u32::MAX))
        .then_with(|| match (&a.location, &b.location) {
            (Location::Xpointer(x), Location::Xpointer(y)) => natural_cmp(x, y),
            _ => Ordering::Equal,
        })
        .then_with(|| a.datetime.cmp(&b.datetime))
}

/// Compares two strings, reading each run of digits as one number.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let mut a = a.chars().peekable();
    let mut b = b.chars().peekable();
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = take_number(&mut a);
                let nb = take_number(&mut b);
                match na.cmp(&nb) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (Some(x), Some(y)) => {
                match x.cmp(&y) {
                    Ordering::Equal => {}
                    other => return other,
                }
                a.next();
                b.next();
            }
        }
    }
}

fn take_number(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> u128 {
    let mut n: u128 = 0;
    while let Some(c) = chars.peek().copied().filter(|c| c.is_ascii_digit()) {
        n = n.saturating_mul(10).saturating_add(u128::from(c as u8 - b'0'));
        chars.next();
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_reads_numbers_as_numbers() {
        assert_eq!(
            natural_cmp("/body/DocFragment[2]", "/body/DocFragment[10]"),
            Ordering::Less
        );
        assert_eq!(natural_cmp("a.10", "a.9"), Ordering::Greater);
        assert_eq!(natural_cmp("same", "same"), Ordering::Equal);
    }

    #[test]
    fn filters_match_their_kind() {
        let highlight = Note {
            sticker: None,
            id: 1,
            text: Some("quote".into()),
            note: None,
            chapter: None,
            pageno: None,
            location: Location::Page(0),
            datetime: String::new(),
            orphaned: false,
            highlight: true,
            bookmark: false,
        };
        assert!(Filter::Highlights.matches(&highlight));
        assert!(!Filter::Notes.matches(&highlight));
        assert!(!Filter::Bookmarks.matches(&highlight));
        assert!(Filter::All.matches(&highlight));
    }
}
