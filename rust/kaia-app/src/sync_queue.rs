//! App queue writes are durable. Network calls run without holding its file lock.
use kaia_sync::{Error, Progress, QueuedProgress};
use std::fs;
use std::path::PathBuf;

pub(crate) struct SyncQueue {
    path: PathBuf,
}
impl SyncQueue {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    fn lock(&self) -> Result<fs::File, Error> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir).map_err(error)?;
        }
        let mut name = self.path.as_os_str().to_owned();
        name.push(".lock");
        crate::durable::lock(&PathBuf::from(name), true).map_err(error)
    }
    pub fn load(&self) -> Result<Vec<QueuedProgress>, Error> {
        let _lock = self.lock()?;
        self.read()
    }
    fn read(&self) -> Result<Vec<QueuedProgress>, Error> {
        kaia_sync::SyncQueue::new(&self.path).load()
    }
    fn recover(&self) -> Result<Vec<QueuedProgress>, Error> {
        match self.read() {
            Ok(items) => Ok(items),
            Err(_) => {
                crate::durable::preserve(&self.path).map_err(error)?;
                self.save(&[])?;
                Err(Error::Protocol(
                    "The sync queue was unreadable. Kept a copy beside it. Retry sync.".into(),
                ))
            }
        }
    }
    fn save(&self, items: &[QueuedProgress]) -> Result<(), Error> {
        let staging = crate::durable::unique(&self.path, "queue");
        let result = (|| {
            let queue = kaia_sync::SyncQueue::new(&staging);
            let mut bytes = vec![b'['];
            for (index, item) in items.iter().enumerate() {
                // Serialize one entry at a time so rebuilding cannot expire other entries.
                queue.clear()?;
                queue.push(item.progress.clone(), item.queued_at)?;
                let entry = fs::read(&staging).map_err(error)?;
                if index > 0 {
                    bytes.push(b',');
                }
                bytes.extend_from_slice(&entry[1..entry.len() - 1]);
            }
            bytes.push(b']');
            crate::durable::atomic_write(&self.path, &bytes).map_err(error)
        })();
        let _ = fs::remove_file(staging);
        result
    }
    pub fn push(&self, progress: Progress, now: i64) -> Result<(), Error> {
        let _lock = self.lock()?;
        let mut items = self.recover()?;
        items.retain(|item| {
            now.saturating_sub(item.queued_at) < 28 * 86400
                && !(item.progress.document == progress.document
                    && item.queued_at.div_euclid(86400) == now.div_euclid(86400))
        });
        items.push(QueuedProgress {
            progress,
            queued_at: now,
        });
        if items.len() > 200 {
            items.drain(..items.len() - 200);
        }
        self.save(&items)
    }
    pub fn drain(&self, mut send: impl FnMut(&Progress) -> Result<(), Error>) -> Result<usize, Error> {
        let snapshot = self.load()?;
        let mut sent = 0;
        for item in snapshot {
            if send(&item.progress).is_err() {
                break;
            }
            let _lock = self.lock()?;
            let mut current = self.recover()?;
            // A newer push for this document must survive the older request's reply.
            current.retain(|entry| entry != &item);
            self.save(&current)?;
            sent += 1;
        }
        Ok(sent)
    }
}
fn error(e: std::io::Error) -> Error {
    Error::Protocol(e.to_string())
}
