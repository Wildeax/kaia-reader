//! Space on a volume and file access for library deletion.

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path};

use rustix::fd::OwnedFd;
use rustix::fs::{fstat, open, openat, statat, statvfs, unlinkat, AtFlags, Mode, OFlags};

#[derive(Debug, Clone, Copy)]
pub struct VolumeSpace {
    pub available: u64,
    pub total: u64,
}

/// Available space excludes blocks reserved for the system.
pub fn volume_space(path: &Path) -> io::Result<VolumeSpace> {
    let stat = statvfs(path)?;
    Ok(VolumeSpace {
        available: stat.f_bavail.saturating_mul(stat.f_frsize),
        total: stat.f_blocks.saturating_mul(stat.f_frsize),
    })
}

fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "unsafe book path")
}

fn hidden(path: &Path) -> bool {
    path.components().any(|part| match part {
        Component::Normal(name) => name.to_string_lossy().starts_with('.'),
        Component::ParentDir => true,
        _ => false,
    })
}

/// A regular file opened beneath a books folder without following symlinks.
/// The directory handle stays open until deletion, including the final check.
pub struct BookFile {
    parent: OwnedFd,
    name: std::ffi::OsString,
    pub file: File,
}

impl BookFile {
    pub fn open(root: &Path, path: &Path) -> io::Result<Self> {
        let root = std::path::absolute(root)?;
        let path = std::path::absolute(path)?;
        if hidden(&root) || hidden(&path) {
            return Err(refused());
        }
        if fs::symlink_metadata(&root)?.is_symlink() {
            return Err(refused());
        }
        let relative = path.strip_prefix(&root).map_err(|_| refused())?;
        let mut parts = relative.components().peekable();
        // Canonicalize the configured root only. Children must never be symlinks.
        let root = fs::canonicalize(&root)?;
        if hidden(&root) {
            return Err(refused());
        }
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut parent = open(Path::new("/"), flags, Mode::empty())?;
        for component in root.components() {
            if let Component::Normal(name) = component {
                parent = openat(&parent, name, flags, Mode::empty())?;
            }
        }
        while let Some(component) = parts.next() {
            let Component::Normal(name) = component else {
                return Err(refused());
            };
            if parts.peek().is_some() {
                parent = openat(&parent, name, flags, Mode::empty())?;
            } else {
                let fd = openat(
                    &parent,
                    name,
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                )?;
                let file = File::from(fd);
                if !file.metadata()?.is_file() {
                    return Err(refused());
                }
                return Ok(Self {
                    parent,
                    name: name.to_owned(),
                    file,
                });
            }
        }
        Err(refused())
    }

    /// Rechecks the entry against the open file. `unlinkat` never removes a folder
    /// and never follows the final component if another process replaces it.
    pub fn remove(self) -> io::Result<()> {
        let opened = fstat(&self.file)?;
        let current = statat(&self.parent, &self.name, AtFlags::SYMLINK_NOFOLLOW)?;
        if opened.st_dev != current.st_dev
            || opened.st_ino != current.st_ino
            || opened.st_mode != current.st_mode
            || opened.st_size != current.st_size
            || opened.st_mtime != current.st_mtime
            || opened.st_mtime_nsec != current.st_mtime_nsec
        {
            return Err(refused());
        }
        unlinkat(&self.parent, &self.name, AtFlags::empty())?;
        Ok(())
    }
}

/// Counts regular files without following symlinks. Read errors remain visible.
pub fn directory_bytes(path: &Path) -> io::Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    let mut bytes = 0u64;
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            bytes = bytes.saturating_add(directory_bytes(&entry?.path())?);
        }
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_and_directory_sizes_use_a_temp_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("one"), [0; 17]).unwrap();
        fs::create_dir(dir.path().join("child")).unwrap();
        fs::write(dir.path().join("child/two"), [0; 23]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path(), dir.path().join("loop")).unwrap();
        assert_eq!(directory_bytes(dir.path()).unwrap(), 40);
        let space = volume_space(dir.path()).unwrap();
        assert!(space.total > 0);
        assert!(space.available <= space.total);
    }
}
