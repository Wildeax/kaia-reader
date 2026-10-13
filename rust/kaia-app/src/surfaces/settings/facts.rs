//! Facts the Settings sections show that come from outside `settings.json`:
//! reading statistics, the KOReader folder, dictionaries, the library folders
//! and the user's OPDS catalogs.

use std::fs;
use std::path::{Path, PathBuf};

use kaia_events::{Settings, Value};
use kaia_platform::Paths;
use kaia_stats::{BookSummary, DayTotal, Stats, Totals, Weekday};

use super::keys;

/// Reading figures for the Statistics section.
#[derive(Debug, Clone, PartialEq)]
pub struct StatsView {
    pub today: Totals,
    pub week: Totals,
    /// The last seven days, oldest first.
    pub days: Vec<DayTotal>,
    /// Books by time read, longest first.
    pub books: Vec<BookSummary>,
}

/// The statistics database path: `<data>/statistics.sqlite3`.
pub fn stats_path(data_dir: &Path) -> PathBuf {
    data_dir.join("statistics.sqlite3")
}

/// Reads the statistics at `now` (Unix seconds). None when no database exists yet.
/// The database is never created here: `Stats::open` would create it.
pub fn stats_view(data_dir: &Path, now: i64) -> Option<StatsView> {
    let path = stats_path(data_dir);
    if !path.is_file() {
        return None;
    }
    let mut stats = Stats::open(&path).ok()?;
    Some(StatsView {
        today: stats.today(now).ok()?,
        week: stats.this_week(now, Weekday::Monday).ok()?,
        days: stats.daily(now, 7).ok()?,
        books: {
            let mut books = stats.books().ok()?;
            books.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.title.cmp(&b.title)));
            books
        },
    })
}

/// A duration as `1 h 5 min` or `40 min`.
pub fn duration(seconds: i64) -> String {
    let minutes = seconds.max(0) / 60;
    if minutes >= 60 {
        format!("{} h {} min", minutes / 60, minutes % 60)
    } else {
        format!("{minutes} min")
    }
}

/// What the KOReader folder holds that Kaia can import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KoreaderFolder {
    pub path: PathBuf,
    pub history: bool,
    pub collections: bool,
    pub statistics: bool,
}

impl KoreaderFolder {
    /// The file the history import reads.
    pub fn history_path(&self) -> PathBuf {
        self.path.join("history.lua")
    }

    /// The file the collections import reads.
    pub fn collections_path(&self) -> PathBuf {
        self.path.join("collection.lua")
    }

    /// KOReader's statistics database.
    pub fn statistics_path(&self) -> PathBuf {
        self.path.join("settings").join("statistics.sqlite3")
    }

    pub fn is_empty(&self) -> bool {
        !(self.history || self.collections || self.statistics)
    }
}

/// The first KOReader folder that exists. The Kobo folder sits in the books
/// folder (`.adds/koreader`). A desktop folder sits in the user's config folder.
/// `home` is the user's home folder, when the platform has one.
pub fn find_koreader(paths: &Paths, home: Option<&Path>) -> Option<KoreaderFolder> {
    let mut candidates = vec![paths.books_dir.join(".adds").join("koreader")];
    if let Some(home) = home {
        candidates.push(home.join(".config").join("koreader"));
        candidates.push(home.join("Library").join("Application Support").join("koreader"));
    }
    candidates
        .into_iter()
        .find(|dir| dir.is_dir())
        .map(|path| KoreaderFolder {
            history: path.join("history.lua").is_file(),
            collections: path.join("collection.lua").is_file(),
            statistics: path.join("settings").join("statistics.sqlite3").is_file(),
            path,
        })
}

/// The number of StarDict dictionaries (`*.ifo`) in `<data>/dictionaries`.
pub fn dictionary_count(data_dir: &Path) -> usize {
    fs::read_dir(data_dir.join("dict"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "ifo"))
                .count()
        })
        .unwrap_or(0)
}

/// The library folders: `library.folders`, or the books folder when none is set.
pub fn library_folders(settings: &Settings, books_dir: &Path) -> Vec<String> {
    match settings.read(keys::LIBRARY_FOLDERS) {
        Value::Table(_) => {
            let list = settings.read(keys::LIBRARY_FOLDERS);
            (1..=list.array_len() as i64)
                .filter_map(|i| list.get(i).as_str().map(str::to_string))
                .collect()
        }
        _ => vec![books_dir.display().to_string()],
    }
}

/// The user's OPDS catalogs as `(title, url)`, from `opds.catalogs`.
pub fn user_catalogs(settings: &Settings) -> Vec<(String, String)> {
    let list = settings.read(keys::OPDS_CATALOGS);
    (1..=list.array_len() as i64)
        .filter_map(|i| {
            let entry = list.get(i);
            let url = entry.get("url").as_str()?.to_string();
            let title = entry
                .get("title")
                .as_str()
                .map_or_else(|| url.clone(), str::to_string);
            Some((title, url))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_read_in_hours_and_minutes() {
        assert_eq!(duration(0), "0 min");
        assert_eq!(duration(40 * 60 + 59), "40 min");
        assert_eq!(duration(3900), "1 h 5 min");
    }
}
