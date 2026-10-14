//! Kobo import planning and staged writes. Source data never reaches logs.

mod source;

use kaia_book::{BookSession, OpenOptions};
use kaia_library::{Book, Filter, Library, Sort, SortKey, Status};
use kaia_stats::{BookInfo, NickelSession, Stats};
use source::{Row, Snapshot};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Counts shown before and after an import. Notes are also counted as highlights.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub books: usize,
    pub highlights: usize,
    pub notes: usize,
    pub collections: usize,
    pub words: usize,
    pub ratings: usize,
    pub sessions: usize,
}

impl Counts {
    pub fn total(&self) -> usize {
        self.books
            + self.highlights
            + self.notes
            + self.collections
            + self.words
            + self.ratings
            + self.sessions
    }
    fn minus(&self, other: &Self) -> Self {
        Self {
            books: self.books.saturating_sub(other.books),
            highlights: self.highlights.saturating_sub(other.highlights),
            notes: self.notes.saturating_sub(other.notes),
            collections: self.collections.saturating_sub(other.collections),
            words: self.words.saturating_sub(other.words),
            ratings: self.ratings.saturating_sub(other.ratings),
            sessions: self.sessions.saturating_sub(other.sessions),
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub imported: Counts,
    pub skipped: Counts,
}

/// A fixed snapshot. Confirmation imports these rows, even if Nickel changes later.
pub struct Plan {
    pub version: i64,
    pub tables: BTreeMap<String, usize>,
    pub counts: Counts,
    source: Snapshot,
    matched: BTreeMap<String, Book>,
    books_dir: PathBuf,
    candidates: usize,
}

impl Plan {
    pub fn prepare(books_dir: &Path, library: &Library) -> Result<Self> {
        let source = Snapshot::read(&books_dir.join(".kobo/KoboReader.sqlite"))?;
        let books = library.books(
            Sort::new(SortKey::Title, false),
            &Filter::default(),
            0,
            usize::MAX,
        )?;
        let mut matched = BTreeMap::new();
        let mut candidates = 0;
        for row in &source.rows["content"] {
            if !row.text("BookID").is_empty() || row.number("ContentType").is_some_and(|v| v != 6.0) {
                continue;
            }
            candidates += 1;
            if row.text("ContentID").is_empty() {
                continue;
            }
            if let Some(book) = match_book(row, books_dir, &books) {
                matched.insert(row.text("ContentID").into(), book.clone());
            }
        }
        let counts = Counts {
            books: matched.len(),
            highlights: source.counts["Bookmark"],
            notes: source.rows["Bookmark"]
                .iter()
                .filter(|r| !r.text("Annotation").trim().is_empty())
                .count(),
            collections: source.counts["Shelf"],
            words: source.counts["WordList"],
            ratings: source.counts["Ratings"],
            sessions: source.counts["KoboPlusReadingSessions"],
        };
        Ok(Self {
            version: source.version,
            tables: source.counts.clone(),
            counts,
            source,
            matched,
            books_dir: books_dir.to_owned(),
            candidates,
        })
    }

    /// Stages every owner before replacing any target. Repeats skip stored identities.
    pub fn import(self, data: &Path, library: &mut Library) -> Result<Report> {
        reject_kobo_target(data, &self.books_dir)?;
        for relative in [
            "kaia",
            "kaia/books",
            "kaia/backups",
            "kaia/kaia.sqlite3",
            "statistics.sqlite3",
            "library.sqlite3",
            "nickel-import.lock",
        ] {
            reject_kobo_target(&existing_parent(&data.join(relative)), &self.books_dir)?;
        }
        let mut book_locks = Vec::new();
        if data.join("kaia/books").is_dir() {
            for entry in fs::read_dir(data.join("kaia/books"))? {
                let path = entry?.path();
                if path.extension().is_some_and(|e| e == "lock") {
                    reject_kobo_target(&path, &self.books_dir)?;
                    book_locks.push(crate::durable::lock(&path, false)?);
                }
            }
        }
        let _scan_lock = crate::durable::lock(&data.join("nickel-import.lock"), false)?;
        let staging = tempfile::Builder::new()
            .prefix("nickel-stage-")
            .tempdir_in(data)?;
        let stage = staging.path();
        fs::create_dir_all(stage.join("kaia/books"))?;
        if data.join("kaia/books").is_dir() {
            for entry in fs::read_dir(data.join("kaia/books"))? {
                let entry = entry?;
                let name = entry.file_name();
                if entry.file_type()?.is_file()
                    && (name.to_string_lossy().ends_with(".json")
                        || name.to_string_lossy().ends_with(".json.old"))
                {
                    fs::copy(entry.path(), stage.join("kaia/books").join(name))?;
                }
            }
        }
        library.stage_nickel(&stage.join("library.sqlite3"))?;
        let mut index = Library::open(&stage.join("library.sqlite3"))?;
        for relative in ["kaia/kaia.sqlite3", "statistics.sqlite3"] {
            stage_database(&data.join(relative), &stage.join(relative))?;
        }
        let mut store = kaia_storage::Store::open(&stage.join("kaia"))?;
        let mut stats = Stats::open(stage.join("statistics.sqlite3"))?;
        let mut report = Report::default();
        let mut changed_books = Vec::new();
        let mut index_changed = false;
        let mut usable_books = HashSet::new();
        for (content_id, book) in &self.matched {
            if kaia_book::partial_md5(&book.path)? != book.partial_md5 {
                return Err("A book changed after planning".into());
            }
            let relative = PathBuf::from(format!("kaia/books/{}.json", book.partial_md5));
            for suffix in ["", ".old"] {
                let name = format!("{}{suffix}", relative.display());
                if data.join(&name).exists() && !stage.join(&name).exists() {
                    fs::copy(data.join(&name), stage.join(name))?;
                }
            }
            let mut opts = OpenOptions::new(data);
            opts.data_dir = stage.to_owned();
            opts.import_only = true;
            let mut session = match BookSession::open(&book.path, opts) {
                Ok(session) => session,
                Err(_) => continue,
            };
            if session.state_issue().is_some() || session.state_is_stale() {
                return Err("A book state needs review before import".into());
            }
            usable_books.insert(book.id);
            let before = report.imported.total();
            let mut time_changed = false;
            if let Some(row) = self.source.rows["content"]
                .iter()
                .find(|r| r.text("ContentID") == content_id)
            {
                if let (Some(percent), Some(status)) = (
                    row.number("___PercentRead").filter(|p| (0.0..=100.0).contains(p)),
                    status(row),
                ) {
                    if session.import_nickel_progress(content_id, percent / 100.0, status.as_str())? {
                        report.imported.books += 1;
                        index.import_nickel_progress(book.id, percent / 100.0, status)?;
                    }
                }
            }
            for row in self.source.rows["Bookmark"]
                .iter()
                .filter(|r| r.text("VolumeID") == content_id)
            {
                if row.deleted() || row.text("BookmarkID").is_empty() {
                    continue;
                }
                if session.import_nickel_highlight(
                    row.text("BookmarkID"),
                    row.text("Text"),
                    nonempty(row.text("Annotation")),
                    row.text("DateCreated"),
                )? {
                    report.imported.highlights += 1;
                    report.imported.notes += usize::from(nonempty(row.text("Annotation")).is_some());
                }
            }
            for row in self.source.rows["Ratings"]
                .iter()
                .filter(|r| r.text("ContentID") == content_id)
            {
                let rating = row
                    .number("Rating")
                    .filter(|r| (0.0..=5.0).contains(r) && r.fract() == 0.0)
                    .map(|r| r as u8);
                let review = nonempty(row.text("Review"));
                if (rating.is_some() || review.is_some())
                    && session.import_nickel_rating(content_id, rating, review)?
                {
                    report.imported.ratings += 1;
                }
            }
            let info = BookInfo {
                md5: &book.partial_md5,
                display_title: book.title.as_deref().unwrap_or(""),
                authors: book.authors.as_deref().unwrap_or(""),
                series: "",
                language: "",
                highlights: 0,
                notes: 0,
            };
            for row in self.source.rows["KoboPlusReadingSessions"]
                .iter()
                .filter(|r| r.text("ContentId") == content_id)
            {
                if let Some(span) = session_row(row) {
                    let result = stats.import_nickel(&info, std::slice::from_ref(&span))?;
                    report.imported.sessions += result.spans_added as usize;
                    time_changed |= session.import_nickel_time(
                        &span.key(),
                        (span.end - span.start - span.idle_seconds) as f64,
                    )?;
                }
            }
            let saved_percent = session.current().percent;
            let saved_status = session.status().and_then(Status::parse).unwrap_or(Status::New);
            if book.percent != saved_percent || book.status != saved_status {
                index.import_nickel_progress(book.id, saved_percent, saved_status)?;
                index_changed = true;
            }
            if report.imported.total() > before || time_changed {
                changed_books.push(relative);
            }
        }
        for row in &self.source.rows["Shelf"] {
            if row.deleted() || row.text("Name").trim().is_empty() {
                continue;
            }
            let members: Vec<_> = self.source.rows["ShelfContent"]
                .iter()
                .filter(|r| !r.deleted() && r.text("ShelfName") == row.text("Name"))
                .filter_map(|r| {
                    self.matched
                        .get(r.text("ContentId"))
                        .map(|b| b.id)
                        .filter(|id| usable_books.contains(id))
                })
                .collect();
            report.imported.collections +=
                usize::from(index.import_nickel_collection(row.text("Name"), &members)?);
        }
        for row in &self.source.rows["WordList"] {
            let title = self
                .matched
                .get(row.text("VolumeId"))
                .and_then(|b| b.title.as_deref());
            report.imported.words += usize::from(store.import_nickel_word(
                row.text("Text"),
                title,
                nonempty(row.text("DictSuffix")),
                timestamp(row.text("DateCreated")).unwrap_or(0),
            )?);
        }
        drop(store);
        drop(stats);
        drop(index);
        changed_books.sort();
        changed_books.dedup();
        // The scan lock excludes the app's other library connection until replacement ends.
        if report.imported.total() > 0 || !changed_books.is_empty() || index_changed {
            for relative in &changed_books {
                publish(&stage.join(relative), &data.join(relative))?;
            }
            if !changed_books.is_empty() {
                publish(
                    &stage.join("kaia/books/paths.json"),
                    &data.join("kaia/books/paths.json"),
                )?;
            }
            if report.imported.words > 0 {
                let backups = stage.join("kaia/backups");
                if backups.is_dir() {
                    for entry in fs::read_dir(&backups)? {
                        let entry = entry?;
                        let mut target = data.join("kaia/backups").join(entry.file_name());
                        if target.exists() {
                            target = crate::durable::unique(&target, "nickel");
                        }
                        publish(&entry.path(), &target)?;
                    }
                }
                publish(&stage.join("kaia/kaia.sqlite3"), &data.join("kaia/kaia.sqlite3"))?;
            }
            if report.imported.sessions > 0 {
                for entry in fs::read_dir(stage)? {
                    let entry = entry?;
                    if entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with("statistics.sqlite3.bkp.")
                    {
                        let target = data.join(entry.file_name());
                        if !target.exists() {
                            publish(&entry.path(), &target)?;
                        }
                    }
                }
                publish(
                    &stage.join("statistics.sqlite3"),
                    &data.join("statistics.sqlite3"),
                )?;
            }
            if report.imported.books + report.imported.collections > 0 || index_changed {
                library.publish_nickel(&stage.join("library.sqlite3"), &data.join("library.sqlite3"))?;
                fs::File::open(data)?.sync_all()?;
            }
        }
        let mut candidates = self.counts;
        candidates.books = self.candidates;
        report.skipped = candidates.minus(&report.imported);
        eprintln!(
            "Kobo imported: {}; skipped: {}",
            report.imported.total(),
            report.skipped.total()
        );
        Ok(report)
    }
}

fn nonempty(s: &str) -> Option<&str> {
    (!s.trim().is_empty()).then_some(s)
}
fn status(row: &Row) -> Option<Status> {
    match row.number("ReadStatus")? {
        0.0 => Some(Status::New),
        1.0 => Some(Status::Reading),
        2.0 => Some(Status::Complete),
        _ => None,
    }
}
fn match_book<'a>(row: &Row, root: &Path, books: &'a [Book]) -> Option<&'a Book> {
    for value in [row.text("adobe_location"), row.text("ContentID")] {
        if let Some(path) = resolve_path(value, root) {
            if let Some(book) = books
                .iter()
                .find(|b| fs::canonicalize(&b.path).ok().as_ref() == Some(&path))
            {
                return Some(book);
            }
        }
    }
    let title = row.text("Title").trim();
    let author = row.text("Attribution").trim();
    if title.is_empty() || author.is_empty() {
        return None;
    }
    let mut candidates = books.iter().filter(|b| {
        resolve_path(&b.path.to_string_lossy(), root).is_some()
            && b.title
                .as_deref()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case(title))
            && b.authors
                .as_deref()
                .is_some_and(|v| v.trim().eq_ignore_ascii_case(author))
    });
    let found = candidates.next()?;
    candidates.next().is_none().then_some(found)
}
fn resolve_path(value: &str, root: &Path) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    let path = if value.starts_with("file:") {
        url::Url::parse(value).ok()?.to_file_path().ok()?
    } else {
        PathBuf::from(value)
    };
    let path = if let Ok(relative) = path.strip_prefix("/mnt/onboard") {
        root.join(relative)
    } else if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    let path = fs::canonicalize(path).ok()?;
    let root = fs::canonicalize(root).ok()?;
    if !path.starts_with(&root) || path.components().any(|c| c.as_os_str() == ".kobo") || !path.is_file() {
        return None;
    }
    Some(path)
}
fn timestamp(value: &str) -> Option<i64> {
    value
        .parse()
        .ok()
        .or_else(|| {
            chrono::DateTime::parse_from_rfc3339(value)
                .ok()
                .map(|v| v.timestamp())
        })
        .or_else(|| {
            ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"]
                .iter()
                .find_map(|format| chrono::NaiveDateTime::parse_from_str(value, format).ok())
                .map(|v| v.and_utc().timestamp())
        })
        .filter(|seconds| (0..=253_402_300_799).contains(seconds))
}
fn session_row(row: &Row) -> Option<NickelSession> {
    let span = NickelSession {
        start: timestamp(row.text("StartTime")).or_else(|| timestamp(row.text("StartDate")))?,
        end: timestamp(row.text("EndTime")).or_else(|| timestamp(row.text("EndDate")))?,
        start_percent: row.number("StartPercentRead")? / 100.0,
        end_percent: row.number("EndPercentRead")? / 100.0,
        idle_seconds: row.number("IdleTime").unwrap_or(0.0) as i64,
    };
    span.valid().then_some(span)
}
fn reject_kobo_target(data: &Path, books: &Path) -> Result<()> {
    let data = fs::canonicalize(data)?;
    if data.components().any(|c| c.as_os_str() == ".kobo")
        || data.starts_with(fs::canonicalize(books.join(".kobo"))?)
    {
        return Err("The import target is inside Kobo data".into());
    }
    Ok(())
}
fn stage_database(source: &Path, target: &Path) -> Result<()> {
    if source.exists() {
        let source =
            rusqlite::Connection::open_with_flags(source, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let mut target = rusqlite::Connection::open(target)?;
        rusqlite::backup::Backup::new(&source, &mut target)?.run_to_completion(
            64,
            std::time::Duration::ZERO,
            None,
        )?;
    }
    Ok(())
}
fn publish(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target.parent().ok_or("Missing target folder")?)?;
    fs::File::open(source)?.sync_all()?;
    if target.extension().is_some_and(|e| e == "sqlite3") && target.exists() {
        let conn = rusqlite::Connection::open(target)?;
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        drop(conn);
    }
    if target.extension().is_some_and(|e| e == "json") && target.exists() {
        crate::durable::atomic_write(&target.with_extension("json.old"), &fs::read(target)?)?;
    }
    fs::rename(source, target)?;
    fs::File::open(target.parent().unwrap())?.sync_all()?;
    Ok(())
}

fn existing_parent(path: &Path) -> PathBuf {
    let mut current = path;
    while !current.exists() {
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent;
    }
    current.to_owned()
}
