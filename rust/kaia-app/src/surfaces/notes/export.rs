//! Exports notes to Markdown and JSON, or one PDF per book, in `<data>/exports/`.
//!
//! The JSON is written by hand. `kaia-app` has no JSON crate of its own, and
//! the output is a fixed shape of strings, numbers, booleans and arrays.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use kaia_i18n::{format as tr_format, tr};

use super::data::{BookNotes, Location, Note};

/// The folder under the data folder that holds exports.
pub const EXPORTS_DIR: &str = "exports";

/// The files one export wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    pub markdown: PathBuf,
    pub json: PathBuf,
    /// Entries written, including one ink count per page.
    pub count: usize,
}

/// Writes both files for `books` into `<data_dir>/exports/`. `now` is Unix
/// seconds and names the files. The folder is created when missing.
pub fn export(data_dir: &Path, books: &[BookNotes], now: i64) -> io::Result<Exported> {
    let dir = data_dir.join(EXPORTS_DIR);
    fs::create_dir_all(&dir)?;
    let stamp = timestamp(now).replace(':', "-").replace(' ', "T");
    let markdown = dir.join(format!("kaia-notes-{stamp}.md"));
    let json = dir.join(format!("kaia-notes-{stamp}.json"));
    crate::durable::atomic_write(&markdown, markdown_text(books, now).as_bytes())?;
    crate::durable::atomic_write(&json, json_text(books, now).as_bytes())?;
    let count = books.iter().map(|b| b.notes.len() + b.ink_counts.len()).sum();
    Ok(Exported {
        markdown,
        json,
        count,
    })
}

/// Writes one PDF per book. Each completed file replaces its previous export.
/// `fonts` is the bundled fonts directory, with its `noto` subdirectory.
pub fn export_pdf(data_dir: &Path, books: &[BookNotes], fonts: &Path) -> io::Result<Vec<PathBuf>> {
    let dir = data_dir.join(EXPORTS_DIR);
    fs::create_dir_all(&dir)?;
    for face in ["NotoSans-Regular.ttf", "NotoSans-Bold.ttf"] {
        fs::File::open(fonts.join("noto").join(face))?;
    }
    let mut paths = Vec::with_capacity(books.len());
    let mut css = String::from(PDF_CSS);
    if fonts.join("noto/NotoSansCJKsc-Regular.otf").is_file() {
        css.push_str("@font-face { font-family: notes-cjk; src: url(noto/NotoSansCJKsc-Regular.otf); } .cjk { font-family: notes-cjk; }");
    }
    for book in books {
        // Encode the identifier so even an imported state file cannot name a path.
        let key: String = book.md5.bytes().map(|byte| format!("{byte:02x}")).collect();
        if key.is_empty() || key.len() > 128 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid book identifier",
            ));
        }
        let path = dir.join(format!("kaia-notes-{key}.pdf"));
        let temp = tempfile::NamedTempFile::new_in(&dir)?;
        kaia_mupdf_sys::write_story_pdf(temp.path(), &pdf_html(book), &css, fonts)
            .map_err(io::Error::other)?;
        temp.as_file().sync_all()?;
        temp.persist(&path).map_err(|error| error.error)?;
        fs::File::open(&dir)?.sync_all()?;
        paths.push(path);
    }
    Ok(paths)
}

const PDF_CSS: &str = "
@font-face { font-family: notes; src: url(noto/NotoSans-Regular.ttf); }
@font-face { font-family: notes; font-weight: bold; src: url(noto/NotoSans-Bold.ttf); }
body { font-family: notes; margin: 0; line-height: 1.4; overflow-wrap: break-word; }
h1 { font-size: 22pt; margin: 0 0 8pt; }
h2 { font-size: 14pt; margin: 18pt 0 8pt; page-break-after: avoid; }
p { margin: 6pt 0; }
blockquote { margin: 8pt 0 8pt 14pt; border-left: 1pt solid #888; padding-left: 10pt; }
.position { font-size: 9pt; color: #555; margin-bottom: 14pt; }
";

fn pdf_html(book: &BookNotes) -> String {
    let mut out = format!("<html><body><h1>{}</h1>", html_text(&book.title));
    if let Some(author) = &book.author {
        let _ = write!(out, "<p>{}</p>", html_text(author));
    }
    let mut previous_chapter = None;
    for note in &book.notes {
        let chapter = note.chapter.as_deref().filter(|c| !c.trim().is_empty());
        if previous_chapter != Some(chapter) {
            let _ = write!(
                out,
                "<h2>{}</h2>",
                html_text(&chapter.map(str::to_owned).unwrap_or_else(|| tr("Notes")))
            );
            previous_chapter = Some(chapter);
        }
        if let Some(text) = note.text.as_deref().filter(|t| !t.trim().is_empty()) {
            let _ = write!(out, "<blockquote>{}</blockquote>", html_text(text));
        }
        if let Some(text) = note.note.as_deref().filter(|t| !t.trim().is_empty()) {
            let _ = write!(out, "<p>{}</p>", html_text(text));
        }
        let position = match (&note.location, note.pageno) {
            (_, Some(page)) => tr_format(&tr("Page %1"), &[&page.to_string()]),
            (Location::Page(page), None) => tr_format(&tr("Page %1"), &[&(u64::from(*page) + 1).to_string()]),
            (Location::Xpointer(xp), None) => xp.clone(),
        };
        let _ = write!(
            out,
            "<p class='position'>{}<br/>{}</p>",
            html_text(&position),
            html_text(&note.datetime)
        );
    }
    for (page, count) in &book.ink_counts {
        let _ = write!(
            out,
            "<p>{}</p>",
            html_text(&tr_format(
                &tr("Page %1: %2 ink strokes"),
                &[&page.to_string(), &count.to_string()]
            ))
        );
    }
    out.push_str("</body></html>");
    out
}

/// User text is always text, never markup or a resource URL.
fn html_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("<br/>"),
            '\0' => out.push('\u{fffd}'),
            '\u{2e80}'..='\u{9fff}'
            | '\u{ac00}'..='\u{d7af}'
            | '\u{f900}'..='\u{faff}'
            | '\u{ff00}'..='\u{ffef}'
            | '\u{20000}'..='\u{3134f}' => {
                let _ = write!(out, "<span class='cjk'>{c}</span>");
            }
            _ => out.push(c),
        }
    }
    out
}

/// The Markdown text: one heading per book, one list item per entry.
pub fn markdown_text(books: &[BookNotes], now: i64) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# {}\n", tr("Kaia notes"));
    let _ = writeln!(out, "{}\n", tr_format(&tr("Exported %1."), &[&timestamp(now)]));
    for book in books {
        let _ = writeln!(out, "## {}\n", one_line(&book.title));
        for note in &book.notes {
            let _ = writeln!(out, "- {}", one_line(&note.secondary()));
            if let Some(text) = note.text.as_deref().filter(|t| !t.trim().is_empty()) {
                let _ = writeln!(out, "  > {}", one_line(text));
            }
            if let Some(text) = note.note.as_deref().filter(|t| !t.trim().is_empty()) {
                let _ = writeln!(out, "  {}", one_line(text));
            }
        }
        for (page, count) in &book.ink_counts {
            let _ = writeln!(
                out,
                "- {}",
                tr_format(
                    &tr("Page %1: %2 ink strokes"),
                    &[&page.to_string(), &count.to_string()]
                )
            );
        }
        out.push('\n');
    }
    out
}

/// The JSON text: an object with the export time and one entry per book.
pub fn json_text(books: &[BookNotes], now: i64) -> String {
    let mut out = String::from("{\n");
    let _ = writeln!(out, "  \"exported_at\": {},", json_string(&timestamp(now)));
    out.push_str("  \"books\": [\n");
    for (i, book) in books.iter().enumerate() {
        out.push_str("    {\n");
        let _ = writeln!(out, "      \"md5\": {},", json_string(&book.md5));
        let _ = writeln!(out, "      \"title\": {},", json_string(&book.title));
        match book.library_id {
            Some(id) => {
                let _ = writeln!(out, "      \"library_id\": {id},");
            }
            None => out.push_str("      \"library_id\": null,\n"),
        }
        let ink = book
            .ink_counts
            .iter()
            .map(|(page, count)| format!("{{\"page\": {page}, \"count\": {count}}}"))
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(out, "      \"ink_counts\": [{ink}],");
        out.push_str("      \"annotations\": [\n");
        for (j, note) in book.notes.iter().enumerate() {
            out.push_str(&json_note(note));
            out.push_str(if j + 1 < book.notes.len() { ",\n" } else { "\n" });
        }
        out.push_str("      ]\n");
        out.push_str(if i + 1 < books.len() {
            "    },\n"
        } else {
            "    }\n"
        });
    }
    out.push_str("  ]\n}\n");
    out
}

fn json_note(note: &Note) -> String {
    let (location_key, location_value) = match &note.location {
        Location::Xpointer(xp) => ("xpointer", json_string(xp)),
        Location::Page(p) => ("page", (p + 1).to_string()),
    };
    let fields = [
        format!("\"id\": {}", note.id),
        format!("\"sticker_id\": {}", json_opt(note.sticker.as_deref())),
        format!("\"datetime\": {}", json_string(&note.datetime)),
        format!("\"highlight\": {}", note.highlight),
        format!("\"bookmark\": {}", note.bookmark),
        format!("\"text\": {}", json_opt(note.text.as_deref())),
        format!("\"note\": {}", json_opt(note.note.as_deref())),
        format!("\"chapter\": {}", json_opt(note.chapter.as_deref())),
        match note.pageno {
            Some(p) => format!("\"pageno\": {p}"),
            None => "\"pageno\": null".to_string(),
        },
        format!("\"{location_key}\": {location_value}"),
        format!("\"orphaned\": {}", note.orphaned),
    ];
    format!("        {{ {} }}", fields.join(", "))
}

fn json_opt(s: Option<&str>) -> String {
    s.map_or_else(|| "null".to_string(), json_string)
}

/// A JSON string literal with the escapes JSON requires.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Markdown needs one line per entry, so line breaks become spaces.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `YYYY-MM-DD HH:MM:SS` (UTC) for Unix seconds. Used in file names and in the text.
pub fn timestamp(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// The civil date of a day count since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_utc_dates() {
        assert_eq!(timestamp(0), "1970-01-01 00:00:00");
        assert_eq!(timestamp(1_700_000_000), "2023-11-14 22:13:20");
    }

    #[test]
    fn json_strings_escape_quotes_and_controls() {
        assert_eq!(json_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
        assert_eq!(json_string("\u{1}"), "\"\\u0001\"");
    }
}
