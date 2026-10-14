//! Facts the Settings sections show that come from outside `settings.json`:
//! reading statistics, the KOReader folder, dictionaries, the library folders
//! and the user's OPDS catalogs.

use std::fs;
use std::path::{Component, Path, PathBuf};

use kaia_events::{Settings, Value};
use kaia_i18n::{format as tr_format, tr, tr_n};
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
        tr_format(
            &tr("%1 h %2 min"),
            &[&(minutes / 60).to_string(), &(minutes % 60).to_string()],
        )
    } else {
        tr_format(&tr("%1 min"), &[&minutes.to_string()])
    }
}

/// A page count as `1 page` or `12 pages`.
pub fn page_count(pages: i64) -> String {
    tr_format(&tr_n("%1 page", "%1 pages", pages), &[&pages.to_string()])
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

    /// The file the collections import reads. KOReader keeps it in its settings
    /// folder (`frontend/readcollection.lua`). The top-level copy is a fallback.
    pub fn collections_path(&self) -> PathBuf {
        collections_file(&self.path)
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
            collections: collections_file(&path).is_file(),
            statistics: path.join("settings").join("statistics.sqlite3").is_file(),
            path,
        })
}

fn collections_file(koreader: &Path) -> PathBuf {
    let in_settings = koreader.join("settings").join("collection.lua");
    if in_settings.is_file() {
        in_settings
    } else {
        koreader.join("collection.lua")
    }
}

/// The number of StarDict dictionaries (`*.ifo`) in `<data>/dict`.
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

/// The last two parts of `path`, such as `.kaia/dict`, with `…/` in front when
/// the path has more parts. The full path starts with the host's own folders
/// (the home folder, or the temp folder that a test uses), so a row that
/// printed it would differ from one host to the next.
pub fn short_path(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    let kept = parts.len().min(2);
    let tail = parts[parts.len() - kept..].join("/");
    if parts.len() > kept {
        format!("…/{tail}")
    } else {
        tail
    }
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

    #[test]
    fn collections_come_from_the_koreader_settings_folder() {
        let dir = std::env::temp_dir().join(format!("kaia-facts-collections-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("settings")).unwrap();
        // An older layout kept the file at the top. It is the fallback.
        fs::write(dir.join("collection.lua"), "return {}").unwrap();
        assert_eq!(collections_file(&dir), dir.join("collection.lua"));
        // Stock KOReader writes it in settings/, which wins.
        fs::write(dir.join("settings").join("collection.lua"), "return {}").unwrap();
        assert_eq!(
            collections_file(&dir),
            dir.join("settings").join("collection.lua")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn short_paths_keep_the_last_two_parts() {
        assert_eq!(short_path(Path::new("/mnt/onboard/.kaia/dict")), "…/.kaia/dict");
        assert_eq!(
            short_path(Path::new("/tmp/kaia-app-test-7-x/data/dict")),
            "…/data/dict"
        );
        assert_eq!(short_path(Path::new("/data/dict")), "data/dict");
        assert_eq!(short_path(Path::new("dict")), "dict");
    }
}
