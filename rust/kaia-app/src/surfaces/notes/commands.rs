//! Commands of the notes surface. Every id starts with `notes:`.
//!
//! - `notes:filter:<all|highlights|notes|bookmarks>` sets the filter.
//! - `notes:book:<partial md5>` opens a book's list.
//! - `notes:books` returns to the list of books.
//! - `notes:export` offers export formats.
//! - `notes:export:text` and `notes:export:pdf` write to `<data>/exports/`.
//!
//! Tapping an entry does not use these ids. The entry sends the global
//! `open_book:<id>` (see the crate README).

use super::data::Filter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Filter(Filter),
    Book(String),
    Books,
    Export,
    ExportText,
    ExportPdf,
}

/// Parses a notes action id. None for every other id.
pub fn parse(id: &str) -> Option<Command> {
    let rest = id.strip_prefix("notes:")?;
    match rest {
        "books" => Some(Command::Books),
        "export" => Some(Command::Export),
        "export:text" => Some(Command::ExportText),
        "export:pdf" => Some(Command::ExportPdf),
        _ => {
            if let Some(name) = rest.strip_prefix("filter:") {
                return Filter::parse(name).map(Command::Filter);
            }
            let md5 = rest.strip_prefix("book:")?;
            (!md5.is_empty()).then(|| Command::Book(md5.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_notes_ids() {
        assert_eq!(parse("notes:filter:notes"), Some(Command::Filter(Filter::Notes)));
        assert_eq!(parse("notes:filter:nope"), None);
        assert_eq!(parse("notes:book:abc"), Some(Command::Book("abc".into())));
        assert_eq!(parse("notes:book:"), None);
        assert_eq!(parse("notes:books"), Some(Command::Books));
        assert_eq!(parse("notes:export"), Some(Command::Export));
        assert_eq!(parse("settings:notes"), None);
    }
}
