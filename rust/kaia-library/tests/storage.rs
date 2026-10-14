use std::fs;
use std::path::{Path, PathBuf};

use kaia_library::storage::{Source, Sources, StorageBook};
use kaia_library::Library;

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    db: PathBuf,
    library: Library,
    sources: Sources,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("kaia-storage-")
            .tempdir()
            .unwrap();
        let root = fs::canonicalize(dir.path()).unwrap().join("books");
        fs::create_dir_all(root.join("downloads")).unwrap();
        let db = dir.path().join("library.sqlite3");
        let library = Library::open(&db).unwrap();
        let sources = Sources {
            calibre_books: vec![root.join("downloads/calibre.epub")],
            downloads: root.join("downloads"),
        };
        Self {
            _dir: dir,
            root,
            db,
            library,
            sources,
        }
    }

    fn add(&mut self, relative: &str, bytes: &[u8]) -> StorageBook {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        self.index(&path)
    }

    fn index(&mut self, path: &Path) -> StorageBook {
        // Insert the same fields as the scanner without starting a document engine.
        let bytes = fs::metadata(path).unwrap().len();
        let md5 = kaia_library::partial_md5(path).unwrap();
        let conn = rusqlite::Connection::open(&self.db).unwrap();
        conn.execute("INSERT INTO books (path, size, mtime, partial_md5, format, added_at) VALUES (?1, ?2, 0, ?3, 'epub', 0)", rusqlite::params![path.to_str().unwrap(), bytes as i64, md5]).unwrap();
        StorageBook {
            id: conn.last_insert_rowid(),
            path: path.to_owned(),
            bytes,
            partial_md5: md5,
        }
    }
}

#[test]
fn sizes_and_counts_follow_calibre_then_downloads_then_other() {
    let mut f = Fixture::new();
    f.add("downloads/calibre.epub", &[1; 12]);
    f.add("downloads/opds.epub", &[2; 23]);
    f.add("local.epub", &[3; 34]);
    f.add("local-two.epub", &[4; 45]);
    let missing = f.add("gone.epub", &[5; 100]);
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .execute("UPDATE books SET missing_since = 1 WHERE id = ?1", [missing.id])
        .unwrap();
    fs::write(f.root.join("unindexed.epub"), [9; 200]).unwrap();
    let groups = f.library.storage_groups(&f.sources).unwrap();
    let sizes: Vec<_> = groups
        .iter()
        .map(|g| (g.source, g.books.len(), g.bytes))
        .collect();
    assert_eq!(
        sizes,
        vec![
            (Source::Calibre, 1, 12),
            (Source::Opds, 1, 23),
            (Source::Other, 2, 79)
        ]
    );
}

#[test]
fn delete_all_removes_only_the_confirmed_source_and_skips_open_books() {
    let mut f = Fixture::new();
    let calibre = f.add("downloads/calibre.epub", b"calibre");
    let opds = f.add("downloads/opds.epub", b"opds");
    let open = f.add("downloads/open.epub", b"open");
    let other = f.add("other.epub", b"other");
    let unindexed = f.root.join("downloads/unindexed.epub");
    fs::write(&unindexed, b"unindexed").unwrap();
    let group = f.library.storage_groups(&f.sources).unwrap().remove(1);
    let late = f.add("downloads/late.epub", b"late");
    let report = f
        .library
        .delete_storage_group(&group, &f.sources, std::slice::from_ref(&f.root), |b| {
            Ok(b.id == open.id)
        });
    assert_eq!((report.deleted, report.failed, report.skipped_open), (1, 0, 1));
    assert!(!opds.path.exists());
    assert!(f.library.book(opds.id).unwrap().is_none());
    for path in [&calibre.path, &open.path, &other.path, &unindexed, &late.path] {
        assert!(path.exists(), "{}", path.display());
    }
}

#[test]
fn deletion_refuses_outside_hidden_directories_and_changed_files() {
    let mut f = Fixture::new();
    let outside = f.add("../outside.epub", b"outside");
    let mut refused = vec![outside];
    for folder in [".hidden", ".kobo", ".kaia", ".adds", "visible/.hidden"] {
        refused.push(f.add(&format!("{folder}/book.epub"), b"hidden"));
    }
    let changed = f.add("changed.epub", b"before");
    fs::write(&changed.path, b"after!").unwrap();
    refused.push(changed);
    let directory = f.add("directory.epub", b"file");
    fs::remove_file(&directory.path).unwrap();
    fs::create_dir(&directory.path).unwrap();
    refused.push(directory);
    for book in refused {
        assert!(f
            .library
            .delete_indexed_file(&book, std::slice::from_ref(&f.root))
            .is_err());
        assert!(book.path.exists());
        assert!(f.library.book(book.id).unwrap().is_some());
    }
}

#[cfg(unix)]
#[test]
fn deletion_refuses_symlink_files_and_parent_directories() {
    use std::os::unix::fs::symlink;
    let mut f = Fixture::new();
    let outside = f.root.parent().unwrap().join("outside");
    fs::create_dir(&outside).unwrap();
    let target = outside.join("target.epub");
    fs::write(&target, b"keep").unwrap();
    let link = f.root.join("link.epub");
    symlink(&target, &link).unwrap();
    let book = f.index(&link);
    assert!(f
        .library
        .delete_indexed_file(&book, std::slice::from_ref(&f.root))
        .is_err());
    symlink(&outside, f.root.join("folder")).unwrap();
    let book = f.index(&f.root.join("folder/target.epub"));
    assert!(f
        .library
        .delete_indexed_file(&book, std::slice::from_ref(&f.root))
        .is_err());
    let canonical_book = f.index(&target);
    assert!(f
        .library
        .delete_indexed_file(&canonical_book, std::slice::from_ref(&f.root))
        .is_err());
    assert_eq!(fs::read(&target).unwrap(), b"keep");
    assert!(link.is_symlink());
}

#[test]
fn changed_index_row_and_changed_source_need_another_confirmation() {
    let mut f = Fixture::new();
    let book = f.add("downloads/book.epub", b"book");
    let group = f.library.storage_groups(&f.sources).unwrap().remove(1);
    f.sources.calibre_books.push(book.path.clone());
    let report = f
        .library
        .delete_storage_group(&group, &f.sources, std::slice::from_ref(&f.root), |_| Ok(false));
    assert_eq!((report.deleted, report.failed), (0, 1));
    rusqlite::Connection::open(&f.db)
        .unwrap()
        .execute("UPDATE books SET size = size + 1 WHERE id = ?1", [book.id])
        .unwrap();
    assert!(f
        .library
        .delete_indexed_file(&book, std::slice::from_ref(&f.root))
        .is_err());
    assert!(book.path.exists());
}
