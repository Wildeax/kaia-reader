//! Library: the books in five views (covers, list, folders, collections, series),
//! the search page, select mode, and the panels for sort, filter, collections,
//! naming, delete and book details. Panels replace the list in the same window, so their
//! buttons are part of this surface and its commands. A hold on a book opens its
//! details.

pub mod commands;
pub mod data;
pub mod details;
pub mod folders;
pub mod press;
pub mod views;

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};

use kaia_events::{Ctx, Event, Value};
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_library::{CoverSize, Library, Status, RECENT_SEARCHES};
use kaia_ui::components::{Column, CrossAlign, Expanded, Label, Padding};
use kaia_ui::composite::{BookInfo, PagedList};
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, WidgetPod, Window};

use crate::book_index::{self, Mark, MarkReport};
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common;
use crate::surfaces::common::root_window as window;
use crate::surfaces::Route;
use commands::{parse, Command, FolderStep, SeriesStep, View};
use data::Prefs;
use folders::Entry;
use views::Line;

/// Delay between the last key of the search field and the query.
const SEARCH_DEBOUNCE: f64 = 0.2;

/// Books in the search results at most.
const SEARCH_LIMIT: usize = 200;

/// How often the screen asks the batch worker for its report.
const MARK_POLL: f64 = 0.1;

/// The window of the screen, as the screen last built it. Scheduled work reads it
/// when it runs, so it updates the screen that is on show, not one a rebuild has
/// replaced.
type Live = Rc<RefCell<Option<Window>>>;

/// A panel that replaces the list until it is closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Panel {
    Sort,
    /// The filter for the Books tab: a status, a collection, or none.
    Filter,
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
    /// The rows of the search page: the results, or the recent searches when the
    /// field is empty. Shared with the debounced search.
    results: Rc<RefCell<Vec<Line>>>,
    /// The folder the Folders view shows, when the user went into one.
    folder: Option<PathBuf>,
    /// The folder on screen and the root above it, from the last build.
    current: PathBuf,
    root: PathBuf,
    /// The rows of the folder on screen, from the last build. Folder actions index into them.
    entries: Vec<Entry>,
    /// The layout the Books tab shows when the user taps it: covers or list. The
    /// view changes to a tab without a layout, so this keeps the last one.
    books: View,
    /// The series the Series view shows, when the user opened one.
    series: Option<String>,
    /// The series rows of the last build. The series actions index into them.
    series_names: Vec<String>,
    /// True in select mode, where a tap checks a book.
    selecting: bool,
    /// The books checked in select mode.
    selected: BTreeSet<i64>,
    /// True while a batch mark runs on its worker. Shared with the poll.
    marking: Rc<Cell<bool>>,
    /// The window of the last build. Scheduled work updates its widgets by key.
    window: Live,
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
            books: View::Covers,
            series: None,
            series_names: Vec::new(),
            selecting: false,
            selected: BTreeSet::new(),
            marking: Rc::new(Cell::new(false)),
            window: Rc::new(RefCell::new(None)),
        }
    }

    fn save_prefs(&self, env: &mut SurfaceEnv<'_>) {
        self.prefs.save(&mut env.services.settings);
    }

    /// Shows the tab `view` and closes whatever was open on the others. Covers and
    /// list also become the layout of the Books tab.
    fn set_view(&mut self, view: View, env: &mut SurfaceEnv<'_>) {
        if matches!(view, View::Covers | View::List) {
            self.books = view;
        }
        self.prefs.view = view;
        self.panel = None;
        self.search = false;
        self.series = None;
        self.save_prefs(env);
    }

    /// Sends the search for the current query after the debounce delay. A newer
    /// key cancels the older search, so only the last one runs. With no text the
    /// recent searches show at once.
    fn search_changed(&mut self, env: &mut SurfaceEnv<'_>) {
        let gen = self.generation.get() + 1;
        self.generation.set(gen);
        let query = self.query.clone();
        if query.trim().is_empty() {
            let lines = recent_lines(&env.services.library);
            *self.results.borrow_mut() = lines.clone();
            show_results(&self.window, env.ctx, lines);
            return;
        }
        let generation = self.generation.clone();
        let results = self.results.clone();
        let live = self.window.clone();
        let db = env.services.library_path();
        let show_finished = self.prefs.show_finished;
        env.schedule_in(SEARCH_DEBOUNCE, move |ctx| {
            if generation.get() != gen {
                return;
            }
            let lines = match Library::open(&db) {
                Ok(lib) => search_lines(&lib, &query, show_finished),
                Err(_) => Vec::new(),
            };
            *results.borrow_mut() = lines.clone();
            show_results(&live, ctx, lines);
        });
    }

    /// Records the query as a recent search. The history is a convenience, so a
    /// write that fails leaves the list as it was.
    fn record_query(&self, env: &mut SurfaceEnv<'_>) {
        let _ = env.services.library.record_search(&self.query);
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
                    tr("Delete collection?"),
                    tr("The books and their reading progress will stay in the library."),
                    vec![
                        kaia_ui::composite::DialogButton::new(tr("Cancel"), "dialog:dismiss"),
                        kaia_ui::composite::DialogButton::new(
                            tr("Delete"),
                            format!("library:collections:delete-confirm:{id}"),
                        ),
                    ],
                ));
                Outcome::Handled
            }
            Command::ConfirmDeleteCollection(id) => {
                if let Err(e) = env.services.library.delete_collection(id) {
                    env.toast(
                        tr_format(&tr("Cannot delete collection: %1"), &[&e.to_string()]),
                        4.0,
                    );
                }
                if self.prefs.collection == Some(id) {
                    self.prefs.collection = None;
                    self.save_prefs(env);
                }
                Outcome::Rebuild
            }
            Command::ReorderCollection(id, position) => {
                if let Err(e) = env.services.library.reorder_collection(id, position) {
                    env.toast(
                        tr_format(&tr("Cannot move collection: %1"), &[&e.to_string()]),
                        4.0,
                    );
                }
                Outcome::Rebuild
            }
            Command::RemoveHistory(id) => {
                if let Err(e) = env.services.library.remove_from_history(id) {
                    env.toast(
                        tr_format(&tr("Cannot remove history: %1"), &[&e.to_string()]),
                        4.0,
                    );
                }
                Outcome::Rebuild
            }
            Command::ClearHistory => {
                env.dialog(kaia_ui::composite::Dialog::new(
                    env.theme(),
                    tr("Clear reading history?"),
                    tr("Reading positions and annotations will stay saved."),
                    vec![
                        kaia_ui::composite::DialogButton::new(tr("Cancel"), "dialog:dismiss"),
                        kaia_ui::composite::DialogButton::new(tr("Clear"), "library:history:clear-confirm"),
                    ],
                ));
                Outcome::Handled
            }
            Command::ConfirmClearHistory => {
                if let Err(e) = env.services.library.clear_history() {
                    env.toast(tr_format(&tr("Cannot clear history: %1"), &[&e.to_string()]), 4.0);
                }
                Outcome::Rebuild
            }
            Command::View(view) => {
                self.set_view(view, env);
                Outcome::Rebuild
            }
            Command::BooksTab => {
                let books = if matches!(self.prefs.view, View::Covers | View::List) {
                    self.prefs.view
                } else {
                    self.books
                };
                self.set_view(books, env);
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
            Command::OpenFilter => {
                self.panel = Some(Panel::Filter);
                Outcome::Rebuild
            }
            Command::AllBooks => {
                self.prefs.chip = None;
                self.prefs.collection = None;
                self.panel = None;
                self.save_prefs(env);
                Outcome::Rebuild
            }
            Command::Chip(chip) => {
                self.prefs.chip = Some(chip);
                self.prefs.collection = None;
                self.panel = None;
                self.save_prefs(env);
                Outcome::Rebuild
            }
            Command::FilterCollection => {
                self.panel = Some(Panel::Collection { book: None });
                Outcome::Rebuild
            }
            Command::PickCollection(id) => {
                self.prefs.collection = id;
                if id.is_some() {
                    self.prefs.chip = None;
                }
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
                self.panel = None;
                self.generation.set(self.generation.get() + 1);
                *self.results.borrow_mut() = recent_lines(&env.services.library);
                Outcome::Rebuild
            }
            Command::SearchClose => {
                self.record_query(env);
                self.search = false;
                self.generation.set(self.generation.get() + 1);
                Outcome::Rebuild
            }
            Command::SearchChanged(text) => {
                self.query = text;
                self.search_changed(env);
                Outcome::Handled
            }
            Command::SearchSubmit => {
                self.record_query(env);
                Outcome::Handled
            }
            Command::RecentSearch(n) => {
                let recent = env
                    .services
                    .library
                    .recent_searches(RECENT_SEARCHES)
                    .unwrap_or_default();
                let Some(text) = recent.get(n).cloned() else {
                    return Outcome::Handled;
                };
                self.query = text.clone();
                let _ = env.services.library.record_search(&text);
                let lines = search_lines(&env.services.library, &text, self.prefs.show_finished);
                *self.results.borrow_mut() = lines;
                Outcome::Rebuild
            }
            Command::ClearRecentSearches => {
                let _ = env.services.library.clear_recent_searches();
                self.results.borrow_mut().clear();
                Outcome::Rebuild
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
            Command::Series(SeriesStep::Into(n)) => {
                if let Some(name) = self.series_names.get(n) {
                    self.series = Some(name.clone());
                }
                Outcome::Rebuild
            }
            Command::Series(SeriesStep::Up) => {
                self.series = None;
                Outcome::Rebuild
            }
            Command::Open(id) => {
                if self.search {
                    self.record_query(env);
                }
                Outcome::Navigate(Nav::OpenBook {
                    book: id,
                    location: None,
                })
            }
            Command::Details(id) => {
                self.panel = Some(Panel::Details { book: id });
                Outcome::Rebuild
            }
            Command::MarkFinished(id) => self.mark_finished_one(env, id),
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
                            env.toast(tr_format(&tr("Added to %1"), &[&name]), 2.0);
                        }
                        Err(e) => env.notice(tr_format(
                            &tr("cannot add book %1: %2"),
                            &[&book.to_string(), &e.to_string()],
                        )),
                    },
                    None => env.notice(tr_format(&tr("no collection %1"), &[&collection.to_string()])),
                }
                self.panel = Some(Panel::Details { book });
                Outcome::Rebuild
            }
            Command::RemoveFromCollection { book, collection } => {
                let name = env
                    .services
                    .library
                    .collections()
                    .unwrap_or_default()
                    .into_iter()
                    .find(|c| c.id == collection)
                    .map(|c| c.name);
                match name {
                    Some(name) => match env.services.library.remove_from_collection(book, &name) {
                        Ok(true) => {
                            env.toast(tr_format(&tr("Removed from %1"), &[&name]), 2.0);
                        }
                        Ok(false) => env.notice(tr("That book is not in this collection.")),
                        Err(e) => env.notice(tr_format(
                            &tr("cannot remove book %1: %2"),
                            &[&book.to_string(), &e.to_string()],
                        )),
                    },
                    None => env.notice(tr_format(&tr("no collection %1"), &[&collection.to_string()])),
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
                            env.toast(
                                tr_format(&tr("Cannot save collection: %1"), &[&e.to_string()]),
                                4.0,
                            );
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
                    env.toast(tr("Type a name first"), 2.0);
                    return Outcome::Handled;
                }
                match env
                    .services
                    .library
                    .create_collection(&name)
                    .and_then(|_| env.services.library.add_to_collection(book, &name))
                {
                    Ok(()) => {
                        env.toast(tr_format(&tr("Added to %1"), &[&name]), 2.0);
                    }
                    Err(e) => env.notice(tr_format(
                        &tr("cannot create the collection: %1"),
                        &[&e.to_string()],
                    )),
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
            Command::SelectOn => {
                self.selecting = true;
                self.selected.clear();
                self.panel = None;
                self.search = false;
                Outcome::Rebuild
            }
            Command::SelectOff => {
                self.selecting = false;
                self.selected.clear();
                Outcome::Rebuild
            }
            Command::SelectToggle(id) => {
                if !self.selected.remove(&id) {
                    self.selected.insert(id);
                }
                Outcome::Rebuild
            }
            Command::SelectAll => {
                self.selected = data::list(&env.services.library, &self.prefs)
                    .iter()
                    .map(|b| b.id)
                    .collect();
                Outcome::Rebuild
            }
            Command::SelectNone => {
                self.selected.clear();
                Outcome::Rebuild
            }
            Command::MarkSelected(mark) => self.mark_selected(env, mark),
            Command::ClosePanel => {
                self.panel = None;
                Outcome::Rebuild
            }
        }
    }

    /// Marks one book finished from its details. The result is a toast, or a notice
    /// when the book cannot be marked.
    fn mark_finished_one(&mut self, env: &mut SurfaceEnv<'_>, id: i64) -> Outcome {
        match book_index::mark_finished(env.services, id) {
            Ok(()) => {
                env.toast(tr("Marked as finished"), 2.0);
            }
            Err(e) => env.notice(tr_format(&tr("cannot mark book %1: %2"), &[&id.to_string(), &e])),
        }
        Outcome::Rebuild
    }

    /// Starts a batch mark of the checked books. The writes run on a worker, and
    /// the count shows in the select bar when they are done. A batch in progress
    /// takes no second start.
    fn mark_selected(&mut self, env: &mut SurfaceEnv<'_>, mark: Mark) -> Outcome {
        if self.marking.get() {
            env.toast(tr("Marking books…"), 2.0);
            return Outcome::Handled;
        }
        if self.selected.is_empty() {
            env.toast(tr("No books selected"), 2.0);
            return Outcome::Handled;
        }
        let ids: Vec<i64> = self.selected.iter().copied().collect();
        let planned = match book_index::plan_marks(env.services, &ids, mark) {
            Ok(planned) => planned,
            Err(error) => {
                env.notice(tr_format(&tr("cannot mark the books: %1"), &[&error]));
                return Outcome::Handled;
            }
        };
        self.marking.set(true);
        let reply = book_index::run_marks(env.services.library_path(), planned, mark);
        let busy = self.marking.clone();
        let live = self.window.clone();
        env.schedule_in(MARK_POLL, move |ctx| check_marks(ctx, reply, busy, live, mark));
        Outcome::Rebuild
    }

    /// The text of the select bar: the number of checked books, or the progress of
    /// a batch.
    fn select_label(&self) -> String {
        if self.marking.get() {
            return tr("Marking books…");
        }
        match self.selected.len() {
            0 => tr("No books selected"),
            n => tr_format(
                &tr_n("1 book selected", "%1 books selected", n as i64),
                &[&n.to_string()],
            ),
        }
    }

    /// The book lines and cover-less lines of the folder on screen, and the
    /// heading for them. Also records the folder state for the folder actions.
    fn folder_view(&mut self, env: &SurfaceEnv<'_>, t: &Theme) -> Vec<WidgetPod> {
        let all: Vec<_> = data::all(&env.services.library)
            .into_iter()
            .filter(|b| self.prefs.show_finished || b.status != Status::Complete)
            .collect();
        let Some(root) = folders::root_of(&all) else {
            return vec![views::body_text(t, &tr("No books yet."))];
        };
        let current = self
            .folder
            .clone()
            .filter(|f| f.starts_with(&root))
            .unwrap_or_else(|| root.clone());
        let entries = folders::entries(&all, &current);
        #[cfg(feature = "lua")]
        {
            env.services.plugins.library.borrow_mut().folder = current.clone();
        }
        let mut lines = Vec::new();
        if current != root {
            lines.push(Line {
                key: "folder-up".into(),
                title: tr("Up"),
                detail: String::new(),
                cover: None,
                tap: "library:folder:up".into(),
                hold: None,
                enabled: true,
                checked: None,
            });
        }
        for (n, entry) in entries.iter().enumerate() {
            match entry {
                Entry::Dir { name, books, .. } => lines.push(Line {
                    key: format!("folder-{n}"),
                    title: name.clone(),
                    detail: tr_format(&tr_n("%1 book", "%1 books", *books as i64), &[&books.to_string()]),
                    cover: None,
                    tap: format!("library:folder:{n}"),
                    hold: None,
                    enabled: true,
                    checked: None,
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
                            checked: None,
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

    /// The Series view: the list of series, or the books of the series that is open.
    fn series_view(&mut self, env: &SurfaceEnv<'_>, t: &Theme) -> Vec<WidgetPod> {
        if let Some(name) = self.series.clone() {
            let books = data::series_books(&env.services.library, &self.prefs, &name);
            if !books.is_empty() {
                let lines: Vec<Line> = books
                    .iter()
                    .map(|b| Line::book(&data::info_without_cover(b)))
                    .collect();
                return vec![views::series_books(
                    t,
                    &name,
                    lines,
                    self.covers.as_ref().unwrap().clone(),
                )];
            }
            // The series has no books left to show, so the list comes back.
            self.series = None;
        }
        let groups = data::series_groups(&env.services.library, &self.prefs);
        self.series_names = groups.iter().map(|g| g.name.clone()).collect();
        if groups.is_empty() {
            return vec![views::body_text(t, &tr("No series yet."))];
        }
        vec![views::series_list(&groups)]
    }

    /// Select mode: the books of the view as rows with check boxes. A tap checks a book.
    fn select_content(&self, env: &SurfaceEnv<'_>, t: &Theme) -> Vec<WidgetPod> {
        let books = data::list(&env.services.library, &self.prefs);
        if books.is_empty() {
            return common::empty_library(t);
        }
        let lines: Vec<Line> = books
            .iter()
            .map(|b| Line::checkable(&data::info_without_cover(b), self.selected.contains(&b.id)))
            .collect();
        vec![WidgetPod::new(Expanded::new(
            WidgetPod::new(views::list_covers(lines, self.covers.as_ref().unwrap().clone())).with_key("list"),
            1,
        ))]
    }

    /// The content of the page for the current view, or the panel or the search page.
    fn content(&mut self, env: &mut SurfaceEnv<'_>) -> Vec<WidgetPod> {
        let t = env.services.theme.clone();
        if self.search {
            let results = self.results.borrow().clone();
            let has_recent = !env
                .services
                .library
                .recent_searches(1)
                .unwrap_or_default()
                .is_empty();
            return views::search(
                &t,
                &self.query,
                results,
                self.covers.as_ref().unwrap().clone(),
                has_recent,
            );
        }
        if let Some(panel) = self.panel.clone() {
            return vec![self.panel_widget(env, &t, &panel)];
        }
        if self.selecting {
            return self.select_content(env, &t);
        }
        match self.prefs.view {
            View::Folders => self.folder_view(env, &t),
            View::Series => self.series_view(env, &t),
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
            Panel::Filter => {
                let collection = collection_name(lib, self.prefs.collection);
                views::filter_panel(t, self.prefs.chip, collection.as_deref())
            }
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
                    // Inside a collection, the details offer to take the book out of it.
                    let remove = self
                        .prefs
                        .collection
                        .filter(|&cid| data::in_collection(lib, book, cid))
                        .map(|cid| format!("library:remove:{book}:{cid}"));
                    views::details_panel(t, &row, cover, "library:panel:close", remove)
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
            if matches!(self.prefs.view, View::Covers | View::List) {
                self.books = self.prefs.view;
            }
            self.loaded = true;
        }
        let t = env.services.theme.clone();
        let status = data::status_line(env.services);
        let mut head = vec![views::header(&t, &tr("Library"), status, self.selecting)];
        if self.selecting {
            head.push(views::select_bar(&t, self.select_label(), self.selected.len()));
        } else if !self.search {
            head.push(views::view_tabs(&t, self.prefs.view));
            if matches!(self.prefs.view, View::Covers | View::List) {
                let collection = collection_name(&env.services.library, self.prefs.collection);
                let filter = views::filter_name(self.prefs.chip, collection.as_deref());
                let sort = views::sort_name(self.prefs.sort);
                head.push(views::books_controls(&t, &filter, &sort));
            }
        }
        // The header rows sit `space.sm` apart. The content starts `space.md` below
        // the last of them, so the first row of covers never touches the controls.
        let content = self.content(env);
        let body = Padding::new(
            WidgetPod::new(
                Column::new(content)
                    .gap(t.space.md)
                    .cross_align(CrossAlign::Stretch),
            ),
            Insets {
                top: t.space.md,
                bottom: t.space.sm,
                ..Insets::default()
            },
        );
        let children = vec![
            WidgetPod::new(
                Column::new(head)
                    .gap(t.space.sm)
                    .cross_align(CrossAlign::Stretch)
                    .shrink(),
            ),
            WidgetPod::new(Expanded::new(WidgetPod::new(body), 1)),
            WidgetPod::new(kaia_ui::composite::NavBar::new(
                kaia_ui::composite::NavItem::Library,
            ))
            .with_key("nav-bar"),
        ];
        let page = Padding::new(
            WidgetPod::new(Column::new(children).cross_align(CrossAlign::Stretch)),
            Insets::all(t.space.lg),
        );
        let win = window(env.services, env.screen, page);
        *self.window.borrow_mut() = Some(win.clone());
        win
    }
}

/// The name of the collection with id `id`, when it exists.
fn collection_name(lib: &Library, id: Option<i64>) -> Option<String> {
    let id = id?;
    lib.collections()
        .unwrap_or_default()
        .into_iter()
        .find(|c| c.id == id)
        .map(|c| c.name)
}

/// The rows the search page shows for `query`. Finished books are left out when
/// the prefs hide them, and so are books whose file is gone.
fn search_lines(lib: &Library, query: &str, show_finished: bool) -> Vec<Line> {
    lib.search(query, SEARCH_LIMIT)
        .unwrap_or_default()
        .iter()
        .filter(|b| b.path.exists() && (show_finished || b.status != Status::Complete))
        .map(|b| Line::book(&data::info_without_cover(b)))
        .collect()
}

/// The recent searches as rows, newest first. Tapping a row runs that search. The
/// first row is a heading. Empty when there are no recent searches.
fn recent_lines(lib: &Library) -> Vec<Line> {
    let recent = lib.recent_searches(RECENT_SEARCHES).unwrap_or_default();
    if recent.is_empty() {
        return Vec::new();
    }
    let row = |key: String, title: String, tap: String, enabled: bool| Line {
        key,
        title,
        detail: String::new(),
        cover: None,
        tap,
        hold: None,
        enabled,
        checked: None,
    };
    let mut lines = vec![row(
        "recent-heading".into(),
        tr("Recent searches"),
        String::new(),
        false,
    )];
    for (n, text) in recent.iter().enumerate() {
        lines.push(row(
            format!("recent-{n}"),
            common::short(text),
            format!("library:search:recent:{n}"),
            true,
        ));
    }
    lines
}

/// Replaces the rows of the search page in the window that is on show.
fn show_results(live: &Live, ctx: &mut Ctx, lines: Vec<Line>) {
    let window = live.borrow().clone();
    if let Some(window) = window {
        window.update::<PagedList<Line>, _>(ctx, "results", |list, cx| list.set_items(lines, cx));
    }
}

/// The message for a finished batch.
fn mark_text(mark: Mark, report: MarkReport) -> String {
    let marked = report.marked as i64;
    let done = match mark {
        Mark::Finished => tr_format(
            &tr_n(
                "%1 book marked as finished.",
                "%1 books marked as finished.",
                marked,
            ),
            &[&report.marked.to_string()],
        ),
        Mark::Unread => tr_format(
            &tr_n("%1 book marked as unread.", "%1 books marked as unread.", marked),
            &[&report.marked.to_string()],
        ),
    };
    if report.failed == 0 {
        return done;
    }
    let failed = tr_format(
        &tr_n(
            "%1 book could not be changed.",
            "%1 books could not be changed.",
            report.failed as i64,
        ),
        &[&report.failed.to_string()],
    );
    format!("{done} {failed}")
}

/// Asks the batch worker for its report. Shows the report in the select bar when
/// it is in, and asks again later when it is not.
fn check_marks(
    ctx: &mut Ctx,
    reply: Receiver<Result<MarkReport, String>>,
    busy: Rc<Cell<bool>>,
    live: Live,
    mark: Mark,
) {
    match reply.try_recv() {
        Ok(result) => {
            busy.set(false);
            let text = match result {
                Ok(report) => mark_text(mark, report),
                Err(error) => tr_format(&tr("Cannot mark the books: %1"), &[&error]),
            };
            let window = live.borrow().clone();
            if let Some(window) = window {
                window.update::<Label, _>(ctx, "select-count", |label, cx| {
                    label.set_text(text);
                    cx.request_paint();
                });
            }
        }
        Err(TryRecvError::Empty) => {
            ctx.scheduler.schedule_in(
                MARK_POLL,
                Box::new(move |ctx: &mut Ctx| check_marks(ctx, reply, busy, live, mark)),
            );
        }
        Err(TryRecvError::Disconnected) => {
            busy.set(false);
            let window = live.borrow().clone();
            if let Some(window) = window {
                window.update::<Label, _>(ctx, "select-count", |label, cx| {
                    label.set_text(tr("Cannot mark the books."));
                    cx.request_paint();
                });
            }
        }
    }
}

/// Deletes the file of book `id` and reports the result. The row leaves the list
/// the next time the lists are read, because they skip books whose file is gone.
fn delete_file(env: &mut SurfaceEnv<'_>, id: i64) {
    let path = match env.services.library.book(id) {
        Ok(Some(book)) => book.path,
        _ => {
            env.notice(tr_format(&tr("no book %1"), &[&id.to_string()]));
            return;
        }
    };
    match fs::remove_file(&path) {
        Ok(()) => {
            if let Err(e) = env.services.library.remove_book(id) {
                env.toast(
                    tr_format(
                        &tr("File deleted, but the library could not update: %1"),
                        &[&e.to_string()],
                    ),
                    4.0,
                );
            }
            env.toast(tr("File deleted"), 2.0);
        }
        Err(e) => {
            env.notice(tr_format(
                &tr("cannot delete %1: %2"),
                &[&path.display().to_string(), &e.to_string()],
            ));
            env.toast(tr("The file could not be deleted"), 2.0);
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
        #[cfg(feature = "lua")]
        if cmd == "library:plugin-folder" {
            if let Some(path) = args.first().and_then(Value::as_str) {
                self.folder = Some(PathBuf::from(path));
                self.prefs.view = View::Folders;
                return Outcome::Rebuild;
            }
        }
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
    views::body_text(t, &tr("That book is no longer in the library."))
}
