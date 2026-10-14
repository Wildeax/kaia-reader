//! The Kaia per-book state file.
//!
//! One file per book, named by the book's partial MD5, in
//! `<data>/kaia/books/`. It is a `kaia_events::Settings` file, so it is JSON
//! and it is written atomically. Keys follow KOReader's names where KOReader
//! has one (`last_xpointer`, `last_page`, `percent_finished`, `annotations`,
//! `summary`), so a reader of the file recognizes them.
//!
//! Page numbers in this file are 1-based, as KOReader writes them. The Rust
//! API is 0-based.

use std::io;
use std::path::Path;

use kaia_doc::{Location, Margins};
use kaia_events::{Settings, Table, Value};

use crate::annotation::Annotation;
use crate::layout::TextLayout;
use crate::time::today;
use crate::view::ZoomMode;

/// The `format` key of the state file.
pub const STATE_FORMAT: &str = "kaia-book-state";
/// The `version` key of the state file. Files with a newer version are not read.
pub const STATE_VERSION: i64 = 1;
/// Reading-time entries kept per book. The oldest entries go first.
pub const MAX_READING_ENTRIES: usize = 5000;

/// The identity of a book file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentId {
    /// KOReader's partial MD5, 32 lowercase hex characters.
    pub partial_md5: String,
    /// The file size in bytes at the time of the last save.
    pub file_size: u64,
}

/// Time spent on one page during one visit.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadingTime {
    /// When the visit to the page ended, `YYYY-MM-DD HH:MM:SS` local time.
    pub datetime: String,
    /// The page, 1-based.
    pub page: u32,
    /// Seconds on the page, to the millisecond.
    pub seconds: f64,
}

/// Everything Kaia keeps for one book.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BookState {
    pub doc: DocumentId,
    /// The title as the engine reported it, for display only.
    pub title: Option<String>,
    /// The reading position. None means the first page.
    pub position: Option<Location>,
    /// Reading progress as a fraction, from the page shown.
    pub percent: f64,
    pub layout: TextLayout,
    pub zoom: ZoomMode,
    pub annotations: Vec<Annotation>,
    /// The id the next annotation receives.
    pub next_id: u64,
    pub status: Option<String>,
    pub rating: Option<u8>,
    pub summary_note: Option<String>,
    pub reading_time: Vec<ReadingTime>,
    /// The KOReader sidecar this state was imported from, if any.
    pub imported_from: Option<String>,
    /// The layout and viewport that the annotation positions were made in. Engines
    /// whose positions depend on the layout use it to find stale positions.
    pub layout_key: Option<String>,
    /// Remains true until the reader acknowledges that the file changed.
    pub file_change_pending: bool,
    /// Dictionary names to try first for this book. KOReader keeps the same list
    /// as `preferred_dictionaries` in the book's settings. Names come from the
    /// `.ifo` files, as `bookname`.
    pub preferred_dictionaries: Vec<String>,
    /// Kobo record ids already imported, retained after edits and deletions.
    pub nickel_imports: Vec<String>,
    /// Imported active seconds without a known page.
    pub nickel_reading_seconds: f64,
}

impl BookState {
    pub fn new(doc: DocumentId, layout: TextLayout) -> Self {
        Self {
            doc,
            title: None,
            position: None,
            percent: 0.0,
            layout,
            zoom: ZoomMode::default(),
            annotations: Vec::new(),
            next_id: 1,
            status: None,
            rating: None,
            summary_note: None,
            reading_time: Vec::new(),
            imported_from: None,
            layout_key: None,
            file_change_pending: false,
            preferred_dictionaries: Vec::new(),
            nickel_imports: Vec::new(),
            nickel_reading_seconds: 0.0,
        }
    }

    /// Gives the next free id to an annotation and returns it.
    pub fn assign_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Keeps `next_id` above every id in use, after a load or an import.
    pub fn repair_ids(&mut self) {
        let max = self.annotations.iter().map(|a| a.id).max().unwrap_or(0);
        self.next_id = self.next_id.max(max + 1);
    }

    pub fn to_table(&self) -> Table {
        let mut t = Table::new();
        t.insert("format".into(), Value::from(STATE_FORMAT));
        t.insert("version".into(), Value::from(STATE_VERSION));
        t.insert("doc".into(), {
            let mut d = Table::new();
            d.insert("partial_md5".into(), Value::from(self.doc.partial_md5.as_str()));
            d.insert("file_size".into(), Value::from(self.doc.file_size as f64));
            if let Some(title) = &self.title {
                d.insert("title".into(), Value::from(title.as_str()));
            }
            Value::Table(d)
        });
        match &self.position {
            Some(Location::Engine(xp)) => {
                t.insert("last_xpointer".into(), Value::from(xp.as_str()));
            }
            Some(Location::Page(p)) => {
                t.insert("last_page".into(), Value::from(*p as i64 + 1));
            }
            None => {}
        }
        t.insert("percent_finished".into(), Value::from(self.percent));
        t.insert("layout".into(), layout_value(&self.layout));
        t.insert("zoom".into(), zoom_value(self.zoom));
        t.insert(
            "annotations".into(),
            Value::array(self.annotations.iter().map(Annotation::to_value)),
        );
        let mut summary = Table::new();
        if let Some(s) = &self.status {
            summary.insert("status".into(), Value::from(s.as_str()));
        }
        if let Some(r) = self.rating {
            summary.insert("rating".into(), Value::from(r as i64));
        }
        if let Some(n) = &self.summary_note {
            summary.insert("note".into(), Value::from(n.as_str()));
        }
        summary.insert("modified".into(), Value::from(today()));
        t.insert("summary".into(), Value::Table(summary));
        t.insert(
            "reading_time".into(),
            Value::array(self.reading_time.iter().map(|r| {
                let mut e = Table::new();
                e.insert("datetime".into(), Value::from(r.datetime.as_str()));
                e.insert("page".into(), Value::from(r.page as i64));
                e.insert("seconds".into(), Value::from(r.seconds));
                Value::Table(e)
            })),
        );
        if let Some(src) = &self.imported_from {
            t.insert("imported_from".into(), Value::from(src.as_str()));
        }
        if let Some(key) = &self.layout_key {
            t.insert("layout_key".into(), Value::from(key.as_str()));
        }
        if self.file_change_pending {
            t.insert("file_change_pending".into(), Value::Bool(true));
        }
        if !self.preferred_dictionaries.is_empty() {
            t.insert(
                "preferred_dictionaries".into(),
                Value::array(
                    self.preferred_dictionaries
                        .iter()
                        .map(|n| Value::from(n.as_str())),
                ),
            );
        }
        t.insert(
            "nickel_reading_seconds".into(),
            Value::from(self.nickel_reading_seconds),
        );
        t.insert(
            "nickel_imports".into(),
            Value::array(self.nickel_imports.iter().map(|id| Value::from(id.as_str()))),
        );
        t
    }

    /// Reads a state table. Returns None when the table is not a Kaia state
    /// file of a version this code reads. Missing keys take their defaults.
    pub fn from_table(t: &Table) -> Option<BookState> {
        let root = Value::Table(t.clone());
        if root.get("format").as_str() != Some(STATE_FORMAT) {
            return None;
        }
        if root.get("version").as_i64()? > STATE_VERSION {
            return None;
        }
        let doc_v = root.get("doc");
        let doc = DocumentId {
            partial_md5: doc_v.get("partial_md5").as_str()?.to_string(),
            file_size: doc_v.get("file_size").as_f64().filter(|f| *f >= 0.0)? as u64,
        };
        let mut state = BookState::new(doc, layout_from_value(root.get("layout")));
        state.title = doc_v.get("title").as_str().map(str::to_string);
        state.position = if let Some(xp) = root.get("last_xpointer").as_str().filter(|s| !s.is_empty()) {
            Some(Location::Engine(xp.to_string()))
        } else {
            root.get("last_page")
                .as_i64()
                .filter(|p| *p >= 1)
                .map(|p| Location::Page((p - 1) as u32))
        };
        state.percent = root
            .get("percent_finished")
            .as_f64()
            .filter(|p| p.is_finite())
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        state.zoom = zoom_from_value(root.get("zoom"));
        if let Value::Table(list) = root.get("annotations") {
            state.annotations = list.values().filter_map(Annotation::from_value).collect();
        }
        let summary = root.get("summary");
        state.status = summary.get("status").as_str().map(str::to_string);
        state.rating = summary
            .get("rating")
            .as_i64()
            .filter(|r| (0..=5).contains(r))
            .map(|r| r as u8);
        state.summary_note = summary.get("note").as_str().map(str::to_string);
        if let Value::Table(list) = root.get("reading_time") {
            state.reading_time = list
                .values()
                .filter_map(|e| {
                    Some(ReadingTime {
                        datetime: e.get("datetime").as_str()?.to_string(),
                        page: e.get("page").as_i64()?.max(1) as u32,
                        seconds: e.get("seconds").as_f64()?,
                    })
                })
                .collect();
        }
        state.imported_from = root.get("imported_from").as_str().map(str::to_string);
        state.layout_key = root.get("layout_key").as_str().map(str::to_string);
        state.file_change_pending = root.get("file_change_pending").as_bool().unwrap_or(false);
        if let Value::Table(list) = root.get("preferred_dictionaries") {
            state.preferred_dictionaries = list
                .values()
                .filter_map(|v| v.as_str().filter(|n| !n.is_empty()).map(str::to_string))
                .collect();
        }
        if let Value::Table(ids) = root.get("nickel_imports") {
            state.nickel_imports = ids
                .values()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
        }
        state.nickel_reading_seconds = root
            .get("nickel_reading_seconds")
            .as_f64()
            .filter(|s| s.is_finite() && *s >= 0.0)
            .unwrap_or(0.0);
        state.repair_ids();
        Some(state)
    }
}

fn layout_value(l: &TextLayout) -> Value {
    let mut t = Table::new();
    if let Some(f) = &l.font_family {
        t.insert("font_family".into(), Value::from(f.as_str()));
    }
    t.insert("font_size".into(), Value::from(l.font_size as f64));
    let mut m = Table::new();
    m.insert("top".into(), Value::from(l.margins.top as f64));
    m.insert("right".into(), Value::from(l.margins.right as f64));
    m.insert("bottom".into(), Value::from(l.margins.bottom as f64));
    m.insert("left".into(), Value::from(l.margins.left as f64));
    t.insert("margins".into(), Value::Table(m));
    t.insert("line_spacing".into(), Value::from(l.line_spacing as f64));
    t.insert("hyphenation".into(), Value::Bool(l.hyphenation));
    t.insert("justify".into(), Value::Bool(l.justify));
    t.insert("embedded_styles".into(), Value::Bool(l.embedded_styles));
    Value::Table(t)
}

/// Reads the layout table. Missing or invalid values take the defaults.
fn layout_from_value(v: &Value) -> TextLayout {
    let d = TextLayout::default();
    let num = |key: &str, default: f32| {
        v.get(key)
            .as_f64()
            .filter(|n| n.is_finite())
            .map_or(default, |n| n as f32)
    };
    let m = v.get("margins");
    let margin = |key: &str, default: f32| {
        m.get(key)
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map_or(default, |n| n as f32)
    };
    TextLayout {
        font_family: v.get("font_family").as_str().map(str::to_string),
        font_size: num("font_size", d.font_size),
        margins: Margins {
            top: margin("top", d.margins.top),
            right: margin("right", d.margins.right),
            bottom: margin("bottom", d.margins.bottom),
            left: margin("left", d.margins.left),
        },
        line_spacing: num("line_spacing", d.line_spacing),
        hyphenation: v.get("hyphenation").as_bool().unwrap_or(d.hyphenation),
        justify: v.get("justify").as_bool().unwrap_or(d.justify),
        embedded_styles: v.get("embedded_styles").as_bool().unwrap_or(d.embedded_styles),
    }
}

fn zoom_value(z: ZoomMode) -> Value {
    let mut t = Table::new();
    let (mode, percent) = match z {
        ZoomMode::FitPage => ("fit_page", 100),
        ZoomMode::FitWidth => ("fit_width", 100),
        ZoomMode::Manual { percent } => ("manual", percent),
    };
    t.insert("mode".into(), Value::from(mode));
    t.insert("percent".into(), Value::from(percent as i64));
    Value::Table(t)
}

fn zoom_from_value(v: &Value) -> ZoomMode {
    let percent = v
        .get("percent")
        .as_i64()
        .filter(|p| (10..=800).contains(p))
        .unwrap_or(100) as u32;
    match v.get("mode").as_str() {
        Some("fit_width") => ZoomMode::FitWidth,
        Some("manual") => ZoomMode::Manual { percent },
        _ => ZoomMode::FitPage,
    }
}

pub(crate) enum StateLoad {
    Missing,
    Loaded(BookState),
    /// The main file was damaged; an older state may be available.
    Corrupt(Option<BookState>),
    /// A crash may have happened after rotating the main file to `.old`.
    Backup(BookState),
    Newer(i64),
}

/// Reads the main file first, then `.old`. I/O errors stop the open.
pub(crate) fn read_state(path: &Path) -> io::Result<StateLoad> {
    let main = read_one(path)?;
    match main {
        Some(Ok(state)) => return Ok(StateLoad::Loaded(state)),
        Some(Err(Some(version))) => return Ok(StateLoad::Newer(version)),
        _ => {}
    }
    let mut old = path.as_os_str().to_owned();
    old.push(".old");
    let old = read_one(Path::new(&old))?;
    if let Some(Err(Some(version))) = old {
        return Ok(StateLoad::Newer(version));
    }
    let backup = match old {
        Some(Ok(state)) => Some(state),
        _ => None,
    };
    match main {
        None => Ok(backup.map_or(StateLoad::Missing, StateLoad::Backup)),
        Some(Err(_)) => Ok(StateLoad::Corrupt(backup)),
        Some(Ok(_)) => unreachable!(),
    }
}

fn read_one(path: &Path) -> io::Result<Option<Result<BookState, Option<i64>>>> {
    let Some(settings) = Settings::read_exact(path).or_else(|error| {
        if error.kind() == io::ErrorKind::InvalidData {
            Ok(Some(Settings::new()))
        } else {
            Err(error)
        }
    })?
    else {
        return Ok(None);
    };
    let root = Value::Table(settings.table().clone());
    let version = root.get("version").as_i64();
    if version.is_some_and(|v| v > STATE_VERSION) {
        return Ok(Some(Err(version)));
    }
    Ok(Some(BookState::from_table(settings.table()).ok_or(None)))
}

/// Writes the state file atomically. The previous version stays as `.old`.
pub(crate) fn write_state(path: &Path, state: &BookState) -> io::Result<()> {
    #[cfg(all(test, unix))]
    if FAIL_NEXT_WRITE.with(|flag| flag.replace(false)) {
        return Err(io::Error::from_raw_os_error(28));
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    Settings::from_table(state.to_table()).flush_book(path)
}

#[cfg(all(test, unix))]
std::thread_local! {
    static FAIL_NEXT_WRITE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(all(test, unix))]
pub(crate) fn fail_next_write_with_disk_full() {
    FAIL_NEXT_WRITE.with(|flag| flag.set(true));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pos::Pos;

    fn state() -> BookState {
        let mut s = BookState::new(
            DocumentId {
                partial_md5: "378df3409d69b6994febc92492dbb19e".into(),
                file_size: 12345,
            },
            TextLayout {
                font_size: 26.0,
                justify: true,
                ..TextLayout::default()
            },
        );
        s.title = Some("Frankenstein".into());
        s.position = Some(Location::Engine("/body/DocFragment[5]/body/div/p[12].0".into()));
        s.percent = 0.25;
        s.zoom = ZoomMode::Manual { percent: 150 };
        s.status = Some("reading".into());
        s.rating = Some(4);
        let id = s.assign_id();
        s.annotations.push(Annotation {
            id,
            datetime: "2026-10-09 19:12:04".into(),
            datetime_updated: None,
            drawer: Some("lighten".into()),
            color: Some("yellow".into()),
            text: Some("Learn from me".into()),
            text_edited: None,
            note: Some("Read again".into()),
            note_format: None,
            chapter: Some("Chapter 4".into()),
            pageno: Some(12),
            pageref: None,
            pboxes: None,
            ext: None,
            page: Location::Engine("/body/DocFragment[5]/body/div/p[12].0".into()),
            pos0: Some(Pos::Xpointer("/body/DocFragment[5]/body/div/p[12].0".into())),
            pos1: Some(Pos::Xpointer("/body/DocFragment[5]/body/div/p[12].40".into())),
            orphaned: false,
        });
        s.reading_time.push(ReadingTime {
            datetime: "2026-10-13 10:00:00".into(),
            page: 3,
            seconds: 42.5,
        });
        s
    }

    #[test]
    fn table_round_trip() {
        let s = state();
        let back = BookState::from_table(&s.to_table()).expect("state");
        assert_eq!(back.doc, s.doc);
        assert_eq!(back.title, s.title);
        assert_eq!(back.position, s.position);
        assert_eq!(back.percent, s.percent);
        assert_eq!(back.layout, s.layout);
        assert_eq!(back.zoom, s.zoom);
        assert_eq!(back.annotations, s.annotations);
        assert_eq!(back.status, s.status);
        assert_eq!(back.rating, s.rating);
        assert_eq!(back.reading_time, s.reading_time);
        assert_eq!(back.next_id, 2);
    }

    #[test]
    fn preferred_dictionaries_use_koreader_key_and_round_trip() {
        let mut s = state();
        s.preferred_dictionaries = vec!["Second Dictionary".into(), "Test Dictionary".into()];
        let table = s.to_table();
        let list = Value::Table(table.clone());
        assert_eq!(
            list.get("preferred_dictionaries").array_len(),
            2,
            "the KOReader key holds the names in order"
        );
        let back = BookState::from_table(&table).expect("state");
        assert_eq!(back.preferred_dictionaries, s.preferred_dictionaries);
    }

    #[test]
    fn no_preferred_dictionary_writes_no_key() {
        let s = state();
        assert!(s.preferred_dictionaries.is_empty());
        assert!(!s
            .to_table()
            .contains_key(&kaia_events::Key::from("preferred_dictionaries")));
    }

    #[test]
    fn page_positions_are_one_based_on_disk() {
        let mut s = state();
        s.position = Some(Location::Page(4));
        let t = s.to_table();
        assert_eq!(Value::Table(t.clone()).get("last_page").as_i64(), Some(5));
        assert_eq!(
            BookState::from_table(&t).expect("state").position,
            Some(Location::Page(4))
        );
    }

    #[test]
    fn rejects_other_formats_and_newer_versions() {
        let mut t = state().to_table();
        t.insert("version".into(), Value::from(STATE_VERSION + 1));
        assert!(BookState::from_table(&t).is_none());
        let mut t = state().to_table();
        t.insert("format".into(), Value::from("something-else"));
        assert!(BookState::from_table(&t).is_none());
    }
}
