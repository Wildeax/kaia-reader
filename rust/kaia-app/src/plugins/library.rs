//! File manager operations over Kaia's index and data paths.
use kaia_lua::LibraryHost;
use std::path::{Path, PathBuf};

pub(crate) struct Host {
    pub data: PathBuf,
    pub folder: PathBuf,
    pub open: Option<String>,
    pub show: Option<String>,
}
impl Host {
    pub fn new(paths: &kaia_platform::Paths) -> Self {
        Self {
            data: paths.data_dir.clone(),
            folder: paths.books_dir.clone(),
            open: None,
            show: None,
        }
    }
    fn transfer(&mut self, source: &str, folder: &str, moving: bool) -> Result<(), String> {
        let source = Path::new(source);
        let target = Path::new(folder).join(source.file_name().ok_or("Book has no filename")?);
        if target.exists() {
            return Err("The destination already exists".into());
        }
        std::fs::copy(source, &target).map_err(|e| e.to_string())?;
        let mut index =
            kaia_library::Library::open(&self.data.join("library.sqlite3")).map_err(|e| e.to_string())?;
        index
            .scan(
                std::slice::from_ref(&target),
                |_| {},
                &std::sync::atomic::AtomicBool::new(false),
            )
            .map_err(|e| e.to_string())?;
        if moving {
            std::fs::remove_file(source).map_err(|e| e.to_string())?;
            if let Some(book) = index.book_by_path(source).map_err(|e| e.to_string())? {
                index.remove_book(book.id).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
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
