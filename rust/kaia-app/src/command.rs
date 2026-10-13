//! Global commands: the action ids the app owns. They are `nav:home`,
//! `nav:library`, `nav:notes`, `nav:settings`, `open_book:<id or path>`, `back`
//! and `quit`.
//!
//! Every other id belongs to a surface. Each surface parses its own ids in its
//! own module (`surfaces/<name>/commands.rs`). See the crate README for the
//! naming rule.

use std::path::PathBuf;

use crate::surfaces::Route;

/// A global command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `nav:home`, `nav:library`, `nav:notes`, `nav:settings`: replace the root.
    Navigate(Route),
    /// `open_book:<id or path>`: open the book in the reader.
    OpenBook(BookRef),
    /// `back`: close the top pushed screen, or go to Home from a root.
    Back,
    /// `quit`: stop the loop after this iteration.
    Quit,
}

/// A book named by a command: a library row id, or a file path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookRef {
    Id(i64),
    Path(PathBuf),
}

impl Command {
    /// Parses a global action id. Returns None for every other id.
    pub fn parse(id: &str) -> Option<Command> {
        let (head, arg) = match id.split_once(':') {
            Some((head, arg)) => (head, Some(arg)),
            None => (id, None),
        };
        match (head, arg) {
            ("nav", Some("home")) => Some(Command::Navigate(Route::Home)),
            ("nav", Some("library")) => Some(Command::Navigate(Route::Library { page: 0 })),
            ("nav", Some("notes")) => Some(Command::Navigate(Route::Notes)),
            ("nav", Some("settings")) => Some(Command::Navigate(Route::Settings)),
            ("open_book", Some(arg)) if !arg.is_empty() => {
                Some(Command::OpenBook(match arg.parse::<i64>() {
                    Ok(id) => BookRef::Id(id),
                    Err(_) => BookRef::Path(PathBuf::from(arg)),
                }))
            }
            ("back", None) => Some(Command::Back),
            ("quit", None) => Some(Command::Quit),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_navigation_and_books() {
        assert_eq!(Command::parse("nav:home"), Some(Command::Navigate(Route::Home)));
        assert_eq!(
            Command::parse("nav:library"),
            Some(Command::Navigate(Route::Library { page: 0 }))
        );
        assert_eq!(
            Command::parse("open_book:42"),
            Some(Command::OpenBook(BookRef::Id(42)))
        );
        assert_eq!(
            Command::parse("open_book:/books/a:b.epub"),
            Some(Command::OpenBook(BookRef::Path(PathBuf::from("/books/a:b.epub"))))
        );
        assert_eq!(Command::parse("back"), Some(Command::Back));
        assert_eq!(Command::parse("quit"), Some(Command::Quit));
    }

    #[test]
    fn leaves_surface_ids_and_rejects_bad_ids() {
        assert_eq!(Command::parse("library:next"), None);
        assert_eq!(Command::parse("reader:overlay"), None);
        assert_eq!(Command::parse("nav:nowhere"), None);
        assert_eq!(Command::parse("open_book:"), None);
        assert_eq!(Command::parse("Back"), None);
    }
}
