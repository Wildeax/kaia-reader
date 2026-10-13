//! The library scan thread. It opens its own `Library` connection, scans the
//! roots, and reports progress and the result through a channel. The app loop
//! drains the channel and turns each message into a named event.
//!
//! `Library::scan` runs document engines, so it stays off the loop thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread;

use kaia_library::{Library, ScanReport};

/// A message from the scan thread.
#[derive(Debug)]
pub enum ScanMessage {
    /// Book `done` of `total` is about to be checked.
    Progress { done: usize, total: usize },
    /// The scan ended. An error is the text of the failure.
    Finished(Result<ScanReport, String>),
}

/// A running scan. Dropping it asks the thread to stop after its current book.
pub struct LibraryScan {
    rx: Receiver<ScanMessage>,
    cancel: Arc<AtomicBool>,
}

impl LibraryScan {
    /// Starts a scan of `roots` into the database at `db_path`.
    pub fn spawn(db_path: PathBuf, roots: Vec<PathBuf>) -> LibraryScan {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        thread::Builder::new()
            .name("kaia-library-scan".into())
            .spawn(move || {
                let result = scan(&db_path, &roots, &tx, &stop);
                // The loop may have stopped listening. Nothing else needs the message.
                let _ = tx.send(ScanMessage::Finished(result));
            })
            .map(|_| ())
            .unwrap_or_else(|e| eprintln!("kaia: cannot start the library scan: {e}"));
        LibraryScan { rx, cancel }
    }

    /// The next message, or None when nothing is waiting.
    pub fn try_next(&self) -> Option<ScanMessage> {
        match self.rx.try_recv() {
            Ok(message) => Some(message),
            Err(TryRecvError::Empty) => None,
            // The thread ends after it sends `Finished`, so a disconnect without it
            // means the thread panicked. Report it as a failed scan.
            Err(TryRecvError::Disconnected) => Some(ScanMessage::Finished(Err("scan thread stopped".into()))),
        }
    }

    /// Asks the scan to stop after the current book.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Drop for LibraryScan {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn scan(
    db_path: &std::path::Path,
    roots: &[PathBuf],
    tx: &mpsc::Sender<ScanMessage>,
    stop: &AtomicBool,
) -> Result<ScanReport, String> {
    let mut library = Library::open(db_path).map_err(|e| e.to_string())?;
    library
        .scan(
            roots,
            |p| {
                let _ = tx.send(ScanMessage::Progress {
                    done: p.done,
                    total: p.total,
                });
            },
            stop,
        )
        .map_err(|e| e.to_string())
}
