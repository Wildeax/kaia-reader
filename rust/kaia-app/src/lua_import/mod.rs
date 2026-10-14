//! Previewed import of the Lua app's data. Only private copies open writable sources.
mod settings;

use kaia_book::sidecar::{sidecar_candidates, PlannedImport};
use kaia_book::SidecarLocation;
use kaia_events::{Settings, Value};
use kaia_library::{Filter, Library, Sort, SortKey};
use kaia_platform::Paths;
use kaia_stats::Stats;
use kaia_storage::Store;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub const KINDS: [&str; 14] = [
    "Settings",
    "History entries",
    "Collection entries",
    "Book states",
    "Highlights",
    "Bookmarks",
    "Notes",
    "Stickers",
    "Assets",
    "Statistics books",
    "Page records",
    "Plugin data",
    "Words",
    "Other annotations",
];
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Counts(pub BTreeMap<&'static str, usize>);
impl Counts {
    pub fn get(&self, kind: &str) -> usize {
        self.0.get(kind).copied().unwrap_or(0)
    }
    pub fn total(&self) -> usize {
        self.0.values().sum()
    }
    fn add(&mut self, kind: &'static str, n: usize) {
        *self.0.entry(kind).or_default() += n;
    }
}
#[derive(Debug, Default)]
pub struct Report {
    pub imported: Counts,
    pub skipped: Counts,
}
impl Report {
    fn count(&mut self, kind: &'static str, added: usize, skipped: usize) {
        self.imported.add(kind, added);
        self.skipped.add(kind, skipped);
    }
}

/// Follows DataStorage overrides, installed Kobo paths and desktop profile paths.
pub fn find(paths: &Paths) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(home) = std::env::var_os("KO_HOME") {
        candidates.push(PathBuf::from(home));
    }
    candidates.push(paths.books_dir.join(".adds/kaia"));
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        candidates.push(PathBuf::from(config).join("kaia"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(&home).join(".config/kaia"));
        candidates.push(PathBuf::from(home).join("Library/Application Support/kaia"));
    }
    candidates.push(paths.install_dir.clone());
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }
    candidates.into_iter().find(|p| {
        exists(p)
            && (!same(p, &paths.data_dir)
                || p.join("settings.reader.lua").is_file()
                || p.join("history.lua").is_file()
                || p.join("docsettings").is_dir())
    })
}
fn exists(p: &Path) -> bool {
    [
        "settings.reader.lua",
        "history.lua",
        "settings/collection.lua",
        "settings/statistics.sqlite3",
        "kaia/kaia.sqlite3",
    ]
    .iter()
    .any(|name| p.join(name).is_file())
        || p.join("docsettings").is_dir()
        || p.join("hashdocsettings").is_dir()
}

const PLUGIN_FILES: &[&str] = &[
    "calibre-extensions.lua",
    "settings/battery_stats.lua",
    "settings/cloudstorage.lua",
    "settings/directory_defaults.lua",
    "settings/gestures.lua",
    "settings/multiswipes.lua",
    "settings/kosync.lua",
    "settings/kosync_queue.lua",
    "settings/move_to_archive_settings.lua",
    "settings/news_settings.lua",
    "settings/perception_expander.lua",
    "settings/text_editor.lua",
    "settings/bookshortcuts.lua",
    "settings/profiles.lua",
    "settings/opds.lua",
    "settings/wallabag.lua",
    "settings/vocabulary_builder.sqlite3",
];

pub struct Plan {
    pub counts: Counts,
    pub shared_store: bool,
    pub viewport: kaia_gfx::Size,
    source: PathBuf,
    snapshot: tempfile::TempDir,
    settings: Vec<(&'static str, Value)>,
    plugin_settings: Value,
    books: Vec<(PathBuf, PlannedImport)>,
}
impl Plan {
    pub fn prepare(source: &Path, data: &Path, library: &Library) -> Result<Self> {
        let source = fs::canonicalize(source)?;
        let snapshot = tempfile::tempdir()?;
        for name in [
            "settings.reader.lua",
            "history.lua",
            "settings/collection.lua",
            "settings/statistics.sqlite3",
            "docsettings",
            "hashdocsettings",
            "history",
            "kaia/kaia.sqlite3",
            "kaia/assets",
        ]
        .into_iter()
        .chain(PLUGIN_FILES.iter().copied())
        {
            copy_source(&source.join(name), &snapshot.path().join(name))?;
        }
        // Checkpoints only private database copies, including any copied WAL pages.
        for name in [
            "settings/statistics.sqlite3",
            "kaia/kaia.sqlite3",
            "settings/vocabulary_builder.sqlite3",
        ] {
            let p = snapshot.path().join(name);
            if p.is_file() {
                let db = rusqlite::Connection::open(&p)?;
                db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
            }
        }
        for name in PLUGIN_FILES.iter().filter(|name| name.ends_with(".lua")) {
            read_lua(&snapshot.path().join(name))?;
        }
        let root = read_lua(&snapshot.path().join("settings.reader.lua"))?;
        let settings = settings::mapped(&root, &source)?;
        let plugin_settings = settings::plugins(&root);
        let mut paths: BTreeSet<PathBuf> = library
            .books(
                Sort::new(SortKey::Title, false),
                &Filter::default(),
                0,
                usize::MAX,
            )?
            .into_iter()
            .map(|b| b.path)
            .collect();
        for name in ["history.lua", "settings/collection.lua"] {
            collect_paths(&read_lua(&snapshot.path().join(name))?, &mut paths);
        }
        let docroot = snapshot.path().join("docsettings");
        for p in files(&docroot)? {
            let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(ext) = name
                .strip_prefix("metadata.")
                .and_then(|s| s.strip_suffix(".lua").or_else(|| s.strip_suffix(".lua.old")))
            else {
                continue;
            };
            let rel = p.parent().unwrap().strip_prefix(&docroot)?;
            paths.insert(Path::new("/").join(rel).with_extension(ext));
        }
        let locations = vec![
            SidecarLocation::DocSettingsDir(docroot),
            SidecarLocation::HashDocSettingsDir(snapshot.path().join("hashdocsettings")),
            SidecarLocation::HistoryDir(snapshot.path().join("history")),
        ];
        let mut books = Vec::new();
        for path in paths {
            if !path.is_file() {
                continue;
            }
            let hash = kaia_book::partial_md5(&path)?;
            let candidates: Vec<_> = sidecar_candidates(&path, &hash, &locations)
                .into_iter()
                .filter(|p| p.starts_with(snapshot.path()))
                .collect();
            if let Some(mut import) = PlannedImport::prepare(&path, &candidates)? {
                import.source_path(source.join(import.report.source.strip_prefix(snapshot.path())?));
                books.push((path, import));
            }
        }
        let shared_store = same(&source.join("kaia/kaia.sqlite3"), &data.join("kaia/kaia.sqlite3"));
        let mut plan = Self {
            counts: Counts::default(),
            shared_store,
            viewport: kaia_gfx::Size { w: 1264, h: 1680 },
            source,
            snapshot,
            settings,
            plugin_settings,
            books,
        };
        let preview = tempfile::tempdir()?;
        let mut lib = Library::open(&preview.path().join("library.sqlite3"))?;
        let mut settings = Settings::new();
        let report = plan.run(preview.path(), &mut lib, &mut settings, false)?;
        for kind in KINDS {
            plan.counts
                .add(kind, report.imported.get(kind) + report.skipped.get(kind));
        }
        Ok(plan)
    }

    pub fn import(self, data: &Path, library: &mut Library, settings: &mut Settings) -> Result<Report> {
        // A target symlink could point back into the source profile.
        reject_links(data)?;
        let _scan = crate::durable::lock(&data.join("nickel-import.lock"), false)?;
        self.run(data, library, settings, true)
    }
    fn run(
        &self,
        data: &Path,
        library: &mut Library,
        settings: &mut Settings,
        confirmed: bool,
    ) -> Result<Report> {
        let mut report = Report::default();
        let root = self.snapshot.path();
        for name in ["history.lua", "settings/collection.lua"] {
            let p = root.join(name);
            if !p.is_file() {
                continue;
            }
            let r = if name == "history.lua" {
                library.import_koreader_history(&p)?
            } else {
                library.import_koreader_collections(&p)?
            };
            report.count(
                if name == "history.lua" {
                    "History entries"
                } else {
                    "Collection entries"
                },
                r.added,
                r.already_present + r.skipped_invalid + r.skipped_missing + r.skipped_unsupported,
            );
        }
        let mut effective_settings = settings.clone();
        for (key, value) in &self.settings {
            if !settings.has(*key) && !settings.is_true(format!("lua_import.{key}")) {
                effective_settings.save(*key, value.clone());
            }
        }
        let default_layout =
            crate::surfaces::reader::koreader_default_layout(self.viewport, &effective_settings);
        for (book, import) in &self.books {
            let added = import.import(book, data, &default_layout)?;
            report.count("Other annotations", 0, import.report.skipped);
            if added {
                if let Some(indexed) = library.book_by_path(book)? {
                    let status = match import.report.status.as_deref() {
                        Some("complete") => kaia_library::Status::Complete,
                        Some("abandoned") => kaia_library::Status::Abandoned,
                        Some("reading") => kaia_library::Status::Reading,
                        _ => indexed.status,
                    };
                    library.import_nickel_progress(indexed.id, import.percent(), status)?;
                }
            }
            for (kind, n) in [
                ("Book states", 1),
                ("Highlights", import.highlights),
                ("Bookmarks", import.bookmarks),
                ("Notes", import.notes),
            ] {
                report.count(kind, if added { n } else { 0 }, if added { 0 } else { n });
            }
        }
        let stats = root.join("settings/statistics.sqlite3");
        if stats.is_file() {
            let r = Stats::open(data.join("statistics.sqlite3"))?.import_koreader(stats)?;
            report.count(
                "Statistics books",
                r.books_added as usize,
                r.books_matched as usize,
            );
            report.count("Page records", r.spans_added as usize, r.spans_skipped as usize);
        }
        let store = root.join("kaia/kaia.sqlite3");
        if store.is_file() {
            let mut source = Store::open(&root.join("kaia"))?;
            let export = source.export()?;
            let asset_count = files(&export.join("assets"))?.len();
            let sticker_count = source.list_all()?.len();
            let words = source.list_words()?;
            if confirmed && self.shared_store {
                report.count("Stickers", 0, sticker_count);
                report.count("Assets", 0, asset_count);
                report.count("Words", 0, words.len());
            } else {
                let mut target = Store::open(&data.join("kaia"))?;
                let r = target.import(&export)?;
                report.count("Stickers", r.stickers_added as usize, r.stickers_skipped as usize);
                report.count(
                    "Assets",
                    r.assets_added as usize,
                    asset_count.saturating_sub(r.assets_added as usize),
                );
                for word in words {
                    let added = target.import_word(&word)?;
                    report.count("Words", usize::from(added), usize::from(!added));
                }
            }
        }
        for &(key, ref value) in &self.settings {
            let marker = format!("lua_import.{key}");
            if settings.has(key) || settings.is_true(marker.as_str()) {
                report.count("Settings", 0, 1);
            } else {
                settings.save(key, value.clone());
                settings.save(marker, true.into());
                report.count("Settings", 1, 0);
            }
        }
        settings.flush(&data.join("settings.json"))?;
        let plugin_target = data.join("settings.reader.lua");
        if !self.plugin_settings.as_table().is_none_or(|t| t.is_empty()) {
            let shared = confirmed && same(&plugin_target, &self.source.join("settings.reader.lua"));
            if shared {
                report.count("Plugin data", 0, self.plugin_settings.as_table().unwrap().len());
            } else {
                let mut current = read_lua(&plugin_target)?.as_table().cloned().unwrap_or_default();
                for (key, value) in self.plugin_settings.as_table().unwrap() {
                    if current.contains_key(key) {
                        report.count("Plugin data", 0, 1);
                    } else {
                        current.insert(key.clone(), value.clone());
                        report.count("Plugin data", 1, 0);
                    }
                }
                crate::durable::atomic_write(
                    &plugin_target,
                    kaia_luadata::to_string(&Value::Table(current)).as_bytes(),
                )?;
            }
        }
        for name in PLUGIN_FILES {
            if !root.join(name).is_file() {
                continue;
            }
            let target = data.join(name);
            if target.exists() {
                report.count("Plugin data", 0, 1);
            } else {
                fs::create_dir_all(target.parent().unwrap())?;
                crate::durable::atomic_write(&target, &fs::read(root.join(name))?)?;
                report.count("Plugin data", 1, 0);
            }
        }
        Ok(report)
    }
}
fn same(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(a), Ok(b)) = (fs::metadata(a), fs::metadata(b)) {
            if a.dev() == b.dev() && a.ino() == b.ino() {
                return true;
            }
        }
    }
    matches!((fs::canonicalize(a),fs::canonicalize(b)),(Ok(a),Ok(b)) if a == b)
}
fn read_lua(path: &Path) -> Result<Value> {
    if !path.is_file() {
        return Ok(Value::Table(Default::default()));
    }
    let value = kaia_luadata::parse(&String::from_utf8_lossy(&fs::read(path)?))?;
    if value.as_table().is_none() {
        return Err("expected a Lua table".into());
    }
    Ok(value)
}
fn collect_paths(value: &Value, out: &mut BTreeSet<PathBuf>) {
    if let Some(p) = value.get("file").as_str() {
        out.insert(PathBuf::from(p));
    }
    if let Some(t) = value.as_table() {
        for v in t.values() {
            collect_paths(v, out);
        }
    }
}
fn files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    for e in fs::read_dir(root)? {
        let e = e?;
        if e.file_type()?.is_dir() {
            out.extend(files(&e.path())?);
        } else if e.file_type()?.is_file() {
            out.push(e.path());
        }
    }
    out.sort();
    Ok(out)
}
fn copy_source(source: &Path, target: &Path) -> Result<()> {
    if !source.exists() {
        return Ok(());
    }
    if fs::symlink_metadata(source)?.file_type().is_symlink() {
        return Err("source data contains a symbolic link".into());
    }
    if source.is_dir() {
        fs::create_dir_all(target)?;
        for e in fs::read_dir(source)? {
            let e = e?;
            copy_source(&e.path(), &target.join(e.file_name()))?;
        }
    } else {
        fs::create_dir_all(target.parent().unwrap())?;
        fs::copy(source, target)?;
        if source.extension().is_some_and(|e| e == "sqlite3") {
            for suffix in ["-wal", "-shm"] {
                let s = PathBuf::from(format!("{}{suffix}", source.display()));
                if s.is_file() {
                    fs::copy(s, format!("{}{suffix}", target.display()))?;
                }
            }
        }
    }
    Ok(())
}
fn reject_links(path: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() {
        return Err("import target contains a symbolic link".into());
    }
    if path.is_dir() {
        for e in fs::read_dir(path)? {
            reject_links(&e?.path())?;
        }
    }
    Ok(())
}
