//! The Calibre metadata file that a wireless transfer leaves in the books folder.
//!
//! KOReader's Calibre plugin reads the same file (`plugins/calibre.koplugin/metadata.lua`
//! and `search.lua`). This module reads it without a network connection, then searches
//! and browses the books in it. Nothing here writes the file.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

/// The largest metadata file that is read, in bytes. A larger file is refused before
/// it is parsed. A book takes about 300 to 600 bytes in this file, so the limit holds
/// roughly 13,000 to 25,000 books.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;

/// The file names, in the order KOReader tries them. The visible name comes first.
pub const FILE_NAMES: [&str; 2] = ["metadata.calibre", ".metadata.calibre"];

/// Why the metadata file cannot be used.
#[derive(Debug)]
pub enum Error {
    /// The file cannot be opened or read.
    Io(io::Error),
    /// The file is larger than [`MAX_BYTES`]. It is not read.
    TooLarge,
    /// The file is not a JSON array, or its JSON is malformed or nested too deeply.
    Damaged,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "cannot read the Calibre metadata file: {e}"),
            Self::TooLarge => write!(f, "the Calibre metadata file is larger than {MAX_BYTES} bytes"),
            Self::Damaged => write!(f, "the Calibre metadata file is damaged"),
        }
    }
}

impl std::error::Error for Error {}

/// One book from the file. It holds the fields that KOReader's search uses, and the
/// publisher when the file has one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Book {
    /// The book path relative to the books folder, with `/` between folders.
    pub lpath: String,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub series: Option<String>,
    pub series_index: Option<f64>,
    pub tags: Vec<String>,
    pub publisher: Option<String>,
}

impl Book {
    /// The title, or the file name without its extension when the file has no title.
    pub fn display_title(&self) -> String {
        match &self.title {
            Some(title) => title.clone(),
            None => Path::new(&self.lpath)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(&self.lpath)
                .to_owned(),
        }
    }
}

/// A field that the browse views group books by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Author,
    Series,
    Tag,
}

/// Which fields a search reads, and whether it ignores case. The defaults search every
/// field and ignore case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub case_sensitive: bool,
    pub title: bool,
    pub authors: bool,
    pub series: bool,
    pub tags: bool,
    pub publisher: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            case_sensitive: false,
            title: true,
            authors: true,
            series: true,
            tags: true,
            publisher: true,
        }
    }
}

/// The books of one metadata file.
#[derive(Debug, Default)]
pub struct Metadata {
    books: Vec<Book>,
    skipped: usize,
}

impl Metadata {
    pub fn books(&self) -> &[Book] {
        &self.books
    }

    /// Entries that were left out: not an object, or no usable `lpath`.
    pub fn skipped(&self) -> usize {
        self.skipped
    }

    /// The indexes of the books that match `query`, sorted by title.
    ///
    /// The query splits at whitespace. A book matches when every word appears in at
    /// least one enabled field, so the words can match different fields. A blank query
    /// matches nothing.
    pub fn search(&self, query: &str, options: Options) -> Vec<usize> {
        let words: Vec<String> = query
            .split_whitespace()
            .map(|word| fold(word, options.case_sensitive))
            .collect();
        if words.is_empty() {
            return Vec::new();
        }
        let hits: Vec<usize> = self
            .books
            .iter()
            .enumerate()
            .filter(|(_, book)| {
                let fields = searched_fields(book, options);
                words
                    .iter()
                    .all(|word| fields.iter().any(|field| field.contains(word.as_str())))
            })
            .map(|(index, _)| index)
            .collect();
        self.sorted(hits)
    }

    /// The distinct values of `field`, with the number of books that have each one.
    /// A book counts once for each value, even when the file repeats it. The values
    /// sort by name, ignoring case.
    pub fn groups(&self, field: Field) -> Vec<(String, usize)> {
        let mut counts = std::collections::BTreeMap::<&str, usize>::new();
        for book in &self.books {
            for value in values(book, field).into_iter().collect::<BTreeSet<_>>() {
                *counts.entry(value).or_default() += 1;
            }
        }
        let mut groups: Vec<(String, usize)> = counts
            .into_iter()
            .map(|(value, count)| (value.to_owned(), count))
            .collect();
        groups.sort_by(|a, b| {
            a.0.to_lowercase()
                .cmp(&b.0.to_lowercase())
                .then_with(|| a.0.cmp(&b.0))
        });
        groups
    }

    /// The indexes of the books whose `field` holds exactly `value`, sorted by title.
    pub fn books_with(&self, field: Field, value: &str) -> Vec<usize> {
        let hits = self
            .books
            .iter()
            .enumerate()
            .filter(|(_, book)| values(book, field).contains(&value))
            .map(|(index, _)| index)
            .collect();
        self.sorted(hits)
    }

    fn sorted(&self, mut indexes: Vec<usize>) -> Vec<usize> {
        indexes.sort_by_cached_key(|&i| {
            let book = &self.books[i];
            (book.display_title().to_lowercase(), book.lpath.clone())
        });
        indexes
    }
}

/// Reads the metadata file in `root`. Returns `Ok(None)` when the folder has no file.
///
/// The file is read only when it is a regular file no larger than [`MAX_BYTES`]. A
/// named pipe or a directory is refused without being opened, so a bad path cannot
/// block the caller.
pub fn load(root: &Path) -> Result<Option<Metadata>, Error> {
    let Some(path) = FILE_NAMES
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.exists())
    else {
        return Ok(None);
    };
    let meta = fs::metadata(&path).map_err(Error::Io)?;
    if !meta.is_file() {
        return Err(Error::Damaged);
    }
    if meta.len() > MAX_BYTES {
        return Err(Error::TooLarge);
    }
    let mut bytes = Vec::new();
    File::open(&path)
        .map_err(Error::Io)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(Error::Io)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::TooLarge);
    }
    parse(&bytes).map(Some)
}

/// Parses the bytes of a metadata file. An empty file holds no books, as in KOReader.
/// Entries that are not objects, or that have no safe `lpath`, are counted in
/// [`Metadata::skipped`] and left out.
pub fn parse(bytes: &[u8]) -> Result<Metadata, Error> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::TooLarge);
    }
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Ok(Metadata::default());
    }
    let Value::Array(entries) = serde_json::from_slice::<Value>(bytes).map_err(|_| Error::Damaged)? else {
        return Err(Error::Damaged);
    };
    let mut metadata = Metadata::default();
    for entry in &entries {
        match book_from(entry) {
            Some(book) => metadata.books.push(book),
            None => metadata.skipped += 1,
        }
    }
    Ok(metadata)
}

fn book_from(entry: &Value) -> Option<Book> {
    let object = entry.as_object()?;
    let lpath = safe_lpath(object.get("lpath")?.as_str()?)?;
    Some(Book {
        lpath,
        title: text(object.get("title")),
        authors: texts(object.get("authors")),
        series: text(object.get("series")),
        series_index: object
            .get("series_index")
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite()),
        tags: texts(object.get("tags")),
        publisher: text(object.get("publisher")),
    })
}

/// A non-empty string without a root, a drive prefix, `.` or `..` parts.
fn safe_lpath(raw: &str) -> Option<String> {
    let mut parts = Vec::new();
    for component in Path::new(raw).components() {
        match component {
            std::path::Component::Normal(part) => parts.push(part.to_str()?),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// A list of strings. A single string counts as a list of one.
fn texts(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items.iter().filter_map(|item| text(Some(item))).collect(),
        other => text(other).into_iter().collect(),
    }
}

fn values(book: &Book, field: Field) -> Vec<&str> {
    match field {
        Field::Author => book.authors.iter().map(String::as_str).collect(),
        Field::Tag => book.tags.iter().map(String::as_str).collect(),
        Field::Series => book.series.as_deref().into_iter().collect(),
    }
}

/// The enabled fields of `book`, folded for comparison. A book without a title has
/// no title to search.
fn searched_fields(book: &Book, options: Options) -> Vec<String> {
    let fold_with = |text: &str| fold(text, options.case_sensitive);
    let mut fields = Vec::new();
    if options.title {
        fields.extend(book.title.as_deref().map(fold_with));
    }
    if options.authors {
        fields.extend(book.authors.iter().map(|a| fold_with(a)));
    }
    if options.series {
        fields.extend(book.series.as_deref().map(fold_with));
    }
    if options.tags {
        fields.extend(book.tags.iter().map(|t| fold_with(t)));
    }
    if options.publisher {
        fields.extend(book.publisher.as_deref().map(fold_with));
    }
    fields
}

fn fold(text: &str, case_sensitive: bool) -> String {
    if case_sensitive {
        text.to_owned()
    } else {
        text.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    const FIXTURE: &str = r#"[
        {"uuid":"u1","lpath":"Shakespeare, William/Romeo and Juliet (1).epub",
         "title":"Romeo and Juliet","authors":["William Shakespeare"],
         "series":"Classics","series_index":1.0,"tags":["Drama","Tragedy"],
         "publisher":"Penguin","size":1200,"last_modified":"2026-01-02T03:04:05+00:00"},
        {"uuid":"u2","lpath":"Herbert, Frank/Dune (2).epub","title":"Dune",
         "authors":["Frank Herbert"],"series":"Dune","series_index":1,"tags":["Science fiction"]},
        {"uuid":"u3","lpath":"Ångström, Zoë/Café Stories.epub","title":"Café Stories",
         "authors":["Zoë Ångström","Émile Zola"],"tags":["Essays","Drama"],"series":null},
        {"uuid":"u4","lpath":"Loose/Notes.txt"},
        {"uuid":"u5","lpath":"Herbert, Frank/Dune Messiah (3).epub","title":"Dune Messiah",
         "authors":["Frank Herbert"],"series":"Dune","series_index":"2"},
        {"uuid":"u6","lpath":"../outside.epub","title":"Outside"},
        {"uuid":"u7","title":"No path"},
        "not a book",
        {"uuid":"u8","lpath":"/etc/passwd","title":"Absolute"}
    ]"#;

    fn fixture() -> Metadata {
        parse(FIXTURE.as_bytes()).unwrap()
    }

    fn titles(metadata: &Metadata, indexes: &[usize]) -> Vec<String> {
        indexes
            .iter()
            .map(|&i| metadata.books()[i].display_title())
            .collect()
    }

    fn temp_dir(name: &str) -> PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "kaia-calibre-metadata-{}-{}-{name}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_fields_and_skips_unusable_entries() {
        let metadata = fixture();
        assert_eq!(metadata.books().len(), 5);
        assert_eq!(metadata.skipped(), 4);
        let romeo = &metadata.books()[0];
        assert_eq!(romeo.lpath, "Shakespeare, William/Romeo and Juliet (1).epub");
        assert_eq!(romeo.authors, vec!["William Shakespeare"]);
        assert_eq!(romeo.tags, vec!["Drama", "Tragedy"]);
        assert_eq!(romeo.series.as_deref(), Some("Classics"));
        assert_eq!(romeo.series_index, Some(1.0));
        assert_eq!(romeo.publisher.as_deref(), Some("Penguin"));
    }

    #[test]
    fn missing_fields_become_empty_values() {
        let metadata = fixture();
        let notes = &metadata.books()[3];
        assert_eq!(notes.title, None);
        assert!(notes.authors.is_empty());
        assert!(notes.tags.is_empty());
        assert_eq!(notes.series, None);
        assert_eq!(notes.display_title(), "Notes");
        let dune = &metadata.books()[1];
        assert_eq!(dune.series_index, Some(1.0));
        // A string is not a number, so the index is dropped.
        let messiah = &metadata.books()[4];
        assert_eq!(messiah.series_index, None);
    }

    #[test]
    fn search_is_case_insensitive_for_non_ascii() {
        let metadata = fixture();
        assert_eq!(
            titles(&metadata, &metadata.search("CAFÉ", Options::default())),
            ["Café Stories"]
        );
        assert_eq!(
            titles(&metadata, &metadata.search("zoë", Options::default())),
            ["Café Stories"]
        );
        assert_eq!(
            titles(&metadata, &metadata.search("émile zola", Options::default())),
            ["Café Stories"]
        );
    }

    #[test]
    fn search_matches_all_words_across_fields() {
        let metadata = fixture();
        // "dune" is in the title, "herbert" in the author. Both words must appear.
        assert_eq!(
            titles(&metadata, &metadata.search("dune herbert", Options::default())),
            ["Dune", "Dune Messiah"]
        );
        assert_eq!(
            titles(&metadata, &metadata.search("dune messiah", Options::default())),
            ["Dune Messiah"]
        );
        assert!(metadata.search("dune zola", Options::default()).is_empty());
        assert!(metadata.search("   ", Options::default()).is_empty());
    }

    #[test]
    fn search_finds_series_tags_and_publisher() {
        let metadata = fixture();
        let by_tag = metadata.search("tragedy", Options::default());
        assert_eq!(titles(&metadata, &by_tag), ["Romeo and Juliet"]);
        let by_publisher = metadata.search("penguin", Options::default());
        assert_eq!(titles(&metadata, &by_publisher), ["Romeo and Juliet"]);
        let by_series = metadata.search("classics", Options::default());
        assert_eq!(titles(&metadata, &by_series), ["Romeo and Juliet"]);
    }

    #[test]
    fn search_switches_turn_fields_off() {
        let metadata = fixture();
        let no_tags = Options {
            tags: false,
            ..Options::default()
        };
        assert!(metadata.search("tragedy", no_tags).is_empty());
        let titles_only = Options {
            authors: false,
            series: false,
            tags: false,
            publisher: false,
            ..Options::default()
        };
        assert!(metadata.search("herbert", titles_only).is_empty());
        assert_eq!(
            titles(&metadata, &metadata.search("dune", titles_only)),
            ["Dune", "Dune Messiah"]
        );
    }

    #[test]
    fn case_sensitive_search_needs_the_same_case() {
        let metadata = fixture();
        let exact = Options {
            case_sensitive: true,
            ..Options::default()
        };
        assert!(metadata.search("DUNE", exact).is_empty());
        assert_eq!(
            titles(&metadata, &metadata.search("Dune", exact)),
            ["Dune", "Dune Messiah"]
        );
    }

    #[test]
    fn groups_count_books_once_per_value() {
        let metadata = fixture();
        // Accented letters are above ASCII in code point order, so they sort last.
        assert_eq!(
            metadata.groups(Field::Author),
            vec![
                ("Frank Herbert".to_owned(), 2),
                ("William Shakespeare".to_owned(), 1),
                ("Zoë Ångström".to_owned(), 1),
                ("Émile Zola".to_owned(), 1),
            ]
        );
        assert_eq!(
            metadata.groups(Field::Series),
            vec![("Classics".to_owned(), 1), ("Dune".to_owned(), 2)]
        );
        assert_eq!(
            metadata.groups(Field::Tag),
            vec![
                ("Drama".to_owned(), 2),
                ("Essays".to_owned(), 1),
                ("Science fiction".to_owned(), 1),
                ("Tragedy".to_owned(), 1),
            ]
        );
    }

    #[test]
    fn books_with_matches_the_value_exactly() {
        let metadata = fixture();
        assert_eq!(
            titles(&metadata, &metadata.books_with(Field::Author, "Frank Herbert")),
            ["Dune", "Dune Messiah"]
        );
        assert!(metadata.books_with(Field::Author, "Frank").is_empty());
        assert_eq!(
            titles(&metadata, &metadata.books_with(Field::Tag, "Drama")),
            ["Café Stories", "Romeo and Juliet"]
        );
    }

    #[test]
    fn empty_file_has_no_books() {
        let metadata = parse(b"  \n").unwrap();
        assert!(metadata.books().is_empty());
        assert_eq!(metadata.skipped(), 0);
    }

    #[test]
    fn damaged_json_is_an_error_not_a_panic() {
        let cases: [&[u8]; 5] = [
            b"[{\"lpath\":\"a.epub\"",
            b"{\"lpath\":\"a.epub\"}",
            b"\"just a string\"",
            b"null",
            b"[\x00\xff\xfe]",
        ];
        for bytes in cases {
            assert!(matches!(parse(bytes), Err(Error::Damaged)), "{bytes:?}");
        }
    }

    #[test]
    fn deeply_nested_json_is_refused() {
        let mut bytes = vec![b'['; 100_000];
        bytes.extend(vec![b']'; 100_000]);
        assert!(matches!(parse(&bytes), Err(Error::Damaged)));
    }

    #[test]
    fn load_reads_the_visible_or_hidden_file() {
        let dir = temp_dir("names");
        assert!(load(&dir).unwrap().is_none());
        fs::write(dir.join(".metadata.calibre"), FIXTURE).unwrap();
        assert_eq!(load(&dir).unwrap().unwrap().books().len(), 5);
        fs::write(dir.join("metadata.calibre"), b"[]").unwrap();
        assert!(load(&dir).unwrap().unwrap().books().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_refuses_large_files_without_reading_them() {
        let dir = temp_dir("large");
        let file = File::create(dir.join("metadata.calibre")).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        assert!(matches!(load(&dir), Err(Error::TooLarge)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_refuses_a_directory_in_place_of_the_file() {
        let dir = temp_dir("directory");
        fs::create_dir_all(dir.join("metadata.calibre")).unwrap();
        assert!(matches!(load(&dir), Err(Error::Damaged)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn load_reports_a_damaged_file() {
        let dir = temp_dir("damaged");
        fs::write(
            dir.join("metadata.calibre"),
            b"[{\"lpath\": \"Book.epub\", \"title\": ",
        )
        .unwrap();
        assert!(matches!(load(&dir), Err(Error::Damaged)));
        fs::remove_dir_all(dir).unwrap();
    }
}
