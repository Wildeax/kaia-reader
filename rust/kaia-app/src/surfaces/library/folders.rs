//! The Folders view: the folder tree that holds the books. The tree starts at
//! the longest folder that holds every book, so the user never sees the file
//! system above the books.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use kaia_library::Book;

/// One row of a folder: a subfolder or a book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A subfolder, by its full path.
    Dir {
        path: PathBuf,
        name: String,
        books: usize,
    },
    /// A book, by its row id.
    Book { id: i64 },
}

/// The rows of folder `current`: subfolders first, then the books in it.
/// Books are listed by the caller's order, so the folder keeps the sort of the list.
pub fn entries(books: &[Book], current: &Path) -> Vec<Entry> {
    let mut dirs: BTreeSet<PathBuf> = BTreeSet::new();
    let mut files: Vec<i64> = Vec::new();
    let mut counts = std::collections::BTreeMap::<PathBuf, usize>::new();
    for book in books {
        let Some(parent) = book.path.parent() else {
            continue;
        };
        if parent == current {
            files.push(book.id);
        } else if let Ok(rest) = parent.strip_prefix(current) {
            if let Some(first) = rest.components().next() {
                let dir = current.join(first.as_os_str());
                *counts.entry(dir.clone()).or_insert(0) += 1;
                dirs.insert(dir);
            }
        }
    }
    let mut out: Vec<Entry> = dirs
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let books = counts.get(&path).copied().unwrap_or(0);
            Entry::Dir { path, name, books }
        })
        .collect();
    out.extend(files.into_iter().map(|id| Entry::Book { id }));
    out
}

/// The folder every book lies in: the longest common ancestor of the book folders.
pub fn root_of(books: &[Book]) -> Option<PathBuf> {
    let mut root = books.first()?.path.parent()?.to_path_buf();
    for book in books {
        let Some(parent) = book.path.parent() else {
            continue;
        };
        while !parent.starts_with(&root) {
            if !root.pop() {
                break;
            }
        }
    }
    Some(root)
}

/// The folder `current` relative to `root`, for the heading. Empty at the root.
pub fn label(root: &Path, current: &Path) -> String {
    match current.strip_prefix(root) {
        Ok(rest) if rest.as_os_str().is_empty() => "Books".to_string(),
        Ok(rest) => format!("Books / {}", rest.display()),
        Err(_) => current.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_library::Status;

    fn book(id: i64, path: &str) -> Book {
        Book {
            id,
            path: PathBuf::from(path),
            size: 1,
            mtime: 0,
            partial_md5: String::new(),
            title: None,
            authors: None,
            series: None,
            series_index: None,
            language: None,
            description: None,
            pages: None,
            format: "epub".into(),
            added_at: 0,
            last_opened_at: None,
            percent: 0.0,
            status: Status::New,
            extracted: true,
            failure: None,
        }
    }

    #[test]
    fn lists_subfolders_then_books_of_one_folder() {
        let books = vec![
            book(1, "/b/a.epub"),
            book(2, "/b/sci/x.epub"),
            book(3, "/b/sci/deep/y.epub"),
            book(4, "/b/z.pdf"),
        ];
        let root = root_of(&books).unwrap();
        assert_eq!(root, PathBuf::from("/b"));
        assert_eq!(
            entries(&books, &root),
            vec![
                Entry::Dir {
                    path: PathBuf::from("/b/sci"),
                    name: "sci".into(),
                    books: 2
                },
                Entry::Book { id: 1 },
                Entry::Book { id: 4 },
            ]
        );
        assert_eq!(
            entries(&books, Path::new("/b/sci")),
            vec![
                Entry::Dir {
                    path: PathBuf::from("/b/sci/deep"),
                    name: "deep".into(),
                    books: 1
                },
                Entry::Book { id: 2 },
            ]
        );
    }

    #[test]
    fn root_is_the_common_ancestor() {
        let books = vec![book(1, "/a/x/one.epub"), book(2, "/a/y/two.epub")];
        assert_eq!(root_of(&books), Some(PathBuf::from("/a")));
        assert_eq!(label(Path::new("/a"), Path::new("/a")), "Books");
        assert_eq!(label(Path::new("/a"), Path::new("/a/x")), "Books / x");
    }
}
