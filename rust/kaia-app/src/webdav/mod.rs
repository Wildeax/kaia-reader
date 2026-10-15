//! Opt-in notes export. Only the UI thread owns the durable queue and hashes.
mod client;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

use crate::surfaces::{
    notes::{data::BookNotes, export},
    settings::keys,
};
use kaia_events::Settings;
use kaia_i18n::{format as tr_format, tr};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const ENABLED: &str = "webdav.enabled";
pub const SERVER: &str = "webdav.server";
pub const USERNAME: &str = "webdav.username";
pub const PASSWORD: &str = "webdav.password";
pub const FOLDER: &str = "webdav.folder";
const MAX_QUEUE: usize = 100;
const MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
struct Config {
    server: String,
    username: String,
    password: String,
    folder: String,
    #[cfg(test)]
    allow_local_http: bool,
}
impl Config {
    fn read(settings: &Settings, data: &Path) -> Result<Self, Failure> {
        Ok(Self {
            server: keys::text(settings, SERVER),
            username: keys::text(settings, USERNAME),
            password: fs::read_to_string(data.join("webdav-password")).map_err(|_| Failure::Credentials)?,
            folder: keys::text(settings, FOLDER),
            #[cfg(test)]
            allow_local_http: false,
        })
    }
    fn folder_url(&self) -> Result<url::Url, Failure> {
        let mut url = url::Url::parse(self.server.trim()).map_err(|_| Failure::Address)?;
        let secure = url.scheme() == "https";
        #[cfg(test)]
        let secure = secure
            || (self.allow_local_http
                && url.scheme() == "http"
                && url
                    .host_str()
                    .is_some_and(|h| h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())));
        if !secure
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(Failure::Address);
        }
        let parts: Vec<_> = self
            .folder
            .trim_matches('/')
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();
        if parts
            .iter()
            .any(|p| *p == "." || *p == ".." || p.contains('\\') || p.chars().any(char::is_control))
        {
            return Err(Failure::Address);
        }
        {
            let mut path = url.path_segments_mut().map_err(|_| Failure::Address)?;
            path.pop_if_empty();
            for part in parts {
                path.push(part);
            }
            path.push("");
        }
        Ok(url)
    }
    fn destination(&self) -> Result<String, Failure> {
        Ok(hash(
            format!("{}\n{}", self.folder_url()?, self.username).as_bytes(),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Failure {
    Address,
    Credentials,
    Unauthorized,
    Network,
    Status(u16),
    Held,
    Storage,
    Full,
}
impl Failure {
    fn text(&self) -> String {
        match self {
            Self::Address => tr("Use an HTTPS WebDAV address without a login, query or fragment, and a folder without parent paths. HTTP is not allowed."),
            Self::Credentials => tr("Enter a WebDAV username and password in Network and sync."),
            Self::Unauthorized => tr("The WebDAV server refused the login. Check the username and password. Kaia will not retry until you edit the account."),
            Self::Network => tr("WebDAV could not connect. Check Wi-Fi and the server's TLS certificate."),
            Self::Status(code) => tr_format(&tr("The WebDAV server returned HTTP %1."), &[&code.to_string()]),
            Self::Held => tr("WebDAV export is waiting for Wi-Fi with airplane mode off."),
            Self::Storage => tr("The WebDAV queue could not be read or saved. Try again."),
            Self::Full => tr("The WebDAV queue is full. Connect to Wi-Fi and export again."),
        }
    }
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A separate plaintext credential, replaced atomically with owner-only permissions.
pub fn store_password(data: &Path, password: &str) -> io::Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(password.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(data.join("webdav-password")).map_err(|e| e.error)?;
    fs::File::open(data)?.sync_all()
}

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    document: String,
    destination: String,
    name: String,
    hash: String,
    markdown: String,
    json: String,
    attempts: u8,
    due: i64,
    reported: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct State {
    queue: Vec<Entry>,
    hashes: BTreeMap<String, String>,
    #[serde(default)]
    names: BTreeMap<String, String>,
    refused: bool,
}
struct Job {
    result: mpsc::Receiver<Result<(), Failure>>,
    entry: Option<Entry>,
    generation: u64,
}

pub struct WebDav {
    data: PathBuf,
    state: Option<State>,
    job: Option<Job>,
    allowed: Arc<AtomicBool>,
    generation: u64,
    storage_reported: bool,
    dirty: bool,
    #[cfg(test)]
    allow_local_http: bool,
}
impl Drop for WebDav {
    fn drop(&mut self) {
        self.allowed.store(false, Ordering::SeqCst);
    }
}
impl WebDav {
    pub fn new(data: PathBuf) -> Self {
        Self {
            data,
            state: None,
            job: None,
            allowed: Arc::new(AtomicBool::new(false)),
            generation: 0,
            storage_reported: false,
            dirty: false,
            #[cfg(test)]
            allow_local_http: false,
        }
    }
    fn load(&mut self) -> Result<&mut State, Failure> {
        if self.state.is_none() {
            self.state = Some(match fs::read(self.data.join("webdav-queue.json")) {
                Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| Failure::Storage)?,
                Err(e) if e.kind() == io::ErrorKind::NotFound => State::default(),
                Err(_) => return Err(Failure::Storage),
            });
        }
        Ok(self.state.as_mut().unwrap())
    }
    fn save(&mut self) -> Result<(), Failure> {
        self.dirty = true;
        let bytes =
            serde_json::to_vec(self.state.as_ref().ok_or(Failure::Storage)?).map_err(|_| Failure::Storage)?;
        crate::durable::atomic_write(&self.data.join("webdav-queue.json"), &bytes)
            .map_err(|_| Failure::Storage)?;
        self.dirty = false;
        Ok(())
    }
    fn config(&self, settings: &Settings) -> Result<Config, Failure> {
        let config = Config::read(settings, &self.data)?;
        #[cfg(test)]
        let config = Config {
            allow_local_http: self.allow_local_http,
            ..config
        };
        Ok(config)
    }
    /// Account edits cancel unsent requests and release a refused login.
    pub fn account_changed(&mut self) -> Result<(), String> {
        self.allowed.store(false, Ordering::SeqCst);
        self.allowed = Arc::new(AtomicBool::new(false));
        self.generation += 1;
        self.storage_reported = false;
        let state = self.load().map_err(|e| e.text())?;
        state.refused = false;
        for entry in &mut state.queue {
            entry.attempts = 0;
            entry.due = 0;
            entry.reported = false;
        }
        self.save().map_err(|e| e.text())
    }
    pub fn update_gate(&self, settings: &Settings, connected: bool) {
        self.allowed.store(
            settings.is_true(ENABLED) && !keys::airplane_mode(settings) && connected,
            Ordering::SeqCst,
        );
    }
    pub fn enqueue(
        &mut self,
        settings: &Settings,
        books: &[BookNotes],
        now: i64,
        manual: bool,
    ) -> Result<(), String> {
        if !settings.is_true(ENABLED) {
            return Ok(());
        }
        let config = self.config(settings).map_err(|e| e.text())?;
        self.enqueue_for(&config, books, now, manual)
            .map_err(|e| e.text())
    }
    fn enqueue_for(
        &mut self,
        config: &Config,
        books: &[BookNotes],
        now: i64,
        manual: bool,
    ) -> Result<(), Failure> {
        let destination = config.destination()?;
        let in_flight = self.job.as_ref().and_then(|job| job.entry.clone());
        self.load()?;
        self.dirty = true;
        let state = self.load()?;
        for book in books {
            let key = format!("{destination}/{}", book.md5);
            let mut name = filename(book);
            if state.names.iter().any(|(other, used)| {
                other != &key && other.starts_with(&destination) && used.eq_ignore_ascii_case(&name)
            }) {
                name.push_str(&format!(" - {}", &hash(book.md5.as_bytes())[..12]));
            }
            state.names.insert(key.clone(), name.clone());
            let books = std::slice::from_ref(book);
            // The timestamp changes on export. Exclude it from the content hash.
            let fingerprint = hash(
                format!(
                    "{name}\n{}\n{}",
                    export::json_text(books, 0),
                    export::markdown_text(books, 0)
                )
                .as_bytes(),
            );
            if book.notes.is_empty()
                && book.ink_counts.is_empty()
                && !state.hashes.contains_key(&key)
                && !state.queue.iter().any(|e| e.document == book.md5)
            {
                continue;
            }
            state
                .queue
                .retain(|e| e.document != book.md5 || e.destination == destination);
            if state.hashes.get(&key) == Some(&fingerprint)
                && !in_flight.as_ref().is_some_and(|e| {
                    e.document == book.md5 && e.destination == destination && e.hash != fingerprint
                })
            {
                state
                    .queue
                    .retain(|e| e.document != book.md5 || e.destination != destination);
                continue;
            }
            if let Some(old) = state
                .queue
                .iter_mut()
                .find(|e| e.document == book.md5 && e.destination == destination)
            {
                if old.hash == fingerprint {
                    if manual {
                        old.attempts = 0;
                        old.due = 0;
                        old.reported = false;
                    }
                    continue;
                }
            }
            let entry = Entry {
                document: book.md5.clone(),
                destination: destination.clone(),
                name,
                hash: fingerprint,
                markdown: export::markdown_text(books, now),
                json: export::json_text(books, now),
                attempts: 0,
                due: 0,
                reported: false,
            };
            let bytes: usize = state
                .queue
                .iter()
                .filter(|e| e.document != book.md5)
                .map(|e| e.markdown.len() + e.json.len())
                .sum();
            if bytes + entry.markdown.len() + entry.json.len() > MAX_BYTES
                || (state.queue.len() >= MAX_QUEUE && !state.queue.iter().any(|e| e.document == book.md5))
            {
                return Err(Failure::Full);
            }
            state.queue.retain(|e| e.document != book.md5);
            state.queue.push(entry);
        }
        self.save()
    }
    pub fn test_connection(&mut self, settings: &Settings, connected: bool) -> Result<(), String> {
        if !settings.is_true(ENABLED) {
            return Err(tr("Turn on Export notes to WebDAV first."));
        }
        self.update_gate(settings, connected);
        if !self.allowed.load(Ordering::SeqCst) {
            return Err(Failure::Held.text());
        }
        if self.job.is_some() {
            return Err(tr("A WebDAV request is already running."));
        }
        if self.load().map_err(|e| e.text())?.refused {
            return Err(Failure::Unauthorized.text());
        }
        let config = self.config(settings).map_err(|e| e.text())?;
        self.start(config, None).map_err(|e| e.text())
    }
    fn start(&mut self, config: Config, entry: Option<Entry>) -> Result<(), Failure> {
        let client = client::Client::new(&config, self.allowed.clone(), entry.is_none())?;
        let (tx, result) = mpsc::channel();
        let upload = entry.clone();
        std::thread::spawn(move || {
            let reply = match upload {
                Some(e) => client.upload(&e.name, &e.markdown, &e.json),
                None => client.test(),
            };
            let _ = tx.send(reply);
        });
        self.job = Some(Job {
            result,
            entry,
            generation: self.generation,
        });
        Ok(())
    }
    pub fn tick(&mut self, settings: &Settings, connected: bool, now: i64) -> Vec<String> {
        self.update_gate(settings, connected);
        let mut messages = Vec::new();
        if self.dirty && self.save().is_err() {
            if !self.storage_reported {
                messages.push(Failure::Storage.text());
                self.storage_reported = true;
            }
            return messages;
        }
        if let Err(error) = self.poll(now, &mut messages) {
            if !self.storage_reported {
                messages.push(error.text());
                self.storage_reported = true;
            }
            return messages;
        }
        if !self.allowed.load(Ordering::SeqCst) || self.job.is_some() {
            return messages;
        }
        let result = (|| {
            let state = self.load()?;
            if state.refused || !state.queue.iter().any(|e| e.attempts < 3 && e.due <= now) {
                return Ok(());
            }
            let config = self.config(settings)?;
            let destination = config.destination()?;
            self.dispatch(config, &destination, now)
        })();
        if let Err(error) = result {
            if !self.storage_reported {
                messages.push(error.text());
                self.storage_reported = true;
            }
        }
        messages
    }
    fn dispatch(&mut self, config: Config, destination: &str, now: i64) -> Result<(), Failure> {
        let state = self.load()?;
        if state.refused {
            return Ok(());
        }
        let Some(entry) = state
            .queue
            .iter_mut()
            .find(|e| e.destination == destination && e.attempts < 3 && e.due <= now)
        else {
            return Ok(());
        };
        entry.attempts += 1;
        entry.due = now + if entry.attempts == 1 { 30 } else { 120 };
        let entry = entry.clone();
        // A failed pair may have replaced only one file. Until both succeed,
        // no earlier hash proves what the server holds.
        state
            .hashes
            .remove(&format!("{}/{}", entry.destination, entry.document));
        self.save()?;
        self.start(config, Some(entry))
    }
    fn poll(&mut self, now: i64, messages: &mut Vec<String>) -> Result<(), Failure> {
        let Some(job) = self.job.as_ref() else {
            return Ok(());
        };
        let result = match job.result.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return Ok(()),
            Err(_) => Err(Failure::Network),
        };
        let job = self.job.take().unwrap();
        if job.generation != self.generation {
            return Ok(());
        }
        let state = self.load()?;
        if result == Err(Failure::Unauthorized) && !state.refused {
            state.refused = true;
            messages.push(Failure::Unauthorized.text());
        }
        if let Some(sent) = job.entry {
            match result {
                Ok(()) => {
                    state.hashes.insert(
                        format!("{}/{}", sent.destination, sent.document),
                        sent.hash.clone(),
                    );
                    state.queue.retain(|e| {
                        !(e.document == sent.document
                            && e.destination == sent.destination
                            && e.hash == sent.hash)
                    });
                }
                Err(error) => {
                    if let Some(entry) = state.queue.iter_mut().find(|e| {
                        e.document == sent.document
                            && e.destination == sent.destination
                            && e.hash == sent.hash
                    }) {
                        if error == Failure::Held {
                            entry.attempts = entry.attempts.saturating_sub(1);
                            entry.due = now;
                        } else {
                            entry.due = now + if entry.attempts == 1 { 30 } else { 120 };
                            if !entry.reported && error != Failure::Unauthorized {
                                messages.push(error.text());
                            }
                            entry.reported = true;
                        }
                    }
                }
            }
        } else if result != Err(Failure::Unauthorized) {
            messages.push(match result {
                Ok(()) => tr("The WebDAV server answered."),
                Err(error) => error.text(),
            });
        }
        self.save()
    }
    pub fn stop(&mut self) {
        self.allowed.store(false, Ordering::SeqCst);
        self.generation += 1;
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    /// Check queued work and connection state without waking an idle device every frame.
    pub fn wait(&self, settings: &Settings) -> Option<std::time::Duration> {
        (settings.is_true(ENABLED)
            && !keys::airplane_mode(settings)
            && self
                .state
                .as_ref()
                .is_none_or(|s| !s.refused && s.queue.iter().any(|e| e.attempts < 3)))
        .then_some(std::time::Duration::from_secs(5))
    }
}
fn filename(book: &BookNotes) -> String {
    fn clean(text: &str) -> String {
        let text: String = text
            .chars()
            .map(|c| {
                if c.is_control() || "/\\:*?\"<>|".contains(c) {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let mut out = String::new();
        for c in text.trim_matches([' ', '.']).chars() {
            if out.len() + c.len_utf8() > 90 {
                break;
            }
            out.push(c);
        }
        if out.is_empty() {
            tr("Untitled")
        } else {
            out
        }
    }
    format!(
        "{} - {}",
        clean(&book.title),
        clean(book.author.as_deref().unwrap_or(&tr("Unknown author")))
    )
}

/// Loads saved notes after a successful book save, or after an explicit export.
pub fn enqueue_saved(env: &mut crate::surface::SurfaceEnv<'_>, document: Option<&str>, manual: bool) {
    if !env.services.settings.is_true(ENABLED) {
        return;
    }
    let mut books = crate::surfaces::notes::load_books_with_empty(env.services);
    if let Some(document) = document {
        books.retain(|b| b.md5 == document);
    }
    if let Err(message) =
        env.services
            .webdav
            .enqueue(&env.services.settings, &books, crate::sync::now_unix(), manual)
    {
        env.toast(message, 5.0);
    }
}
