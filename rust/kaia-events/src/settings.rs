//! Key-value settings persisted as JSON, mirroring `LuaSettings`
//! (`frontend/luasettings.lua`).
//!
//! - `open` reads the file. When that fails, it reads `<file>.old`. When both
//!   fail, the settings start empty.
//! - `read`, `save`, `del`, `has`, `is_true` and `is_false` behave like
//!   `readSetting`, `saveSetting` (nil deletes), `delSetting`, `has`, `isTrue`
//!   and `isFalse`.
//! - `flush` writes the file atomically. It writes `<file>.tmp`, syncs it, moves
//!   the current file to `<file>.old` when that file is more than 60 seconds
//!   old (as `LuaSettings:backup` does), then renames the temp file into place.
//!
//! The JSON format is described in `json.rs`.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::json;
use crate::value::{Key, Table, Value};

/// `LuaSettings:backup` only moves a file to `.old` when it is older than this.
const BACKUP_AGE: Duration = Duration::from_secs(60);

static NIL: Value = Value::Nil;

/// What `Settings::open` found on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenStatus {
    /// Neither the file nor its backup exists. The settings start empty.
    Missing,
    /// The file was read.
    Loaded,
    /// The file was unreadable, so the `.old` backup was read.
    FromBackup,
    /// The file exists but neither it nor the backup could be read. The settings start empty.
    Unreadable,
}

/// A key-value store. Keys are `Key`s, usually strings. Values are `Value`s.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    data: Table,
    status: OpenStatus,
}

impl Default for Settings {
    fn default() -> Self {
        Self::new()
    }
}

impl Settings {
    /// Empty settings that are not tied to a file.
    pub fn new() -> Self {
        Self {
            data: Table::new(),
            status: OpenStatus::Missing,
        }
    }

    /// Settings with the given contents. Nil entries are removed. The status
    /// is `Missing`, because nothing was read from disk.
    pub fn from_table(data: Table) -> Self {
        Self {
            data: prune_table(data),
            status: OpenStatus::Missing,
        }
    }

    /// Reads the settings file at `path`. Never fails: the status says what happened.
    pub fn open(path: &Path) -> Settings {
        let existed = path.is_file();
        if let Some(data) = read_table(path) {
            return Settings {
                data,
                status: OpenStatus::Loaded,
            };
        }
        if let Some(data) = read_table(&suffixed(path, ".old")) {
            return Settings {
                data,
                status: OpenStatus::FromBackup,
            };
        }
        Settings {
            data: Table::new(),
            status: if existed {
                OpenStatus::Unreadable
            } else {
                OpenStatus::Missing
            },
        }
    }

    pub fn open_status(&self) -> OpenStatus {
        self.status
    }

    pub fn table(&self) -> &Table {
        &self.data
    }

    /// `readSetting(key)`. Returns nil when the key is not set.
    pub fn read(&self, key: impl Into<Key>) -> &Value {
        let key: Key = key.into();
        self.data.get(&key).unwrap_or(&NIL)
    }

    /// `readSetting(key, default)`. When the key is unset, stores `default`
    /// first (with nil entries removed) and returns it, so the reference
    /// belongs to the store.
    pub fn read_or_init(&mut self, key: impl Into<Key>, default: Value) -> &Value {
        let key: Key = key.into();
        if default.is_nil() {
            return self.data.get(&key).unwrap_or(&NIL);
        }
        self.data.entry(key).or_insert_with(|| prune(default))
    }

    /// `saveSetting(key, value)`. Saving nil deletes the key.
    pub fn save(&mut self, key: impl Into<Key>, value: Value) {
        let key: Key = key.into();
        if value.is_nil() {
            self.data.remove(&key);
        } else {
            self.data.insert(key, prune(value));
        }
    }

    /// `delSetting(key)`.
    pub fn del(&mut self, key: impl Into<Key>) {
        let key: Key = key.into();
        self.data.remove(&key);
    }

    /// `has(key)`: true when the key is set to a non-nil value.
    pub fn has(&self, key: impl Into<Key>) -> bool {
        let key: Key = key.into();
        self.data.contains_key(&key)
    }

    /// `isTrue(key)`: true only for the boolean `true`.
    pub fn is_true(&self, key: impl Into<Key>) -> bool {
        matches!(self.read(key), Value::Bool(true))
    }

    /// `isFalse(key)`: true only for the boolean `false`.
    pub fn is_false(&self, key: impl Into<Key>) -> bool {
        matches!(self.read(key), Value::Bool(false))
    }

    /// Writes the settings to `path` with the steps described in the module comment.
    pub fn flush(&self, path: &Path) -> io::Result<()> {
        let text = serde_json::to_string_pretty(&json::encode_table(&self.data)).map_err(io::Error::other)?;
        let temp = suffixed(path, ".tmp");
        write_synced(&temp, text.as_bytes())?;
        let dir = parent_dir(path);
        if is_older_than(path, BACKUP_AGE) {
            fs::rename(path, suffixed(path, ".old"))?;
            sync_dir(dir)?;
        }
        fs::rename(&temp, path)?;
        sync_dir(dir)
    }
}

/// Reads a settings file. Returns None when the file is missing or does not decode.
fn read_table(path: &Path) -> Option<Table> {
    let text = fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json::decode_table(&json).ok()
}

/// Removes nil entries from a value, recursively, so the store never holds nil.
fn prune(value: Value) -> Value {
    match value {
        Value::Table(table) => Value::Table(prune_table(table)),
        other => other,
    }
}

fn prune_table(table: Table) -> Table {
    table
        .into_iter()
        .filter(|(_, value)| !value.is_nil())
        .map(|(key, value)| (key, prune(value)))
        .collect()
}

fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn parent_dir(path: &Path) -> &Path {
    path.parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// True when the file exists and was last modified more than `age` ago.
fn is_older_than(path: &Path, age: Duration) -> bool {
    let Ok(modified) = fs::metadata(path).and_then(|meta| meta.modified()) else {
        return false;
    };
    SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|elapsed| elapsed > age)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A fresh directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::SeqCst);
            let dir = std::env::temp_dir().join(format!("kaia-events-{label}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            TempDir(dir)
        }

        fn path(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn read_save_del_and_predicates() {
        let mut s = Settings::new();
        assert!(s.read("missing").is_nil());
        assert!(!s.has("missing"));
        s.save("flag", Value::Bool(true));
        s.save("off", Value::Bool(false));
        s.save("text", Value::from("hello"));
        assert!(s.is_true("flag"));
        assert!(!s.is_true("text"));
        assert!(s.is_false("off"));
        assert!(!s.is_false("flag"));
        assert!(s.has("text"));
        s.save("text", Value::Nil);
        assert!(!s.has("text"));
        s.del("flag");
        assert!(!s.has("flag"));
    }

    #[test]
    fn read_or_init_stores_the_default_once() {
        let mut s = Settings::new();
        assert_eq!(s.read_or_init("count", Value::from(3i64)), &Value::from(3i64));
        let mut with_nil = Table::new();
        with_nil.insert(Key::from("gone"), Value::Nil);
        let stored = s.read_or_init("table", Value::Table(with_nil));
        assert!(stored.as_table().unwrap().is_empty());
        s.save("count", Value::from(9i64));
        assert_eq!(s.read_or_init("count", Value::from(3i64)), &Value::from(9i64));
    }

    #[test]
    fn saved_tables_are_pruned_of_nil() {
        let mut inner = Table::new();
        inner.insert(Key::from("a"), Value::from(1i64));
        inner.insert(Key::from("b"), Value::Nil);
        let mut s = Settings::new();
        s.save("outer", Value::Table(inner));
        let stored = s.read("outer").as_table().unwrap();
        assert_eq!(stored.len(), 1);
    }

    #[test]
    fn flush_then_open_round_trips() {
        let dir = TempDir::new("roundtrip");
        let path = dir.path("settings.json");
        let mut s = Settings::new();
        s.save("name", Value::from("Kaia"));
        s.save("count", Value::from(12i64));
        s.save("ratio", Value::Number(0.25));
        s.save("list", Value::array([Value::from(1i64), Value::from("two")]));
        s.flush(&path).unwrap();

        let reopened = Settings::open(&path);
        assert_eq!(reopened.open_status(), OpenStatus::Loaded);
        assert_eq!(reopened.table(), s.table());
        assert!(!dir.path("settings.json.tmp").exists());
    }

    #[test]
    fn open_missing_file_is_empty() {
        let dir = TempDir::new("missing");
        let s = Settings::open(&dir.path("nope.json"));
        assert_eq!(s.open_status(), OpenStatus::Missing);
        assert!(s.table().is_empty());
    }

    #[test]
    fn corrupt_file_falls_back_to_backup() {
        let dir = TempDir::new("backup");
        let path = dir.path("settings.json");
        fs::write(&path, "{ not json").unwrap();
        fs::write(dir.path("settings.json.old"), r#"{"from": "backup"}"#).unwrap();
        let s = Settings::open(&path);
        assert_eq!(s.open_status(), OpenStatus::FromBackup);
        assert_eq!(s.read("from"), &Value::from("backup"));
    }

    #[test]
    fn corrupt_file_without_backup_is_unreadable() {
        let dir = TempDir::new("unreadable");
        let path = dir.path("settings.json");
        fs::write(&path, "[1, 2,").unwrap();
        let s = Settings::open(&path);
        assert_eq!(s.open_status(), OpenStatus::Unreadable);
        assert!(s.table().is_empty());
    }

    #[test]
    fn recent_file_is_replaced_without_a_backup() {
        let dir = TempDir::new("recent");
        let path = dir.path("settings.json");
        let mut first = Settings::new();
        first.save("v", Value::from(1i64));
        first.flush(&path).unwrap();
        let mut second = Settings::new();
        second.save("v", Value::from(2i64));
        second.flush(&path).unwrap();
        // The file is only seconds old, so LuaSettings would not rotate it.
        assert!(!dir.path("settings.json.old").exists());
        assert_eq!(Settings::open(&path).read("v"), &Value::from(2i64));
    }

    #[test]
    fn old_file_is_rotated_to_backup_on_flush() {
        let dir = TempDir::new("rotate");
        let path = dir.path("settings.json");
        let mut first = Settings::new();
        first.save("v", Value::from(1i64));
        first.flush(&path).unwrap();
        // Make the file look two minutes old, as if it was written long ago.
        let old = SystemTime::now() - Duration::from_secs(120);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(old)
            .unwrap();

        let mut second = Settings::new();
        second.save("v", Value::from(2i64));
        second.flush(&path).unwrap();

        assert_eq!(Settings::open(&path).read("v"), &Value::from(2i64));
        assert_eq!(
            Settings::open(&dir.path("settings.json.old")).read("v"),
            &Value::from(1i64)
        );
    }
}
