//! Progress sync is opt-in. A single worker runs requests away from the UI thread.

use kaia_i18n::tr;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kaia_book::{BookSession, Location};
use kaia_events::Settings;
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
    password: String,
    device: String,
    invalid_certs: bool,
}
impl Config {
    fn read(settings: &Settings) -> Self {
        let text = |key| settings.read(key).as_str().unwrap_or("").to_owned();
        Self {
            server: text("sync.server"),
            user: text("sync.username"),
            password: text("sync.password"),
            device: text("sync.device_id"),
            invalid_certs: settings.is_true("sync.accept_invalid_certs"),
        }
    }
    fn client(&self) -> Result<SyncClient, kaia_sync::Error> {
        SyncClient::new_with_options(
            &self.server,
            Credentials::new(&self.user, &self.password)?,
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
    pub fn finish(&mut self, settings: &Settings, _now: Duration) -> Result<(), String> {
        self.enabled.store(false, Ordering::SeqCst);
        if settings.is_true("sync.enabled") {
            let keys: Vec<_> = self.pending.keys().cloned().collect();
            for key in keys {
                let (_, progress, path) = self.pending.get(&key).unwrap();
                SyncQueue::new(path)
                    .push(progress.clone(), now_unix())
                    .map_err(|e| e.to_string())?;
                self.pending.remove(&key);
            }
        } else {
            self.pending.clear();
        }
        self.sender.take();
        // An in-flight request may finish. Queued requests stay on disk for the next run.
        self.worker.take();
        Ok(())
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
        sync.finish(&Settings::new(), Duration::ZERO).unwrap();
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
