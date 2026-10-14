//! Reads only selected columns from a private, read-only Kobo snapshot.

use super::Result;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

pub(super) const TABLES: &[(&str, &[&str])] = &[
    ("DbVersion", &["version"]),
    (
        "content",
        &[
            "ContentID",
            "ContentType",
            "BookID",
            "adobe_location",
            "Title",
            "Attribution",
            "___PercentRead",
            "ReadStatus",
        ],
    ),
    (
        "Bookmark",
        &[
            "BookmarkID",
            "VolumeID",
            "Text",
            "Annotation",
            "DateCreated",
            "_IsDeleted",
        ],
    ),
    ("Shelf", &["Id", "Name", "_IsDeleted"]),
    ("ShelfContent", &["ShelfName", "ContentId", "_IsDeleted"]),
    ("WordList", &["Text", "VolumeId", "DictSuffix", "DateCreated"]),
    ("Ratings", &["ContentID", "Rating", "Review"]),
    (
        "KoboPlusReadingSessions",
        &[
            "ContentId",
            "StartDate",
            "EndDate",
            "StartTime",
            "EndTime",
            "StartPercentRead",
            "EndPercentRead",
            "IdleTime",
        ],
    ),
];

#[derive(Clone, Default)]
pub(super) struct Row(pub BTreeMap<String, String>);
impl Row {
    pub fn text(&self, name: &str) -> &str {
        self.0.get(name).map(String::as_str).unwrap_or("")
    }
    pub fn number(&self, name: &str) -> Option<f64> {
        self.text(name).parse().ok().filter(|n: &f64| n.is_finite())
    }
    pub fn deleted(&self) -> bool {
        matches!(
            self.text("_IsDeleted").to_ascii_lowercase().as_str(),
            "true" | "1"
        )
    }
}

pub(super) struct Snapshot {
    pub rows: BTreeMap<String, Vec<Row>>,
    pub counts: BTreeMap<String, usize>,
    pub version: i64,
}

impl Snapshot {
    pub fn read(source: &Path) -> Result<Self> {
        let temp_root = fs::canonicalize(std::env::temp_dir())?;
        if temp_root.components().any(|c| c.as_os_str() == ".kobo") {
            return Err("The scratch folder is inside Kobo data".into());
        }
        let scratch = tempfile::tempdir_in(temp_root)?;
        let mut before = Vec::new();
        for suffix in ["", "-wal", "-shm"] {
            let name = format!("KoboReader.sqlite{suffix}");
            let path = source.with_file_name(&name);
            let stamp = match fs::metadata(&path) {
                Ok(metadata) => Some((metadata.len(), metadata.modified()?)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && !suffix.is_empty() => None,
                Err(e) => return Err(e.into()),
            };
            before.push((path, name, stamp));
        }
        for (path, name, stamp) in &before {
            if stamp.is_some() {
                fs::copy(path, scratch.path().join(name))?;
            }
        }
        for (path, _, stamp) in before {
            let after = match fs::metadata(path) {
                Ok(metadata) => Some((metadata.len(), metadata.modified()?)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            if after != stamp {
                return Err("Kobo data changed during the copy".into());
            }
        }
        // Do not use immutable=1 here. SQLite must read committed rows in the copied WAL.
        let conn = Connection::open_with_flags(
            scratch.path().join("KoboReader.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?;
        conn.pragma_update(None, "query_only", true)?;
        conn.authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
            AuthAction::Read { table_name, .. }
                if !TABLES.iter().any(|(t, _)| t.eq_ignore_ascii_case(table_name))
                    && !matches!(table_name, "sqlite_master" | "sqlite_schema") =>
            {
                Authorization::Deny
            }
            _ => Authorization::Allow,
        }))?;
        let mut snapshot = Self {
            rows: BTreeMap::new(),
            counts: BTreeMap::new(),
            version: 0,
        };
        for (table, columns) in TABLES {
            let kind: Option<String> = conn
                .query_row(
                    "SELECT type FROM sqlite_schema WHERE name=?1 COLLATE NOCASE",
                    [table],
                    |r| r.get("type"),
                )
                .optional()?;
            let mut data = Vec::new();
            if kind.as_deref() == Some("table") {
                let count: i64 =
                    conn.query_row(&format!("SELECT count(*) AS count FROM \"{table}\""), [], |r| {
                        r.get("count")
                    })?;
                snapshot.counts.insert((*table).into(), count as usize);
                let mut stmt = conn.prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
                let available = stmt
                    .query_map([], |r| r.get::<_, String>("name"))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let selected: Vec<_> = columns
                    .iter()
                    .filter(|c| available.iter().any(|a| a.eq_ignore_ascii_case(c)))
                    .copied()
                    .collect();
                if !selected.is_empty() {
                    let select = selected
                        .iter()
                        .map(|c| format!("CAST(\"{c}\" AS TEXT) AS \"{c}\""))
                        .collect::<Vec<_>>()
                        .join(",");
                    let mut stmt = conn.prepare(&format!("SELECT {select} FROM \"{table}\""))?;
                    data = stmt
                        .query_map([], |r| {
                            let mut values = BTreeMap::new();
                            for name in &selected {
                                if let Some(value) = r.get::<_, Option<String>>(*name)? {
                                    values.insert((*name).into(), value);
                                }
                            }
                            Ok(Row(values))
                        })?
                        .collect::<rusqlite::Result<_>>()?;
                }
            } else if kind.is_some() {
                return Err("Kobo import requires tables, not views".into());
            } else {
                snapshot.counts.insert((*table).into(), 0);
            }
            snapshot.rows.insert((*table).into(), data);
        }
        snapshot.version = snapshot.rows["DbVersion"]
            .first()
            .and_then(|r| r.text("version").parse().ok())
            .ok_or("Kobo schema version is missing")?;
        eprintln!("Kobo schema version: {}", snapshot.version);
        for (table, count) in &snapshot.counts {
            eprintln!("Kobo {table}: {count}");
        }
        Ok(snapshot)
    }
}
