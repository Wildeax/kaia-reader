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
use kaia_i18n::{format as tr_format, tr, tr_n};
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
    fn on_event(&mut self, event: &kaia_events::Event, _env: &mut SurfaceEnv<'_>) -> Outcome {
        if event.name == "LibraryUpdated" {
            Outcome::Rebuild
        } else {
            Outcome::NotMine
        }
    }

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
    fn on_event(&mut self, event: &kaia_events::Event, _env: &mut SurfaceEnv<'_>) -> Outcome {
        if event.name == "LibraryUpdated" {
            Outcome::Rebuild
        } else {
            Outcome::NotMine
        }
    }

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
    if let Some(id) = cmd.strip_prefix("notes:sticker:") {
        for book in load_books(env.services) {
            if let Some(note) = book.notes.iter().find(|n| n.sticker.as_deref() == Some(id)) {
                if let Some(book) = book.library_id {
                    let key = if note.orphaned {
                        "repair_sticker"
                    } else {
                        "open_sticker"
                    };
                    let value = Value::Table([(kaia_events::Key::from(key), Value::from(id))].into());
                    return Outcome::Navigate(Nav::OpenBook {
                        book,
                        location: Some(value),
                    });
                }
            }
        }
        env.toast(tr("This sticker's book is not in the library."), 4.0);
        return Outcome::Handled;
    }
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
            use kaia_ui::composite::{Dialog, DialogButton};
            env.dialog(Dialog::new(
                env.theme(),
                tr("Export all notes"),
                tr("Choose a format. PDF creates one file per book."),
                vec![
                    DialogButton::new(tr("Cancel"), "dialog:dismiss"),
                    DialogButton::new(tr("Markdown and JSON"), "notes:export:text"),
                    DialogButton::new(tr("PDF"), "notes:export:pdf"),
                ],
            ));
            Outcome::Handled
        }
        Some(Command::ExportPdf) => {
            export_pdfs(env);
            Outcome::Handled
        }
        Some(Command::ExportText) => {
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
    crate::webdav::enqueue_saved(env, None, true);
    let books = load_books(env.services);
    if books.is_empty() {
        env.toast(tr("There are no notes to export."), 3.0);
        return;
    }
    let data_dir = env.services.paths.data_dir.clone();
    match export::export(&data_dir, &books, now_unix()) {
        Ok(done) => {
            let message = tr_format(
                &tr_n(
                    "Exported %1 note to %2",
                    "Exported %1 notes to %2",
                    done.count as i64,
                ),
                &[&done.count.to_string(), &done.markdown.display().to_string()],
            );
            env.toast(message, 6.0);
        }
        Err(e) => {
            env.toast(tr_format(&tr("Export failed: %1"), &[&e.to_string()]), 6.0);
        }
    }
}

fn export_pdfs(env: &mut SurfaceEnv<'_>) {
    crate::webdav::enqueue_saved(env, None, true);
    let books = load_books(env.services);
    if books.is_empty() {
        env.toast(tr("There are no notes to export."), 3.0);
        return;
    }
    let fonts = crate::services::resources_dir(&env.services.platform).join("fonts");
    match export::export_pdf(&env.services.paths.data_dir, &books, &fonts) {
        Ok(paths) => env.toast(
            tr_format(
                &tr_n(
                    "Exported %1 PDF file to %2",
                    "Exported %1 PDF files to %2",
                    paths.len() as i64,
                ),
                &[
                    &paths.len().to_string(),
                    &env.services
                        .paths
                        .data_dir
                        .join(export::EXPORTS_DIR)
                        .display()
                        .to_string(),
                ],
            ),
            6.0,
        ),
        Err(error) => env.toast(tr_format(&tr("Export failed: %1"), &[&error.to_string()]), 6.0),
    };
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The books with notes, read from the state files. The library gives the
/// titles and the library ids. A library that cannot be read gives no titles.
pub fn load_books(services: &Services) -> Vec<BookNotes> {
    load_books_with_empty(services)
        .into_iter()
        .filter(|b| !b.notes.is_empty() || !b.ink_counts.is_empty())
        .collect()
}

pub(crate) fn load_books_with_empty(services: &Services) -> Vec<BookNotes> {
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
    data::load_with_empty(&services.paths.data_dir, &library)
}

/// One line of a list: a book, or an entry of the open book.
#[derive(Clone)]
enum Item {
    Heading(String),
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
    let rows = Filter::ALL
        .chunks(3)
        .map(|group| {
            let buttons = group
                .iter()
                .map(|&f| {
                    let style = if f == filter {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    };
                    WidgetPod::new(Expanded::new(
                        button(
                            style,
                            &f.label(),
                            &format!("notes:filter:{}", f.name()),
                            &format!("notes-filter-{}", f.name()),
                        ),
                        1,
                    ))
                })
                .collect();
            WidgetPod::new(Row::new(buttons).gap(services.theme.space.sm))
        })
        .collect();
    WidgetPod::new(
        kaia_ui::components::Column::new(rows)
            .shrink()
            .gap(services.theme.space.sm),
    )
}

fn list_page(services: &Services, screen: Size, books: &[BookNotes], filter: Filter) -> Window {
    let mut children = vec![
        crate::surfaces::common::status_header(
            Header::new(&services.theme, tr("Notes"))
                .status(crate::surfaces::library::data::status_line(services).unwrap_or_default()),
        ),
        filter_row(services, filter),
        button(
            ButtonStyle::Text,
            &tr("Export all notes"),
            "notes:export",
            "notes-export",
        ),
    ];
    if books.is_empty() {
        children.push(body(
            services,
            tr("No highlights, notes or bookmarks yet. Select text in a book to start."),
        ));
    } else {
        let mut items: Vec<Item> = books
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
        let mut repairs = Vec::new();
        for book in books {
            for note in book.notes.iter().filter(|n| n.orphaned && filter.matches(n)) {
                if repairs.is_empty() {
                    repairs.push(Item::Heading(tr("Needs repair")));
                }
                repairs.push(entry_item(book, note, repairs.len()));
            }
        }
        if !repairs.is_empty() {
            repairs.push(Item::Heading(tr("Books")));
            repairs.append(&mut items);
            items = repairs;
        }
        children.push(list(items));
    }
    window(
        services,
        screen,
        crate::surfaces::common::tabbed_page(page(services, children), kaia_ui::composite::NavItem::More),
    )
}

fn book_page(services: &Services, screen: Size, book: &BookNotes, filter: Filter) -> Window {
    let theme = &services.theme;
    let header = WidgetPod::new(Header::new(theme, book.title.clone()).back("notes:books"));
    let matching: Vec<&Note> = book.notes.iter().filter(|n| filter.matches(n)).collect();
    let mut items = Vec::new();
    for repair in [true, false] {
        let group: Vec<_> = matching.iter().filter(|n| n.orphaned == repair).collect();
        if group.is_empty() {
            continue;
        }
        if repair {
            items.push(Item::Heading(tr("Needs repair")));
        } else if matching.iter().any(|n| n.orphaned) {
            items.push(Item::Heading(tr("In the book")));
        }
        for note in group {
            items.push(entry_item(book, note, items.len()));
        }
    }
    let mut children = vec![header, filter_row(services, filter)];
    if items.is_empty() {
        children.push(body(services, tr("Nothing matches this filter.")));
    } else {
        children.push(list(items));
    }
    window(
        services,
        screen,
        crate::surfaces::common::tabbed_page(page(services, children), kaia_ui::composite::NavItem::More),
    )
}

fn entry_item(book: &BookNotes, note: &Note, index: usize) -> Item {
    let open = book.library_id.map(|id| {
        let location = match &note.location {
            data::Location::Xpointer(xp) => Value::from(xp.as_str()),
            data::Location::Page(p) => Value::from(*p as i64),
        };
        if let Some(sticker) = &note.sticker {
            (format!("notes:sticker:{sticker}"), Vec::new())
        } else {
            (format!("open_book:{id}"), vec![location])
        }
    });
    Item::Entry {
        primary: note.primary(),
        secondary: if note.orphaned && note.sticker.is_some() {
            tr_format(&tr("%1 · Repair"), &[book.title.as_str()])
        } else {
            note.secondary()
        },
        open,
        index,
    }
}

/// `2 highlights, 1 note, 3 bookmarks`, for the kinds that occur.
fn counts(notes: &[&Note]) -> String {
    let highlights = notes.iter().filter(|n| n.highlight).count();
    let with_note = notes
        .iter()
        .filter(|n| n.sticker.is_none() && n.note.as_deref().is_some_and(|t| !t.trim().is_empty()))
        .count();
    let bookmarks = notes.iter().filter(|n| n.bookmark).count();
    let mut parts = Vec::new();
    let stickers = notes.iter().filter(|n| n.sticker.is_some()).count();
    if stickers > 0 {
        parts.push(tr_format(
            &tr_n("%1 sticker", "%1 stickers", stickers as i64),
            &[&stickers.to_string()],
        ));
    }
    if highlights > 0 {
        parts.push(tr_format(
            &tr_n("%1 highlight", "%1 highlights", highlights as i64),
            &[&highlights.to_string()],
        ));
    }
    if with_note > 0 {
        parts.push(tr_format(
            &tr_n("%1 note", "%1 notes", with_note as i64),
            &[&with_note.to_string()],
        ));
    }
    if bookmarks > 0 {
        parts.push(tr_format(
            &tr_n("%1 bookmark", "%1 bookmarks", bookmarks as i64),
            &[&bookmarks.to_string()],
        ));
    }
    parts.join(", ")
}

/// The list of items, one page at a time.
fn list(items: Vec<Item>) -> WidgetPod {
    WidgetPod::new(Expanded::new(
        WidgetPod::new(PagedList::new(items, build_item)),
        1,
    ))
}

fn build_item(item: &Item, theme: &Theme) -> WidgetPod {
    match item {
        Item::Heading(text) => WidgetPod::new(kaia_ui::components::Label::new(
            text.clone(),
            theme.font.title.clone(),
            kaia_ui::Tone::Ink,
        )),
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
            sticker: None,
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
        let noted = Note {
            note: Some("text".into()),
            ..note(false, false, None)
        };
        assert_eq!(counts(&[&noted, &noted]), "2 notes");
    }
}
