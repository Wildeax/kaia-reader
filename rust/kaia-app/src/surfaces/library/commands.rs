//! Commands of the library surface: `library:next`, `library:prev` and
//! `library:book:<id>` (open the details of a book).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStep {
    Next,
    Previous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Turn a page of the list.
    Page(PageStep),
    /// Open the details of the book with this row id.
    Book(i64),
}

/// Parses a library action id. None for every other id.
pub fn parse(id: &str) -> Option<Command> {
    let rest = id.strip_prefix("library:")?;
    Some(match rest {
        "next" => Command::Page(PageStep::Next),
        "prev" => Command::Page(PageStep::Previous),
        _ => Command::Book(rest.strip_prefix("book:")?.parse().ok()?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pages_and_book_ids() {
        assert_eq!(parse("library:next"), Some(Command::Page(PageStep::Next)));
        assert_eq!(parse("library:prev"), Some(Command::Page(PageStep::Previous)));
        assert_eq!(parse("library:book:7"), Some(Command::Book(7)));
    }

    #[test]
    fn rejects_other_ids() {
        assert_eq!(parse("library:book:"), None);
        assert_eq!(parse("library:book:x"), None);
        assert_eq!(parse("book:7"), None);
        assert_eq!(parse("reader:overlay"), None);
    }
}
