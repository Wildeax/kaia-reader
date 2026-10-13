//! Durable app files and advisory writer locks.
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
pub(crate) fn unique(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(
        ".{}-{}.{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed),
        suffix
    ));
    PathBuf::from(name)
}
pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = unique(path, "tmp");
    let result = (|| {
        let mut file = File::options().write(true).create_new(true).open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temp, path)?;
        File::open(path.parent().unwrap())?.sync_all()
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
/// Copy before replacement. A failed read or copy prevents replacement.
pub(crate) fn preserve(path: &Path) -> io::Result<PathBuf> {
    let mut backup = unique(path, "unreadable");
    while backup.exists() {
        backup = unique(path, "unreadable");
    }
    atomic_write(&backup, &fs::read(path)?)?;
    Ok(backup)
}
pub(crate) fn lock(path: &Path, wait: bool) -> io::Result<File> {
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    if wait {
        file.lock()?;
    } else {
        file.try_lock().map_err(io::Error::other)?;
    }
    Ok(file)
}
