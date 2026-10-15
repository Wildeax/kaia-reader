//! The in-book search history: the recent searches of each book, newest first.
//!
//! The lists live in one file, `<data dir>/kaia/search-history.json`, keyed by
//! the book's partial MD5. They are not in the book's state file. An open book
//! session rewrites that file from memory on every save, so a second writer
//! would lose the list. This module is the only writer of the file. Erase all
//! Kaia data removes it with the rest of the data folder. Reset settings keeps it.

use std::io;
use std::path::{Path, PathBuf};

use kaia_events::{Key, Settings, Table, Value};

/// The most searches kept for each book.
pub const MAX_SEARCHES: usize = 10;

const FORMAT: &str = "kaia-search-history";
const VERSION: i64 = 1;

/// The file that holds the searches of every book.
pub fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("kaia").join("search-history.json")
}

/// The recent searches of the book `partial_md5`, newest first. A missing or
/// unreadable file gives an empty list.
pub fn load(path: &Path, partial_md5: &str) -> Vec<String> {
    let Some(books) = read_books(path) else {
        return Vec::new();
    };
    match books.get(&Key::from(partial_md5)) {
        Some(Value::Table(list)) => list
            .values()
            .filter_map(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .take(MAX_SEARCHES)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// Records a search of the book `partial_md5`. The search moves to the front of
/// the book's list. A blank search changes nothing.
pub fn record(path: &Path, partial_md5: &str, search: &str) -> io::Result<()> {
    if search.trim().is_empty() {
        return Ok(());
    }
    let mut list = load(path, partial_md5);
    push(&mut list, search);
    store(path, partial_md5, &list)
}

/// Empties the list of the book `partial_md5`. Other books keep theirs.
pub fn clear(path: &Path, partial_md5: &str) -> io::Result<()> {
    store(path, partial_md5, &[])
}

/// Puts `search` first in `list`. The text is trimmed, and a blank text does
/// nothing. An entry with the same text, without regard to case, is removed
/// first, so the list has no repeats and keeps the new spelling. The list keeps
/// the newest `MAX_SEARCHES` entries.
pub fn push(list: &mut Vec<String>, search: &str) {
    let search = search.trim();
    if search.is_empty() {
        return;
    }
    let folded = search.to_lowercase();
    list.retain(|old| old.to_lowercase() != folded);
    list.insert(0, search.to_string());
    list.truncate(MAX_SEARCHES);
}

/// The `books` table of the file. A file that is missing, damaged or of another
/// format gives None.
fn read_books(path: &Path) -> Option<Table> {
    let settings = match Settings::read_exact(path) {
        Ok(Some(settings)) => settings,
        _ => return None,
    };
    if settings.read("format").as_str() != Some(FORMAT) || settings.read("version").as_i64() != Some(VERSION)
    {
        return None;
    }
    settings.read("books").as_table().cloned()
}

/// Writes the list of one book and keeps the lists of the other books. An empty
/// list removes the book's entry.
fn store(path: &Path, partial_md5: &str, list: &[String]) -> io::Result<()> {
    let mut books = read_books(path).unwrap_or_default();
    let key = Key::from(partial_md5);
    if list.is_empty() {
        books.remove(&key);
    } else {
        books.insert(key, Value::array(list.iter().map(|s| Value::from(s.as_str()))));
    }
    let mut root = Table::new();
    root.insert("format".into(), Value::from(FORMAT));
    root.insert("version".into(), Value::from(VERSION));
    root.insert("books".into(), Value::Table(books));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    Settings::from_table(root).flush(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_repeat_moves_to_the_front_without_a_duplicate() {
        let mut list = words(&["Juliet", "Romeo"]);
        push(&mut list, "romeo");
        assert_eq!(list, words(&["romeo", "Juliet"]));
    }

    #[test]
    fn blank_searches_are_ignored_and_text_is_trimmed() {
        let mut list = words(&["Romeo"]);
        push(&mut list, "   ");
        push(&mut list, "");
        assert_eq!(list, words(&["Romeo"]));
        push(&mut list, "  Juliet ");
        assert_eq!(list, words(&["Juliet", "Romeo"]));
    }

    #[test]
    fn the_eleventh_search_drops_the_oldest() {
        let mut list = Vec::new();
        for i in 1..=11 {
            push(&mut list, &format!("query {i}"));
        }
        assert_eq!(list.len(), MAX_SEARCHES);
        assert_eq!(list.first().map(String::as_str), Some("query 11"));
        assert_eq!(list.last().map(String::as_str), Some("query 2"));
    }

    #[test]
    fn lists_are_per_book_and_survive_a_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = path(dir.path());
        assert!(load(&path, "aaaa").is_empty(), "no file, no searches");
        record(&path, "aaaa", "Romeo").unwrap();
        record(&path, "aaaa", "Juliet").unwrap();
        record(&path, "bbbb", "Leaves").unwrap();
        record(&path, "aaaa", "   ").unwrap();
        assert_eq!(load(&path, "aaaa"), words(&["Juliet", "Romeo"]));
        assert_eq!(load(&path, "bbbb"), words(&["Leaves"]));

        clear(&path, "aaaa").unwrap();
        assert!(load(&path, "aaaa").is_empty());
        assert_eq!(
            load(&path, "bbbb"),
            words(&["Leaves"]),
            "clearing one book keeps the other"
        );
    }

    #[test]
    fn a_damaged_file_reads_as_empty_and_the_next_search_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{ not json").unwrap();
        assert!(load(&path, "aaaa").is_empty());
        record(&path, "aaaa", "Romeo").unwrap();
        assert_eq!(load(&path, "aaaa"), words(&["Romeo"]));
    }
}
