//! Notes: every book with highlights, notes or bookmarks, newest first. Tapping
//! a book opens its entries in reading order. Tapping an entry opens the book
//! at that place.
//!
//! The root is [`NotesSurface`], a unit struct so the route table can build it
//! with no state. A filter or book change replaces the root with a
//! [`NotesView`] that carries the new state (`Nav::Replace` on the root is a
//! root switch to the same route). See `data.rs` for where the notes come from.

pub mod commands;
pub mod data;
pub mod export;

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use kaia_events::Value;
use kaia_gfx::Size;
use kaia_library::{Book, Filter as LibraryFilter, Sort, SortKey};
use kaia_ui::components::{Expanded, Row};
use kaia_ui::composite::{Header, ListRow, PagedList};
use kaia_ui::controls::ButtonStyle;
use kaia_ui::{AppAction, Theme, WidgetPod, Window};

use crate::services::Services;
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{body, button, page, root_window as window};
use crate::surfaces::Route;
use commands::Command;
use data::{BookNotes, Filter, Note};

/// The Notes root. It shows the list of books. See the module docs.
pub struct NotesSurface;

impl Surface for NotesSurface {
    fn route(&self) -> Route {
        Route::Notes
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        NotesView::default().build(env)
    }

    fn on_command(&mut self, cmd: &str, _args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        handle(&NotesView::default(), cmd, env)
    }
}

/// The Notes root with a filter and an open book. Each change of state is a
/// new value of this type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotesView {
    pub filter: Filter,
    /// The partial MD5 of the open book. None shows the list of books.
    pub book: Option<String>,
}

impl Surface for NotesView {
    fn route(&self) -> Route {
        Route::Notes
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        let books = load_books(env.services);
        Some(match &self.book {
            Some(md5) => match books.iter().find(|b| &b.md5 == md5) {
                Some(book) => book_page(env.services, env.screen, book, self.filter),
                None => list_page(env.services, env.screen, &books, self.filter),
            },
            None => list_page(env.services, env.screen, &books, self.filter),
        })
    }

    fn on_command(&mut self, cmd: &str, _args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        handle(self, cmd, env)
    }
}

/// Runs a notes command against `state`. Returns NotMine for another id.
fn handle(state: &NotesView, cmd: &str, env: &mut SurfaceEnv<'_>) -> Outcome {
    match commands::parse(cmd) {
        Some(Command::Filter(filter)) => replace(NotesView {
            filter,
            book: state.book.clone(),
        }),
        Some(Command::Book(md5)) => replace(NotesView {
            filter: state.filter,
            book: Some(md5),
        }),
        Some(Command::Books) => replace(NotesView {
            filter: state.filter,
            book: None,
        }),
        Some(Command::Export) => {
            export_all(env);
            Outcome::Handled
        }
        None => Outcome::NotMine,
    }
}

fn replace(view: NotesView) -> Outcome {
    Outcome::Navigate(Nav::Replace(Box::new(view)))
}

/// Writes every note and shows the path in a toast.
pub fn export_all(env: &mut SurfaceEnv<'_>) {
    let books = load_books(env.services);
    if books.is_empty() {
        env.toast("There are no notes to export.", 3.0);
        return;
    }
    let data_dir = env.services.paths.data_dir.clone();
    match export::export(&data_dir, &books, now_unix()) {
        Ok(done) => {
            let message = format!("Exported {} notes to {}", done.count, done.markdown.display());
            env.toast(message, 6.0);
        }
        Err(e) => {
            env.toast(format!("Export failed: {e}"), 6.0);
        }
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The books with notes, read from the state files. The library gives the
/// titles and the library ids. A library that cannot be read gives no titles.
pub fn load_books(services: &Services) -> Vec<BookNotes> {
    let library: HashMap<String, Book> = services
        .library
        .books(
            Sort::new(SortKey::Title, false),
            &LibraryFilter::default(),
            0,
            100_000,
        )
        .unwrap_or_default()
        .into_iter()
        .map(|book| (book.partial_md5.clone(), book))
        .collect();
    data::load(&services.paths.data_dir, &library)
}

/// One line of a list: a book, or an entry of the open book.
#[derive(Clone)]
enum Item {
    Book {
        md5: String,
        title: String,
        detail: String,
    },
    Entry {
        primary: String,
        secondary: String,
        /// The action and its arguments, when the book is in the library.
        open: Option<(String, Vec<Value>)>,
        /// Position in the list, for the widget key.
        index: usize,
    },
}

fn filter_row(services: &Services, filter: Filter) -> WidgetPod {
    let buttons: Vec<WidgetPod> = Filter::ALL
        .into_iter()
        .map(|f| {
            let style = if f == filter {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            };
            WidgetPod::new(Expanded::new(
                button(
                    style,
                    f.label(),
                    &format!("notes:filter:{}", f.name()),
                    &format!("notes-filter-{}", f.name()),
                ),
                1,
            ))
        })
        .collect();
    WidgetPod::new(Row::new(buttons).gap(services.theme.space.sm))
}

fn list_page(services: &Services, screen: Size, books: &[BookNotes], filter: Filter) -> Window {
    let mut children = vec![
        crate::surfaces::common::status_header(
            Header::new(&services.theme, "Notes")
                .status(crate::surfaces::library::data::status_line(services).unwrap_or_default()),
        ),
        filter_row(services, filter),
        button(
            ButtonStyle::Text,
            "Export all notes",
            "notes:export",
            "notes-export",
        ),
    ];
    if books.is_empty() {
        children.push(body(
            services,
            "No highlights, notes or bookmarks yet. Select text in a book to start.",
        ));
    } else {
        let items: Vec<Item> = books
            .iter()
            .filter_map(|book| {
                let matching: Vec<&Note> = book.notes.iter().filter(|n| filter.matches(n)).collect();
                if matching.is_empty() {
                    return None;
                }
                Some(Item::Book {
                    md5: book.md5.clone(),
                    title: book.title.clone(),
                    detail: counts(&matching),
                })
            })
            .collect();
        children.push(list(items));
    }
    children.push(
        WidgetPod::new(kaia_ui::composite::NavBar::new(
            kaia_ui::composite::NavItem::Notes,
        ))
        .with_key("nav-bar"),
    );
    window(services, screen, page(services, children))
}

fn book_page(services: &Services, screen: Size, book: &BookNotes, filter: Filter) -> Window {
    let theme = &services.theme;
    let header = WidgetPod::new(Header::new(theme, book.title.clone()).back("notes:books"));
    let matching: Vec<&Note> = book.notes.iter().filter(|n| filter.matches(n)).collect();
    let items: Vec<Item> = matching
        .iter()
        .enumerate()
        .map(|(index, note)| entry_item(book, note, index))
        .collect();
    let mut children = vec![header, filter_row(services, filter)];
    if items.is_empty() {
        children.push(body(services, "Nothing matches this filter."));
    } else {
        children.push(list(items));
    }
    children.push(
        WidgetPod::new(kaia_ui::composite::NavBar::new(
            kaia_ui::composite::NavItem::Notes,
        ))
        .with_key("nav-bar"),
    );
    window(services, screen, page(services, children))
}

fn entry_item(book: &BookNotes, note: &Note, index: usize) -> Item {
    let open = book.library_id.map(|id| {
        let location = match &note.location {
            data::Location::Xpointer(xp) => Value::from(xp.as_str()),
            data::Location::Page(p) => Value::from(*p as i64),
        };
        (format!("open_book:{id}"), vec![location])
    });
    Item::Entry {
        primary: note.primary(),
        secondary: note.secondary(),
        open,
        index,
    }
}

/// `2 highlights, 1 note, 3 bookmarks`, for the kinds that occur.
fn counts(notes: &[&Note]) -> String {
    let highlights = notes.iter().filter(|n| n.highlight).count();
    let with_note = notes
        .iter()
        .filter(|n| n.note.as_deref().is_some_and(|t| !t.trim().is_empty()))
        .count();
    let bookmarks = notes.iter().filter(|n| n.bookmark).count();
    let mut parts = Vec::new();
    if highlights > 0 {
        parts.push(plural(highlights, "highlight"));
    }
    if with_note > 0 {
        parts.push(plural(with_note, "note"));
    }
    if bookmarks > 0 {
        parts.push(plural(bookmarks, "bookmark"));
    }
    parts.join(", ")
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// The list of items, one page at a time.
fn list(items: Vec<Item>) -> WidgetPod {
    WidgetPod::new(Expanded::new(
        WidgetPod::new(PagedList::new(items, build_item)),
        1,
    ))
}

fn build_item(item: &Item, _theme: &Theme) -> WidgetPod {
    match item {
        Item::Book { md5, title, detail } => {
            let row = ListRow::new(title.clone())
                .secondary(detail.clone())
                .chevron()
                .on_action(format!("notes:book:{md5}"));
            WidgetPod::new(row).with_key(format!("notes-book-{md5}"))
        }
        Item::Entry {
            primary,
            secondary,
            open,
            index,
        } => {
            let mut row = ListRow::new(primary.clone()).secondary(secondary.clone());
            if let Some((id, args)) = open.clone() {
                // A row that opens its place in the book shows the same chevron as a book row.
                row = row.chevron().on_hook(move |cx| {
                    cx.action(AppAction {
                        id: id.clone(),
                        args: args.clone(),
                    });
                });
            } else {
                row = row.enabled(false);
            }
            WidgetPod::new(row).with_key(format!("notes-entry-{index}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_name_each_kind() {
        let note = |highlight: bool, bookmark: bool, text: Option<&str>| Note {
            id: 0,
            text: text.map(str::to_string),
            note: None,
            chapter: None,
            pageno: None,
            location: data::Location::Page(0),
            datetime: String::new(),
            orphaned: false,
            highlight,
            bookmark,
        };
        let a = note(true, false, Some("q"));
        let b = note(false, true, None);
        assert_eq!(counts(&[&a, &b]), "1 highlight, 1 bookmark");
        assert_eq!(plural(2, "note"), "2 notes");
    }
}
