//! Picks the document engine handler by file content and extension.
//!
//! Content wins when the bytes identify a format (PDF, MOBI, FB2, HTML). ZIP
//! containers (EPUB, CBZ, XPS) share one signature, so the extension decides
//! for them. Everything else falls back to the extension.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::error::{DocError, Result};

/// Formats that MuPDF opens. The magic string is the handler name MuPDF uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Pdf,
    Epub,
    Cbz,
    Xps,
    Fb2,
    Mobi,
    Html,
    Svg,
}

impl Format {
    /// The handler name passed to MuPDF, such as `.epub`.
    pub fn magic(self) -> &'static str {
        match self {
            Format::Pdf => ".pdf",
            Format::Epub => ".epub",
            Format::Cbz => ".cbz",
            Format::Xps => ".xps",
            Format::Fb2 => ".fb2",
            Format::Mobi => ".mobi",
            Format::Html => ".html",
            Format::Svg => ".svg",
        }
    }

    /// Maps a file extension, without the dot and in any case, to a format.
    pub fn from_extension(ext: &str) -> Option<Format> {
        match ext.to_ascii_lowercase().as_str() {
            "pdf" => Some(Format::Pdf),
            "epub" => Some(Format::Epub),
            "cbz" => Some(Format::Cbz),
            "xps" => Some(Format::Xps),
            "fb2" => Some(Format::Fb2),
            "mobi" | "azw" | "azw3" | "prc" => Some(Format::Mobi),
            "htm" | "html" | "xhtml" => Some(Format::Html),
            "svg" => Some(Format::Svg),
            _ => None,
        }
    }
}

const SNIFF_BYTES: usize = 4096;

/// Chooses the format of a file. Reads at most 4 KiB.
pub fn detect(path: &Path) -> Result<Format> {
    let head = read_head(path)?;
    if let Some(f) = sniff(&head) {
        return Ok(f);
    }
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(Format::from_extension)
        .ok_or_else(|| DocError::UnknownFormat(path.to_path_buf()))
}

fn read_head(path: &Path) -> Result<Vec<u8>> {
    let io = |source| DocError::Io {
        path: path.to_path_buf(),
        source,
    };
    let mut file = File::open(path).map_err(io)?;
    let mut buf = vec![0u8; SNIFF_BYTES];
    let mut filled = 0;
    while filled < buf.len() {
        let n = file.read(&mut buf[filled..]).map_err(io)?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(buf)
}

/// Returns the format named by the bytes, or None when the bytes do not decide.
pub fn sniff(head: &[u8]) -> Option<Format> {
    if head.starts_with(b"%PDF-") {
        return Some(Format::Pdf);
    }
    // PalmDoc header: the database type and creator at offset 60 read "BOOKMOBI".
    if head.len() >= 68 && &head[60..68] == b"BOOKMOBI" {
        return Some(Format::Mobi);
    }
    let text = String::from_utf8_lossy(head);
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("<fictionbook") {
        return Some(Format::Fb2);
    }
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        return Some(Format::Html);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_header_wins_over_extension() {
        assert_eq!(sniff(b"%PDF-1.7\n"), Some(Format::Pdf));
    }

    #[test]
    fn mobi_is_found_at_offset_60() {
        let mut head = vec![0u8; 80];
        head[60..68].copy_from_slice(b"BOOKMOBI");
        assert_eq!(sniff(&head), Some(Format::Mobi));
    }

    #[test]
    fn fb2_and_html_are_sniffed() {
        assert_eq!(
            sniff(b"<?xml version=\"1.0\"?>\n<FictionBook xmlns=\"x\">"),
            Some(Format::Fb2)
        );
        assert_eq!(sniff(b"  <!DOCTYPE html>\n<html>"), Some(Format::Html));
    }

    #[test]
    fn zip_needs_the_extension() {
        assert_eq!(sniff(b"PK\x03\x04rest"), None);
        assert_eq!(Format::from_extension("EPUB"), Some(Format::Epub));
        assert_eq!(Format::from_extension("txt"), None);
    }

    #[test]
    fn detect_reports_missing_file_as_io() {
        let err = detect(Path::new("/nonexistent/kaia/none.epub")).unwrap_err();
        assert!(matches!(err, DocError::Io { .. }));
    }
}
