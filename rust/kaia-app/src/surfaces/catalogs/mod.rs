//! Catalog browsing and Calibre wireless, opened from Network and sync. The Calibre
//! view also searches and browses the metadata file that a wireless transfer leaves.
mod cache;
pub mod store;
mod views;
pub mod worker;

use std::path::PathBuf;

use kaia_calibre::metadata::{Field, Options};
use kaia_events::{Event, ShowOpts, TaskId, Value};
use kaia_gfx::Pixmap;
use kaia_i18n::tr;
use kaia_opds::{Catalog, Entry, Feed};
use kaia_ui::Window;

use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::settings::keys;
use crate::surfaces::Route;
use cache::{parse_field, Cache};
use worker::{worker_text, Reply, Request, Worker};

/// The text field of the Calibre search. The address is field 0 and the password is 1.
const CALIBRE_QUERY_FIELD: usize = 2;

pub(super) enum View {
    Catalogs,
    Suggestions,
    Editor {
        index: Option<usize>,
        catalog: Catalog,
    },
    Feed {
        catalog: Catalog,
        feed: Feed,
    },
    Book {
        catalog: Catalog,
        entry: Entry,
        cover: Option<Pixmap>,
    },
    Calibre,
    /// Books from a Calibre search, or from a browse group. Each index is a position in
    /// the loaded metadata.
    CalibreBooks {
        title: String,
        books: Vec<usize>,
    },
    /// The values of one browse field, with the number of books that have each one.
    CalibreGroups {
        field: Field,
        groups: Vec<(String, usize)>,
    },
    Downloads,
}

pub struct CatalogSurface {
    view: View,
    history: Vec<View>,
    worker: Option<Worker>,
    pending: Option<Request>,
    status: String,
    query: String,
    typing: bool,
    field: usize,
    servers: Vec<kaia_calibre::Server>,
    address: String,
    password: String,
    download_folder: String,
    book: Option<i64>,
    timer: Option<TaskId>,
    loading_first: bool,
    connection: bool,
    initialized: bool,
    /// The Calibre metadata file, read when the Calibre view first opens.
    cache: Cache,
    /// The folder that holds the metadata file: the downloads folder.
    cache_root: PathBuf,
    calibre_query: String,
    /// The last problem in the Calibre session, shown when the session ends.
    summary: Option<String>,
}

impl Default for CatalogSurface {
    fn default() -> Self {
        Self::new(View::Catalogs)
    }
}
impl CatalogSurface {
    fn new(view: View) -> Self {
        Self {
            view,
            history: Vec::new(),
            worker: None,
            pending: None,
            status: String::new(),
            query: String::new(),
            typing: false,
            field: 0,
            servers: Vec::new(),
            address: String::new(),
            password: String::new(),
            download_folder: String::new(),
            book: None,
            timer: None,
            loading_first: false,
            connection: false,
            initialized: false,
            cache: Cache::default(),
            cache_root: PathBuf::new(),
            calibre_query: String::new(),
            summary: None,
        }
    }
    pub fn calibre() -> Self {
        Self::new(View::Calibre)
    }

    fn enter(&mut self, view: View) {
        let old = std::mem::replace(&mut self.view, view);
        self.history.push(old);
        self.status.clear();
        self.typing = false;
    }

    fn request(&mut self, request: Request, env: &mut SurfaceEnv<'_>) {
        if self.worker.is_some() {
            return;
        }
        if matches!(env.services.platform.kind, kaia_platform::platform::Kind::Kobo(_))
            && !env.services.platform.network.is_wifi_on()
        {
            // Airplane mode: Kaia does not offer to turn Wi-Fi on. Calibre takes this path too.
            if keys::airplane_mode(&env.services.settings) {
                env.notice(tr("Airplane mode is on. Turn it off to use Wi-Fi."));
                return;
            }
            self.pending = Some(request);
            self.status = tr("Wi-Fi is off. Turn it on to continue?");
        } else {
            self.start(request, false, env);
        }
    }

    fn start(&mut self, request: Request, wifi: bool, env: &mut SurfaceEnv<'_>) {
        self.status = tr("Starting...");
        self.summary = None;
        self.connection = matches!(request, Request::Connect { .. });
        self.worker = Some(Worker::start(
            request,
            store::downloads(env.services),
            env.services.library_path(),
            wifi,
        ));
        self.wake(env);
    }
    fn wake(&mut self, env: &mut SurfaceEnv<'_>) {
        if self.worker.is_some() {
            if let Some(timer) = self.timer.take() {
                env.ctx.scheduler.unschedule(timer);
            }
            self.timer = Some(env.schedule_in(0.1, |_| {}));
        }
    }
    fn flush(&mut self, env: &mut SurfaceEnv<'_>) {
        if env
            .services
            .settings
            .flush(&env.services.settings_path())
            .is_err()
        {
            self.status = tr("Cannot save settings. Check device storage.");
        }
    }
}

impl Surface for CatalogSurface {
    fn route(&self) -> Route {
        match self.view {
            View::Calibre | View::CalibreBooks { .. } | View::CalibreGroups { .. } => Route::Calibre,
            View::Book { .. } => Route::CatalogBook,
            _ => Route::Catalogs,
        }
    }
    fn show_opts(&self) -> ShowOpts {
        if matches!(self.view, View::Book { .. }) {
            ShowOpts {
                modal: true,
                refresh: Some(kaia_events::Refresh::new(kaia_events::RefreshMode::Ui)),
                ..ShowOpts::default()
            }
        } else {
            ShowOpts {
                covers_fullscreen: true,
                refresh: Some(kaia_events::Refresh::new(kaia_events::RefreshMode::Ui)),
                ..ShowOpts::default()
            }
        }
    }
    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        if !self.initialized {
            self.initialized = true;
            if let View::Book { catalog, entry, .. } = &self.view {
                if let Some(url) = entry.image.as_ref().or(entry.thumbnail.as_ref()) {
                    self.request(Request::Cover(catalog.clone(), url.clone()), env);
                }
            }
            if matches!(self.view, View::Calibre) {
                self.address = env
                    .services
                    .settings
                    .read("calibre.address")
                    .as_str()
                    .unwrap_or("")
                    .into();
                self.cache_root = store::downloads(env.services);
                self.reload_cache();
            }
        }
        Some(views::build(self, env.services, env.screen))
    }
    fn on_hide(&mut self, env: &mut SurfaceEnv<'_>) {
        self.worker.take();
        self.pending.take();
        if let Some(timer) = self.timer.take() {
            env.ctx.scheduler.unschedule(timer);
        }
    }
    fn on_event(&mut self, event: &Event, env: &mut SurfaceEnv<'_>) -> Outcome {
        if event.name == "SaveState" || event.name == "Suspend" {
            if let Some(worker) = &self.worker {
                worker.cancel();
            }
            self.pending = None;
        }
        if event.name != "CatalogTick" {
            return Outcome::NotMine;
        }
        let mut changed = false;
        while let Some(reply) = self.worker.as_ref().and_then(Worker::next) {
            let cancelled = self.worker.as_ref().is_some_and(Worker::cancelled);
            if cancelled && !matches!(reply, Reply::Finished(_)) {
                continue;
            }
            match reply {
                Reply::Progress(progress) => {
                    let text = progress.text();
                    changed |= self.status != text;
                    self.status = text;
                }
                Reply::Feed(feed) => {
                    let catalog = match &self.view {
                        View::Feed { catalog, .. } => Some(catalog.clone()),
                        _ => None,
                    };
                    if let Some(catalog) = catalog {
                        if self.loading_first {
                            self.view = View::Feed { catalog, feed };
                            self.loading_first = false;
                        } else {
                            self.enter(View::Feed { catalog, feed });
                        }
                    }
                    changed = true;
                }
                Reply::Cover(image) => {
                    if let View::Book { cover, .. } = &mut self.view {
                        *cover = Some(image);
                    }
                    changed = true;
                }
                Reply::Servers(servers) => {
                    self.servers = servers;
                    changed = true;
                }
                Reply::Book(id) => {
                    self.book = Some(id);
                    changed = true;
                }
                Reply::Summary(problem) => self.summary = Some(problem),
                Reply::Finished(result) => {
                    self.worker = None;
                    // A transfer may have rewritten the metadata file.
                    if matches!(self.view, View::Calibre) {
                        self.reload_cache();
                    }
                    self.status = match (result, self.summary.take()) {
                        (Err(message), _) => worker_text(&message),
                        // A session that went on past a problem ends by showing it.
                        (Ok(()), Some(problem)) => worker_text(&problem),
                        (Ok(()), None) if self.book.is_some() => tr("Book saved to the library."),
                        (Ok(()), None) if matches!(self.view, View::Calibre) => {
                            if self.servers.is_empty() {
                                tr("No active connection. Use a manual address or discover servers.")
                            } else {
                                tr("Choose a server to connect.")
                            }
                        }
                        (Ok(()), None) => String::new(),
                    };
                    self.pending = None;
                    changed = true;
                }
            }
        }
        self.wake(env);
        if changed {
            Outcome::Rebuild
        } else {
            Outcome::NotMine
        }
    }

    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        if cmd == "back" && matches!(self.view, View::Book { .. }) {
            return Outcome::Navigate(Nav::Pop);
        }
        if cmd == "back" && !matches!(self.view, View::Book { .. }) && !self.history.is_empty() {
            self.worker.take();
            self.pending = None;
            self.view = self.history.pop().unwrap();
            self.status.clear();
            self.typing = false;
            return Outcome::Rebuild;
        }
        let Some(action) = cmd.strip_prefix("catalogs:") else {
            return Outcome::NotMine;
        };
        if action == "cancel" {
            self.pending = None;
            if let Some(worker) = &self.worker {
                worker.cancel();
                self.status = tr("Cancelling...");
            } else {
                self.status.clear();
            }
            return Outcome::Rebuild;
        }
        if action == "wifi" {
            // A prompt from before airplane mode was turned on must not start Wi-Fi.
            if keys::airplane_mode(&env.services.settings) {
                self.pending = None;
                self.status.clear();
                env.notice(tr("Airplane mode is on. Turn it off to use Wi-Fi."));
            } else if let Some(request) = self.pending.take() {
                self.start(request, true, env);
            }
            return Outcome::Rebuild;
        }
        if action == "open-book" {
            if let Some(book) = self.book {
                self.worker.take();
                self.status = tr("Book saved to the library.");
                return Outcome::Navigate(Nav::OpenBook { book, location: None });
            }
        }
        if let Some(field) = action.strip_prefix("change:") {
            let text = args.first().and_then(Value::as_str).unwrap_or("").to_string();
            match field {
                "name" => {
                    if let View::Editor { catalog, .. } = &mut self.view {
                        catalog.title = text;
                    }
                }
                "url" => {
                    if let View::Editor { catalog, .. } = &mut self.view {
                        catalog.url = text;
                    }
                }
                "username" => {
                    if let View::Editor { catalog, .. } = &mut self.view {
                        catalog.username = (!text.is_empty()).then_some(text);
                    }
                }
                "password" => {
                    if let View::Editor { catalog, .. } = &mut self.view {
                        catalog.password = (!text.is_empty()).then_some(text);
                    }
                }
                "query" => self.query = text,
                "address" => self.address = text,
                "calibre-password" => self.password = text,
                "folder" => self.download_folder = text,
                "calibre-query" => self.calibre_query = text,
                _ => {}
            }
            return Outcome::Handled;
        }
        if self.worker.is_some() || self.pending.is_some() {
            return Outcome::Handled;
        }
        if let Some(outcome) = self.calibre_command(action, env) {
            return outcome;
        }
        if let Some(index) = action
            .strip_prefix("field:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            self.field = index;
            self.typing = true;
        } else if let Some(index) = action
            .strip_prefix("browse:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let Some(catalog) = store::catalogs(&env.services.settings).get(index).cloned() {
                // Keep the catalog identity while the first feed loads.
                let request = Request::Feed(catalog.clone(), catalog.url.clone());
                self.enter(View::Feed {
                    catalog: catalog.clone(),
                    feed: empty_feed(&catalog.url, &catalog.title),
                });
                self.loading_first = true;
                self.request(request, env);
            }
        } else if let Some(index) = action.strip_prefix("edit:").and_then(|n| n.parse::<usize>().ok()) {
            if let Some(catalog) = store::catalogs(&env.services.settings).get(index).cloned() {
                self.enter(View::Editor {
                    index: Some(index),
                    catalog,
                });
            }
        } else if let Some(index) = action
            .strip_prefix("suggest:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let Some(catalog) = kaia_opds::default_catalogs().get(index).cloned() {
                self.enter(View::Editor { index: None, catalog });
            }
        } else if let Some(index) = action
            .strip_prefix("entry:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let View::Feed { catalog, feed } = &self.view {
                if let Some(entry) = feed.entries.get(index) {
                    if !entry.acquisitions.is_empty() {
                        return Outcome::Navigate(Nav::Push(Box::new(Self::new(View::Book {
                            catalog: catalog.clone(),
                            entry: entry.clone(),
                            cover: None,
                        }))));
                    }
                    if let Some(url) = &entry.navigation {
                        self.request(Request::Feed(catalog.clone(), url.clone()), env);
                    }
                }
            }
        } else if let Some(index) = action
            .strip_prefix("download:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let View::Book { catalog, entry, .. } = &self.view {
                let acquisition = entry.downloads().nth(index).cloned();
                if let Some(acquisition) = acquisition {
                    self.book = None;
                    self.request(Request::Download(catalog.clone(), acquisition), env);
                }
            }
        } else if let Some(index) = action
            .strip_prefix("server:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if let Some(server) = self.servers.get(index) {
                self.address = server.address.to_string();
                self.connect(env);
            }
        } else {
            match action {
                "suggestions" => self.enter(View::Suggestions),
                "add" => self.enter(View::Editor {
                    index: None,
                    catalog: Catalog {
                        title: String::new(),
                        url: String::new(),
                        username: None,
                        password: None,
                    },
                }),
                "save" => {
                    if let View::Editor { index, catalog } = &mut self.view {
                        if catalog.title.trim().is_empty() || !valid_url(&catalog.url) {
                            self.status = tr("Enter a name and an HTTP or HTTPS address.");
                            return Outcome::Rebuild;
                        }
                        catalog.url = kaia_opds::normalize_url(&catalog.url);
                        let mut list = store::catalogs(&env.services.settings);
                        if let Some(i) = index {
                            if let Some(old) = list.get_mut(*i) {
                                *old = catalog.clone();
                            }
                        } else {
                            list.push(catalog.clone());
                        }
                        store::save(&mut env.services.settings, &list);
                        self.view = View::Catalogs;
                        self.history.clear();
                        self.typing = false;
                        self.status = tr("Catalog saved.");
                        self.flush(env);
                    }
                }
                "delete" => {
                    if let View::Editor { index: Some(i), .. } = &self.view {
                        let mut list = store::catalogs(&env.services.settings);
                        if *i < list.len() {
                            list.remove(*i);
                        }
                        store::save(&mut env.services.settings, &list);
                        self.view = View::Catalogs;
                        self.history.clear();
                        self.status = tr("Catalog deleted.");
                        self.flush(env);
                    }
                }
                "next" | "previous" => {
                    if let View::Feed { catalog, feed } = &self.view {
                        let url = if action == "next" { &feed.next } else { &feed.prev };
                        if let Some(url) = url {
                            self.request(Request::Feed(catalog.clone(), url.clone()), env);
                        }
                    }
                }
                "search" => {
                    self.typing = true;
                    self.field = 0;
                }
                "search-go" => {
                    if let View::Feed { catalog, feed } = &self.view {
                        let request = Request::Search(catalog.clone(), feed.clone(), self.query.clone());
                        self.typing = false;
                        self.request(request, env);
                    }
                }
                "hide-keyboard" => self.typing = false,
                "discover" => {
                    self.servers.clear();
                    self.request(Request::Discover, env);
                }
                "connect" => self.connect(env),
                "downloads" => {
                    self.download_folder = store::downloads(env.services).display().to_string();
                    self.enter(View::Downloads);
                    self.typing = true;
                }
                "save-folder" => {
                    let path = std::path::Path::new(&self.download_folder);
                    if !path.is_absolute() {
                        self.status = tr("Enter an absolute folder path.");
                    } else {
                        env.services
                            .settings
                            .save(store::DOWNLOADS, Value::from(self.download_folder.clone()));
                        self.typing = false;
                        self.status = tr("Downloads folder saved.");
                        self.flush(env);
                    }
                }
                _ => return Outcome::NotMine,
            }
        }
        Outcome::Rebuild
    }
}
impl CatalogSurface {
    fn reload_cache(&mut self) {
        if !self.cache_root.as_os_str().is_empty() {
            self.cache = Cache::load(&self.cache_root);
        }
    }

    /// The offline Calibre search and browse actions. Returns None for any other action.
    fn calibre_command(&mut self, action: &str, env: &mut SurfaceEnv<'_>) -> Option<Outcome> {
        if action == "calibre-search" {
            self.typing = true;
            self.field = CALIBRE_QUERY_FIELD;
            return Some(Outcome::Rebuild);
        }
        if action == "calibre-search-go" {
            let query = self.calibre_query.trim().to_owned();
            if query.is_empty() {
                self.status = tr("Enter a word to search.");
                return Some(Outcome::Rebuild);
            }
            let books = self
                .cache
                .metadata()
                .map(|m| m.search(&query, Options::default()))
                .unwrap_or_default();
            self.status.clear();
            self.enter(View::CalibreBooks {
                title: tr("Search results"),
                books,
            });
            return Some(Outcome::Rebuild);
        }
        if let Some(key) = action.strip_prefix("calibre-browse:") {
            let field = parse_field(key)?;
            let groups = self.cache.metadata().map(|m| m.groups(field)).unwrap_or_default();
            self.status.clear();
            self.enter(View::CalibreGroups { field, groups });
            return Some(Outcome::Rebuild);
        }
        if let Some(n) = action
            .strip_prefix("calibre-group:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            let (field, value) = match &self.view {
                View::CalibreGroups { field, groups } => match groups.get(n) {
                    Some((value, _)) => (*field, value.clone()),
                    None => return Some(Outcome::Handled),
                },
                _ => return Some(Outcome::Handled),
            };
            let books = self
                .cache
                .metadata()
                .map(|m| m.books_with(field, &value))
                .unwrap_or_default();
            self.status.clear();
            self.enter(View::CalibreBooks { title: value, books });
            return Some(Outcome::Rebuild);
        }
        if let Some(pos) = action
            .strip_prefix("calibre-book:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            let index = match &self.view {
                View::CalibreBooks { books, .. } => books.get(pos).copied(),
                _ => None,
            };
            let Some(index) = index else {
                return Some(Outcome::Handled);
            };
            return Some(match self.cache.library_id(&env.services.library, index) {
                Some(book) => Outcome::Navigate(Nav::OpenBook { book, location: None }),
                None => {
                    self.status = tr("This book is not on the device.");
                    Outcome::Rebuild
                }
            });
        }
        None
    }

    fn connect(&mut self, env: &mut SurfaceEnv<'_>) {
        if self.address.trim().is_empty() {
            self.status = tr("Enter the server address and port.");
            return;
        }
        env.services
            .settings
            .save("calibre.address", Value::from(self.address.clone()));
        self.flush(env);
        self.typing = false;
        self.request(
            Request::Connect {
                address: self.address.clone(),
                password: self.password.clone(),
            },
            env,
        );
    }
}
fn valid_url(text: &str) -> bool {
    url::Url::parse(&kaia_opds::normalize_url(text)).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
    })
}
fn empty_feed(url: &str, title: &str) -> Feed {
    Feed {
        title: title.into(),
        url: url.into(),
        kind: kaia_opds::FeedKind::Navigation,
        entries: Vec::new(),
        next: None,
        prev: None,
        search_template: None,
        search_description: None,
        facets: Vec::new(),
    }
}
