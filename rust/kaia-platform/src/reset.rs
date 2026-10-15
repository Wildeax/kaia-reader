//! Scoped resets of Kaia's data folder.
//!
//! Two actions, both limited to the data folder `<data>` (`/mnt/onboard/.kaia` on the
//! Kobo, see [`crate::paths::Paths`]):
//!
//! - [`reset_settings`] keeps a copy of `settings.json` as `settings.json.before-reset`,
//!   then removes `settings.json` and `settings.json.old`. Reading data stays.
//! - [`erase`] moves the whole data folder aside, creates an empty one, and deletes the
//!   moved folder. Each step is a separate call here, so an erase that stops part way
//!   is finished by [`finish_leftover_erases`] on the next start.
//!
//! Nothing outside the data folder is read or written, except the rename that moves it
//! aside. Book files, `.kobo`, `.adds/koreader` and the Lua app's `.adds/kaia` are never
//! named here. Every entry is opened or removed through a handle on its parent folder
//! with `O_NOFOLLOW`, so a link inside the data folder is removed as a link and its
//! target stays.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rustix::fd::OwnedFd;
use rustix::fs::{
    fstat, fsync, open, openat, renameat, statat, unlinkat, AtFlags, Dir, FileType, Mode, OFlags,
};
use rustix::io::Errno;

/// The settings file.
pub const SETTINGS_FILE: &str = "settings.json";
/// The copy kept by the last settings reset.
pub const SETTINGS_BACKUP_FILE: &str = "settings.json.before-reset";
/// The older settings file that the settings reader falls back to.
const SETTINGS_OLD_FILE: &str = "settings.json.old";
/// Holds the copy until it is complete. An interrupted reset can leave it behind.
const SETTINGS_PARTIAL_FILE: &str = "settings.json.before-reset.partial";
/// Marks a data folder moved aside by an erase: `<data folder name>.erasing-<time>`.
const ERASING_MARK: &str = ".erasing-";

/// An erase that failed. `moved` is true once the data folder is aside. The caller
/// must then stop using handles and state that point into the old folder.
#[derive(Debug)]
pub struct EraseError {
    pub moved: bool,
    pub source: io::Error,
}

impl fmt::Display for EraseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.source)
    }
}

impl std::error::Error for EraseError {}

/// Resets the settings in `data_dir`. Returns the copy kept, or None when there was no
/// settings file. The settings stay in effect until the last removal succeeds.
pub fn reset_settings(data_dir: &Path) -> io::Result<Option<PathBuf>> {
    let folder = open_folder(data_dir)?;
    let copy = match read_file(&folder, SETTINGS_FILE)? {
        Some(bytes) => {
            remove_if_present(&folder, SETTINGS_PARTIAL_FILE)?;
            write_new_file(&folder, SETTINGS_PARTIAL_FILE, &bytes)?;
            renameat(&folder, SETTINGS_PARTIAL_FILE, &folder, SETTINGS_BACKUP_FILE)?;
            fsync(&folder)?;
            Some(data_dir.join(SETTINGS_BACKUP_FILE))
        }
        None => None,
    };
    // The older file goes first. Without the settings file, a leftover older file would
    // load in its place on the next start.
    remove_if_present(&folder, SETTINGS_OLD_FILE)?;
    remove_if_present(&folder, SETTINGS_FILE)?;
    fsync(&folder)?;
    Ok(copy)
}

/// Step one of an erase: renames the data folder to a new name beside it. Returns the
/// new path. When there was no data folder, this makes an empty one and returns None.
pub fn move_aside(data_dir: &Path) -> io::Result<Option<PathBuf>> {
    let parent = parent_of(data_dir)?;
    let name = name_of(data_dir)?;
    match fs::symlink_metadata(data_dir) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir_all(data_dir)?;
            return Ok(None);
        }
        Err(error) => return Err(error),
    }
    // Refuses a link or a file. Only a real folder is moved.
    drop(open_folder(data_dir)?);
    let mut aside_name = OsString::from(name);
    aside_name.push(ERASING_MARK);
    aside_name.push(stamp());
    let aside = parent.join(aside_name);
    fs::rename(data_dir, &aside)?;
    fsync(&open_dir(parent)?)?;
    Ok(Some(aside))
}

/// Step two of an erase: creates an empty data folder where the moved one was.
pub fn create_fresh(data_dir: &Path) -> io::Result<()> {
    fs::create_dir(data_dir)?;
    fsync(&open_dir(parent_of(data_dir)?)?)?;
    Ok(())
}

/// Step three of an erase: deletes a folder moved aside by [`move_aside`]. Only a name
/// with the erasing mark is accepted, so this cannot delete any other folder.
pub fn finish_erase(aside: &Path) -> io::Result<()> {
    let name = name_of(aside)?;
    if !name.to_string_lossy().contains(ERASING_MARK) {
        return Err(refused(format!("{} is not an erase folder", aside.display())));
    }
    let folder = open_dir(parent_of(aside)?)?;
    remove_entry(&folder, name)?;
    fsync(&folder)?;
    Ok(())
}

/// Erases the data folder: [`move_aside`], [`create_fresh`], then [`finish_erase`].
/// Nothing outside the folder changes.
pub fn erase(data_dir: &Path) -> Result<(), EraseError> {
    let aside = move_aside(data_dir).map_err(|source| EraseError { moved: false, source })?;
    let Some(aside) = aside else {
        return Ok(());
    };
    create_fresh(data_dir).map_err(|source| EraseError { moved: true, source })?;
    finish_erase(&aside).map_err(|source| EraseError { moved: true, source })
}

/// Finishes every erase of `data_dir` that stopped after [`move_aside`]. Returns how
/// many folders it deleted. Call it at start, before the data folder is read. A file
/// with the same name is not an erase folder, and is left alone.
pub fn finish_leftover_erases(data_dir: &Path) -> io::Result<usize> {
    let parent = parent_of(data_dir)?;
    let mut prefix = OsString::from(name_of(data_dir)?);
    prefix.push(ERASING_MARK);
    let mut leftovers = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        if !entry.file_name().as_bytes().starts_with(prefix.as_bytes()) {
            continue;
        }
        // A link counts, so it is removed as a link. A regular file does not.
        if entry.file_type()?.is_file() {
            continue;
        }
        leftovers.push(entry.path());
    }
    for path in &leftovers {
        finish_erase(path)?;
    }
    Ok(leftovers.len())
}

fn refused(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message.into())
}

fn parent_of(path: &Path) -> io::Result<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| refused(format!("{} has no parent folder", path.display())))
}

fn name_of(path: &Path) -> io::Result<&OsStr> {
    path.file_name()
        .ok_or_else(|| refused(format!("{} has no name", path.display())))
}

/// `seconds-nanoseconds` since the Unix epoch. It names the folder moved aside.
fn stamp() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}-{:09}", now.as_secs(), now.subsec_nanos())
}

fn open_dir(path: &Path) -> io::Result<OwnedFd> {
    Ok(open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?)
}

/// Opens the data folder. A link or a file at that path is an error.
fn open_folder(path: &Path) -> io::Result<OwnedFd> {
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(refused(format!("{} is not a plain folder", path.display())));
    }
    open_dir(path)
}

/// The bytes of the regular file `name` in `folder`, or None when it is missing. A
/// link, a folder or any other kind of entry is an error.
fn read_file(folder: &OwnedFd, name: &str) -> io::Result<Option<Vec<u8>>> {
    let fd = match openat(
        folder,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(fd) => fd,
        Err(Errno::NOENT) => return Ok(None),
        Err(Errno::LOOP) => return Err(refused(format!("{name} is a link"))),
        Err(error) => return Err(error.into()),
    };
    let mut file = File::from(fd);
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{name} is not a regular file"),
        ));
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// Creates `name` in `folder` with `bytes` and syncs it. Fails when the name exists.
fn write_new_file(folder: &OwnedFd, name: &str, bytes: &[u8]) -> io::Result<()> {
    let fd = openat(
        folder,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR | Mode::RGRP | Mode::ROTH,
    )?;
    let mut file = File::from(fd);
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn remove_if_present(folder: &OwnedFd, name: &str) -> io::Result<()> {
    match remove_entry(folder, OsStr::new(name)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Removes `name` from `dir`. A folder is emptied first. Every entry is opened or
/// unlinked through a handle on its parent, never by path. A link is removed as a link.
fn remove_entry(dir: &OwnedFd, name: &OsStr) -> io::Result<()> {
    let stat = statat(dir, name, AtFlags::SYMLINK_NOFOLLOW)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
        unlinkat(dir, name, AtFlags::empty())?;
        return Ok(());
    }
    let child = openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let opened = fstat(&child)?;
    if opened.st_dev != stat.st_dev || opened.st_ino != stat.st_ino {
        return Err(io::Error::other(format!(
            "{} changed while it was being erased",
            name.to_string_lossy()
        )));
    }
    let mut names = Vec::new();
    for entry in Dir::read_from(&child)? {
        let entry = entry?;
        let bytes = entry.file_name().to_bytes();
        if bytes != b"." && bytes != b".." {
            names.push(OsStr::from_bytes(bytes).to_os_string());
        }
    }
    for child_name in &names {
        remove_entry(&child, child_name)?;
    }
    unlinkat(dir, name, AtFlags::REMOVEDIR)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::os::unix::fs::symlink;
    use std::path::Component;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Node {
        Folder,
        File(Vec<u8>),
        Link(PathBuf),
    }

    /// Every entry under `root`, keyed by its path relative to `root`. File contents
    /// are compared byte for byte, which is a stronger check than a hash.
    fn snapshot(root: &Path) -> BTreeMap<PathBuf, Node> {
        let mut out = BTreeMap::new();
        walk(root, Path::new(""), &mut out);
        out
    }

    fn walk(root: &Path, rel: &Path, out: &mut BTreeMap<PathBuf, Node>) {
        for entry in fs::read_dir(root.join(rel)).unwrap() {
            let entry = entry.unwrap();
            let child = rel.join(entry.file_name());
            let kind = fs::symlink_metadata(entry.path()).unwrap().file_type();
            if kind.is_symlink() {
                out.insert(child, Node::Link(fs::read_link(entry.path()).unwrap()));
            } else if kind.is_dir() {
                out.insert(child.clone(), Node::Folder);
                walk(root, &child, out);
            } else {
                out.insert(child, Node::File(fs::read(entry.path()).unwrap()));
            }
        }
    }

    /// The entries that are not inside the data folder `.kaia`.
    fn outside(map: &BTreeMap<PathBuf, Node>) -> BTreeMap<PathBuf, Node> {
        let kaia = Component::Normal(OsStr::new(".kaia"));
        map.iter()
            .filter(|(path, _)| path.components().next() != Some(kaia))
            .map(|(path, node)| (path.clone(), node.clone()))
            .collect()
    }

    /// A fake Kobo volume. `root` is the `/mnt/onboard` of the device.
    struct Volume {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Volume {
        fn data(&self) -> PathBuf {
            self.root.join(".kaia")
        }

        /// A folder beside the volume, outside it.
        fn outside_path(&self, name: &str) -> PathBuf {
            self.root.parent().unwrap().join(name)
        }
    }

    fn volume() -> Volume {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("mnt/onboard");
        let files = [
            ("books/juliet.epub", "book one"),
            ("books/sub/leaves.epub", "book two"),
            (".kobo/KoboReader.sqlite", "nickel"),
            (".adds/koreader/settings.reader.lua", "koreader"),
            (".adds/kaia/bin/kaia", "app"),
            (".kaia/settings.json", "{\"display.warmth\": 40}"),
            (".kaia/settings.json.old", "{\"display.warmth\": 10}"),
            (".kaia/kaia.lock", ""),
            (".kaia/library.sqlite3", "index"),
            (".kaia/kaia/books/abc.json", "progress"),
            (".kaia/statistics.sqlite3", "stats"),
            (".kaia/fonts/Mine.ttf", "font"),
            (".kaia/dict/x.ifo", "dict"),
            (".kaia/cache/cover.jpg", "cover"),
        ];
        for (path, bytes) in files {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        Volume { _dir: dir, root }
    }

    fn no_erase_folders_left(map: &BTreeMap<PathBuf, Node>) -> bool {
        map.keys()
            .all(|path| !path.to_string_lossy().contains(ERASING_MARK))
    }

    #[test]
    fn reset_keeps_one_copy_and_removes_only_the_settings_files() {
        let v = volume();
        let before = snapshot(&v.root);
        let copy = reset_settings(&v.data()).unwrap();
        assert_eq!(copy, Some(v.data().join(SETTINGS_BACKUP_FILE)));
        let mut expected = before;
        expected.remove(Path::new(".kaia/settings.json"));
        expected.remove(Path::new(".kaia/settings.json.old"));
        expected.insert(
            PathBuf::from(".kaia/settings.json.before-reset"),
            Node::File(b"{\"display.warmth\": 40}".to_vec()),
        );
        assert_eq!(snapshot(&v.root), expected);
    }

    #[test]
    fn a_second_reset_replaces_the_copy_and_the_folder_stays_usable() {
        let v = volume();
        reset_settings(&v.data()).unwrap();
        fs::write(v.data().join(SETTINGS_FILE), "{\"display.warmth\": 70}").unwrap();
        let copy = reset_settings(&v.data()).unwrap();
        assert_eq!(fs::read(copy.unwrap()).unwrap(), b"{\"display.warmth\": 70}");
        // Nothing left to copy: the reset does nothing and keeps the copy.
        assert_eq!(reset_settings(&v.data()).unwrap(), None);
        assert!(!v.data().join(SETTINGS_FILE).exists());
        assert!(v.data().join(SETTINGS_BACKUP_FILE).exists());
        assert!(v.data().join("library.sqlite3").exists());
    }

    #[test]
    fn reset_refuses_a_linked_settings_file_and_changes_nothing() {
        let v = volume();
        let target = v.outside_path("outside.json");
        fs::write(&target, "secret").unwrap();
        fs::remove_file(v.data().join(SETTINGS_FILE)).unwrap();
        symlink(&target, v.data().join(SETTINGS_FILE)).unwrap();
        let before = snapshot(&v.root);
        assert!(reset_settings(&v.data()).is_err());
        assert_eq!(snapshot(&v.root), before);
        assert_eq!(fs::read(&target).unwrap(), b"secret");
    }

    #[test]
    fn erase_empties_the_data_folder_and_leaves_every_other_entry_identical() {
        let v = volume();
        let before = outside(&snapshot(&v.root));
        erase(&v.data()).unwrap();
        let after = snapshot(&v.root);
        assert_eq!(outside(&after), before);
        assert!(v.data().is_dir());
        assert_eq!(fs::read_dir(v.data()).unwrap().count(), 0);
        assert!(no_erase_folders_left(&after));
    }

    #[test]
    fn erase_removes_links_as_links_and_leaves_their_targets() {
        let v = volume();
        let external = v.outside_path("external");
        fs::create_dir_all(external.join("keep")).unwrap();
        fs::write(external.join("keep/notes.txt"), "keep me").unwrap();
        symlink(&external, v.data().join("link-dir")).unwrap();
        symlink(
            v.root.join("books/juliet.epub"),
            v.data().join("kaia/books/link-file"),
        )
        .unwrap();
        fs::create_dir_all(v.data().join("cache/nested")).unwrap();
        symlink(&external, v.data().join("cache/nested/link-dir")).unwrap();
        let outside_before = outside(&snapshot(&v.root));
        let external_before = snapshot(&external);
        erase(&v.data()).unwrap();
        assert_eq!(snapshot(&external), external_before);
        assert_eq!(outside(&snapshot(&v.root)), outside_before);
        assert_eq!(fs::read_dir(v.data()).unwrap().count(), 0);
    }

    #[test]
    fn erase_refuses_a_linked_data_folder() {
        let v = volume();
        let moved = v.outside_path("moved-data");
        fs::rename(v.data(), &moved).unwrap();
        symlink(&moved, v.data()).unwrap();
        let before = snapshot(&v.root);
        let moved_before = snapshot(&moved);
        let error = erase(&v.data()).unwrap_err();
        assert!(!error.moved);
        assert_eq!(snapshot(&v.root), before);
        assert_eq!(snapshot(&moved), moved_before);
    }

    #[test]
    fn erase_of_a_missing_folder_makes_an_empty_one() {
        let v = volume();
        fs::remove_dir_all(v.data()).unwrap();
        erase(&v.data()).unwrap();
        assert_eq!(fs::read_dir(v.data()).unwrap().count(), 0);
    }

    #[test]
    fn an_erase_stopped_after_the_rename_is_finished_on_the_next_start() {
        let v = volume();
        let outside_before = outside(&snapshot(&v.root));
        // Step one only. The data folder is aside and nothing else has happened.
        let aside = move_aside(&v.data()).unwrap().unwrap();
        assert!(!v.data().exists());
        assert_eq!(finish_leftover_erases(&v.data()).unwrap(), 1);
        assert!(!aside.exists());
        // This call does not make the data folder. Startup does, before it reads it.
        assert!(!v.data().exists());
        assert_eq!(outside(&snapshot(&v.root)), outside_before);
    }

    #[test]
    fn an_erase_stopped_during_the_delete_leaves_a_usable_folder() {
        let v = volume();
        let outside_before = outside(&snapshot(&v.root));
        let aside = move_aside(&v.data()).unwrap().unwrap();
        create_fresh(&v.data()).unwrap();
        // Stop half way: one file of the moved folder is already gone.
        fs::remove_file(aside.join("library.sqlite3")).unwrap();
        fs::write(v.data().join(SETTINGS_FILE), "{}").unwrap();
        assert_eq!(finish_leftover_erases(&v.data()).unwrap(), 1);
        assert!(!aside.exists());
        assert!(v.data().is_dir());
        assert_eq!(fs::read(v.data().join(SETTINGS_FILE)).unwrap(), b"{}");
        assert_eq!(outside(&snapshot(&v.root)), outside_before);
    }

    #[test]
    fn leftover_cleanup_ignores_files_and_other_names() {
        let v = volume();
        fs::write(v.root.join(".kaia.erasing-stray"), "keep").unwrap();
        fs::create_dir(v.root.join("kaia.erasing-1")).unwrap();
        let before = snapshot(&v.root);
        assert_eq!(finish_leftover_erases(&v.data()).unwrap(), 0);
        assert_eq!(snapshot(&v.root), before);
    }

    #[test]
    fn finish_erase_deletes_only_names_with_the_erasing_mark() {
        let v = volume();
        let before = snapshot(&v.root);
        assert!(finish_erase(&v.root.join(".adds")).is_err());
        assert!(finish_erase(&v.data()).is_err());
        assert_eq!(snapshot(&v.root), before);
    }
}
