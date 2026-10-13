//! App commands: the action ids that widgets send, such as `nav:library` or `open_book:42`.
//!
//! Widgets send them as [`kaia_ui::AppAction`]s or as named events. The app turns
//! them into navigation, and an id it does not know is recorded and ignored.

use std::path::PathBuf;

use crate::router::Route;

/// What an action id asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `nav:home`, `nav:library`, `nav:notes`, `nav:settings`: replace the root surface.
    Navigate(Route),
    /// `open_book:<id or path>`: open the book in the reader.
    OpenBook(BookRef),
    /// `back`: close the top pushed surface, or go to Home from a root.
    Back,
    /// `library:next` and `library:prev`: turn a page of the library list.
    LibraryPage(PageStep),
    /// `reader:overlay`: show the reader overlay (Back button and page number).
    ReaderOverlay,
    /// `book:<id>`: open the details of a book.
    BookDetails(i64),
    /// `settings:<name>`: open a settings section.
    SettingsSection(String),
    /// `quit`: stop the loop after this iteration.
    Quit,
}

/// A book named by a command: a library row id, or a file path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookRef {
    Id(i64),
    Path(PathBuf),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageStep {
    Next,
    Previous,
}

impl Command {
    /// Parses an action id. Returns None for ids the app does not know.
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
            ("library", Some("next")) => Some(Command::LibraryPage(PageStep::Next)),
            ("library", Some("prev")) => Some(Command::LibraryPage(PageStep::Previous)),
            ("reader", Some("overlay")) => Some(Command::ReaderOverlay),
            ("book", Some(arg)) => arg.parse::<i64>().ok().map(Command::BookDetails),
            ("settings", Some(name)) if !name.is_empty() => Some(Command::SettingsSection(name.to_string())),
            ("quit", None) => Some(Command::Quit),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_navigation_and_book_ids() {
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
        assert_eq!(Command::parse("book:7"), Some(Command::BookDetails(7)));
    }

    #[test]
    fn rejects_unknown_and_empty_ids() {
        assert_eq!(Command::parse("nav:nowhere"), None);
        assert_eq!(Command::parse("open_book:"), None);
        assert_eq!(Command::parse("settings:"), None);
        assert_eq!(Command::parse("Back"), None);
    }
}
