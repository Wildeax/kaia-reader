//! Rules for the names a Calibre server may send.
//!
//! The server names files under the downloads root. On a Kobo that root is
//! /mnt/onboard. The same folder holds Kaia's data (.kaia), the install and
//! KOReader (.adds), the Kobo database (.kobo) and the sidecar folders that
//! KOReader writes beside books (*.sdr). Every name from the server passes
//! [`check_lpath`] before a connection reads, writes or removes a file. The
//! storage layer runs [`check_target`] on the path it receives.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Extensions of the books that Kaia accepts from Calibre. They are the formats
/// the library indexes through `Format::from_extension` in kaia-doc. This
/// includes plain text, which the library now indexes and CREngine opens.
pub const BOOK_EXTENSIONS: &[&str] = &[
    "epub", "pdf", "cbz", "xps", "fb2", "mobi", "azw", "azw3", "prc", "htm", "html", "xhtml", "djvu", "djv",
    "txt",
];

/// The files that the driver writes for itself. KOReader's Calibre plugin uses
/// the same names. It also accepts a hidden copy with a leading dot.
const DRIVER_FILES: &[&str] = &["metadata.calibre", "driveinfo.calibre"];

/// The message for a target that exists but may not be replaced.
pub const NOT_REPLACED: &str = "Kaia does not replace this file.";

/// The rule that refused a name. Each variant is the first rule that failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The name is empty, absolute, or has an empty, "." or ".." part.
    NotRelative,
    /// A part starts with "." (hidden folders such as .kaia, .kobo and .adds, and dotfiles).
    Hidden,
    /// A part ends with ".sdr", the KOReader sidecar folder.
    Sidecar,
    /// The name has a backslash, a colon or a control character.
    Characters,
    /// The name is not a book. Its extension is not a book format, or a part is a driver file.
    NotBook,
}

impl Refusal {
    /// The message that goes to the server.
    pub fn message(self) -> &'static str {
        match self {
            Refusal::NotBook => "The file type is not supported.",
            _ => "The file name is not allowed.",
        }
    }
}

/// Checks a name from the server and returns the first rule it breaks.
///
/// The name must be a relative path to a book. The rules run in this order, so
/// a name with a backslash fails as `Characters` even when it also has a ".."
/// part. Colons are illegal in Windows and FAT names, so a colon marks a drive
/// prefix or a stream name, and both are refused.
pub fn check_lpath(lpath: &str) -> Result<(), Refusal> {
    if lpath.is_empty() {
        return Err(Refusal::NotRelative);
    }
    if lpath.chars().any(|c| c == '\\' || c == ':' || c.is_control()) {
        return Err(Refusal::Characters);
    }
    for part in lpath.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return Err(Refusal::NotRelative);
        }
        if part.starts_with('.') {
            return Err(Refusal::Hidden);
        }
        if part.to_ascii_lowercase().ends_with(".sdr") {
            return Err(Refusal::Sidecar);
        }
        if DRIVER_FILES.contains(&part) {
            return Err(Refusal::NotBook);
        }
    }
    let name = lpath.rsplit('/').next().unwrap_or(lpath);
    match name.rsplit_once('.') {
        Some((_, extension)) if BOOK_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str()) => Ok(()),
        _ => Err(Refusal::NotBook),
    }
}

/// Repeats the checks for a path that the storage is about to write or remove.
///
/// The path must lie under `root`. The name rules apply to the part under the
/// root, no part may be a link, and an existing target must be a regular file.
pub fn check_target(root: &Path, path: &Path) -> io::Result<()> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| refused(Refusal::NotRelative.message()))?;
    let lpath = relative
        .to_str()
        .ok_or_else(|| refused(Refusal::Characters.message()))?;
    check_lpath(lpath).map_err(|refusal| refused(refusal.message()))?;
    reject_links(root, relative)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(refused(NOT_REPLACED)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// The path under `root` for a name from the server. The name must pass
/// [`check_lpath`], and no part of the path may be a link.
pub(crate) fn safe_path(root: &Path, lpath: &str) -> io::Result<PathBuf> {
    check_lpath(lpath).map_err(|refusal| refused(refusal.message()))?;
    reject_links(root, Path::new(lpath))?;
    Ok(root.join(lpath))
}

/// The error for a refused name or target. Its kind tells the worker that the
/// server was refused, and its message goes to the server.
pub(crate) fn refused(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}

/// Refuses a symlink at any part of `relative` below `root`. A part that does
/// not exist yet is fine.
fn reject_links(root: &Path, relative: &Path) -> io::Result<()> {
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(refused("Links are not allowed in the book path."));
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("kaia-calibre-policy-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn lpath_table() {
        use Refusal::*;
        let cases: &[(&str, Result<(), Refusal>)] = &[
            // Book names that pass.
            ("Book.epub", Ok(())),
            ("Author/Book.epub", Ok(())),
            ("Author/Book.kepub.epub", Ok(())),
            ("Upper/CASE.EPUB", Ok(())),
            ("Comics/Issue 1.cbz", Ok(())),
            ("Manual.pdf", Ok(())),
            ("Old/Scan.djvu", Ok(())),
            ("Web/index.html", Ok(())),
            ("Reader/Story.fb2", Ok(())),
            ("Pocket/Novel.mobi", Ok(())),
            // The library now indexes text books.
            ("Notes/Read me.txt", Ok(())),
            ("Books/Novel.TXT", Ok(())),
            // Empty, absolute and escaping names.
            ("", Err(NotRelative)),
            ("/etc/Book.epub", Err(NotRelative)),
            ("../Book.epub", Err(NotRelative)),
            ("Author/../Book.epub", Err(NotRelative)),
            ("..", Err(NotRelative)),
            ("./Book.epub", Err(NotRelative)),
            ("Author/./Book.epub", Err(NotRelative)),
            ("Author//Book.epub", Err(NotRelative)),
            ("Author/Book.epub/", Err(NotRelative)),
            // Hidden folders and dotfiles.
            (".kaia/settings.json", Err(Hidden)),
            (".kobo/KoboReader.sqlite", Err(Hidden)),
            (".adds/kaia/bin/kaia", Err(Hidden)),
            ("Books/.hidden.epub", Err(Hidden)),
            (".metadata.calibre", Err(Hidden)),
            (".driveinfo.calibre", Err(Hidden)),
            ("...epub", Err(Hidden)),
            // KOReader sidecar folders.
            ("Book.sdr/metadata.epub.lua", Err(Sidecar)),
            ("Book.epub.sdr/Book.epub", Err(Sidecar)),
            ("Book.SDR/Book.epub", Err(Sidecar)),
            // Backslashes, drive prefixes and control characters.
            ("Books\\..\\settings.epub", Err(Characters)),
            ("Books\\Book.epub", Err(Characters)),
            ("C:/Books/Book.epub", Err(Characters)),
            ("C:Book.epub", Err(Characters)),
            ("Book: Subtitle.epub", Err(Characters)),
            ("Line\nBreak.epub", Err(Characters)),
            ("Nul\0Byte.epub", Err(Characters)),
            // Names that are not books.
            ("Run.exe", Err(NotBook)),
            ("setup.sh", Err(NotBook)),
            ("Book.epub.exe", Err(NotBook)),
            ("Book", Err(NotBook)),
            ("Book.", Err(NotBook)),
            ("Note.md", Err(NotBook)),
            ("Old.doc", Err(NotBook)),
            ("Old.docx", Err(NotBook)),
            ("Archive.zip", Err(NotBook)),
            ("Vector.svg", Err(NotBook)),
            ("Pictures/cover.jpg", Err(NotBook)),
            ("metadata.calibre", Err(NotBook)),
            ("driveinfo.calibre", Err(NotBook)),
            ("Lists/metadata.calibre", Err(NotBook)),
            ("metadata.calibre.tmp.123", Err(NotBook)),
        ];
        for (lpath, expected) in cases {
            assert_eq!(check_lpath(lpath), *expected, "lpath {lpath:?}");
        }
    }

    #[test]
    fn target_checks_links_and_existing_kind() {
        let root = temp_root("target");
        fs::create_dir_all(root.join("Shelf")).unwrap();
        fs::write(root.join("Shelf/Book.epub"), b"book").unwrap();
        fs::create_dir_all(root.join("Folder.epub")).unwrap();
        symlink(root.join("Shelf"), root.join("Link")).unwrap();
        check_target(&root, &root.join("Shelf/New.epub")).unwrap();
        check_target(&root, &root.join("Shelf/Book.epub")).unwrap();
        assert_eq!(
            check_target(&root, &root.join("Folder.epub")).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert!(check_target(&root, &root.join("../outside.epub")).is_err());
        assert!(check_target(&root, &root.join("Link/New.epub")).is_err());
        assert!(check_target(&root, &root.join("Link")).is_err());
        assert!(check_target(&root, Path::new("/elsewhere/Book.epub")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
