//! Action ids of the Library surface and its book details page. Each id is
//! `library:<what>` with an optional `:<arg>`. Tiles send the book id as an
//! argument instead, so `library:open` and `library:details` also parse
//! without the suffix.

use kaia_events::Value;
use kaia_library::storage::Source;

use crate::book_index::Mark;

/// The tabs of the Library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Covers,
    List,
    Folders,
    Collections,
    Series,
    Authors,
}

/// Sort choices shown in the sort panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortChoice {
    Title,
    Author,
    Read,
    Added,
    Progress,
    Series,
}

/// The reading status a filter shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Reading,
    NotStarted,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `library:view:<name>`: switch the tab. `covers` and `list` also set the
    /// layout of the Books tab.
    View(View),
    /// `library:view:books`: the Books tab. It shows the layout the Books tab
    /// last had, covers or list.
    BooksTab,
    /// `library:sort`: open the sort panel.
    OpenSort,
    /// `library:sort:<key>`: sort by a key and close the panel.
    Sort(SortChoice),
    /// `library:filter`: open the filter panel.
    OpenFilter,
    /// `library:filter:all`: show every status. The other filters stay.
    AllBooks,
    /// `library:filter:<status>`: show the books with that status. The collection,
    /// source and file type filters stay.
    Chip(Chip),
    /// `library:filter:collection`: open the collection picker for the filter.
    FilterCollection,
    /// `library:collection:<id>` or `library:collection:none`: filter by a collection
    /// (None clears the collection filter). The status and other filters stay.
    /// The picker uses it too.
    PickCollection(Option<i64>),
    /// `library:filter:source:<calibre|opds|other>` or `library:filter:source:all`:
    /// show the books from one source. None shows every source.
    Source(Option<Source>),
    /// `library:filter:type`: open the file type picker.
    OpenFileType,
    /// `library:format:<extension>` or `library:format:all`: show the books of one
    /// file type. None shows every type.
    PickFormat(Option<String>),
    /// `library:rating:open:<book>`: show the rating sheet of a book.
    OpenRating(i64),
    /// `library:rating:back:<book>`: leave the rating sheet for the details.
    BackRating(i64),
    /// `library:rating:<book>:<stars>`: set the rating of a book from 1 to 5, or
    /// clear it with 0.
    SetRating {
        book: i64,
        rating: u8,
    },
    /// `library:search:open` and `library:search:close`.
    SearchOpen,
    SearchClose,
    /// The search field changed. Carries the text.
    SearchChanged(String),
    /// `library:search:submit`: the Enter key of the search field.
    SearchSubmit,
    /// `library:search:recent:<n>`: search for the n-th recent search.
    RecentSearch(usize),
    /// `library:search:recent:clear`: forget the recent searches.
    ClearRecentSearches,
    /// `library:search:suggest:<n>`: search for the n-th suggestion, or open its book.
    Suggestion(usize),
    /// `library:folder:<n>` or `library:folder:up`.
    Folder(FolderStep),
    /// `library:series:<n>` opens a series, and `library:series:up` closes it.
    Series(SeriesStep),
    /// `library:author:<n>` opens an author, and `library:author:up` closes it.
    Author(AuthorStep),
    /// `library:open:<id>`: open the book in the reader.
    Open(i64),
    /// `library:details:<id>`: show the details of a book (hold).
    Details(i64),
    /// `library:book:finished:<id>`.
    MarkFinished(i64),
    /// `library:book:collection:<id>`: the collection picker for one book.
    AddToCollection(i64),
    /// `library:add:<book>:<collection>`: add a book to an existing collection.
    AddBookTo {
        book: i64,
        collection: i64,
    },
    /// `library:remove:<book>:<collection>`: take a book out of a collection.
    RemoveFromCollection {
        book: i64,
        collection: i64,
    },
    /// `library:add:new:<book>`: name a new collection for a book.
    NewCollection(i64),
    /// `library:name:save`: create the named collection and add the book.
    SaveName,
    /// `library:name:<text>` from the name field's change event.
    NameChanged(String),
    /// `library:book:delete:<id>`: ask before deleting the file.
    AskDelete(i64),
    /// `library:book:delete-confirm:<id>`: delete the file.
    ConfirmDelete(i64),
    /// `library:panel:close`: close the sheet and show the list again.
    ClosePanel,
    CreateCollection,
    RenameCollection(i64),
    DeleteCollection(i64),
    ConfirmDeleteCollection(i64),
    ReorderCollection(i64, usize),
    RemoveHistory(i64),
    ClearHistory,
    ConfirmClearHistory,
    /// `library:select:on` and `library:select:off`: enter or leave select mode.
    SelectOn,
    SelectOff,
    /// `library:select:toggle:<id>`: check or uncheck a book.
    SelectToggle(i64),
    /// `library:select:all` and `library:select:none`.
    SelectAll,
    SelectNone,
    /// `library:select:finished` and `library:select:unread`: mark the checked books.
    MarkSelected(Mark),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderStep {
    Up,
    Into(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeriesStep {
    Up,
    Into(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorStep {
    Up,
    Into(usize),
}

/// Parses a library action id. `args` carries the value a control sent, such as
/// the book id of a tile or the text of a field. None for ids that are not the
/// library's.
pub fn parse(id: &str, args: &[Value]) -> Option<Command> {
    let rest = id.strip_prefix("library:")?;
    let arg_text = args.first().and_then(Value::as_str).map(str::to_owned);
    let arg_id = args
        .first()
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<i64>().ok());
    let (head, tail) = match rest.split_once(':') {
        Some((h, t)) => (h, Some(t)),
        None => (rest, None),
    };
    let tail_id = tail.and_then(|t| t.parse::<i64>().ok());
    if let Some(rest) = rest.strip_prefix("collections:") {
        let parts: Vec<_> = rest.split(':').collect();
        return Some(match parts.as_slice() {
            ["new"] => Command::CreateCollection,
            ["rename", id] => Command::RenameCollection(id.parse().ok()?),
            ["delete", id] => Command::DeleteCollection(id.parse().ok()?),
            ["delete-confirm", id] => Command::ConfirmDeleteCollection(id.parse().ok()?),
            ["move", id, position] => Command::ReorderCollection(id.parse().ok()?, position.parse().ok()?),
            _ => return None,
        });
    }
    Some(match (head, tail) {
        ("history", Some("clear")) => Command::ClearHistory,
        ("history", Some("clear-confirm")) => Command::ConfirmClearHistory,
        ("view", Some("books")) => Command::BooksTab,
        ("view", Some(v)) => Command::View(match v {
            "covers" => View::Covers,
            "list" => View::List,
            "folders" => View::Folders,
            "collections" => View::Collections,
            "series" => View::Series,
            "authors" => View::Authors,
            _ => return None,
        }),
        ("sort", None) => Command::OpenSort,
        ("sort", Some(k)) => Command::Sort(match k {
            "title" => SortChoice::Title,
            "author" => SortChoice::Author,
            "read" => SortChoice::Read,
            "added" => SortChoice::Added,
            "progress" => SortChoice::Progress,
            "series" => SortChoice::Series,
            _ => return None,
        }),
        ("filter", None) => Command::OpenFilter,
        ("filter", Some("all")) => Command::AllBooks,
        ("filter", Some("reading")) => Command::Chip(Chip::Reading),
        ("filter", Some("not-started")) => Command::Chip(Chip::NotStarted),
        ("filter", Some("finished")) => Command::Chip(Chip::Finished),
        ("filter", Some("collection")) => Command::FilterCollection,
        ("filter", Some("type")) => Command::OpenFileType,
        ("filter", Some(t)) if t.starts_with("source:") => Command::Source(match &t["source:".len()..] {
            "all" => None,
            "calibre" => Some(Source::Calibre),
            "opds" => Some(Source::Opds),
            "other" => Some(Source::Other),
            _ => return None,
        }),
        ("format", Some("all")) => Command::PickFormat(None),
        ("format", Some(f)) if !f.is_empty() && f.chars().all(|c| c.is_ascii_alphanumeric()) => {
            Command::PickFormat(Some(f.to_ascii_lowercase()))
        }
        ("rating", Some(t)) => match t.split_once(':')? {
            ("open", book) => Command::OpenRating(book.parse().ok()?),
            ("back", book) => Command::BackRating(book.parse().ok()?),
            (book, stars) => {
                let rating: u8 = stars.parse().ok()?;
                if rating > 5 {
                    return None;
                }
                Command::SetRating {
                    book: book.parse().ok()?,
                    rating,
                }
            }
        },
        ("collection", Some("none")) => Command::PickCollection(None),
        ("collection", _) => Command::PickCollection(Some(tail_id.or(arg_id)?)),
        ("search", Some("open")) => Command::SearchOpen,
        ("search", Some("close")) => Command::SearchClose,
        ("search", Some("submit")) => Command::SearchSubmit,
        ("search", Some(t)) if t.starts_with("recent:") => match &t["recent:".len()..] {
            "clear" => Command::ClearRecentSearches,
            n => Command::RecentSearch(n.parse().ok()?),
        },
        ("search", Some(t)) if t.starts_with("suggest:") => {
            Command::Suggestion(t["suggest:".len()..].parse().ok()?)
        }
        ("folder", Some("up")) => Command::Folder(FolderStep::Up),
        ("folder", Some(n)) => Command::Folder(FolderStep::Into(n.parse().ok()?)),
        ("series", Some("up")) => Command::Series(SeriesStep::Up),
        ("series", Some(n)) => Command::Series(SeriesStep::Into(n.parse().ok()?)),
        ("author", Some("up")) => Command::Author(AuthorStep::Up),
        ("author", Some(n)) => Command::Author(AuthorStep::Into(n.parse().ok()?)),
        ("open", _) => Command::Open(tail_id.or(arg_id)?),
        ("details", _) => Command::Details(tail_id.or(arg_id)?),
        ("book", Some(t)) => {
            let (action, id) = t.split_once(':')?;
            let id: i64 = id.parse().ok()?;
            match action {
                "history" => Command::RemoveHistory(id),
                "finished" => Command::MarkFinished(id),
                "collection" => Command::AddToCollection(id),
                "delete" => Command::AskDelete(id),
                "delete-confirm" => Command::ConfirmDelete(id),
                _ => return None,
            }
        }
        ("add", Some(t)) => {
            let (book, collection) = t.split_once(':')?;
            let book: i64 = book.parse().ok()?;
            if collection == "new" {
                Command::NewCollection(book)
            } else {
                Command::AddBookTo {
                    book,
                    collection: collection.parse().ok()?,
                }
            }
        }
        ("remove", Some(t)) => {
            let (book, collection) = t.split_once(':')?;
            Command::RemoveFromCollection {
                book: book.parse().ok()?,
                collection: collection.parse().ok()?,
            }
        }
        ("select", Some("on")) => Command::SelectOn,
        ("select", Some("off")) => Command::SelectOff,
        ("select", Some("all")) => Command::SelectAll,
        ("select", Some("none")) => Command::SelectNone,
        ("select", Some("finished")) => Command::MarkSelected(Mark::Finished),
        ("select", Some("unread")) => Command::MarkSelected(Mark::Unread),
        ("select", Some(t)) if t.starts_with("toggle:") => {
            Command::SelectToggle(t["toggle:".len()..].parse().ok()?)
        }
        ("name", Some("save")) => Command::SaveName,
        ("name", None) => Command::NameChanged(arg_text?),
        ("search", None) => Command::SearchChanged(arg_text?),
        ("panel", Some("close")) => Command::ClosePanel,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> Vec<Value> {
        Vec::new()
    }

    #[test]
    fn parses_views_sorts_and_chips() {
        assert_eq!(
            parse("library:view:list", &none()),
            Some(Command::View(View::List))
        );
        assert_eq!(
            parse("library:view:series", &none()),
            Some(Command::View(View::Series))
        );
        assert_eq!(
            parse("library:view:authors", &none()),
            Some(Command::View(View::Authors))
        );
        assert_eq!(parse("library:sort", &none()), Some(Command::OpenSort));
        assert_eq!(
            parse("library:sort:read", &none()),
            Some(Command::Sort(SortChoice::Read))
        );
        assert_eq!(
            parse("library:filter:not-started", &none()),
            Some(Command::Chip(Chip::NotStarted))
        );
        assert_eq!(
            parse("library:filter:collection", &none()),
            Some(Command::FilterCollection)
        );
        assert_eq!(parse("library:filter", &none()), Some(Command::OpenFilter));
        assert_eq!(parse("library:filter:all", &none()), Some(Command::AllBooks));
        assert_eq!(parse("library:view:books", &none()), Some(Command::BooksTab));
    }

    #[test]
    fn parses_source_type_and_rating_actions() {
        assert_eq!(
            parse("library:filter:source:opds", &none()),
            Some(Command::Source(Some(Source::Opds)))
        );
        assert_eq!(
            parse("library:filter:source:all", &none()),
            Some(Command::Source(None))
        );
        assert_eq!(parse("library:filter:source:nope", &none()), None);
        assert_eq!(parse("library:filter:type", &none()), Some(Command::OpenFileType));
        assert_eq!(
            parse("library:format:epub", &none()),
            Some(Command::PickFormat(Some("epub".into())))
        );
        assert_eq!(
            parse("library:format:all", &none()),
            Some(Command::PickFormat(None))
        );
        assert_eq!(parse("library:format:ep/ub", &none()), None);
        assert_eq!(
            parse("library:rating:42:4", &none()),
            Some(Command::SetRating { book: 42, rating: 4 })
        );
        assert_eq!(
            parse("library:rating:42:0", &none()),
            Some(Command::SetRating { book: 42, rating: 0 })
        );
        assert_eq!(parse("library:rating:42:6", &none()), None);
        assert_eq!(
            parse("library:rating:open:42", &none()),
            Some(Command::OpenRating(42))
        );
        assert_eq!(
            parse("library:rating:back:42", &none()),
            Some(Command::BackRating(42))
        );
    }

    #[test]
    fn parses_book_ids_from_the_id_or_the_arguments() {
        assert_eq!(parse("library:open:42", &none()), Some(Command::Open(42)));
        assert_eq!(
            parse("library:open", &[Value::Str("7".into())]),
            Some(Command::Open(7))
        );
        assert_eq!(
            parse("library:details", &[Value::Str("9".into())]),
            Some(Command::Details(9))
        );
        assert_eq!(
            parse("library:book:delete:3", &none()),
            Some(Command::AskDelete(3))
        );
        assert_eq!(
            parse("library:add:3:5", &none()),
            Some(Command::AddBookTo {
                book: 3,
                collection: 5
            })
        );
        assert_eq!(
            parse("library:add:3:new", &none()),
            Some(Command::NewCollection(3))
        );
        assert_eq!(
            parse("library:remove:3:5", &none()),
            Some(Command::RemoveFromCollection {
                book: 3,
                collection: 5
            })
        );
    }

    #[test]
    fn parses_text_events_and_folders() {
        assert_eq!(
            parse("library:name", &[Value::Str("Sci-fi".into())]),
            Some(Command::NameChanged("Sci-fi".into()))
        );
        assert_eq!(
            parse("library:search", &[Value::Str("leaves".into())]),
            Some(Command::SearchChanged("leaves".into()))
        );
        assert_eq!(
            parse("library:folder:2", &none()),
            Some(Command::Folder(FolderStep::Into(2)))
        );
        assert_eq!(
            parse("library:folder:up", &none()),
            Some(Command::Folder(FolderStep::Up))
        );
        assert_eq!(
            parse("library:series:3", &none()),
            Some(Command::Series(SeriesStep::Into(3)))
        );
        assert_eq!(
            parse("library:series:up", &none()),
            Some(Command::Series(SeriesStep::Up))
        );
        assert_eq!(
            parse("library:author:4", &none()),
            Some(Command::Author(AuthorStep::Into(4)))
        );
        assert_eq!(
            parse("library:author:up", &none()),
            Some(Command::Author(AuthorStep::Up))
        );
        assert_eq!(parse("library:author:x", &none()), None);
    }

    #[test]
    fn parses_recent_searches_and_select_mode() {
        assert_eq!(
            parse("library:search:submit", &none()),
            Some(Command::SearchSubmit)
        );
        assert_eq!(
            parse("library:search:recent:2", &none()),
            Some(Command::RecentSearch(2))
        );
        assert_eq!(
            parse("library:search:recent:clear", &none()),
            Some(Command::ClearRecentSearches)
        );
        assert_eq!(
            parse("library:search:suggest:4", &none()),
            Some(Command::Suggestion(4))
        );
        assert_eq!(parse("library:search:suggest:x", &none()), None);
        assert_eq!(parse("library:select:on", &none()), Some(Command::SelectOn));
        assert_eq!(
            parse("library:select:toggle:12", &[Value::Bool(true)]),
            Some(Command::SelectToggle(12))
        );
        assert_eq!(
            parse("library:select:unread", &none()),
            Some(Command::MarkSelected(Mark::Unread))
        );
        assert_eq!(
            parse("library:select:finished", &none()),
            Some(Command::MarkSelected(Mark::Finished))
        );
        assert_eq!(parse("library:select:toggle:x", &none()), None);
    }

    #[test]
    fn rejects_other_ids() {
        assert_eq!(parse("library:open", &none()), None);
        assert_eq!(parse("library:view:maps", &none()), None);
        assert_eq!(parse("library:next", &none()), None);
        assert_eq!(parse("reader:overlay", &none()), None);
        assert_eq!(parse("nav:library", &none()), None);
    }
}
