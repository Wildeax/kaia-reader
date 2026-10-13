//! Action ids of the Library surface and its book details page. Each id is
//! `library:<what>` with an optional `:<arg>`. Tiles send the book id as an
//! argument instead, so `library:open` and `library:details` also parse
//! without the suffix.

use kaia_events::Value;

/// The four tabs of the Library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Covers,
    List,
    Folders,
    Collections,
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

/// Filter chips for the reading status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    Reading,
    NotStarted,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `library:view:<name>`: switch the tab.
    View(View),
    /// `library:sort`: open the sort panel.
    OpenSort,
    /// `library:sort:<key>`: sort by a key and close the panel.
    Sort(SortChoice),
    /// `library:filter:<chip>`: toggle a status chip.
    Chip(Chip),
    /// `library:filter:collection`: toggle the collection chip. Opens the picker when it is off.
    CollectionChip,
    /// `library:collection:<id>` or `library:collection:none`: filter by a collection
    /// (None clears the filter). The picker uses it too.
    PickCollection(Option<i64>),
    /// `library:search:open` and `library:search:close`.
    SearchOpen,
    SearchClose,
    /// The search field changed. Carries the text.
    SearchChanged(String),
    /// `library:folder:<n>` or `library:folder:up`.
    Folder(FolderStep),
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderStep {
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
        ("view", Some(v)) => Command::View(match v {
            "covers" => View::Covers,
            "list" => View::List,
            "folders" => View::Folders,
            "collections" => View::Collections,
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
        ("filter", Some("reading")) => Command::Chip(Chip::Reading),
        ("filter", Some("not-started")) => Command::Chip(Chip::NotStarted),
        ("filter", Some("finished")) => Command::Chip(Chip::Finished),
        ("filter", Some("collection")) => Command::CollectionChip,
        ("collection", Some("none")) => Command::PickCollection(None),
        ("collection", _) => Command::PickCollection(Some(tail_id.or(arg_id)?)),
        ("search", Some("open")) => Command::SearchOpen,
        ("search", Some("close")) => Command::SearchClose,
        ("folder", Some("up")) => Command::Folder(FolderStep::Up),
        ("folder", Some(n)) => Command::Folder(FolderStep::Into(n.parse().ok()?)),
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
            Some(Command::CollectionChip)
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
