//! Progress sync is opt-in. A single worker runs requests away from the UI thread.
//!
//! The worker has no translation catalog. It reports failures as [`Failure`] values,
//! and the UI thread writes their text in [`ProgressSync::tick`] and [`ProgressSync::health`].

use kaia_i18n::{format as tr_format, tr};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex, MutexGuard,
};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kaia_book::{BookSession, Location};
use kaia_events::{Settings, Value};
use kaia_platform::{DeviceInfo, Kind};
use kaia_sync::{Credentials, Progress, SyncClient, SyncOptions, Timeouts};

use crate::surfaces::settings::keys;
use crate::sync_queue::SyncQueue;

/// The device name for a device without a model name. KOReader sends the model name or the hostname.
pub const DEFAULT_DEVICE_NAME: &str = "Kaia Reader";

/// What a worker sends back to the UI thread.
pub enum Reply {
    Remote(i64, Progress),
    Pushed(String),
    Error(String),
    ServerAnswered,
    Done,
}

/// A failed job. The UI thread writes its text, see [`error_text`].
enum Failure {
    Request(kaia_sync::Error),
    /// The durable queue could not be read or written.
    Queue,
}

impl From<kaia_sync::Error> for Failure {
    fn from(error: kaia_sync::Error) -> Self {
        Failure::Request(error)
    }
}

/// What the worker sends. It is a reply or a failure.
enum Msg {
    Reply(Reply),
    Failed(Failure),
}

struct Config {
    server: String,
    user: String,
    /// The MD5 user key of the password. The password itself is not stored.
    userkey: String,
    /// The name this device sends, such as `Kobo Libra Colour`.
    device_name: String,
    /// The id of this installation (`sync.device_id`).
    device_id: String,
    invalid_certs: bool,
}
impl Config {
    fn read(settings: &Settings, device: &str) -> Self {
        let text = |key| settings.read(key).as_str().unwrap_or("").to_owned();
        Self {
            server: text("sync.server"),
            user: text("sync.username"),
            userkey: text("sync.userkey"),
            device_name: device.to_owned(),
            device_id: text("sync.device_id"),
            invalid_certs: settings.is_true("sync.accept_invalid_certs"),
        }
    }
    fn client(&self) -> Result<SyncClient, kaia_sync::Error> {
        SyncClient::new_with_options(
            &self.server,
            Credentials::from_stored(&self.user, &self.userkey)?,
            &self.device_name,
            &self.device_id,
            Timeouts::default(),
            SyncOptions {
                accept_invalid_certs: self.invalid_certs,
                ..SyncOptions::default()
            },
        )
    }
    /// Identifies the login without keeping the key in plain text. It is an MD5 of the
    /// address, the account and the user key.
    fn fingerprint(&self) -> String {
        kaia_sync::md5_hex(format!("{}\n{}\n{}", self.server, self.user, self.userkey).as_bytes())
    }
}

enum Job {
    Health(Config),
    Pull(Config, i64, String),
    Push(Config, Progress, PathBuf),
}

#[derive(Default)]
pub struct ProgressSync {
    sender: Option<mpsc::Sender<Job>>,
    receiver: Option<mpsc::Receiver<Msg>>,
    worker: Option<JoinHandle<()>>,
    enabled: Arc<AtomicBool>,
    /// Fingerprint of the login the server refused with 401 (see [`Config::fingerprint`]).
    /// Requests with that login wait until the user saves new credentials. The worker sets it.
    refused: Arc<Mutex<Option<String>>>,
    /// The name this device sends. `None` means [`DEFAULT_DEVICE_NAME`].
    device: Option<String>,
    pending: BTreeMap<String, (Duration, Progress, PathBuf)>,
    pulls: BTreeMap<String, Duration>,
    in_flight: usize,
}

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

pub fn position(session: &BookSession) -> String {
    match session.current().location {
        Location::Page(page) => (page + 1).to_string(),
        Location::Engine(p) => p,
    }
}

/// The name this device sends to the sync server. A Kobo sends its model name, as KOReader
/// sends `Device.model`. Other devices send [`DEFAULT_DEVICE_NAME`].
pub fn device_name_for(kind: &Kind, device: &DeviceInfo) -> String {
    match kind {
        Kind::Kobo(_) => device.model_name.clone(),
        Kind::Desktop => DEFAULT_DEVICE_NAME.to_owned(),
    }
}

/// The client for the settings, built from the stored user key. It fails when the
/// username or the key is blank. It sends no request.
pub fn client(settings: &Settings) -> Result<SyncClient, kaia_sync::Error> {
    Config::read(settings, DEFAULT_DEVICE_NAME).client()
}

/// Stores what a typed password leaves behind: its MD5 user key under `sync.userkey`.
/// The password is never stored. Blank text removes the key, so sync reports an
/// invalid password. A plain `sync.password` from an older version is removed too.
pub fn store_password(settings: &mut Settings, typed: &str) {
    match kaia_sync::userkey_for(typed) {
        Some(key) => settings.save("sync.userkey", Value::from(key)),
        None => settings.del("sync.userkey"),
    }
    settings.del("sync.password");
}

/// Replaces a plain `sync.password` from an older version with its user key. Returns
/// true when the settings held a password. The password key is gone after the first
/// call, so a second call returns false.
pub fn migrate_password(settings: &mut Settings) -> bool {
    if !settings.has("sync.password") {
        return false;
    }
    let typed = settings.read("sync.password").as_str().unwrap_or("").to_owned();
    store_password(settings, &typed);
    true
}

/// Runs [`migrate_password`] and writes the result to `path`. Returns true when the file
/// was written. A flush keeps the previous file as `<path>.old`. That copy still holds the
/// password, so it is removed when it does.
pub fn migrate_password_file(settings: &mut Settings, path: &Path) -> io::Result<bool> {
    if !migrate_password(settings) {
        return Ok(false);
    }
    settings.flush(path)?;
    let mut backup = path.as_os_str().to_owned();
    backup.push(".old");
    let backup = PathBuf::from(backup);
    let holds_password =
        fs::read(&backup).is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains("sync.password"));
    if holds_password {
        fs::remove_file(&backup)?;
    }
    Ok(true)
}

/// The text for a sync failure, in the language of the screens. The crate's messages are
/// English and are not shown. Call this on the UI thread, where the catalog is loaded.
pub fn error_text(error: &kaia_sync::Error) -> String {
    use kaia_sync::{Error, InvalidField};
    match error {
        Error::InvalidCredentials(InvalidField::Username) => tr("Enter a username in Network and sync."),
        Error::InvalidCredentials(InvalidField::Password) => tr("Enter a password in Network and sync."),
        Error::InvalidCredentials(InvalidField::Both) => {
            tr("Enter a username and a password in Network and sync.")
        }
        Error::InvalidServerUrl(_) => tr("The server address must start with http:// or https://."),
        Error::TlsUnavailable => tr("This build cannot connect to https servers. Use http://."),
        Error::Unauthorized { .. } => {
            tr("The server refused the login. Check the account in Network and sync.")
        }
        Error::Server { status, .. } => {
            tr_format(&tr("The server returned status %1."), &[&status.to_string()])
        }
        Error::Timeout => tr("The server did not answer in time."),
        Error::Network(_) => tr("Could not reach the server. Check the address and the Wi-Fi connection."),
        Error::Protocol(_) => tr("The server sent a reply that Kaia does not understand."),
        _ => tr("Unknown sync error."),
    }
}

/// The text for a queue that cannot be read or written.
fn queue_text() -> String {
    tr("The sync queue could not be read or saved. Try again.")
}

/// Locks a mutex and reads through a poisoned lock. The guarded value stays usable after a panic.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Whether `config` is the login the server refused.
fn is_refused(refused: &Mutex<Option<String>>, config: &Config) -> bool {
    lock(refused).as_deref() == Some(config.fingerprint().as_str())
}

/// Whether `job` sends the refused login. The health check sends no login, so it always runs.
fn uses_refused_login(refused: &Mutex<Option<String>>, job: &Job) -> bool {
    match job {
        Job::Health(_) => false,
        Job::Pull(config, ..) | Job::Push(config, ..) => is_refused(refused, config),
    }
}

/// Runs one job on the worker thread. The job reports its replies and returns its failure.
fn run(
    job: Job,
    enabled: &AtomicBool,
    refused: &Mutex<Option<String>>,
    replies: &mpsc::Sender<Msg>,
) -> Result<(), Failure> {
    match job {
        Job::Health(config) => {
            config.client()?.health()?;
            let _ = replies.send(Msg::Reply(Reply::ServerAnswered));
            Ok(())
        }
        Job::Pull(config, book, document) => {
            let client = config.client()?;
            let found = client
                .pull(&document)
                .map_err(|error| note_refusal(error, &config, refused))?;
            if let Some(progress) = found {
                let _ = replies.send(Msg::Reply(Reply::Remote(book, progress)));
            }
            Ok(())
        }
        Job::Push(config, progress, path) => {
            push_queue(&config, &progress.document, path, enabled, refused, replies)
        }
    }
}

/// Records the login when the server refuses it, so later requests with that login wait.
/// Returns the error unchanged.
fn note_refusal(
    error: kaia_sync::Error,
    config: &Config,
    refused: &Mutex<Option<String>>,
) -> kaia_sync::Error {
    if matches!(error, kaia_sync::Error::Unauthorized { .. }) {
        *lock(refused) = Some(config.fingerprint());
    }
    error
}

/// Sends the queued updates in order. An update that fails with a network or server
/// error stays queued, and so do the later ones.
///
/// An update the server refuses with 401 is dropped, as KOReader drops it (`main.lua`,
/// `updateProgress`). The later updates stay queued, and the login is refused until the
/// user saves new credentials.
fn push_queue(
    config: &Config,
    document: &str,
    path: PathBuf,
    enabled: &AtomicBool,
    refused: &Mutex<Option<String>>,
    replies: &mpsc::Sender<Msg>,
) -> Result<(), Failure> {
    let queue = SyncQueue::new(path);
    let client = config.client()?;
    let mut accepted = 0usize;
    let mut refusal = false;
    let mut failure = None;
    queue
        .drain(|item| {
            // An error keeps the entry and stops the drain. Its value is not used.
            if refusal || failure.is_some() || !enabled.load(Ordering::SeqCst) {
                return Err(keep_queued());
            }
            match client.push(item) {
                Ok(()) => {
                    accepted += 1;
                    Ok(())
                }
                // Ok removes the entry from the queue. The next call sees `refusal` and stops.
                Err(kaia_sync::Error::Unauthorized { .. }) => {
                    refusal = true;
                    Ok(())
                }
                Err(error) => {
                    failure = Some(error);
                    Err(keep_queued())
                }
            }
        })
        .map_err(|_| Failure::Queue)?;
    if accepted > 0 {
        let _ = replies.send(Msg::Reply(Reply::Pushed(document.to_owned())));
    }
    if refusal {
        *lock(refused) = Some(config.fingerprint());
        return Err(Failure::Request(kaia_sync::Error::Unauthorized {
            code: None,
            message: "Unauthorized".into(),
        }));
    }
    match failure {
        Some(error) => Err(Failure::Request(error)),
        None => Ok(()),
    }
}

/// The error a drain closure returns to keep the entry it is sending.
fn keep_queued() -> kaia_sync::Error {
    kaia_sync::Error::Protocol(String::new())
}

impl ProgressSync {
    /// A sync state that names the device the way the platform does. Call it once at startup.
    pub fn for_device(kind: &Kind, device: &DeviceInfo) -> Self {
        Self {
            device: Some(device_name_for(kind, device)),
            ..Self::default()
        }
    }

    /// The name this device sends with each progress update.
    pub fn device_name(&self) -> &str {
        self.device.as_deref().unwrap_or(DEFAULT_DEVICE_NAME)
    }

    /// Forgets a refused login, so requests run again. The password field calls this when the
    /// user types a password.
    pub fn forget_refusal(&self) {
        *lock(&self.refused) = None;
    }

    /// Whether requests must wait. Airplane mode is on, or the server refused the current
    /// login. Due pushes then go to the queue and wait there.
    fn held(&self, settings: &Settings) -> bool {
        if keys::airplane_mode(settings) {
            return true;
        }
        // Most ticks find no refusal, so the key is not hashed on each one.
        lock(&self.refused).is_some() && is_refused(&self.refused, &self.config(settings))
    }

    /// Whether an automatic pull, on open or resume, may send a request. It needs sync on,
    /// `sync.auto` on, and no hold (airplane mode or a refused login).
    pub fn automatic_pull_allowed(&self, settings: &Settings) -> bool {
        settings.is_true(keys::SYNC_ENABLED) && settings.is_true(keys::SYNC_AUTO) && !self.held(settings)
    }

    fn config(&self, settings: &Settings) -> Config {
        Config::read(settings, self.device_name())
    }

    fn submit(&mut self, job: Job) {
        if self.sender.is_none() {
            let (tx, rx) = mpsc::channel::<Job>();
            let (replies, received) = mpsc::channel::<Msg>();
            let enabled = self.enabled.clone();
            let refused = self.refused.clone();
            self.worker = Some(std::thread::spawn(move || {
                for job in rx {
                    if enabled.load(Ordering::SeqCst) && !uses_refused_login(&refused, &job) {
                        if let Err(failure) = run(job, &enabled, &refused, &replies) {
                            let _ = replies.send(Msg::Failed(failure));
                        }
                    }
                    let _ = replies.send(Msg::Reply(Reply::Done));
                }
            }));
            self.sender = Some(tx);
            self.receiver = Some(received);
        }
        if self.sender.as_ref().unwrap().send(job).is_ok() {
            self.in_flight += 1;
        }
    }

    /// Tests the server. Sync must be on, and airplane mode must be off.
    pub fn health(&mut self, settings: &Settings) -> Result<(), String> {
        if !settings.is_true(keys::SYNC_ENABLED) {
            return Err(tr("Turn on progress sync first."));
        }
        if keys::airplane_mode(settings) {
            return Err(tr("Airplane mode is on. Turn it off to use Wi-Fi."));
        }
        let config = self.config(settings);
        config.client().map_err(|e| error_text(&e))?;
        self.enabled.store(true, Ordering::SeqCst);
        self.submit(Job::Health(config));
        Ok(())
    }

    pub fn pull(&mut self, settings: &Settings, book: i64, session: &BookSession, now: Duration) {
        self.enabled
            .store(settings.is_true(keys::SYNC_ENABLED), Ordering::SeqCst);
        if !self.automatic_pull_allowed(settings) {
            return;
        }
        let document = session.identity().partial_md5.clone();
        if self
            .pulls
            .get(&document)
            .is_some_and(|last| kaia_sync::pull_debounced(*last, now, false))
        {
            return;
        }
        self.pulls.insert(document.clone(), now);
        let config = self.config(settings);
        self.submit(Job::Pull(config, book, document));
    }

    /// Queues this device's position for the server. Pushes happen on save, suspend
    /// and close, so they follow `sync.auto` as KOReader's do: with it off, a push could
    /// replace a newer position from another device.
    pub fn push(&mut self, settings: &Settings, session: &BookSession, path: PathBuf, now: Duration) {
        if !settings.is_true(keys::SYNC_ENABLED) || !settings.is_true(keys::SYNC_AUTO) {
            return;
        }
        let progress = Progress {
            document: session.identity().partial_md5.clone(),
            progress: position(session),
            percentage: session.current().percent,
            device: self.device_name().to_owned(),
            device_id: settings.read("sync.device_id").as_str().unwrap_or("").into(),
            timestamp: None,
            metadata: None,
        };
        self.pending.insert(
            progress.document.clone(),
            (now + Duration::from_secs(1), progress, path),
        );
    }

    pub fn periodic(&mut self, settings: &Settings, session: &BookSession, path: PathBuf, due: Duration) {
        self.push(
            settings,
            session,
            path,
            due.saturating_sub(Duration::from_secs(1)),
        );
    }

    /// Writes the due pushes to the queue, starts a drain when requests may run, and returns
    /// the replies the worker sent. A due push always reaches the queue first, so it survives
    /// a hold. A hold (airplane mode or a refused login) leaves the queue to wait for later.
    pub fn tick(&mut self, settings: &Settings, now: Duration, force: bool) -> Vec<Reply> {
        let enabled = settings.is_true(keys::SYNC_ENABLED);
        self.enabled.store(enabled, Ordering::SeqCst);
        if !enabled {
            self.pending.clear();
        }
        let hold = self.held(settings);
        let mut replies = Vec::new();
        let due: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, (at, _, _))| force || *at <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for key in due {
            let (_, progress, path) = self.pending.get(&key).unwrap();
            if SyncQueue::new(path).push(progress.clone(), now_unix()).is_err() {
                replies.push(Reply::Error(queue_text()));
                if let Some((due, _, _)) = self.pending.get_mut(&key) {
                    *due = now + Duration::from_secs(5);
                }
                continue;
            }
            let (_, progress, path) = self.pending.remove(&key).unwrap();
            if !hold {
                let config = self.config(settings);
                self.submit(Job::Push(config, progress, path));
            }
        }
        let received: Vec<Msg> = self.receiver.iter().flat_map(|rx| rx.try_iter()).collect();
        for msg in received {
            match msg {
                Msg::Reply(reply) => {
                    if matches!(reply, Reply::Done) {
                        self.in_flight = self.in_flight.saturating_sub(1);
                    }
                    replies.push(reply);
                }
                Msg::Failed(Failure::Request(error)) => replies.push(Reply::Error(error_text(&error))),
                Msg::Failed(Failure::Queue) => replies.push(Reply::Error(queue_text())),
            }
        }
        replies
    }

    pub(crate) fn defer_pending(&mut self, document: &str, now: Duration) {
        if let Some((due, _, _)) = self.pending.get_mut(document) {
            *due = now + Duration::from_secs(10);
        }
    }

    /// Latest position waiting for its idle push deadline.
    pub fn pending_progress(&self, document: &str) -> Option<&Progress> {
        self.pending.get(document).map(|(_, progress, _)| progress)
    }

    pub fn busy(&self) -> bool {
        self.in_flight > 0 || !self.pending.is_empty()
    }

    /// Writes every pending update to the durable queue. An update leaves `pending`
    /// only after its write succeeds. A failed write returns the error and leaves that
    /// update and the later ones pending, so a later call retries them. With sync off,
    /// the pending updates are dropped without a write.
    pub fn persist_pending(&mut self, settings: &Settings) -> Result<(), String> {
        if !settings.is_true(keys::SYNC_ENABLED) {
            self.pending.clear();
            return Ok(());
        }
        let keys: Vec<_> = self.pending.keys().cloned().collect();
        for key in keys {
            let (_, progress, path) = self.pending.get(&key).unwrap();
            SyncQueue::new(path)
                .push(progress.clone(), now_unix())
                .map_err(|e| e.to_string())?;
            self.pending.remove(&key);
        }
        Ok(())
    }

    /// Stops the worker for shutdown and drops the pending updates that could not be
    /// written. Call it after the last [`ProgressSync::persist_pending`]. The worker is
    /// not joined, so shutdown does not wait on the network. An in-flight request may
    /// finish in the background, and queued requests stay on disk for the next run.
    pub fn close(&mut self) {
        self.enabled.store(false, Ordering::SeqCst);
        self.sender.take();
        self.worker.take();
        self.pending.clear();
    }
}

#[cfg(test)]
mod review2_tests {
    use super::*;
    #[test]
    fn shutdown_does_not_wait_for_network_worker() {
        let mut sync = ProgressSync {
            worker: Some(std::thread::spawn(|| {
                std::thread::sleep(Duration::from_millis(400))
            })),
            ..ProgressSync::default()
        };
        let now = std::time::Instant::now();
        sync.close();
        assert!(now.elapsed() < Duration::from_millis(100));
    }
    #[test]
    fn queue_keeps_push_made_during_drain() {
        let path = std::env::temp_dir().join(format!("r2-queue-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let queue = SyncQueue::new(&path);
        let progress = |name: &str| Progress {
            document: name.into(),
            progress: "1".into(),
            percentage: 0.1,
            device: "test".into(),
            device_id: "id".into(),
            timestamp: None,
            metadata: None,
        };
        queue.push(progress("a"), 1000).unwrap();
        queue
            .drain(|_| {
                queue.push(progress("b"), 1001).unwrap();
                Ok(())
            })
            .unwrap();
        assert_eq!(queue.load().unwrap().len(), 1);
        assert_eq!(queue.load().unwrap()[0].progress.document, "b");
    }
}

#[cfg(test)]
mod queue_recovery_tests {
    use super::*;
    #[test]
    fn unreadable_queue_keeps_bytes_and_accepts_retry() {
        let dir = std::env::temp_dir().join(format!("r2-queue-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("queue.json");
        let bad = b"[{\"document\":\"recoverable\",";
        std::fs::write(&path, bad).unwrap();
        let queue = SyncQueue::new(&path);
        let p = Progress {
            document: "a".into(),
            progress: "12".into(),
            percentage: 0.1,
            device: "test".into(),
            device_id: "id".into(),
            timestamp: None,
            metadata: None,
        };
        assert!(queue
            .push(p.clone(), 1000)
            .unwrap_err()
            .to_string()
            .contains("Kept a copy"));
        assert!(std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .any(|entry| std::fs::read(entry.path()).is_ok_and(|b| b == bad)));
        queue.push(p, 1000).unwrap();
        assert_eq!(queue.load().unwrap().len(), 1);
    }
}

#[cfg(test)]
mod password_tests {
    use super::*;

    /// md5("password"), the digest KOReader stores for that password.
    const USER_KEY: &str = "5f4dcc3b5aa765d61d8327deb882cf99";

    fn settings_with(pairs: &[(&str, &str)]) -> Settings {
        let mut settings = Settings::new();
        for (key, value) in pairs {
            settings.save(*key, Value::from(*value));
        }
        settings
    }

    #[test]
    fn typing_stores_the_user_key_and_never_the_password() {
        let mut settings = Settings::new();
        store_password(&mut settings, "password");
        assert_eq!(settings.read("sync.userkey").as_str(), Some(USER_KEY));
        assert!(!settings.has("sync.password"));
        // Blank text removes the key, so the next request reports an invalid password.
        store_password(&mut settings, "  ");
        assert!(!settings.has("sync.userkey"));
    }

    #[test]
    fn a_plain_password_is_replaced_once() {
        let mut settings = settings_with(&[("sync.username", "reader"), ("sync.password", "password")]);
        assert!(migrate_password(&mut settings));
        assert_eq!(settings.read("sync.userkey").as_str(), Some(USER_KEY));
        assert!(!settings.has("sync.password"));
        assert!(!migrate_password(&mut settings));
    }

    #[test]
    fn the_client_needs_the_key_and_no_password() {
        let stored = settings_with(&[
            ("sync.server", "http://127.0.0.1:9"),
            ("sync.username", "reader"),
            ("sync.userkey", USER_KEY),
        ]);
        assert!(client(&stored).is_ok());
        let missing = settings_with(&[("sync.server", "http://127.0.0.1:9"), ("sync.username", "reader")]);
        assert!(matches!(
            client(&missing),
            Err(kaia_sync::Error::InvalidCredentials(
                kaia_sync::InvalidField::Password
            ))
        ));
    }

    #[test]
    fn the_settings_file_loses_the_password_and_its_old_copy() {
        let dir = std::env::temp_dir().join(format!("kaia-sync-password-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let old = dir.join("settings.json.old");
        let mut settings = settings_with(&[("sync.password", "password")]);
        settings.flush(&path).unwrap();
        fs::write(&old, br#"{"sync.password": "password"}"#).unwrap();

        assert!(migrate_password_file(&mut settings, &path).unwrap());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(USER_KEY), "{text}");
        assert!(!text.contains("sync.password"), "{text}");
        assert!(!text.contains("\"password\""), "{text}");
        assert!(!old.exists(), "the old copy held the password");
        assert!(!migrate_password_file(&mut settings, &path).unwrap());
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod hold_tests {
    use super::*;
    use crate::sync_queue::SyncQueue as Queue;
    use kaia_platform::Model;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicUsize;
    use std::time::Instant;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kaia-sync-hold-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn progress(document: &str) -> Progress {
        Progress {
            document: document.into(),
            progress: "12".into(),
            percentage: 0.5,
            device: "Kobo Libra Colour".into(),
            device_id: "id".into(),
            timestamp: None,
            metadata: None,
        }
    }

    fn sync_settings(url: &str, password: &str) -> Settings {
        let mut settings = Settings::new();
        settings.save(keys::SYNC_ENABLED, Value::Bool(true));
        settings.save(keys::SYNC_SERVER, Value::from(url));
        settings.save(keys::SYNC_USERNAME, Value::from("reader"));
        settings.save(
            keys::SYNC_USERKEY,
            Value::from(kaia_sync::userkey_for(password).unwrap()),
        );
        settings
    }

    /// The document ids in the queue file, in order.
    fn queued(path: &Path) -> Vec<String> {
        Queue::new(path)
            .load()
            .unwrap()
            .iter()
            .map(|item| item.progress.document.clone())
            .collect()
    }

    /// Ticks until no request is in flight, then returns every reply seen. Five seconds at most.
    fn settle(sync: &mut ProgressSync, settings: &Settings, force: bool) -> Vec<Reply> {
        let mut replies = sync.tick(settings, Duration::ZERO, force);
        let deadline = Instant::now() + Duration::from_secs(5);
        while sync.busy() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            replies.extend(sync.tick(settings, Duration::ZERO, false));
        }
        replies
    }

    fn error_texts(replies: &[Reply]) -> Vec<&str> {
        replies
            .iter()
            .filter_map(|reply| match reply {
                Reply::Error(text) => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }

    /// A server that refuses every request with 401 and counts them.
    fn refusing_server() -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = Arc::clone(&count);
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut length = 0usize;
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    let trimmed = line.trim_end();
                    if trimmed.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = trimmed.split_once(':') {
                        if name.trim().eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap_or(0);
                        }
                    }
                }
                let mut body = vec![0u8; length];
                let _ = reader.read_exact(&mut body);
                seen.fetch_add(1, Ordering::SeqCst);
                let reply = r#"{"code":2001,"message":"Unauthorized"}"#;
                let _ = write!(
                    stream,
                    "HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.len(),
                    reply
                );
            }
        });
        (url, count)
    }

    fn refused_text() -> String {
        error_text(&kaia_sync::Error::Unauthorized {
            code: None,
            message: String::new(),
        })
    }

    /// Pushes due now are sent in order. A 401 drops one entry, stops the drain and holds
    /// later requests until the user saves the password again.
    #[test]
    fn a_refused_login_drops_one_update_and_holds_the_rest_until_new_credentials() {
        let dir = scratch_dir("refused");
        let path = dir.join("sync-queue.json");
        let (url, hits) = refusing_server();
        let queue = Queue::new(&path);
        // Entries use the current time, or the next push expires them as old.
        queue.push(progress("a"), now_unix()).unwrap();
        queue.push(progress("b"), now_unix()).unwrap();

        let mut settings = sync_settings(&url, "password");
        let mut sync = ProgressSync::default();

        // The due push "c" reaches the queue, and the drain sends "a" once. The server
        // refuses it, so "a" is dropped. "b" and "c" stay queued, and the user sees one message.
        sync.pending
            .insert("c".into(), (Duration::ZERO, progress("c"), path.clone()));
        let replies = settle(&mut sync, &settings, true);
        assert_eq!(error_texts(&replies), vec![refused_text().as_str()]);
        assert_eq!(hits.load(Ordering::SeqCst), 1, "one request, refused");
        assert_eq!(queued(&path), ["b", "c"]);

        // A later push is queued and sends nothing while the login is refused.
        sync.pending
            .insert("d".into(), (Duration::ZERO, progress("d"), path.clone()));
        let replies = settle(&mut sync, &settings, true);
        assert!(replies.is_empty(), "no message and no request while refused");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert_eq!(queued(&path), ["b", "c", "d"]);
        assert!(!sync.automatic_pull_allowed(&settings));

        // Typing a password clears the refusal, so the next drain sends again.
        sync.forget_refusal();
        sync.pending
            .insert("e".into(), (Duration::ZERO, progress("e"), path.clone()));
        let replies = settle(&mut sync, &settings, true);
        assert_eq!(error_texts(&replies), vec![refused_text().as_str()]);
        assert_eq!(
            hits.load(Ordering::SeqCst),
            2,
            "the retry happens once new credentials are saved"
        );
        assert_eq!(queued(&path), ["c", "d", "e"]);

        // A changed account also clears the refusal for the next push.
        settings.save(keys::SYNC_USERNAME, Value::from("reader2"));
        assert!(!sync.held(&settings));
        sync.pending
            .insert("f".into(), (Duration::ZERO, progress("f"), path.clone()));
        let replies = settle(&mut sync, &settings, true);
        assert_eq!(error_texts(&replies), vec![refused_text().as_str()]);
        assert_eq!(hits.load(Ordering::SeqCst), 3);
        sync.close();
    }

    /// With airplane mode on, a due push is written to the queue and no job starts.
    #[test]
    fn airplane_mode_queues_due_pushes_without_a_request() {
        let dir = scratch_dir("airplane");
        let path = dir.join("sync-queue.json");
        let mut settings = sync_settings("http://127.0.0.1:9", "password");
        settings.save(keys::NETWORK_AIRPLANE_MODE, Value::Bool(true));
        let mut sync = ProgressSync::default();
        sync.pending
            .insert("a".into(), (Duration::ZERO, progress("a"), path.clone()));

        let replies = sync.tick(&settings, Duration::from_secs(1), true);
        assert!(replies.is_empty(), "no toast for a held push");
        assert!(sync.worker.is_none(), "no worker job starts");
        assert!(sync.pending.is_empty(), "the push left the pending list");
        assert_eq!(queued(&path), ["a"], "the push waits in the queue");
        assert!(!sync.busy(), "the app does not poll for a held push");
    }

    /// Automatic pulls need sync on, `sync.auto` on, and no airplane mode or refused login.
    #[test]
    fn automatic_pulls_need_auto_sync_and_no_hold() {
        let mut settings = sync_settings("http://127.0.0.1:9", "password");
        let sync = ProgressSync::default();
        assert!(
            !sync.automatic_pull_allowed(&settings),
            "auto sync is off by default"
        );

        settings.save(keys::SYNC_AUTO, Value::Bool(true));
        assert!(sync.automatic_pull_allowed(&settings));

        settings.save(keys::NETWORK_AIRPLANE_MODE, Value::Bool(true));
        assert!(
            !sync.automatic_pull_allowed(&settings),
            "airplane mode holds pulls"
        );
        settings.save(keys::NETWORK_AIRPLANE_MODE, Value::Bool(false));

        *lock(&sync.refused) = Some(Config::read(&settings, DEFAULT_DEVICE_NAME).fingerprint());
        assert!(
            !sync.automatic_pull_allowed(&settings),
            "a refused login holds pulls"
        );

        settings.save(keys::SYNC_ENABLED, Value::Bool(false));
        settings.save(keys::SYNC_ENABLED, Value::Bool(true));
        sync.forget_refusal();
        assert!(sync.automatic_pull_allowed(&settings));
    }

    /// Each failure kind reads through the catalog, and none shows the crate's English.
    #[test]
    fn failures_show_catalog_text_not_the_crate_english() {
        let po = concat!(
            "msgid \"The server refused the login. Check the account in Network and sync.\"\n",
            "msgstr \"Le serveur a refuse la connexion.\"\n",
        );
        kaia_i18n::set_current(kaia_i18n::Catalog::from_po(po).unwrap());
        assert_eq!(
            error_text(&kaia_sync::Error::Unauthorized {
                code: None,
                message: "Unauthorized".into(),
            }),
            "Le serveur a refuse la connexion."
        );
        let errors = [
            kaia_sync::Error::InvalidCredentials(kaia_sync::InvalidField::Username),
            kaia_sync::Error::InvalidCredentials(kaia_sync::InvalidField::Password),
            kaia_sync::Error::InvalidCredentials(kaia_sync::InvalidField::Both),
            kaia_sync::Error::InvalidServerUrl("ftp://x".into()),
            kaia_sync::Error::TlsUnavailable,
            kaia_sync::Error::Unauthorized {
                code: Some(2001),
                message: "bad".into(),
            },
            kaia_sync::Error::Server {
                status: 500,
                code: None,
                message: "Unknown server error".into(),
            },
            kaia_sync::Error::Timeout,
            kaia_sync::Error::Network("connection refused".into()),
            kaia_sync::Error::Protocol("progress body: expected value".into()),
        ];
        for error in &errors {
            let text = error_text(error);
            assert!(!text.is_empty());
            assert_ne!(text, error.to_string(), "{error:?} shows the crate English");
            assert!(!text.contains("network error"), "{text}");
        }
        assert_eq!(
            error_text(&kaia_sync::Error::Server {
                status: 500,
                code: None,
                message: String::new(),
            }),
            "The server returned status 500."
        );
    }

    /// A 401 on a pull records the refused login, as a 401 on a push does.
    #[test]
    fn a_refused_pull_holds_later_requests_with_that_login() {
        let settings = sync_settings("http://127.0.0.1:9", "password");
        let config = Config::read(&settings, DEFAULT_DEVICE_NAME);
        let refused = Mutex::new(None);
        let other = Config::read(&sync_settings("http://127.0.0.1:9", "other"), DEFAULT_DEVICE_NAME);

        let error = note_refusal(
            kaia_sync::Error::Unauthorized {
                code: None,
                message: "Unauthorized".into(),
            },
            &config,
            &refused,
        );
        assert!(
            matches!(error, kaia_sync::Error::Unauthorized { .. }),
            "the error is passed on"
        );
        assert!(is_refused(&refused, &config));
        assert!(!is_refused(&refused, &other), "another login is not held");

        let timeout = note_refusal(kaia_sync::Error::Timeout, &other, &refused);
        assert!(matches!(timeout, kaia_sync::Error::Timeout));
        assert!(
            is_refused(&refused, &config),
            "a timeout does not clear or set the refusal"
        );
    }

    /// A Kobo sends its model name. Other devices, and a sync state with no device, send "Kaia Reader".
    #[test]
    fn a_kobo_sends_its_model_name_and_others_send_the_default() {
        let kobo = ProgressSync::for_device(
            &Kind::Kobo(Model::LibraColour),
            &DeviceInfo::libra_colour("monza".into(), String::new(), None),
        );
        assert_eq!(kobo.device_name(), "Kobo Libra Colour");
        let desktop = ProgressSync::for_device(&Kind::Desktop, &DeviceInfo::desktop(1264, 1680, 300));
        assert_eq!(desktop.device_name(), "Kaia Reader");
        assert_eq!(ProgressSync::default().device_name(), "Kaia Reader");
    }
}
