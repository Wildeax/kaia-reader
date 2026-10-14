//! Exports all notes to Markdown and JSON in `<data>/exports/`.
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
    /// Annotations written, across all books.
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
    let count = books.iter().map(|b| b.notes.len()).sum();
    Ok(Exported {
        markdown,
        json,
        count,
    })
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
