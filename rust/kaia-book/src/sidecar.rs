//! Import of KOReader's sidecar files (`metadata.<ext>.lua`). Kaia reads them
//! through `kaia-luadata` and never writes them.
//!
//! The sidecar is looked up where KOReader keeps it. Each lookup location
//! corresponds to one value of KOReader's `document_metadata_folder` setting.

use std::path::{Path, PathBuf};

use kaia_doc::{Location, Margins};
use kaia_events::Value;

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
}

/// The sidecar files to try for a book, in order. Each location yields the
/// main file, then its `.old` backup.
pub fn sidecar_candidates(book: &Path, partial_md5: &str, locations: &[SidecarLocation]) -> Vec<PathBuf> {
    let file_name = book
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stem, ext) = match file_name.rfind('.') {
        Some(i) if i > 0 => (file_name[..i].to_string(), file_name[i + 1..].to_string()),
        _ => (file_name.clone(), "_".to_string()),
    };
    let sidecar_name = format!("metadata.{ext}.lua");
    let mut dirs: Vec<PathBuf> = Vec::new();
    for loc in locations {
        match loc {
            SidecarLocation::BesideBook => {
                let parent = book.parent().unwrap_or(Path::new(""));
                dirs.push(parent.join(format!("{stem}.sdr")));
            }
            SidecarLocation::DocSettingsDir(root) => {
                let parent = book.parent().unwrap_or(Path::new(""));
                let rel = parent.strip_prefix("/").unwrap_or(parent);
                dirs.push(root.join(rel).join(format!("{stem}.sdr")));
            }
            SidecarLocation::HashDocSettingsDir(root) => {
                if partial_md5.len() >= 2 {
                    dirs.push(root.join(&partial_md5[..2]).join(format!("{partial_md5}.sdr")));
                }
            }
        }
    }
    let mut out = Vec::new();
    for dir in dirs {
        let main = dir.join(&sidecar_name);
        out.push(main.clone());
        out.push(with_suffix(&main, ".old"));
    }
    out
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Reads the first candidate that parses as a Lua table. Returns the table,
/// source file and whether UTF-8 conversion replaced any bytes.
pub(crate) fn read_first(candidates: &[PathBuf]) -> Option<(Value, PathBuf, bool)> {
    for path in candidates {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let src = String::from_utf8_lossy(&bytes);
        if let Ok(v @ Value::Table(_)) = kaia_luadata::parse(&src) {
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
    pub report: ImportReport,
}

/// Maps a parsed sidecar table onto Kaia's state. `fixed` is true for
/// fixed-layout books. Entries whose shape does not fit the book are skipped.
pub(crate) fn map_sidecar(root: &Value, fixed: bool, source: &Path) -> Imported {
    let mut skipped = 0usize;
    let mut annotations = Vec::new();
    if let Value::Table(list) = root.get("annotations") {
        for entry in list.values() {
            match Annotation::from_value(entry) {
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
        report,
    }
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
            c[2],
            PathBuf::from("/kr/docsettings/books/84.sdr/metadata.epub.lua")
        );
        assert_eq!(
            c[4],
            PathBuf::from("/kr/hashdocsettings/37/378df3409d69b6994febc92492dbb19e.sdr/metadata.epub.lua")
        );
        assert_eq!(c.len(), 6);
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
        assert_eq!(imported.status.as_deref(), Some("complete"));
        assert_eq!(imported.rating, Some(5));
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
}
