//! File types the reader opens, and the names it gives downloaded files.
//!
//! The table lists the (extension, MIME type) pairs that KOReader registers
//! for its document engines (`pdfdocument.lua` for MuPDF, `djvudocument.lua`,
//! `picdocument.lua`, `credocument.lua` for CREngine). An acquisition is
//! offered for download when its URL extension or its MIME type is in the
//! table. The first row for a MIME type gives the extension for that type.
//! Rows are ordered as KOReader's MuPDF rows first, so `application/zip` maps
//! to `zip`.

use url::Url;

use crate::resolve::percent_decode;

/// Longest file name stem in bytes, as in KOReader's `getSafeFilename`.
const NAME_LIMIT: usize = 240;
/// Longest extension KOReader treats as an extension. Longer text counts as
/// part of the name.
const EXT_LIMIT: usize = 10;

const PROVIDERS: &[(&str, &str)] = &[
    // MuPDF
    ("cbr", "application/vnd.comicbook-rar"),
    ("cbr", "application/vnd.rar"),
    ("cbt", "application/vnd.comicbook+tar"),
    ("cbz", "application/vnd.comicbook+zip"),
    ("cbz", "application/x-cbz"),
    ("cfb", "application/octet-stream"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("epub", "application/epub+zip"),
    ("epub3", "application/epub+zip"),
    ("fb2", "application/fb2"),
    ("htm", "text/html"),
    ("html", "text/html"),
    ("mobi", "application/x-mobipocket-ebook"),
    ("pdf", "application/pdf"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("tar", "application/x-tar"),
    ("txt", "text/plain"),
    ("xhtml", "application/xhtml+xml"),
    ("xml", "application/xml"),
    ("xps", "application/oxps"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("zip", "application/zip"),
    ("gif", "image/gif"),
    ("hdp", "image/vnd.ms-photo"),
    ("j2k", "image/jp2"),
    ("jp2", "image/jp2"),
    ("jpeg", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("jxr", "image/jxr"),
    ("pam", "image/x-portable-arbitrarymap"),
    ("pbm", "image/x-portable-bitmap"),
    ("pgm", "image/x-portable-bitmap"),
    ("pnm", "image/x-portable-bitmap"),
    ("ppm", "image/x-portable-bitmap"),
    ("png", "image/png"),
    ("svg", "image/svg+xml"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("wdp", "image/vnd.ms-photo"),
    // DjVuLibre
    ("djvu", "image/vnd.djvu"),
    ("djvu", "application/djvu"),
    ("djvu", "image/x-djvu"),
    ("djv", "image/vnd.djvu"),
    // Pictures
    ("webp", "image/webp"),
    // CREngine
    ("azw", "application/vnd.amazon.mobi8-ebook"),
    ("azw", "application/x-mobi8-ebook"),
    ("chm", "application/vnd.ms-htmlhelp"),
    ("doc", "application/msword"),
    ("docm", "application/vnd.ms-word.document.macroEnabled.12"),
    ("epub", "application/epub"),
    ("fb2", "application/x-fictionbook+xml"),
    ("fb2", "text/fb2+xml"),
    ("fb2.zip", "application/fb2+zip"),
    ("fb3", "application/fb3"),
    ("htmlz", "application/html+zip"),
    ("html.zip", "application/html+zip"),
    ("htm.zip", "application/zip"),
    ("log", "text/plain"),
    ("md", "text/plain"),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("pdb", "application/vnd.palm"),
    ("prc", "application/vnd.palm"),
    ("rtf", "application/rtf"),
    ("rtf.zip", "application/rtf+zip"),
    ("tcr", "application/tcr"),
    ("txt.zip", "application/txt+zip"),
    ("py", "text/x-python"),
    ("sh", "application/x-shellscript"),
];

/// True when the reader can open files with this MIME type.
pub fn is_supported_mime(mime: &str) -> bool {
    mime_extension(mime).is_some()
}

/// The extension the reader uses for a file, from its URL first and then from
/// its MIME type. `None` when neither is known to the reader.
pub fn file_type(url: &str, mime: Option<&str>) -> Option<&'static str> {
    url_extension(url)
        .and_then(|ext| {
            PROVIDERS
                .iter()
                .find(|(known, _)| *known == ext)
                .map(|(known, _)| *known)
        })
        .or_else(|| mime.and_then(mime_extension))
}

fn mime_extension(mime: &str) -> Option<&'static str> {
    let mime = mime.trim();
    PROVIDERS
        .iter()
        .find(|(_, known)| known.eq_ignore_ascii_case(mime))
        .map(|(ext, _)| *ext)
}

/// The lowercase extension of the last path segment, if it has one.
fn url_extension(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let segment = path.rsplit('/').next().unwrap_or("");
    let (_, ext) = segment.rsplit_once('.')?;
    (!ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

/// A file name a caller chose, sanitized the way downloads are. The stem is
/// cut to 240 bytes and keeps its extension.
pub fn safe_file_name(name: &str) -> String {
    finish(name, "download")
}

/// The name for a book from an entry: `book_name` (usually "Author - Title")
/// with the extension, sanitized like KOReader's download names.
pub fn book_file_name(book_name: &str, extension: &str) -> String {
    finish(&format!("{book_name}.{}", extension.to_ascii_lowercase()), "book")
}

/// The name a server gives a download. It comes from `Content-Disposition`,
/// then from the last path segment of the final URL. The extension is added
/// when the name has none.
pub fn server_file_name(disposition: Option<&str>, final_url: &Url, extension: Option<&str>) -> String {
    let mut name = disposition
        .and_then(disposition_filename)
        .or_else(|| {
            final_url
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .filter(|segment| !segment.is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_default();
    if !name.contains('.') {
        if let Some(ext) = extension {
            name.push('.');
            name.push_str(ext);
        }
    }
    finish(&percent_decode(&name), "download")
}

/// Pulls the value of `filename=` out of a `Content-Disposition` header.
fn disposition_filename(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let start = lower.find("filename=")? + "filename=".len();
    let rest = &value[start..];
    let quoted = rest
        .strip_prefix('"')
        .and_then(|inner| inner.find('"').map(|end| inner[..end].to_owned()));
    let name = quoted.unwrap_or_else(|| rest.split(';').next().unwrap_or("").trim().to_owned());
    (!name.is_empty()).then_some(name)
}

/// Splits off the extension (the text after the last dot, at most
/// `EXT_LIMIT` bytes), sanitizes both parts, and cuts the stem to
/// `NAME_LIMIT` bytes. An empty stem becomes `fallback`.
fn finish(name: &str, fallback: &str) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(dot) if dot + 1 < name.len() && name.len() - dot - 1 <= EXT_LIMIT => {
            (&name[..dot], Some(&name[dot + 1..]))
        }
        _ => (name, None),
    };
    let mut stem = clean(stem);
    truncate_bytes(&mut stem, NAME_LIMIT);
    let stem = clean(&stem);
    let stem = if stem.is_empty() {
        fallback.to_owned()
    } else {
        stem
    };
    match ext.map(|e| replace_invalid(e).trim_matches(' ').to_owned()) {
        Some(ext) if !ext.is_empty() => format!("{stem}.{ext}"),
        _ => stem,
    }
}

/// Replaces characters that FAT and Windows reject, flattens line breaks, and
/// removes trailing dots and white space.
fn clean(text: &str) -> String {
    replace_invalid(text)
        .trim_end_matches(|c: char| c == '.' || c.is_whitespace())
        .to_owned()
}

fn replace_invalid(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\r' | '\n' | '\t' => ' ',
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect()
}

fn truncate_bytes(text: &mut String, limit: usize) {
    if text.len() <= limit {
        return;
    }
    let mut cut = limit;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text.truncate(cut);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_type_prefers_the_url_extension() {
        assert_eq!(file_type("https://x.org/a/1342.epub", None), Some("epub"));
        assert_eq!(file_type("https://x.org/a/1.PDF?x=1", None), Some("pdf"));
    }

    #[test]
    fn file_type_falls_back_to_mime() {
        // Gutenberg links end in `.epub.noimages`, which the reader does not know.
        assert_eq!(
            file_type(
                "https://x.org/ebooks/1342.epub.noimages",
                Some("application/epub+zip")
            ),
            Some("epub")
        );
        // Calibre download links end in the format name with a slash.
        assert_eq!(
            file_type(
                "https://x.org/opds/download/42/epub/",
                Some("application/epub+zip")
            ),
            Some("epub")
        );
        assert_eq!(file_type("https://x.org/a/b", Some("image/png")), Some("png"));
    }

    #[test]
    fn unknown_types_are_not_offered() {
        assert_eq!(file_type("https://x.org/a/b.mp3", Some("audio/mpeg")), None);
        assert!(!is_supported_mime("application/x-unknown"));
        assert!(is_supported_mime("application/vnd.comicbook+zip"));
    }

    #[test]
    fn book_names_follow_the_plugin() {
        assert_eq!(
            book_file_name("Jane Doe - A Tale", "EPUB"),
            "Jane Doe - A Tale.epub"
        );
        assert_eq!(
            book_file_name("Jane/Doe - Who? Why: \"Now\"", "pdf"),
            "Jane_Doe - Who_ Why_ _Now_.pdf"
        );
        // Trailing dots and spaces go, as KOReader's replaceAllInvalidChars does.
        assert_eq!(book_file_name("Title. ", "txt"), "Title.txt");
        assert_eq!(book_file_name("Line\nbreak\tTab", "txt"), "Line break Tab.txt");
    }

    #[test]
    fn long_names_keep_the_extension() {
        let long = format!("{}.epub", "x".repeat(300));
        let name = finish(&long, "book");
        assert!(name.ends_with(".epub"));
        assert_eq!(name.len(), NAME_LIMIT + ".epub".len());
    }

    #[test]
    fn long_multibyte_names_cut_on_a_char_boundary() {
        let title = "\u{e9}".repeat(200);
        let name = book_file_name(&title, "epub");
        assert_eq!(name.len(), NAME_LIMIT + ".epub".len());
        assert!(name.ends_with(".epub"));
    }

    #[test]
    fn empty_book_name_gets_a_fallback() {
        assert_eq!(book_file_name("", "epub"), "book.epub");
    }

    #[test]
    fn server_names_come_from_the_header_or_the_path() {
        let url = Url::parse("https://x.org/dl/get?id=3").expect("url");
        assert_eq!(
            server_file_name(
                Some("attachment; filename=\"Moby Dick.epub\""),
                &url,
                Some("epub")
            ),
            "Moby Dick.epub"
        );
        assert_eq!(
            server_file_name(
                Some("attachment; filename=moby%20dick.txt; size=3"),
                &url,
                Some("txt")
            ),
            "moby dick.txt"
        );
        let plain = Url::parse("https://x.org/files/1342.epub").expect("url");
        assert_eq!(server_file_name(None, &plain, Some("epub")), "1342.epub");
        // No extension in the name: the reader adds the file type.
        let bare = Url::parse("https://x.org/files/download").expect("url");
        assert_eq!(server_file_name(None, &bare, Some("pdf")), "download.pdf");
    }
}
