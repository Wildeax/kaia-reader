//! Offline progress queue and scheduling helpers from KOSyncQueue and main.lua.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Error, Progress};

const MAX_AGE: i64 = 28 * 24 * 3600;
const MAX_ENTRIES: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueuedProgress {
    pub progress: Progress,
    pub queued_at: i64,
}

/// JSON queue. Call `push` after a failed send and `drain` when online.
pub struct SyncQueue {
    path: PathBuf,
}

impl SyncQueue {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> Result<Vec<QueuedProgress>, Error> {
        match fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| Error::Protocol(e.to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(Error::Protocol(e.to_string())),
        }
    }

    fn save(&self, items: &[QueuedProgress]) -> Result<(), Error> {
        let bytes = serde_json::to_vec(items).map_err(|e| Error::Protocol(e.to_string()))?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::Protocol(e.to_string()))?;
        }
        let temp = temp_path(&self.path);
        fs::write(&temp, bytes).map_err(|e| Error::Protocol(e.to_string()))?;
        fs::rename(temp, &self.path).map_err(|e| Error::Protocol(e.to_string()))
    }

    /// Keep one update per document per UTC day and expire entries after four weeks.
    pub fn push(&self, progress: Progress, now: i64) -> Result<(), Error> {
        let mut items = self.load()?;
        items.retain(|item| {
            now - item.queued_at < MAX_AGE
                && !(item.progress.document == progress.document
                    && item.queued_at.div_euclid(86400) == now.div_euclid(86400))
        });
        items.push(QueuedProgress {
            progress,
            queued_at: now,
        });
        if items.len() > MAX_ENTRIES {
            items.drain(..items.len() - MAX_ENTRIES);
        }
        self.save(&items)
    }

    /// Send in order. Stop on the first failure and retain that item and later ones.
    pub fn drain(&self, mut send: impl FnMut(&Progress) -> Result<(), Error>) -> Result<usize, Error> {
        let items = self.load()?;
        let mut sent = 0;
        for item in &items {
            if send(&item.progress).is_err() {
                break;
            }
            sent += 1;
        }
        self.save(&items[sent..])?;
        Ok(sent)
    }

    pub fn clear(&self) -> Result<(), Error> {
        self.save(&[])
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let mut out = path.as_os_str().to_owned();
    out.push(".tmp");
    PathBuf::from(out)
}

/// KOReader suppresses automatic pulls for 25 seconds after the last pull.
pub fn pull_debounced(last_pull: Duration, now: Duration, interactive: bool) -> bool {
    !interactive && now.saturating_sub(last_pull) <= Duration::from_secs(25)
}

/// A page change starts or restarts a periodic push after 10 idle seconds.
/// Call `reset` after the push completes.
#[derive(Debug, Default)]
pub struct PeriodicPush {
    last_page: Option<String>,
    changes: usize,
    due: Option<Duration>,
}

impl PeriodicPush {
    pub fn page_update(
        &mut self,
        page: &str,
        now: Duration,
        pages_before_update: Option<usize>,
    ) -> Option<Duration> {
        if self.last_page.as_deref() == Some(page) {
            return self.due;
        }
        self.last_page = Some(page.to_owned());
        self.changes += 1;
        if self.due.is_some() || pages_before_update.is_some_and(|n| n > 0 && self.changes >= n) {
            self.due = Some(now + Duration::from_secs(10));
        }
        self.due
    }

    pub fn due(&self, now: Duration) -> bool {
        self.due.is_some_and(|due| now >= due)
    }
    pub fn reset(&mut self) {
        self.changes = 0;
        self.due = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(document: &str) -> Progress {
        Progress {
            document: document.into(),
            progress: "12".into(),
            percentage: 0.5,
            device: "test".into(),
            device_id: "id".into(),
            timestamp: None,
            metadata: None,
        }
    }

    #[test]
    fn queue_deduplicates_by_document_and_day_then_retries_in_order() {
        let path = std::env::temp_dir().join(format!("kaia-sync-queue-{}.json", std::process::id()));
        let queue = SyncQueue::new(&path);
        queue.clear().unwrap();
        queue.push(progress("a"), 86400).unwrap();
        queue.push(progress("a"), 86401).unwrap();
        queue.push(progress("b"), 86402).unwrap();
        assert_eq!(queue.load().unwrap().len(), 2);
        assert_eq!(
            queue
                .drain(|p| if p.document == "b" {
                    Err(Error::Timeout)
                } else {
                    Ok(())
                })
                .unwrap(),
            1
        );
        assert_eq!(queue.load().unwrap()[0].progress.document, "b");
        assert_eq!(queue.drain(|_| Ok(())).unwrap(), 1);
        assert!(queue.load().unwrap().is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn debounce_and_page_counter_follow_plugin_delays() {
        assert!(pull_debounced(
            Duration::from_secs(10),
            Duration::from_secs(35),
            false
        ));
        assert!(!pull_debounced(
            Duration::from_secs(10),
            Duration::from_secs(36),
            false
        ));
        assert!(!pull_debounced(
            Duration::from_secs(10),
            Duration::from_secs(11),
            true
        ));
        let mut push = PeriodicPush::default();
        assert_eq!(push.page_update("1", Duration::from_secs(1), Some(2)), None);
        assert_eq!(
            push.page_update("2", Duration::from_secs(2), Some(2)),
            Some(Duration::from_secs(12))
        );
        assert_eq!(
            push.page_update("3", Duration::from_secs(5), Some(2)),
            Some(Duration::from_secs(15))
        );
        assert!(!push.due(Duration::from_secs(14)));
        assert!(push.due(Duration::from_secs(15)));
        push.reset();
        assert!(!push.due(Duration::from_secs(16)));
    }
}
