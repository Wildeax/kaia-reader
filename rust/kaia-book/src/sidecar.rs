//! Import of KOReader's sidecar files (`metadata.<ext>.lua`). Kaia reads them
//! through `kaia-luadata` and never writes them.
//!
//! The sidecar is looked up where KOReader keeps it. Each lookup location
//! corresponds to one value of KOReader's `document_metadata_folder` setting.

use std::path::{Path, PathBuf};

use kaia_doc::{Location, Margins};
use kaia_events::{Key, Table, Value};

use crate::annotation::Annotation;
use crate::layout::TextLayout;
use crate::view::ZoomMode;

/// Where KOReader keeps a book's sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SidecarLocation {
    /// `<book>.sdr/metadata.<ext>.lua` next to the book. KOReader's "doc" setting.
    BesideBook,
    /// `<root>/<book path without the leading slash, without the extension>.sdr/`.
    /// KOReader's "dir" setting. `root` is KOReader's docsettings directory.
    DocSettingsDir(PathBuf),
    /// `<root>/<first two hex digits of the MD5>/<MD5>.sdr/`. KOReader's "hash"
    /// setting. `root` is KOReader's hashdocsettings directory.
    HashDocSettingsDir(PathBuf),
    /// KOReader's legacy history directory.
    HistoryDir(PathBuf),
}

/// The sidecar files KOReader checks. The reader chooses the newest valid one.
pub fn sidecar_candidates(book: &Path, partial_md5: &str, locations: &[SidecarLocation]) -> Vec<PathBuf> {
    let path = book.to_string_lossy();
    let stem_path = path.rfind('.').map_or(path.as_ref(), |i| &path[..i]);
    let ext = path
        .rsplit_once('.')
        .filter(|(_, ext)| !ext.is_empty())
        .map_or("_", |(_, ext)| ext);
    let sidecar_name = format!("metadata.{ext}.lua");
    let mut out = Vec::new();
    for loc in locations {
        match loc {
            SidecarLocation::BesideBook => {
                let dir = PathBuf::from(format!("{stem_path}.sdr"));
                let main = dir.join(&sidecar_name);
                out.push(main.clone());
                out.push(with_suffix(&main, ".old"));
                let name = book.file_name().unwrap_or_default();
                out.push(dir.join(format!("{}.lua", name.to_string_lossy())));
            }
            SidecarLocation::DocSettingsDir(root) => {
                let rel = stem_path.strip_prefix('/').unwrap_or(stem_path);
                let main = root.join(format!("{rel}.sdr")).join(&sidecar_name);
                out.push(main.clone());
                out.push(with_suffix(&main, ".old"));
            }
            SidecarLocation::HashDocSettingsDir(root) => {
                if partial_md5.len() >= 2 {
                    let main = root
                        .join(&partial_md5[..2])
                        .join(format!("{partial_md5}.sdr"))
                        .join(&sidecar_name);
                    out.push(main.clone());
                    out.push(with_suffix(&main, ".old"));
                }
            }
            SidecarLocation::HistoryDir(root) => {
                let parent = book.parent().unwrap_or(Path::new(""));
                let encoded = format!("{}/", parent.display()).replace('/', "#");
                let name = book.file_name().unwrap_or_default().to_string_lossy();
                let main = root.join(format!("[{encoded}] {name}.lua"));
                out.push(main.clone());
                out.push(with_suffix(&main, ".old"));
            }
        }
    }
    out.push(with_suffix(book, ".kpdfview.lua"));
    out
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Reads the newest nonempty candidate that parses as a Lua table. Returns the table,
/// source file and whether UTF-8 conversion replaced any bytes.
pub(crate) fn read_first(candidates: &[PathBuf]) -> Option<(Value, PathBuf, bool)> {
    let mut ordered: Vec<_> = candidates
        .iter()
        .enumerate()
        .filter_map(|(i, path)| {
            let meta = std::fs::metadata(path).ok()?;
            if meta.len() == 0 {
                return None;
            }
            Some((i, path, meta.modified().ok()?))
        })
        .collect();
    for pair in candidates.windows(2) {
        if pair.len() == 2 && pair[1] == with_suffix(&pair[0], ".old") {
            let primary = ordered
                .iter()
                .find(|(_, path, _)| **path == pair[0])
                .map(|(_, _, time)| *time);
            let backup = ordered
                .iter()
                .find(|(_, path, _)| **path == pair[1])
                .map(|(_, _, time)| *time);
            if let (Some(a), Some(b)) = (primary, backup) {
                if b > a {
                    if let Some((_, _, time)) = ordered.iter_mut().find(|(_, path, _)| **path == pair[0]) {
                        *time = b;
                    }
                }
            }
        }
    }
    ordered.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    for (_, path, _) in ordered {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let src = String::from_utf8_lossy(&bytes);
        if let Ok(v @ Value::Table(_)) = kaia_luadata::parse(&src) {
            if matches!(&v, Value::Table(t) if t.is_empty()) {
                continue;
            }
            return Some((v, path.clone(), matches!(src, std::borrow::Cow::Owned(_))));
        }
    }
    None
}

/// What the session takes from a sidecar, and what it could not take.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportReport {
    /// The sidecar file that was read.
    pub source: PathBuf,
    /// True when invalid UTF-8 bytes in the source were replaced during import.
    pub lossy: bool,
    /// Annotations and bookmarks taken from the sidecar.
    pub annotations: usize,
    /// Entries that do not fit this book, such as PDF positions in an EPUB.
    pub skipped: usize,
    /// Imported annotations whose position does not resolve in the book. They
    /// are kept and marked orphaned. Set when the book is opened.
    pub unresolved: usize,
    /// True when the sidecar gave a reading position.
    pub position: bool,
    /// True when the sidecar gave layout settings.
    pub layout: bool,
    /// The status from the sidecar's summary, if any.
    pub status: Option<String>,
    /// The rating from the sidecar's summary, if any.
    pub rating: Option<u8>,
}

/// The parts of a sidecar that the session copies into its state.
pub(crate) struct Imported {
    pub position: Option<Location>,
    pub percent: Option<f64>,
    pub layout: Option<TextLayout>,
    pub zoom: Option<ZoomMode>,
    pub annotations: Vec<Annotation>,
    pub status: Option<String>,
    pub rating: Option<u8>,
    pub summary_note: Option<String>,
    /// KOReader's `preferred_dictionaries`, in its order.
    pub preferred_dictionaries: Vec<String>,
    pub report: ImportReport,
}

/// Maps a parsed sidecar table onto Kaia's state. `fixed` is true for
/// fixed-layout books. Entries whose shape does not fit the book are skipped.
pub(crate) fn map_sidecar(root: &Value, fixed: bool, source: &Path) -> Imported {
    let mut skipped = 0usize;
    let mut annotations = Vec::new();
    if let Value::Table(list) = annotation_table(root, fixed) {
        for entry in list.values() {
            match Annotation::from_value(entry) {
                Some(a) if fits(&a, fixed) => annotations.push(a),
                _ => skipped += 1,
            }
        }
    } else {
        for entry in legacy_annotations(root, fixed) {
            match Annotation::from_value(&entry) {
                Some(a) if fits(&a, fixed) => annotations.push(a),
                _ => skipped += 1,
            }
        }
    }

    let position = if fixed {
        root.get("last_page")
            .as_i64()
            .filter(|p| *p >= 1)
            .map(|p| Location::Page((p - 1) as u32))
    } else {
        root.get("last_xpointer")
            .as_str()
            .filter(|s| !s.is_empty())
            .map(|s| Location::Engine(s.to_string()))
    };
    let percent = root
        .get("percent_finished")
        .as_f64()
        .filter(|p| p.is_finite())
        .map(|p| p.clamp(0.0, 1.0));

    let layout = layout_from_sidecar(root);
    let zoom = if fixed { zoom_from_sidecar(root) } else { None };

    let summary = root.get("summary");
    let status = summary.get("status").as_str().map(str::to_string);
    let rating = summary
        .get("rating")
        .as_i64()
        .filter(|r| (0..=5).contains(r))
        .map(|r| r as u8);
    let summary_note = summary.get("note").as_str().map(str::to_string);
    let preferred_dictionaries = match root.get("preferred_dictionaries") {
        Value::Table(list) => list
            .values()
            .filter_map(|v| v.as_str().filter(|n| !n.is_empty()).map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };

    let report = ImportReport {
        source: source.to_path_buf(),
        lossy: false,
        annotations: annotations.len(),
        skipped,
        unresolved: 0,
        position: position.is_some(),
        layout: layout.is_some(),
        status: status.clone(),
        rating,
    };
    Imported {
        position,
        percent,
        layout,
        zoom,
        annotations,
        status,
        rating,
        summary_note,
        preferred_dictionaries,
        report,
    }
}

fn annotation_table(root: &Value, fixed: bool) -> &Value {
    let primary = root.get("annotations");
    let first = match primary {
        Value::Table(t) => t.values().next(),
        _ => None,
    };
    if let Some(first) = first {
        if matches!(first.get("page"), Value::Number(_)) == fixed {
            return primary;
        }
    }
    root.get(if fixed {
        "annotations_paging"
    } else {
        "annotations_rolling"
    })
}

fn legacy_annotations(root: &Value, fixed: bool) -> Vec<Value> {
    let suffix = if fixed { "paging" } else { "rolling" };
    let primary = root.get("bookmarks");
    let first = match primary {
        Value::Table(t) => t.values().next(),
        _ => None,
    };
    let compatible = first.is_some_and(|bm| matches!(bm.get("page"), Value::Number(_)) == fixed);
    let bookmarks = if compatible {
        primary
    } else {
        root.get(format!("bookmarks_{suffix}"))
    };
    let highlights = if compatible {
        root.get("highlight")
    } else {
        root.get(format!("highlight_{suffix}"))
    };
    let mut out = Vec::new();
    if let Value::Table(list) = bookmarks {
        for bm in list.values() {
            let page = bm.get("page");
            if page.is_nil() {
                continue;
            }
            let highlighted = bm.get("highlighted").truthy() && !bm.get("highlighted").is_nil();
            let found = if highlighted {
                find_legacy_highlight(highlights, bm)
            } else {
                None
            };
            let mut item = Table::new();
            for key in ["datetime", "chapter", "pageno", "pageref", "pos0", "pos1"] {
                copy_key(&mut item, bm, key, key);
            }
            item.insert("page".into(), page.clone());
            if highlighted {
                item.insert(
                    "drawer".into(),
                    found
                        .and_then(|h| h.get("drawer").as_str())
                        .unwrap_or("lighten")
                        .into(),
                );
                item.insert(
                    "color".into(),
                    found
                        .and_then(|h| h.get("color").as_str())
                        .unwrap_or("yellow")
                        .into(),
                );
                copy_key(&mut item, bm, "notes", "text");
                if let Some(hl) = found {
                    copy_key(&mut item, hl, "edited", "text_edited");
                    for key in ["pboxes", "ext"] {
                        copy_key(&mut item, hl, key, key);
                    }
                }
                copy_key(&mut item, bm, "text", "note");
            } else {
                copy_key(&mut item, bm, "text", "text");
            }
            out.push(Value::Table(item));
        }
    }
    if root.get("highlights_imported").is_nil() {
        if let Value::Table(pages) = highlights {
            for (page, entries) in pages {
                let Value::Table(entries) = entries else {
                    continue;
                };
                for hl in entries.values() {
                    if hl.get("pos0").is_nil() {
                        continue;
                    }
                    let already = out
                        .iter()
                        .any(|bm| bm.get("pos0") == hl.get("pos0") && bm.get("pos1") == hl.get("pos1"));
                    if already {
                        continue;
                    }
                    let mut item = Table::new();
                    let location = if fixed {
                        match page {
                            Key::Int(n) => Value::from(*n),
                            _ => continue,
                        }
                    } else {
                        hl.get("pos0").clone()
                    };
                    item.insert("page".into(), location);
                    for key in ["datetime", "pos0", "pos1", "pboxes", "ext"] {
                        copy_key(&mut item, hl, key, key);
                    }
                    copy_key(&mut item, hl, "text", "text");
                    copy_key(&mut item, hl, "edited", "text_edited");
                    item.insert(
                        "drawer".into(),
                        hl.get("drawer").as_str().unwrap_or("lighten").into(),
                    );
                    item.insert(
                        "color".into(),
                        hl.get("color").as_str().unwrap_or("yellow").into(),
                    );
                    out.push(Value::Table(item));
                }
            }
        }
    }
    out
}

fn copy_key(target: &mut Table, source: &Value, from: &str, to: &str) {
    let value = source.get(from);
    if !value.is_nil() {
        target.insert(to.into(), value.clone());
    }
}

fn find_legacy_highlight<'a>(highlights: &'a Value, bookmark: &Value) -> Option<&'a Value> {
    let Value::Table(pages) = highlights else {
        return None;
    };
    for entries in pages.values() {
        let Value::Table(entries) = entries else {
            continue;
        };
        for hl in entries.values() {
            if hl.get("pos0") == bookmark.get("pos0") && hl.get("pos1") == bookmark.get("pos1") {
                return Some(hl);
            }
        }
    }
    None
}

/// An annotation fits a book when its positions have the book's kind.
fn fits(a: &Annotation, fixed: bool) -> bool {
    use crate::pos::Pos;
    let page_ok = match a.page {
        Location::Engine(_) => !fixed,
        Location::Page(_) => fixed,
    };
    let pos_ok = |p: &Option<Pos>| match p {
        None => true,
        Some(Pos::Xpointer(_)) => !fixed,
        Some(Pos::Point { .. }) => fixed,
    };
    page_ok && pos_ok(&a.pos0) && pos_ok(&a.pos1)
}

/// Reads the layout keys that KOReader's CREngine options write. Returns None
/// when the sidecar has none of them.
fn layout_from_sidecar(root: &Value) -> Option<TextLayout> {
    let keys = [
        "copt_font_size",
        "font_face",
        "copt_h_page_margins",
        "copt_t_page_margin",
        "copt_b_page_margin",
        "copt_line_spacing",
        "hyphenation",
        "copt_embedded_css",
        "copt_font_base_weight",
    ];
    if !keys.iter().any(|k| !root.get(*k).is_nil()) {
        return None;
    }
    let d = TextLayout::default();
    let num = |v: &Value| v.as_f64().filter(|n| n.is_finite() && *n >= 0.0);
    let margins = root.get("copt_h_page_margins");
    let left = num(margins.get(1i64)).map(|v| v as f32).unwrap_or(d.margins.left);
    let right = num(margins.get(2i64))
        .map(|v| v as f32)
        .unwrap_or(d.margins.right);
    Some(TextLayout {
        font_family: root.get("font_face").as_str().map(str::to_string),
        font_size: root
            .get("copt_font_size")
            .as_f64()
            .filter(|n| *n > 0.0 && n.is_finite())
            .map_or(d.font_size, |n| n as f32),
        margins: Margins {
            top: num(root.get("copt_t_page_margin")).map_or(d.margins.top, |v| v as f32),
            right,
            bottom: num(root.get("copt_b_page_margin")).map_or(d.margins.bottom, |v| v as f32),
            left,
        },
        line_spacing: root
            .get("copt_line_spacing")
            .as_f64()
            .filter(|n| *n > 0.0 && n.is_finite())
            .map_or(d.line_spacing, |n| (n / 100.0) as f32),
        hyphenation: root.get("hyphenation").as_bool().unwrap_or(d.hyphenation),
        // KOReader's justification option is not in the fixtures this was
        // written against, so it is not imported.
        justify: d.justify,
        embedded_styles: root
            .get("copt_embedded_css")
            .as_i64()
            .map_or(d.embedded_styles, |n| n != 0),
        // KOReader stores the delta value itself, as `configurable.font_base_weight`.
        font_weight: root
            .get("copt_font_base_weight")
            .as_f64()
            .filter(|n| n.is_finite())
            .map_or(d.font_weight, |n| n as f32),
    })
}

/// Reads the zoom of a fixed-layout book. Unknown modes become fit-page.
fn zoom_from_sidecar(root: &Value) -> Option<ZoomMode> {
    match root.get("zoom_mode").as_str()? {
        "width" => Some(ZoomMode::FitWidth),
        "free" => root
            .get("zoom_factor")
            .as_f64()
            .filter(|f| f.is_finite() && *f > 0.0)
            .map(|f| ZoomMode::Manual {
                percent: ((f * 100.0).round() as u32).clamp(10, 800),
            })
            .or(Some(ZoomMode::FitPage)),
        _ => Some(ZoomMode::FitPage),
    }
}

/// A parsed sidecar ready for a confirmed import. No engine or source writer is opened.
pub struct PlannedImport {
    state: crate::state::BookState,
    pub report: ImportReport,
    pub highlights: usize,
    pub bookmarks: usize,
    pub notes: usize,
}

impl PlannedImport {
    pub fn prepare(book: &Path, candidates: &[PathBuf]) -> crate::Result<Option<Self>> {
        let Some((root, source, lossy)) = read_first(candidates) else {
            return Ok(None);
        };
        let format = kaia_doc::format::detect(book)?;
        let fixed = matches!(
            format,
            kaia_doc::Format::Pdf
                | kaia_doc::Format::Cbz
                | kaia_doc::Format::Cbr
                | kaia_doc::Format::Xps
                | kaia_doc::Format::Djvu
                | kaia_doc::Format::Svg
        );
        let imported = map_sidecar(&root, fixed, &source);
        let io = |source| crate::BookError::Io {
            path: book.to_owned(),
            source,
        };
        let identity = crate::DocumentId {
            partial_md5: crate::partial_md5(book).map_err(io)?,
            file_size: std::fs::metadata(book).map_err(io)?.len(),
        };
        let mut state = crate::state::BookState::new(identity, imported.layout.unwrap_or_default());
        state.position = imported.position;
        state.percent = imported.percent.unwrap_or(0.0);
        state.zoom = imported.zoom.unwrap_or_default();
        state.status = imported.status;
        state.rating = imported.rating;
        state.summary_note = imported.summary_note;
        state.preferred_dictionaries = imported.preferred_dictionaries;
        state.imported_from = Some(source.display().to_string());
        let mut highlights = 0;
        let mut bookmarks = 0;
        let mut notes = 0;
        for mut annotation in imported.annotations {
            if annotation.drawer.is_some() {
                highlights += 1;
            } else {
                bookmarks += 1;
            }
            if annotation.note.is_some() {
                notes += 1;
            }
            annotation.id = state.assign_id();
            state.annotations.push(annotation);
        }
        let mut report = imported.report;
        report.lossy = lossy;
        Ok(Some(Self {
            state,
            report,
            highlights,
            bookmarks,
            notes,
        }))
    }

    pub fn percent(&self) -> f64 {
        self.state.percent
    }

    /// Keeps the original source name when the parser read a private snapshot.
    pub fn source_path(&mut self, source: PathBuf) {
        self.state.imported_from = Some(source.display().to_string());
        self.report.source = source;
    }

    /// Existing state, including its backup, wins. Repeated imports preserve later edits.
    pub fn import(&self, book: &Path, data: &Path, default_layout: &TextLayout) -> crate::Result<bool> {
        let hash = crate::partial_md5(book).map_err(|source| crate::BookError::Io {
            path: book.to_owned(),
            source,
        })?;
        if hash != self.state.doc.partial_md5
            || std::fs::metadata(book)
                .map_err(|source| crate::BookError::Io {
                    path: book.to_owned(),
                    source,
                })?
                .len()
                != self.state.doc.file_size
        {
            return Err(crate::BookError::Invalid("book changed after the preview".into()));
        }
        let _guard = crate::session::hold_closed_book(book, data, &hash)?
            .ok_or_else(|| crate::BookError::AlreadyOpen(book.to_owned()))?;
        let path = data.join("kaia/books").join(format!("{hash}.json"));
        if path.exists() || path.with_extension("json.old").exists() {
            return Ok(false);
        }
        let mut state = self.state.clone();
        if !self.report.layout {
            state.layout = default_layout.clone();
        }
        crate::state::write_state(&path, &state).map_err(|source| crate::BookError::Io { path, source })?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_follow_koreader_locations() {
        let book = Path::new("/books/84.epub");
        let c = sidecar_candidates(
            book,
            "378df3409d69b6994febc92492dbb19e",
            &[
                SidecarLocation::BesideBook,
                SidecarLocation::DocSettingsDir(PathBuf::from("/kr/docsettings")),
                SidecarLocation::HashDocSettingsDir(PathBuf::from("/kr/hashdocsettings")),
            ],
        );
        assert_eq!(c[0], PathBuf::from("/books/84.sdr/metadata.epub.lua"));
        assert_eq!(c[1], PathBuf::from("/books/84.sdr/metadata.epub.lua.old"));
        assert_eq!(
            c[3],
            PathBuf::from("/kr/docsettings/books/84.sdr/metadata.epub.lua")
        );
        assert_eq!(
            c[5],
            PathBuf::from("/kr/hashdocsettings/37/378df3409d69b6994febc92492dbb19e.sdr/metadata.epub.lua")
        );
        assert_eq!(c.len(), 8);
    }

    #[test]
    fn maps_positions_layout_and_skips_wrong_kind() {
        let src = r#"return {
            ["annotations"] = {
                [1] = { ["chapter"] = "Chapter 1", ["color"] = "yellow", ["datetime"] = "2026-10-12 07:45:00",
                        ["drawer"] = "lighten", ["page"] = "/body/DocFragment[2]/body/div/p[1].0",
                        ["pos0"] = "/body/DocFragment[2]/body/div/p[1].0",
                        ["pos1"] = "/body/DocFragment[2]/body/div/p[1].40", ["text"] = "It is a truth" },
                [2] = { ["datetime"] = "2026-10-05 16:44:10", ["page"] = 7 },
                [3] = { ["page"] = 3, ["pos0"] = { ["page"] = 3, ["x"] = 1.5, ["y"] = 2.0, ["zoom"] = 1.0 } },
            },
            ["last_xpointer"] = "/body/DocFragment[1]/body/div/svg.0",
            ["percent_finished"] = 0.5,
            ["font_face"] = "Noto Serif",
            ["copt_font_size"] = 26,
            ["copt_h_page_margins"] = { [1] = 12, [2] = 14 },
            ["copt_line_spacing"] = 110,
            ["hyphenation"] = false,
            ["copt_font_base_weight"] = 1.5,
            ["summary"] = { ["status"] = "complete", ["rating"] = 5 },
        }"#;
        let root = kaia_luadata::parse(src).expect("parse");
        let imported = map_sidecar(&root, false, Path::new("/x/metadata.epub.lua"));
        assert_eq!(imported.report.annotations, 1);
        assert_eq!(
            imported.report.skipped, 2,
            "a PDF-style point and a page-only entry do not fit an EPUB"
        );
        assert!(matches!(imported.position, Some(Location::Engine(_))));
        assert_eq!(imported.percent, Some(0.5));
        let layout = imported.layout.expect("layout");
        assert_eq!(layout.font_size, 26.0);
        assert_eq!(layout.margins.left, 12.0);
        assert_eq!(layout.margins.right, 14.0);
        assert!((layout.line_spacing - 1.1).abs() < 1e-6);
        assert!(!layout.hyphenation);
        assert_eq!(
            layout.font_weight, 1.5,
            "KOReader's font weight delta is imported"
        );
        assert_eq!(imported.status.as_deref(), Some("complete"));
        assert_eq!(imported.rating, Some(5));
    }

    #[test]
    fn imports_koreader_preferred_dictionaries_in_order() {
        let src = r#"return {
            ["preferred_dictionaries"] = { "Second Dictionary", "", "Test Dictionary" },
            ["annotations"] = {},
        }"#;
        let root = kaia_luadata::parse(src).expect("parse");
        let imported = map_sidecar(&root, false, Path::new("/x/metadata.epub.lua"));
        assert_eq!(
            imported.preferred_dictionaries,
            ["Second Dictionary", "Test Dictionary"],
            "order is kept and empty names are dropped"
        );
        let none = map_sidecar(
            &kaia_luadata::parse(r#"return { ["annotations"] = {} }"#).expect("parse"),
            false,
            Path::new("/x/metadata.epub.lua"),
        );
        assert!(none.preferred_dictionaries.is_empty());
    }

    #[test]
    fn cbr_import_keeps_fixed_page_position_and_zoom() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("comic.cbr");
        let sidecar = dir.path().join("metadata.cbr.lua");
        std::fs::write(&book, b"Rar!\x1a\x07\x00").unwrap();
        std::fs::write(&sidecar, r#"return { last_page = 12, zoom_mode = "width" }"#).unwrap();
        let plan = PlannedImport::prepare(&book, &[sidecar]).unwrap().unwrap();
        assert_eq!(plan.state.position, Some(Location::Page(11)));
        assert_eq!(plan.state.zoom, ZoomMode::FitWidth);
    }

    #[test]
    fn fixed_books_take_last_page() {
        let src = r#"return { ["last_page"] = 12, ["zoom_mode"] = "width", ["annotations"] = {} }"#;
        let root = kaia_luadata::parse(src).expect("parse");
        let imported = map_sidecar(&root, true, Path::new("/x/m.pdf.lua"));
        assert_eq!(imported.position, Some(Location::Page(11)));
        assert_eq!(imported.zoom, Some(ZoomMode::FitWidth));
        assert!(imported.layout.is_none());
    }

    #[test]
    fn non_utf8_main_sidecar_wins_over_old_backup() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("metadata.pdf.lua");
        let old = dir.path().join("metadata.pdf.lua.old");
        std::fs::write(
            &main,
            b"return { [\"summary\"] = { [\"note\"] = \"bad \xff byte\" } }",
        )
        .unwrap();
        std::fs::write(&old, b"return { [\"summary\"] = { [\"note\"] = \"old\" } }").unwrap();
        let (value, source, lossy) = read_first(&[main.clone(), old]).unwrap();
        assert_eq!(source, main);
        assert!(lossy);
        assert!(value
            .get("summary")
            .get("note")
            .as_str()
            .unwrap()
            .contains('\u{fffd}'));
    }

    #[test]
    fn candidate_paths_match_dotted_folders_and_hidden_names() {
        let c = sidecar_candidates(
            Path::new("/books/v1.2/.foo"),
            "abcd",
            &[SidecarLocation::BesideBook],
        );
        assert_eq!(c[0], PathBuf::from("/books/v1.2/.sdr/metadata.foo.lua"));
        assert_eq!(c[2], PathBuf::from("/books/v1.2/.sdr/.foo.lua"));
        let c = sidecar_candidates(
            Path::new("/books/v1.2/plain"),
            "abcd",
            &[SidecarLocation::BesideBook],
        );
        assert_eq!(c[0], PathBuf::from("/books/v1.sdr/metadata.2/plain.lua"));
    }

    #[test]
    fn newest_nonempty_candidate_wins() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old.lua");
        let newest = dir.path().join("new.lua");
        std::fs::write(&old, "return { [\"last_page\"] = 2 }").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&newest, "return { [\"last_page\"] = 3 }").unwrap();
        let (_, source, _) = read_first(&[old.clone(), newest.clone()]).unwrap();
        assert_eq!(source, newest);
        std::fs::write(&newest, "return {}").unwrap();
        let (_, source, _) = read_first(&[old.clone(), newest]).unwrap();
        assert_eq!(source, old);
    }

    #[test]
    fn migrates_old_bookmarks_and_highlights() {
        let src = r#"return { ["bookmarks"] = {
            [1] = { ["page"] = 2, ["datetime"] = "2020-01-01 12:00:00", ["highlighted"] = true,
                ["notes"] = "line one\nline two", ["text"] = "my note",
                ["pos0"] = { ["page"] = 2, ["x"] = 10, ["y"] = 20 },
                ["pos1"] = { ["page"] = 2, ["x"] = 50, ["y"] = 20 } }
        }, ["highlight"] = { [2] = { [1] = {
            ["pos0"] = { ["page"] = 2, ["x"] = 10, ["y"] = 20 },
            ["pos1"] = { ["page"] = 2, ["x"] = 50, ["y"] = 20 },
            ["edited"] = true, ["pboxes"] = { [1] = { ["x"] = 10 } },
            ["ext"] = { ["page"] = 3 }
        } } } }"#;
        let root = kaia_luadata::parse(src).unwrap();
        let imported = map_sidecar(&root, true, Path::new("/metadata.pdf.lua"));
        assert_eq!(imported.report.annotations, 1);
        let a = &imported.annotations[0];
        assert_eq!(a.text.as_deref(), Some("line one\nline two"));
        assert_eq!(a.note.as_deref(), Some("my note"));
        assert_eq!(a.text_edited, Some(true));
        assert!(a.pboxes.is_some());
        assert!(a.ext.is_some());
    }

    #[test]
    fn imports_compatible_annotation_backup() {
        let src = r#"return { ["annotations"] = {
            [1] = { ["page"] = 3, ["datetime"] = "2020-01-01 12:00:00" }
        }, ["annotations_rolling"] = {
            [1] = { ["page"] = "/body/DocFragment[2]/body/p[1].0",
                ["datetime"] = "2020-01-02 12:00:00" }
        } }"#;
        let root = kaia_luadata::parse(src).unwrap();
        let imported = map_sidecar(&root, false, Path::new("/metadata.epub.lua"));
        assert_eq!(imported.report.annotations, 1);
        assert_eq!(imported.annotations[0].datetime, "2020-01-02 12:00:00");
    }
}
