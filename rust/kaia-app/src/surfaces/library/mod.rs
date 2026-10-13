//! Library: the books in four views (covers, list, folders, collections), the
//! search page, and the panels for sort, collections, naming, delete and book
//! details. Panels replace the list in the same window, so their buttons are
//! part of this surface and its commands. A hold on a book opens its details.

pub mod commands;
pub mod data;
pub mod details;
pub mod folders;
pub mod press;
pub mod views;

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kaia_events::{Event, Value};
use kaia_library::{CoverSize, Library};
use kaia_ui::components::{Column, CrossAlign, Expanded, Padding};
use kaia_ui::composite::{BookInfo, PagedList};
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, WidgetPod, Window};

use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common;
use crate::surfaces::common::root_window as window;
use crate::surfaces::Route;
use commands::{parse, Command, FolderStep, View};
use data::Prefs;
use folders::Entry;
use views::Line;

/// Delay between the last key of the search field and the query.
const SEARCH_DEBOUNCE: f64 = 0.2;

/// Books in the search results at most.
const SEARCH_LIMIT: usize = 200;

/// A panel that replaces the list until it is closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Panel {
    Sort,
    CollectionName {
        id: Option<i64>,
    },
    /// The collection picker: for a book it adds the book, otherwise it filters.
    Collection {
        book: Option<i64>,
    },
    /// Names a new collection that gets `book`.
    Name {
        book: i64,
    },
    /// Asks before the file of `book` is deleted.
    Confirm {
        book: i64,
    },
    /// The details of `book`.
    Details {
        book: i64,
    },
}

/// The Library root screen.
pub struct LibrarySurface {
    covers: Option<crate::covers::Shared>,
    page: usize,
    prefs: Prefs,
    loaded: bool,
    panel: Option<Panel>,
    search: bool,
    query: String,
    name: String,
    /// Bumped by each query. A debounced search runs only for the latest one.
    generation: Rc<Cell<u64>>,
    /// The results of the last search, shared with the debounced task.
    results: Rc<RefCell<Vec<Line>>>,
    /// The folder the Folders view shows, when the user went into one.
    folder: Option<PathBuf>,
    /// The folder on screen and the root above it, from the last build.
    current: PathBuf,
    root: PathBuf,
    /// The rows of the folder on screen, from the last build. Folder actions index into them.
    entries: Vec<Entry>,
    /// The window of the last build. Debounced work updates its widgets by key.
    window: Option<Window>,
}

impl LibrarySurface {
    /// The Library at `page`. The grid and the list page themselves, so the page
    /// only matters for the route.
    pub fn new(page: usize) -> Self {
        Self {
            covers: None,
            page,
            prefs: Prefs::default(),
            loaded: false,
            panel: None,
            search: false,
            query: String::new(),
            name: String::new(),
            generation: Rc::new(Cell::new(0)),
            results: Rc::new(RefCell::new(Vec::new())),
            folder: None,
            current: PathBuf::new(),
            root: PathBuf::new(),
            entries: Vec::new(),
            window: None,
        }
    }

    fn save_prefs(&self, env: &mut SurfaceEnv<'_>) {
        self.prefs.save(&mut env.services.settings);
    }

    /// Sends the search for the current query after the debounce delay. A newer
    /// key cancels the older search, so only the last one runs.
    fn search_changed(&mut self, env: &mut SurfaceEnv<'_>) {
        let gen = self.generation.get() + 1;
        self.generation.set(gen);
        let query = self.query.clone();
        if query.trim().is_empty() {
            self.results.borrow_mut().clear();
            if let Some(window) = self.window.clone() {
                window.update::<PagedList<Line>, _>(env.ctx, "results", |list, cx| {
                    list.set_items(Vec::new(), cx)
                });
            }
            return;
        }
        let generation = self.generation.clone();
        let results = self.results.clone();
        let window = self.window.clone();
        let db = env.services.library_path();
        let show_finished = self.prefs.show_finished;
        env.schedule_in(SEARCH_DEBOUNCE, move |ctx| {
            if generation.get() != gen {
                return;
            }
            let lines: Vec<Line> = match Library::open(&db) {
                Ok(lib) => lib
                    .search(&query, SEARCH_LIMIT)
                    .unwrap_or_default()
                    .iter()
                    .filter(|b| {
                        b.path.exists() && (show_finished || b.status != kaia_library::Status::Complete)
                    })
                    .map(|b| Line::book(&data::info_without_cover(b)))
                    .collect(),
                Err(_) => Vec::new(),
            };
            *results.borrow_mut() = lines.clone();
            if let Some(window) = &window {
                window.update::<PagedList<Line>, _>(ctx, "results", |list, cx| list.set_items(lines, cx));
            }
        });
    }

    fn handle(&mut self, command: Command, env: &mut SurfaceEnv<'_>) -> Outcome {
        match command {
            Command::CreateCollection => {
                self.name.clear();
                self.panel = Some(Panel::CollectionName { id: None });
                Outcome::Rebuild
            }
            Command::RenameCollection(id) => {
                self.name = env
                    .services
                    .library
                    .collections()
                    .unwrap_or_default()
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.name.clone())
                    .unwrap_or_default();
                self.panel = Some(Panel::CollectionName { id: Some(id) });
                Outcome::Rebuild
            }
            Command::DeleteCollection(id) => {
                env.dialog(kaia_ui::composite::Dialog::new(
                    env.theme(),
                    "Delete collection?",
                    "The books and their reading progress will stay in the library.",
                    vec![
                        kaia_ui::composite::DialogButton::new("Cancel", "dialog:dismiss"),
                        kaia_ui::composite::DialogButton::new(
                            "Delete",
                            format!("library:collections:delete-confirm:{id}"),
                        ),
                    ],
                ));
                Outcome::Handled
            }
            Command::ConfirmDeleteCollection(id) => {
                if let Err(e) = env.services.library.delete_collection(id) {
                    env.toast(format!("Cannot delete collection: {e}"), 4.0);
                }
                if self.prefs.collection == Some(id) {
                    self.prefs.collection = None;
                    self.save_prefs(env);
                }
                Outcome::Rebuild
            }
            Command::ReorderCollection(id, position) => {
                if let Err(e) = env.services.library.reorder_collection(id, position) {
                    env.toast(format!("Cannot move collection: {e}"), 4.0);
                }
                Outcome::Rebuild
            }
            Command::RemoveHistory(id) => {
                if let Err(e) = env.services.library.remove_from_history(id) {
                    env.toast(format!("Cannot remove history: {e}"), 4.0);
                }
                Outcome::Rebuild
            }
            Command::ClearHistory => {
                env.dialog(kaia_ui::composite::Dialog::new(
                    env.theme(),
                    "Clear reading history?",
                    "Reading positions and annotations will stay saved.",
                    vec![
                        kaia_ui::composite::DialogButton::new("Cancel", "dialog:dismiss"),
                        kaia_ui::composite::DialogButton::new("Clear", "library:history:clear-confirm"),
                    ],
                ));
                Outcome::Handled
            }
            Command::ConfirmClearHistory => {
                if let Err(e) = env.services.library.clear_history() {
                    env.toast(format!("Cannot clear history: {e}"), 4.0);
                }
                Outcome::Rebuild
            }
            Command::View(view) => {
                self.prefs.view = view;
                self.panel = None;
                self.search = false;
                self.save_prefs(env);
                Outcome::Rebuild
            }
            Command::OpenSort => {
                self.panel = Some(Panel::Sort);
                Outcome::Rebuild
            }
            Command::Sort(choice) => {
                self.prefs.sort = choice;
                self.panel = None;
                self.save_prefs(env);
                Outcome::Rebuild
            }
            Command::Chip(chip) => {
                self.prefs.chip = if self.prefs.chip == Some(chip) {
                    None
                } else {
                    Some(chip)
                };
                self.save_prefs(env);
                Outcome::Rebuild
            }
            Command::CollectionChip => {
                if self.prefs.collection.is_some() {
                    self.prefs.collection = None;
                    self.save_prefs(env);
                } else {
                    self.panel = Some(Panel::Collection { book: None });
                }
                Outcome::Rebuild
            }
            Command::PickCollection(id) => {
                self.prefs.collection = id;
                if self.prefs.view == View::Collections {
                    self.prefs.view = View::Covers;
                }
                self.panel = None;
                self.save_prefs(env);
                Outcome::Rebuild
            }
            Command::SearchOpen => {
                self.search = true;
                self.query.clear();
                self.results.borrow_mut().clear();
                self.panel = None;
                self.generation.set(self.generation.get() + 1);
                Outcome::Rebuild
            }
            Command::SearchClose => {
                self.search = false;
                self.generation.set(self.generation.get() + 1);
                Outcome::Rebuild
            }
            Command::SearchChanged(text) => {
                self.query = text;
                self.search_changed(env);
                Outcome::Handled
            }
            Command::Folder(FolderStep::Up) => {
                self.folder = self.current.parent().map(Path::to_path_buf);
                Outcome::Rebuild
            }
            Command::Folder(FolderStep::Into(n)) => {
                if let Some(Entry::Dir { path, .. }) = self.entries.get(n) {
                    self.folder = Some(path.clone());
                }
                Outcome::Rebuild
            }
            Command::Open(id) => Outcome::Navigate(Nav::OpenBook {
                book: id,
                location: None,
            }),
            Command::Details(id) => {
                self.panel = Some(Panel::Details { book: id });
                Outcome::Rebuild
            }
            Command::MarkFinished(id) => {
                match crate::book_index::mark_finished(env.services, id) {
                    Ok(()) => {
                        env.toast("Marked as finished", 2.0);
                    }
                    Err(e) => env.notice(format!("cannot mark book {id}: {e}")),
                }
                Outcome::Rebuild
            }
            Command::AddToCollection(id) => {
                self.panel = Some(Panel::Collection { book: Some(id) });
                Outcome::Rebuild
            }
            Command::AddBookTo { book, collection } => {
                let name = env
                    .services
                    .library
                    .collections()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|c| c.id == collection)
                    .map(|c| c.name);
                match name {
                    Some(name) => match env.services.library.add_to_collection(book, &name) {
                        Ok(()) => {
                            env.toast(format!("Added to {name}"), 2.0);
                        }
                        Err(e) => env.notice(format!("cannot add book {book}: {e}")),
                    },
                    None => env.notice(format!("no collection {collection}")),
                }
                self.panel = Some(Panel::Details { book });
                Outcome::Rebuild
            }
            Command::NewCollection(book) => {
                self.name.clear();
                self.panel = Some(Panel::Name { book });
                Outcome::Rebuild
            }
            Command::NameChanged(text) => {
                self.name = text;
                Outcome::Handled
            }
            Command::SaveName => {
                if let Some(Panel::CollectionName { id }) = self.panel {
                    let result = match id {
                        Some(id) => env.services.library.rename_collection(id, self.name.trim()),
                        None => env
                            .services
                            .library
                            .create_collection(self.name.trim())
                            .map(|_| ()),
                    };
                    match result {
                        Ok(()) => self.panel = None,
                        Err(e) => {
                            env.toast(format!("Cannot save collection: {e}"), 4.0);
                            return Outcome::Handled;
                        }
                    }
                    return Outcome::Rebuild;
                }
                let Some(Panel::Name { book }) = self.panel.clone() else {
                    return Outcome::Handled;
                };
                let name = self.name.trim().to_string();
                if name.is_empty() {
                    env.toast("Type a name first", 2.0);
                    return Outcome::Handled;
                }
                match env
                    .services
                    .library
                    .create_collection(&name)
                    .and_then(|_| env.services.library.add_to_collection(book, &name))
                {
                    Ok(()) => {
                        env.toast(format!("Added to {name}"), 2.0);
                    }
                    Err(e) => env.notice(format!("cannot create the collection: {e}")),
                }
                self.name.clear();
                self.panel = Some(Panel::Details { book });
                Outcome::Rebuild
            }
            Command::AskDelete(id) => {
                self.panel = Some(Panel::Confirm { book: id });
                Outcome::Rebuild
            }
            Command::ConfirmDelete(id) => {
                delete_file(env, id);
                self.panel = None;
                Outcome::Rebuild
            }
            Command::ClosePanel => {
                self.panel = None;
                Outcome::Rebuild
            }
        }
    }

    /// The book lines and cover-less lines of the folder on screen, and the
    /// heading for them. Also records the folder state for the folder actions.
    fn folder_view(&mut self, env: &SurfaceEnv<'_>, t: &Theme) -> Vec<WidgetPod> {
        let all: Vec<_> = data::all(&env.services.library)
            .into_iter()
            .filter(|b| self.prefs.show_finished || b.status != kaia_library::Status::Complete)
            .collect();
        let Some(root) = folders::root_of(&all) else {
            return vec![views::body_text(t, "No books yet.")];
        };
        let current = self
            .folder
            .clone()
            .filter(|f| f.starts_with(&root))
            .unwrap_or_else(|| root.clone());
        let entries = folders::entries(&all, &current);
        let mut lines = Vec::new();
        if current != root {
            lines.push(Line {
                key: "folder-up".into(),
                title: "Up".into(),
                detail: String::new(),
                cover: None,
                tap: "library:folder:up".into(),
                hold: None,
                enabled: true,
            });
        }
        for (n, entry) in entries.iter().enumerate() {
            match entry {
                Entry::Dir { name, books, .. } => lines.push(Line {
                    key: format!("folder-{n}"),
                    title: name.clone(),
                    detail: format!("{books} books"),
                    cover: None,
                    tap: format!("library:folder:{n}"),
                    hold: None,
                    enabled: true,
                }),
                Entry::Book { id } => {
                    if let Some(book) = all.iter().find(|b| b.id == *id) {
                        lines.push(Line {
                            key: format!("book-{id}"),
                            title: data::title(book),
                            detail: format!("{}%", data::percent(book)),
                            cover: None,
                            tap: format!("library:open:{id}"),
                            hold: Some(format!("library:details:{id}")),
                            enabled: true,
                        });
                    }
                }
            }
        }
        self.current = current.clone();
        self.root = root.clone();
        self.entries = entries;
        let heading = folders::label(&root, &current);
        vec![views::folder_rows(t, heading, lines)]
    }

    /// The content of the page for the current view, or the panel or the search page.
    fn content(&mut self, env: &mut SurfaceEnv<'_>) -> Vec<WidgetPod> {
        let t = env.services.theme.clone();
        if self.search {
            let results = self.results.borrow().clone();
            return views::search(&t, &self.query, results, self.covers.as_ref().unwrap().clone());
        }
        if let Some(panel) = self.panel.clone() {
            return vec![self.panel_widget(env, &t, &panel)];
        }
        match self.prefs.view {
            View::Folders => self.folder_view(env, &t),
            View::Collections => {
                let collections = env.services.library.collections().unwrap_or_default();
                vec![views::collections(&t, &collections)]
            }
            View::Covers | View::List => {
                let books = data::list(&env.services.library, &self.prefs);
                if books.is_empty() {
                    return common::empty_library(&t);
                }
                let infos: Vec<BookInfo> = books.iter().map(data::info_without_cover).collect();
                if self.prefs.view == View::Covers {
                    vec![views::covers(
                        &t,
                        env.screen,
                        infos,
                        self.covers.as_ref().unwrap().clone(),
                    )]
                } else {
                    let lines: Vec<Line> = infos.iter().map(Line::book).collect();
                    vec![WidgetPod::new(Expanded::new(
                        WidgetPod::new(views::list_covers(lines, self.covers.as_ref().unwrap().clone()))
                            .with_key("list"),
                        1,
                    ))]
                }
            }
        }
    }

    fn panel_widget(&self, env: &SurfaceEnv<'_>, t: &Theme, panel: &Panel) -> WidgetPod {
        let lib = &env.services.library;
        match *panel {
            Panel::Sort => views::sort_panel(t, self.prefs.sort),
            Panel::Collection { book } => {
                let collections = lib.collections().unwrap_or_default();
                views::collection_panel(t, &collections, book, self.prefs.collection)
            }
            Panel::Name { .. } | Panel::CollectionName { .. } => views::name_panel(t, &self.name),
            Panel::Confirm { book } => match lib.book(book).ok().flatten() {
                Some(row) => views::confirm_panel(t, &row),
                None => gone(t),
            },
            Panel::Details { book } => match lib.book(book).ok().flatten() {
                Some(row) => {
                    let cover = data::cover(lib, book, CoverSize::Large);
                    views::details_panel(t, &row, cover, "library:panel:close")
                }
                None => gone(t),
            },
        }
    }

    fn build_window(&mut self, env: &mut SurfaceEnv<'_>) -> Window {
        if self.covers.is_none() {
            self.covers = Some(crate::covers::Loader::new(env.services.library_path()));
        }
        if !self.loaded {
            self.prefs = Prefs::load(&env.services.settings);
            self.loaded = true;
        }
        let t = env.services.theme.clone();
        let status = data::status_line(env.services);
        let mut children = vec![views::header(&t, "Library", status)];
        if !self.search {
            children.push(views::view_tabs(&t, self.prefs.view));
            children.push(views::filter_row(&t, &self.prefs));
        }
        let content = self.content(env);
        children.push(WidgetPod::new(Expanded::new(
            WidgetPod::new(
                Column::new(content)
                    .gap(t.space.md)
                    .cross_align(CrossAlign::Stretch),
            ),
            1,
        )));
        children.push(
            WidgetPod::new(kaia_ui::composite::NavBar::new(
                kaia_ui::composite::NavItem::Library,
            ))
            .with_key("nav-bar"),
        );
        let page = Padding::new(
            WidgetPod::new(
                Column::new(children)
                    .gap(t.space.sm)
                    .cross_align(CrossAlign::Stretch),
            ),
            Insets::all(t.space.lg),
        );
        let win = window(env.services, env.screen, page);
        self.window = Some(win.clone());
        win
    }
}

/// The steps to add books to an empty library. The heading is "No books yet".
pub const EMPTY_LIBRARY: &str = "Connect the Kobo to a computer over USB and copy book files into the Books folder. Kaia adds them after the next scan.";

/// Deletes the file of book `id` and reports the result. The row leaves the list
/// the next time the lists are read, because they skip books whose file is gone.
fn delete_file(env: &mut SurfaceEnv<'_>, id: i64) {
    let path = match env.services.library.book(id) {
        Ok(Some(book)) => book.path,
        _ => {
            env.notice(format!("no book {id}"));
            return;
        }
    };
    match fs::remove_file(&path) {
        Ok(()) => {
            if let Err(e) = env.services.library.remove_book(id) {
                env.toast(
                    format!("File deleted, but the library could not update: {e}"),
                    4.0,
                );
            }
            env.toast("File deleted", 2.0);
        }
        Err(e) => {
            env.notice(format!("cannot delete {}: {e}", path.display()));
            env.toast("The file could not be deleted", 2.0);
        }
    }
}

impl Surface for LibrarySurface {
    fn route(&self) -> Route {
        Route::Library { page: self.page }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(self.build_window(env))
    }

    /// Covers and list rows show pictures, so every refresh dithers them.
    fn content_dither(&self, _mode: kaia_events::RefreshMode) -> bool {
        true
    }

    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        match parse(cmd, args) {
            Some(command) => self.handle(command, env),
            None => Outcome::NotMine,
        }
    }

    /// The lists show titles and progress, so a library change rebuilds them.
    fn on_event(&mut self, ev: &Event, _env: &mut SurfaceEnv<'_>) -> Outcome {
        if ev.name == "LibraryUpdated" {
            self.covers = None;
            Outcome::Rebuild
        } else {
            Outcome::NotMine
        }
    }
}

/// The message for a panel whose book has gone from the library.
fn gone(t: &Theme) -> WidgetPod {
    views::body_text(t, "That book is no longer in the library.")
}
