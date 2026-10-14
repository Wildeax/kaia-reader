//! Keep the library cache in step with saved book state.
//!
//! A mark (finished or unread) is written to two places: the book's state file,
//! or the pending status in settings for a book that has none yet, and the status
//! in the library index. A single book is written on the UI thread. A batch is
//! planned on the UI thread and written on a worker, so a large batch does not
//! hold up the screen. The worker keeps no translated text: it reports counts,
//! and the screen builds the sentence.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::services::Services;
use kaia_book::state::{STATE_FORMAT, STATE_VERSION};
use kaia_events::{Settings, Value};
use kaia_i18n::{format as tr_format, tr};
use kaia_library::{Library, Sort, SortKey, Status};

/// A mark the library applies to books.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Finished,
    /// Not started. KOReader's word for it is `new`.
    Unread,
}

impl Mark {
    /// The reading status in the library index.
    pub fn status(self) -> Status {
        match self {
            Mark::Finished => Status::Complete,
            Mark::Unread => Status::New,
        }
    }

    /// The `summary.status` value in the book's state file.
    fn value(self) -> &'static str {
        match self {
            Mark::Finished => "complete",
            Mark::Unread => "new",
        }
    }
}

/// Why a state file could not take a mark. The reader and the worker both use
/// this, so it carries no translated text.
#[derive(Debug)]
enum StateError {
    /// The file is from another format or version. It is kept unchanged.
    Unsupported,
    /// The file could not be read. A copy is kept at `kept`.
    Unreadable { kept: PathBuf, detail: String },
    /// The file could not be written or copied.
    Failed(String),
}

fn state_path(services: &Services, partial_md5: &str) -> PathBuf {
    services
        .paths
        .data_dir
        .join("kaia/books")
        .join(format!("{partial_md5}.json"))
}

/// The settings key that holds a status for a book with no state file yet.
fn pending_key(partial_md5: &str) -> String {
    format!("book.status.{partial_md5}")
}

/// Writes `mark` into the state file at `path`. Returns Ok(false) when the file
/// does not exist, so the caller can keep the status for the first open.
fn write_state(path: &Path, mark: Mark) -> Result<bool, StateError> {
    match Settings::read_exact(path) {
        Ok(Some(mut settings)) => {
            if settings.read("format").as_str() != Some(STATE_FORMAT)
                || settings.read("version").as_i64() != Some(STATE_VERSION)
            {
                return Err(StateError::Unsupported);
            }
            let mut summary = settings.read("summary").as_table().cloned().unwrap_or_default();
            summary.insert("status".into(), Value::from(mark.value()));
            settings.save("summary", Value::Table(summary));
            settings
                .flush_book(path)
                .map_err(|e| StateError::Failed(e.to_string()))?;
            Ok(true)
        }
        Ok(None) => Ok(false),
        Err(error) => {
            let kept = crate::durable::preserve(path).map_err(|e| StateError::Failed(e.to_string()))?;
            Err(StateError::Unreadable {
                kept,
                detail: error.to_string(),
            })
        }
    }
}

/// Marks one book finished or unread, in its state file (or the pending settings)
/// and in the library index.
fn mark_one(services: &mut Services, id: i64, mark: Mark) -> Result<(), String> {
    let row = services
        .library
        .book(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| tr("Book not found"))?;
    let path = state_path(services, &row.partial_md5);
    match write_state(&path, mark) {
        Ok(true) => {}
        Ok(false) => {
            // Let first open import the sidecar before applying the status.
            services
                .settings
                .save(pending_key(&row.partial_md5), Value::from(mark.value()));
            services
                .settings
                .flush(&services.settings_path())
                .map_err(|e| e.to_string())?;
        }
        Err(StateError::Unsupported) => {
            return Err(tr("Cannot read this book's saved state. It was kept unchanged."));
        }
        Err(StateError::Unreadable { kept, detail }) => {
            return Err(tr_format(
                &tr("Cannot read book state. Kept a copy at %1. %2"),
                &[kept.display().to_string().as_str(), detail.as_str()],
            ));
        }
        Err(StateError::Failed(detail)) => return Err(detail),
    }
    services
        .library
        .set_status(id, mark.status())
        .map_err(|e| e.to_string())
}

pub(crate) fn mark_finished(services: &mut Services, id: i64) -> Result<(), String> {
    mark_one(services, id, Mark::Finished)
}

/// One book of a batch. `state` is its state file when it exists.
pub(crate) struct Planned {
    id: i64,
    state: Option<PathBuf>,
}

/// What a batch did: the books written and the books that could not be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MarkReport {
    pub marked: usize,
    pub failed: usize,
}

/// Plans a batch on the UI thread. A book with a state file is written by
/// [`run_marks`]. A book with none keeps the status in settings, which the first
/// open applies, and the settings are flushed once for all of them. Ids with no
/// book are skipped.
pub(crate) fn plan_marks(services: &mut Services, ids: &[i64], mark: Mark) -> Result<Vec<Planned>, String> {
    let mut planned = Vec::with_capacity(ids.len());
    let mut pending = Vec::new();
    for &id in ids {
        let Some(row) = services.library.book(id).map_err(|e| e.to_string())? else {
            continue;
        };
        let path = state_path(services, &row.partial_md5);
        if path.exists() {
            planned.push(Planned {
                id,
                state: Some(path),
            });
        } else {
            let key = pending_key(&row.partial_md5);
            services.settings.save(key.clone(), Value::from(mark.value()));
            pending.push(key);
            planned.push(Planned { id, state: None });
        }
    }
    if !pending.is_empty() {
        if let Err(error) = services.settings.flush(&services.settings_path()) {
            for key in pending {
                services.settings.del(key);
            }
            return Err(error.to_string());
        }
    }
    Ok(planned)
}

/// Writes a planned batch on a worker thread. The worker writes each state file,
/// then sets the status of every book that did not fail, in one transaction on
/// its own library connection. The report arrives on the returned channel.
pub(crate) fn run_marks(
    library_path: PathBuf,
    planned: Vec<Planned>,
    mark: Mark,
) -> Receiver<Result<MarkReport, String>> {
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let result = (|| {
            let mut ids = Vec::with_capacity(planned.len());
            let mut failed = 0;
            for item in &planned {
                let written = match &item.state {
                    Some(path) => write_state(path, mark).map(|_| ()),
                    None => Ok(()),
                };
                match written {
                    Ok(()) => ids.push(item.id),
                    Err(_) => failed += 1,
                }
            }
            let mut library = Library::open(&library_path).map_err(|e| e.to_string())?;
            let marked = library
                .set_status_many(&ids, mark.status())
                .map_err(|e| e.to_string())?;
            Ok(MarkReport { marked, failed })
        })();
        let _ = send.send(result);
    });
    receive
}

pub(crate) fn reconcile(services: &mut Services) -> Result<(), String> {
    let books = services
        .library
        .books(Sort::new(SortKey::Title, false), &Default::default(), 0, 100_000)
        .map_err(|e| e.to_string())?;
    for row in books {
        let path = services
            .paths
            .data_dir
            .join("kaia/books")
            .join(format!("{}.json", row.partial_md5));
        let saved = Settings::open(&path);
        if saved.read("format").as_str() != Some(STATE_FORMAT)
            || saved.read("version").as_i64() != Some(STATE_VERSION)
        {
            continue;
        }
        if let Some(percent) = saved.read("percent_finished").as_f64().filter(|p| p.is_finite()) {
            if (percent - row.percent).abs() > f64::EPSILON {
                services
                    .library
                    .record_open(row.id, percent)
                    .map_err(|e| e.to_string())?;
            }
        }
        let pending = services.settings.read(pending_key(&row.partial_md5));
        let status = pending
            .as_str()
            .or_else(|| saved.read("summary").get("status").as_str());
        // A state of `new` is left alone. The reader does not rewrite the status
        // when it opens a book, so `new` may be stale, and the library keeps the
        // status it has.
        let status = match status {
            Some("complete") => Status::Complete,
            Some("abandoned") => Status::Abandoned,
            Some("reading") => Status::Reading,
            _ => continue,
        };
        if row.status != status {
            services
                .library
                .set_status(row.id, status)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
