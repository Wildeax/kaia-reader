//! File manager operations over Kaia's index and data paths.
//!
//! A copy adds a new book to the index. A move keeps the book's row, so its
//! history, reading progress, status, cover and collections stay with it. Each
//! operation writes the destination in full, then updates the index, then
//! removes the source. When a step fails, the steps before it are undone, so
//! the source and its rows stay as they were.
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use kaia_lua::LibraryHost;

/// The file manager host for plugins. It keeps the library index at
/// `<data>/library.sqlite3` in step with the files it copies and moves.
pub struct Host {
    pub(crate) data: PathBuf,
    pub(crate) folder: PathBuf,
    pub(crate) open: Option<String>,
    pub(crate) show: Option<String>,
}

impl Host {
    /// A host for the data folder and the default book folder in `paths`. Each
    /// copy or move opens the index on its own.
    pub fn new(paths: &kaia_platform::Paths) -> Self {
        Self {
            data: paths.data_dir.clone(),
            folder: paths.books_dir.clone(),
            open: None,
            show: None,
        }
    }

    fn index(&self) -> Result<kaia_library::Library, String> {
        kaia_library::Library::open(&self.data.join("library.sqlite3")).map_err(|e| e.to_string())
    }

    /// Copies or moves `source` into `folder` and updates the index to match.
    fn transfer(&mut self, source: &str, folder: &str, moving: bool) -> Result<(), String> {
        if !Path::new(source).is_file() {
            return Err("Book does not exist".into());
        }
        let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
        let folder = fs::canonicalize(folder)
            .ok()
            .filter(|f| f.is_dir())
            .ok_or("Not a folder")?;
        let target = folder.join(source.file_name().ok_or("Book has no filename")?);
        // symlink_metadata also sees a dangling link, which exists() does not.
        if fs::symlink_metadata(&target).is_ok() {
            return Err("The destination already exists".into());
        }

        let mut index = self.index()?;
        let row = index.book_by_path(&source).map_err(|e| e.to_string())?;
        if index.book_by_path(&target).map_err(|e| e.to_string())?.is_some() {
            return Err(
                "The library has a book at the destination. Scan the library, then try again.".into(),
            );
        }

        let placed = Placed::write(&source, &target, moving)
            .map_err(|e| format!("The destination could not be written. {e}"))?;
        let indexed = match (&row, moving) {
            (Some(old), true) => index.move_book(old.id, &target),
            _ => index.index_file(&target),
        };
        let book = match indexed {
            Ok(book) => book,
            Err(error) => {
                // move_book changes nothing when it fails. index_file can leave
                // the row it added, so that row goes too.
                if row.is_none() || !moving {
                    if let Ok(Some(stray)) = index.book_by_path(&target) {
                        let _ = index.remove_book(stray.id);
                    }
                }
                return Err(match placed.undo(&source, &target) {
                    Ok(()) => format!("The library could not be updated. {error}"),
                    Err(undo) => format!(
                        "The library could not be updated. {error} The file could not be moved back. It is at {}. {undo}",
                        target.display()
                    ),
                });
            }
        };

        // A copy leaves the source in place. Only a move removes it, and only
        // after the index describes the destination.
        if moving && matches!(placed, Placed::Copied) {
            match fs::remove_file(&source) {
                Ok(()) => {}
                // Already gone, so the move has nothing left to do.
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    // The index must again say the book is at the source. Then
                    // the copy goes.
                    let restored = match &row {
                        Some(old) => index.move_book(old.id, &source).map(|_| ()),
                        None => index.remove_book(book.id).map(|_| ()),
                    };
                    return Err(match restored {
                        Ok(()) => match fs::remove_file(&target) {
                            Ok(()) => format!("The original file could not be removed. {error}"),
                            Err(e) => format!(
                                "The original file could not be removed. {error} The copy at {} stays. {e}",
                                target.display()
                            ),
                        },
                        Err(e) => format!(
                            "The original file could not be removed. {error} The library still lists the book at {}. {e}",
                            target.display()
                        ),
                    });
                }
            }
        }
        Ok(())
    }
}

/// How the destination was written. [`Placed::undo`] reverses the write.
enum Placed {
    /// The source was renamed onto the destination, so the source is gone.
    Renamed,
    /// The source stays. The destination is a copy.
    Copied,
}

impl Placed {
    /// Writes `target` in full. A move on one file system is a rename. Any other
    /// move is a copy, and the caller removes the source afterwards.
    fn write(source: &Path, target: &Path, moving: bool) -> io::Result<Placed> {
        if moving {
            match fs::rename(source, target) {
                Ok(()) => return Ok(Placed::Renamed),
                Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {}
                Err(e) => return Err(e),
            }
        }
        copy_into_place(source, target)?;
        Ok(Placed::Copied)
    }

    fn undo(self, source: &Path, target: &Path) -> io::Result<()> {
        match self {
            Placed::Renamed => fs::rename(target, source),
            Placed::Copied => fs::remove_file(target),
        }
    }
}

/// Copies `source` to `target` through a hidden temporary file in the same
/// folder. The copy is synced before it is renamed into place, so `target`
/// appears only when it is complete. A scan skips names that start with a dot.
fn copy_into_place(source: &Path, target: &Path) -> io::Result<()> {
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the destination has no name"))?;
    let temp = target.with_file_name(format!(".{}.kaia-part", name.to_string_lossy()));
    let result = write_copy(source, &temp).and_then(|()| fs::rename(&temp, target));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn write_copy(source: &Path, temp: &Path) -> io::Result<()> {
    let mut from = File::open(source)?;
    let modified = from.metadata()?.modified()?;
    let mut to = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(temp)?;
    io::copy(&mut from, &mut to)?;
    // The copy keeps the modification time of the source.
    to.set_modified(modified)?;
    to.sync_all()
}

impl LibraryHost for Host {
    fn current_folder(&self) -> String {
        self.folder.to_string_lossy().into()
    }
    fn files(&self) -> Vec<String> {
        kaia_library::Library::open(&self.data.join("library.sqlite3"))
            .and_then(|lib| {
                lib.books(
                    kaia_library::Sort::new(kaia_library::SortKey::Title, false),
                    &Default::default(),
                    0,
                    usize::MAX,
                )
            })
            .unwrap_or_default()
            .into_iter()
            .filter(|b| b.path.starts_with(&self.folder))
            .map(|b| b.path.to_string_lossy().into())
            .collect()
    }
    fn open_book(&mut self, path: &str) -> Result<(), String> {
        if !Path::new(path).is_file() {
            return Err("Book does not exist".into());
        }
        self.open = Some(path.into());
        Ok(())
    }
    fn copy_book(&mut self, source: &str, folder: &str) -> Result<(), String> {
        self.transfer(source, folder, false)
    }
    fn move_book(&mut self, source: &str, folder: &str) -> Result<(), String> {
        self.transfer(source, folder, true)
    }
    fn show_folder(&mut self, path: &str) -> Result<(), String> {
        self.folder = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        if !self.folder.is_dir() {
            return Err("Not a folder".into());
        }
        self.show = Some(self.current_folder());
        Ok(())
    }
}
