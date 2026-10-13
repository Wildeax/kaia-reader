//! Text shaping, line breaking and rendering on top of `cosmic-text`.
//!
//! Fonts come only from the directories the caller passes in. The system font
//! database is never read, so output does not depend on what the device or
//! desktop has installed. Shaping and bidi run through cosmic-text, so mixed
//! scripts and right-to-left text work.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cosmic_text::{
    Attrs, Buffer, Color as CtColor, Fallback, Family, Metrics, Shaping, Style, SwashCache, Weight, Wrap,
};
use unicode_script::Script;

use crate::geom::Point;
use crate::pixmap::{Color, Pixmap};

/// File name prefixes of the Noto fonts the UI uses by default. Arabic needs
/// `NotoNaskhArabic-`, which is not in this list.
pub const NOTO_FONT_PREFIXES: &[&str] = &["NotoSans-", "NotoSerif-"];

/// Errors while loading fonts.
#[derive(Debug)]
pub enum TextError {
    /// A font directory or font file could not be read.
    Io { path: PathBuf, source: io::Error },
    /// No font file matched the directories and prefixes.
    NoFonts,
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextError::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
            TextError::NoFonts => write!(f, "no font files found"),
        }
    }
}

impl std::error::Error for TextError {}

/// A font set and the shaping state for it. Create one per process and share it.
pub struct FontSystem {
    inner: cosmic_text::FontSystem,
    swash: SwashCache,
}

impl fmt::Debug for FontSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontSystem").finish_non_exhaustive()
    }
}

impl FontSystem {
    /// Loads the `.ttf` and `.otf` files in `dirs` whose file names start with
    /// one of `prefixes`. An empty `prefixes` loads every font file. Files are
    /// loaded in sorted order, so the result does not depend on directory order.
    pub fn load<P: AsRef<Path>>(dirs: &[P], prefixes: &[&str]) -> Result<Self, TextError> {
        let mut files = Vec::new();
        for dir in dirs {
            let dir = dir.as_ref();
            let entries = fs::read_dir(dir).map_err(|source| TextError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
            for entry in entries {
                let path = entry
                    .map_err(|source| TextError::Io {
                        path: dir.to_path_buf(),
                        source,
                    })?
                    .path();
                if is_font_file(&path) && matches_prefix(&path, prefixes) {
                    files.push(path);
                }
            }
        }
        files.sort();

        let mut db = cosmic_text::fontdb::Database::new();
        for path in &files {
            let data = fs::read(path).map_err(|source| TextError::Io {
                path: path.clone(),
                source,
            })?;
            db.load_font_data(data);
        }
        if db.is_empty() {
            return Err(TextError::NoFonts);
        }
        db.set_sans_serif_family("Noto Sans");
        db.set_serif_family("Noto Serif");

        let inner = cosmic_text::FontSystem::new_with_locale_and_db_and_fallback(
            "en-US".to_string(),
            db,
            NotoFallback,
        );
        Ok(Self {
            inner,
            swash: SwashCache::new(),
        })
    }

    /// Breaks `text` into lines no wider than `max_width` pixels.
    ///
    /// Words wrap at spaces. A word wider than the box breaks between glyphs.
    /// With `max_lines` set, the text after the last allowed line is dropped and
    /// `truncated` is true. With `ellipsis`, "…" ends the last line and must fit
    /// on it.
    pub fn layout(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: u32,
        max_lines: Option<usize>,
        ellipsis: bool,
    ) -> TextLayout {
        let full = self.shape(text, style, max_width);
        let count = full.layout_runs().count();
        let Some(max) = max_lines.filter(|&m| count > m) else {
            return TextLayout::new(full, false);
        };
        if max == 0 {
            // An empty buffer still has one line, so the lines are cleared by hand.
            let mut layout = TextLayout::new(self.shape("", style, max_width), true);
            layout.lines.clear();
            layout.width = 0;
            layout.height = 0;
            return layout;
        }

        // Keep the longest prefix whose text plus the ellipsis fits in `max` lines.
        // Prefixes end on character boundaries and are found by bisection.
        // `k` counts prefixes: 0 is the empty prefix and `k` > 0 ends at ends[k - 1].
        let suffix = if ellipsis { "…" } else { "" };
        let ends: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .filter(|&i| i > 0)
            .chain(std::iter::once(text.len()))
            .collect();
        let end_of = |k: usize| if k == 0 { 0 } else { ends[k - 1] };
        let candidate = |k: usize| format!("{}{suffix}", text[..end_of(k)].trim_end());
        let (mut lo, mut hi) = (0usize, ends.len() + 1);
        // Invariant: prefix `lo` fits (the empty prefix always does) and prefix `hi` does not.
        while hi - lo > 1 {
            let mid = (lo + hi) / 2;
            if self
                .shape(&candidate(mid), style, max_width)
                .layout_runs()
                .count()
                <= max
            {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let buffer = self.shape(&candidate(lo), style, max_width);
        TextLayout::new(buffer, true)
    }

    /// Draws a layout onto `pm`. `origin` is the top-left corner of the layout
    /// box. The text color comes from the style used in [`FontSystem::layout`].
    pub fn draw_layout(&mut self, pm: &mut Pixmap, layout: &TextLayout, origin: Point) {
        let (ox, oy) = (origin.x, origin.y);
        layout.buffer.draw(
            &mut self.inner,
            &mut self.swash,
            CtColor::rgb(0, 0, 0),
            |x, y, w, h, c| {
                let alpha = c.a();
                if alpha == 0 {
                    return;
                }
                let color = Color::rgb(c.r(), c.g(), c.b());
                for py in y..y + h as i32 {
                    for px in x..x + w as i32 {
                        crate::draw::blend_pixel(pm, ox + px, oy + py, color, alpha);
                    }
                }
            },
        );
    }

    fn shape(&mut self, text: &str, style: &TextStyle, max_width: u32) -> Buffer {
        let metrics = Metrics::new(style.size_px, style.line_height);
        let mut buffer = Buffer::new(&mut self.inner, metrics);
        buffer.set_size(&mut self.inner, Some(max_width as f32), None);
        buffer.set_wrap(&mut self.inner, Wrap::WordOrGlyph);
        let attrs = attrs_for(style);
        buffer.set_text(&mut self.inner, text, &attrs, Shaping::Advanced, None);
        buffer
    }
}

fn is_font_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("ttf") || e.eq_ignore_ascii_case("otf"))
}

fn matches_prefix(path: &Path, prefixes: &[&str]) -> bool {
    if prefixes.is_empty() {
        return true;
    }
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    prefixes.iter().any(|p| name.starts_with(p))
}

fn attrs_for(style: &TextStyle) -> Attrs<'_> {
    let family = match style.family.as_str() {
        "serif" => Family::Serif,
        "sans-serif" | "sans" => Family::SansSerif,
        "monospace" => Family::Monospace,
        name => Family::Name(name),
    };
    let c = style.color;
    Attrs::new()
        .family(family)
        .weight(Weight(style.weight))
        .style(if style.italic {
            Style::Italic
        } else {
            Style::Normal
        })
        .color(CtColor::rgb(c.r, c.g, c.b))
        .metrics(Metrics::new(style.size_px, style.line_height))
}

/// Script fallback for the Noto families. Families that are not loaded are
/// skipped by cosmic-text.
struct NotoFallback;

impl Fallback for NotoFallback {
    fn common_fallback(&self) -> &[&'static str] {
        &["Noto Sans", "Noto Serif"]
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }

    fn script_fallback(&self, script: Script, _locale: &str) -> &[&'static str] {
        match script {
            Script::Arabic => &["Noto Naskh Arabic", "Noto Sans Arabic UI", "Noto Sans Arabic"],
            Script::Bengali => &["Noto Sans Bengali UI"],
            Script::Devanagari => &["Noto Sans Devanagari UI"],
            _ => &[],
        }
    }
}

/// How text looks. Sizes and line height are in pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// Family name as stored in the font file, or `serif`, `sans-serif` or `monospace`.
    pub family: String,
    pub size_px: f32,
    /// CSS-style weight from 100 to 900. 400 is regular and 700 is bold.
    pub weight: u16,
    pub italic: bool,
    pub color: Color,
    /// Distance from one baseline to the next, in pixels.
    pub line_height: f32,
}

impl TextStyle {
    /// Noto Sans, regular, black, with a line height of 1.25 times the size.
    pub fn new(size_px: f32) -> Self {
        Self {
            family: "Noto Sans".to_string(),
            size_px,
            weight: 400,
            italic: false,
            color: Color::BLACK,
            line_height: size_px * 1.25,
        }
    }
}

/// One line of a laid out text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineInfo {
    /// Advance width of the line in pixels.
    pub width: f32,
    /// Distance from the top of the layout box to the top of the line.
    pub top: f32,
    /// Height of the line in pixels.
    pub height: f32,
    /// True when the paragraph direction is right to left.
    pub rtl: bool,
}

/// The result of [`FontSystem::layout`]. Draw it with [`FontSystem::draw_layout`].
#[derive(Clone)]
pub struct TextLayout {
    /// Lines from top to bottom.
    pub lines: Vec<LineInfo>,
    /// The widest line, rounded up to whole pixels.
    pub width: u32,
    /// Height of all lines, rounded up to whole pixels.
    pub height: u32,
    /// True when `max_lines` cut some of the text.
    pub truncated: bool,
    buffer: Buffer,
}

impl fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextLayout")
            .field("lines", &self.lines)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("truncated", &self.truncated)
            .finish()
    }
}

impl TextLayout {
    fn new(buffer: Buffer, truncated: bool) -> Self {
        let lines: Vec<LineInfo> = buffer
            .layout_runs()
            .map(|run| LineInfo {
                width: run.line_w,
                top: run.line_top,
                height: run.line_height,
                rtl: run.rtl,
            })
            .collect();
        let width = lines.iter().fold(0.0_f32, |m, l| m.max(l.width));
        let height = lines.last().map_or(0.0, |l| l.top + l.height);
        Self {
            width: width.ceil() as u32,
            height: height.ceil() as u32,
            lines,
            truncated,
            buffer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixmap::PixelFormat;

    const FONT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/fonts/noto");

    fn fonts() -> FontSystem {
        FontSystem::load(&[FONT_DIR], &["NotoSans-", "NotoSerif-", "NotoNaskhArabic-"])
            .expect("Noto fonts in resources/fonts/noto")
    }

    const ENGLISH: &str = "The quick brown fox jumps over the lazy dog while the reader turns \
        the page, notes a passage, and keeps going until the chapter ends.";

    #[test]
    fn missing_directory_is_an_error() {
        let err = FontSystem::load(&["/definitely/not/a/kaia/font/dir"], &[]).unwrap_err();
        assert!(matches!(err, TextError::Io { .. }));
    }

    #[test]
    fn prefixes_select_files() {
        let err = FontSystem::load(&[FONT_DIR], &["NoSuchFamily-"]).unwrap_err();
        assert!(matches!(err, TextError::NoFonts));
    }

    #[test]
    fn short_text_is_one_line() {
        let mut fs = fonts();
        let style = TextStyle::new(32.0);
        let l = fs.layout("Hello", &style, 500, None, false);
        assert_eq!(l.lines.len(), 1);
        assert!(!l.truncated);
        assert!(l.width > 20 && l.width < 200, "width {}", l.width);
        assert_eq!(l.height, 40);
    }

    #[test]
    fn long_text_wraps_inside_width() {
        let mut fs = fonts();
        let style = TextStyle::new(28.0);
        let l = fs.layout(ENGLISH, &style, 300, None, false);
        assert!(l.lines.len() > 2, "{} lines", l.lines.len());
        for line in &l.lines {
            assert!(line.width <= 300.5, "line width {}", line.width);
        }
        assert!(l.lines.iter().all(|line| !line.rtl));
    }

    #[test]
    fn max_lines_truncates() {
        let mut fs = fonts();
        let style = TextStyle::new(28.0);
        let l = fs.layout(ENGLISH, &style, 300, Some(2), true);
        assert!(l.truncated);
        assert_eq!(l.lines.len(), 2);
        assert!(l.lines.iter().all(|line| line.width <= 300.5));
        let kept = l.buffer.lines[0].text();
        assert!(kept.ends_with('…'), "ellipsis ends the text, got {kept:?}");
        assert!(kept.len() < ENGLISH.len(), "text is cut");

        let no_mark = fs.layout(ENGLISH, &style, 300, Some(2), false);
        assert!(no_mark.truncated);
        assert!(!no_mark.buffer.lines[0].text().ends_with('…'));
    }

    #[test]
    fn zero_lines_is_empty_and_truncated() {
        let mut fs = fonts();
        let l = fs.layout(ENGLISH, &TextStyle::new(28.0), 300, Some(0), true);
        assert!(l.truncated);
        assert!(l.lines.is_empty());
        assert_eq!(l.height, 0);
    }

    #[test]
    fn long_word_breaks_only_when_needed() {
        let mut fs = fonts();
        let style = TextStyle::new(28.0);
        let word = "Antidisestablishmentarianism";
        let l = fs.layout(word, &style, 120, None, false);
        assert!(l.lines.len() > 1, "word split into {} lines", l.lines.len());
        assert!(l.lines.iter().all(|line| line.width <= 120.5));

        let short = fs.layout("Cat dog", &style, 400, None, false);
        assert_eq!(short.lines.len(), 1, "short words do not break");
    }

    #[test]
    fn arabic_is_right_to_left_and_mixed_text_shapes() {
        let mut fs = fonts();
        let style = TextStyle::new(32.0);
        let ar = fs.layout("مرحبا بالعالم", &style, 400, None, false);
        assert_eq!(ar.lines.len(), 1);
        assert!(ar.lines[0].rtl);
        assert!(ar.width > 40);

        let mixed = fs.layout("Read the كتاب today", &style, 400, None, false);
        assert_eq!(mixed.lines.len(), 1);
        assert!(mixed.width > 0);
    }

    #[test]
    fn draw_layout_marks_pixels() {
        let mut fs = fonts();
        let style = TextStyle::new(32.0);
        let layout = fs.layout("Ink", &style, 200, None, false);
        let mut pm = Pixmap::new(layout.width + 4, layout.height + 4, PixelFormat::Gray8);
        fs.draw_layout(&mut pm, &layout, Point::new(2, 2));
        let dark = pm.data().iter().filter(|&&v| v < 128).count();
        let partial = pm.data().iter().filter(|&&v| v > 0 && v < 255).count();
        assert!(dark > 20, "dark pixels {dark}");
        assert!(partial > 0, "anti-aliased edges exist");
    }
}
