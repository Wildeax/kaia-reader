//! Progress sync is opt-in. A single worker runs requests away from the UI thread.

use kaia_i18n::tr;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kaia_book::{BookSession, Location};
use kaia_events::{Settings, Value};
use kaia_sync::{Credentials, Progress, SyncClient, SyncOptions, Timeouts};

use crate::sync_queue::SyncQueue;

/// What a worker sends back. A worker thread has no translation catalog, so the
/// worker sends a variant for a Kaia message and the UI thread writes the text.
pub enum Reply {
    Remote(i64, Progress),
    Pushed(String),
    Error(String),
    ServerAnswered,
    Done,
}

struct Config {
    server: String,
    user: String,
    /// The MD5 user key of the password. The password itself is not stored.
    userkey: String,
    device: String,
    invalid_certs: bool,
}
impl Config {
    fn read(settings: &Settings) -> Self {
        let text = |key| settings.read(key).as_str().unwrap_or("").to_owned();
        Self {
            server: text("sync.server"),
            user: text("sync.username"),
            userkey: text("sync.userkey"),
            device: text("sync.device_id"),
            invalid_certs: settings.is_true("sync.accept_invalid_certs"),
        }
    }
    fn client(&self) -> Result<SyncClient, kaia_sync::Error> {
        SyncClient::new_with_options(
            &self.server,
            Credentials::from_stored(&self.user, &self.userkey)?,
            "Kaia Reader",
            &self.device,
            Timeouts::default(),
            SyncOptions {
                accept_invalid_certs: self.invalid_certs,
                ..SyncOptions::default()
            },
        )
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
    receiver: Option<mpsc::Receiver<Reply>>,
    worker: Option<JoinHandle<()>>,
    enabled: Arc<AtomicBool>,
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

/// The client for the settings, built from the stored user key. It fails when the
/// username or the key is blank. It sends no request.
pub fn client(settings: &Settings) -> Result<SyncClient, kaia_sync::Error> {
    Config::read(settings).client()
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

impl ProgressSync {
    fn submit(&mut self, job: Job) {
        if self.sender.is_none() {
            let (tx, rx) = mpsc::channel();
            let (replies, received) = mpsc::channel();
            let enabled = self.enabled.clone();
            self.worker = Some(std::thread::spawn(move || {
                for job in rx {
                    if enabled.load(Ordering::SeqCst) {
                        let result = match job {
                            Job::Health(config) => {
                                config.client().and_then(|client| client.health()).map(|()| {
                                    let _ = replies.send(Reply::ServerAnswered);
                                })
                            }
                            Job::Pull(config, book, document) => config.client().and_then(|client| {
                                if let Some(progress) = client.pull(&document)? {
                                    let _ = replies.send(Reply::Remote(book, progress));
                                }
                                Ok(())
                            }),
                            Job::Push(config, progress, path) => {
                                let document = progress.document.clone();
                                let queue = SyncQueue::new(path);
                                config.client().and_then(|client| {
                                    let mut failure = None;
                                    let sent = queue.drain(|progress| {
                                        if !enabled.load(Ordering::SeqCst) {
                                            return Err(kaia_sync::Error::Protocol("Sync turned off".into()));
                                        }
                                        let result = client.push(progress);
                                        if let Err(e) = &result {
                                            failure = Some(e.to_string());
                                        }
                                        result
                                    })?;
                                    if let Some(error) = failure {
                                        return Err(kaia_sync::Error::Protocol(error));
                                    }
                                    if sent > 0 {
                                        let _ = replies.send(Reply::Pushed(document));
                                    }
                                    Ok(())
                                })
                            }
                        };
                        if let Err(e) = result {
                            let _ = replies.send(Reply::Error(e.to_string()));
                        }
                    }
                    let _ = replies.send(Reply::Done);
                }
            }));
            self.sender = Some(tx);
            self.receiver = Some(received);
        }
        if self.sender.as_ref().unwrap().send(job).is_ok() {
            self.in_flight += 1;
        }
    }

    pub fn health(&mut self, settings: &Settings) -> Result<(), String> {
        if !settings.is_true("sync.enabled") {
            return Err(tr("Turn on progress sync first."));
        }
        let config = Config::read(settings);
        config.client().map_err(|e| e.to_string())?;
        self.enabled.store(true, Ordering::SeqCst);
        self.submit(Job::Health(config));
        Ok(())
    }

    pub fn pull(&mut self, settings: &Settings, book: i64, session: &BookSession, now: Duration) {
        self.enabled
            .store(settings.is_true("sync.enabled"), Ordering::SeqCst);
        if !settings.is_true("sync.enabled") {
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
        self.submit(Job::Pull(Config::read(settings), book, document));
    }

    pub fn push(&mut self, settings: &Settings, session: &BookSession, path: PathBuf, now: Duration) {
        if !settings.is_true("sync.enabled") {
            return;
        }
        let progress = Progress {
            document: session.identity().partial_md5.clone(),
            progress: position(session),
            percentage: session.current().percent,
            device: "Kaia Reader".into(),
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

    pub fn tick(&mut self, settings: &Settings, now: Duration, force: bool) -> Vec<Reply> {
        let enabled = settings.is_true("sync.enabled");
        self.enabled.store(enabled, Ordering::SeqCst);
        if !enabled {
            self.pending.clear();
        }
        let mut failures = Vec::new();
        let due: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, (at, _, _))| force || *at <= now)
            .map(|(key, _)| key.clone())
            .collect();
        for key in due {
            let (_, progress, path) = self.pending.get(&key).unwrap();
            let queue = SyncQueue::new(path);
            if let Err(error) = queue.push(progress.clone(), now_unix()) {
                failures.push(Reply::Error(error.to_string()));
                if let Some((due, _, _)) = self.pending.get_mut(&key) {
                    *due = now + Duration::from_secs(5);
                }
                continue;
            }
            let (_, progress, path) = self.pending.remove(&key).unwrap();
            self.submit(Job::Push(Config::read(settings), progress, path));
        }
        let replies: Vec<_> = self.receiver.iter().flat_map(|rx| rx.try_iter()).collect();
        for reply in &replies {
            if matches!(reply, Reply::Done) {
                self.in_flight = self.in_flight.saturating_sub(1);
            }
        }
        failures.extend(replies);
        failures
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
        if !settings.is_true("sync.enabled") {
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
