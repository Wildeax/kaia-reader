//! Where Kaia keeps its install, data and books.
//!
//! On the Kobo, KOReader lives in `/mnt/onboard/.adds/koreader` and reads
//! books from `/mnt/onboard` (set by the launcher, which is not in this repository; `frontend/device/kobo/device.lua:127`
//! `home_dir = "/mnt/onboard"`). Kaia installs to `/mnt/onboard/.adds/kaia`
//! and keeps its data in `/mnt/onboard/.kaia`, a separate folder, so an
//! update that replaces the install folder never touches settings, reading
//! progress or annotations. Unverified on hardware.

use std::path::{Path, PathBuf};

use crate::sysfs::Root;

/// Install, data and books locations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// Holds the scripts and bundled tools.
    pub install_dir: PathBuf,
    /// Holds Kaia's own data files.
    pub data_dir: PathBuf,
    /// Default library root.
    pub books_dir: PathBuf,
}

impl Paths {
    /// Kobo paths under `root`. `home_dir` is `/mnt/onboard` (`device.lua:127`).
    pub fn kobo(root: &Root) -> Self {
        Paths {
            install_dir: root.path("/mnt/onboard/.adds/kaia"),
            data_dir: root.path("/mnt/onboard/.kaia"),
            books_dir: root.path("/mnt/onboard"),
        }
    }

    /// Desktop paths. The data directory is `$XDG_DATA_HOME/kaia`. On macOS it is
    /// `~/Library/Application Support/kaia`. Otherwise it is `~/.local/share/kaia`.
    /// Books default to `~/Books`. Set another library path with [`Paths::with_books_dir`].
    pub fn desktop(home: &Path, xdg_data_home: Option<&Path>, macos: bool) -> Self {
        let data = match xdg_data_home {
            Some(xdg) => xdg.join("kaia"),
            None if macos => home.join("Library/Application Support/kaia"),
            None => home.join(".local/share/kaia"),
        };
        Paths {
            install_dir: data.clone(),
            data_dir: data,
            books_dir: home.join("Books"),
        }
    }

    /// Replaces the library root.
    pub fn with_books_dir(mut self, books_dir: PathBuf) -> Self {
        self.books_dir = books_dir;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kobo_paths() {
        let paths = Paths::kobo(&Root::new("/"));
        assert_eq!(paths.install_dir, PathBuf::from("/mnt/onboard/.adds/kaia"));
        assert_eq!(paths.data_dir, PathBuf::from("/mnt/onboard/.kaia"));
        assert_eq!(paths.books_dir, PathBuf::from("/mnt/onboard"));
    }

    #[test]
    fn desktop_paths_follow_xdg_then_platform() {
        let home = Path::new("/home/reader");
        let xdg = Paths::desktop(home, Some(Path::new("/data")), false);
        assert_eq!(xdg.data_dir, PathBuf::from("/data/kaia"));
        let mac = Paths::desktop(home, None, true);
        assert_eq!(
            mac.data_dir,
            PathBuf::from("/home/reader/Library/Application Support/kaia")
        );
        let linux = Paths::desktop(home, None, false);
        assert_eq!(linux.data_dir, PathBuf::from("/home/reader/.local/share/kaia"));
        let moved = linux.with_books_dir(PathBuf::from("/srv/books"));
        assert_eq!(moved.books_dir, PathBuf::from("/srv/books"));
    }
}
