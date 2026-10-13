//! Document backend over KOReader's MuPDF build (through `kaia-mupdf-sys`).
//!
//! MuPDF opens PDF, EPUB, CBZ, XPS, FB2, MOBI and HTML. It lays out EPUB, FB2,
//! MOBI and HTML as reflowable pages. Layout takes a page size, a font size and
//! user CSS, so the other `Layout` fields become CSS rules.

use std::path::Path;

use kaia_gfx::{PixelFormat, Pixmap, Rect, Size};
use kaia_mupdf_sys::{Document as Raw, StextPage};

use crate::document::Document;
use crate::error::{DocError, Result};
use crate::format::Format;
use crate::types::{
    Layout, Link, LinkTarget, Location, Margins, Metadata, PageSize, PageText, PointF, Quad, RectF, TextChar,
    TextLine, TocEntry,
};

/// Hits returned by one search call.
const SEARCH_MAX_HITS: u32 = 500;

/// An open MuPDF document. It owns its MuPDF context.
pub struct MupdfDocument {
    raw: Raw,
    format: Format,
}

impl MupdfDocument {
    /// Opens a file with the given format. The format decides the handler.
    pub fn open(path: &Path, format: Format) -> Result<Self> {
        let raw = Raw::open_as(path, format.magic())?;
        Ok(Self { raw, format })
    }

    pub fn format(&self) -> Format {
        self.format
    }

    fn count(&self) -> Result<u32> {
        Ok(self.raw.page_count()?)
    }

    /// Fails with `PageOutOfRange` when `page` is not in the document.
    fn check_page(&self, page: u32) -> Result<()> {
        let count = self.count()?;
        if page >= count {
            return Err(DocError::PageOutOfRange {
                page,
                count: count as usize,
            });
        }
        Ok(())
    }

    fn render_inner(
        &self,
        page: u32,
        scale: f32,
        format: PixelFormat,
        clip: Option<[i32; 4]>,
    ) -> Result<Pixmap> {
        self.check_page(page)?;
        if !(scale.is_finite() && scale > 0.0) {
            return Err(DocError::InvalidArgument(format!("scale {scale}")));
        }
        let gray = format == PixelFormat::Gray8;
        let px = self.raw.render(page, scale, gray, clip)?;
        Pixmap::from_data(px.width, px.height, format, px.data)
            .ok_or_else(|| DocError::Engine("rendered buffer does not match its size".to_string()))
    }
}

impl Document for MupdfDocument {
    fn page_count(&self) -> usize {
        self.raw.page_count().map(|n| n as usize).unwrap_or(0)
    }

    fn page_size(&self, page: u32) -> Result<PageSize> {
        self.check_page(page)?;
        let b = self.raw.page_bounds(page)?;
        let r = rect(b);
        Ok(PageSize {
            width: r.width(),
            height: r.height(),
        })
    }

    fn render(&self, page: u32, scale: f32, format: PixelFormat) -> Result<Pixmap> {
        self.render_inner(page, scale, format, None)
    }

    fn render_clip(&self, page: u32, scale: f32, format: PixelFormat, clip: Rect) -> Result<Pixmap> {
        let area = [clip.x, clip.y, clip.right(), clip.bottom()];
        self.render_inner(page, scale, format, Some(area))
    }

    fn toc(&self) -> Vec<TocEntry> {
        self.raw
            .toc()
            .unwrap_or_default()
            .into_iter()
            .map(|t| TocEntry {
                xpointer: None,
                level: t.level,
                title: t.title,
                page: t.page,
            })
            .collect()
    }

    fn metadata(&self) -> Metadata {
        let get = |key: &str| {
            self.raw
                .metadata(key)
                .ok()
                .flatten()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        Metadata {
            title: get("info:Title"),
            author: get("info:Author"),
        }
    }

    fn page_text(&self, page: u32) -> Result<PageText> {
        self.check_page(page)?;
        let stext = self.raw.stext(page)?;
        Ok(page_text_from(&stext))
    }

    fn search(&self, page: u32, needle: &str) -> Result<Vec<Quad>> {
        self.check_page(page)?;
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let hits = self.raw.search(page, needle, SEARCH_MAX_HITS)?;
        Ok(hits.into_iter().map(quad_from).collect())
    }

    fn links(&self, page: u32) -> Result<Vec<Link>> {
        self.check_page(page)?;
        let links = self.raw.links(page)?;
        Ok(links
            .into_iter()
            .map(|l| Link {
                bbox: rect(l.bbox),
                target: match l.page {
                    Some(p) => LinkTarget::Page(p),
                    None => LinkTarget::Uri(l.uri),
                },
            })
            .collect())
    }

    fn is_reflowable(&self) -> bool {
        self.raw.is_reflowable().unwrap_or(false)
    }

    fn set_layout(&mut self, layout: &Layout) -> Result<()> {
        let finite_positive = |v: f32| v.is_finite() && v > 0.0;
        if !(finite_positive(layout.width) && finite_positive(layout.height) && finite_positive(layout.em)) {
            return Err(DocError::InvalidArgument(
                "layout width, height and em must be positive".to_string(),
            ));
        }
        if !self.is_reflowable() {
            return Err(DocError::NotReflowable);
        }
        // The user CSS must be set before the layout runs.
        self.raw.set_user_css(&layout_css(layout))?;
        self.raw.layout(layout.width, layout.height, layout.em)?;
        Ok(())
    }

    fn cover(&self, max_size: Size) -> Option<Pixmap> {
        if max_size.w == 0 || max_size.h == 0 || self.page_count() == 0 {
            return None;
        }
        let size = self.page_size(0).ok()?;
        if !(size.width > 0.0 && size.height > 0.0) {
            return None;
        }
        let scale = (max_size.w as f32 / size.width).min(max_size.h as f32 / size.height);
        if !(scale.is_finite() && scale > 0.0) {
            return None;
        }
        self.render(0, scale, PixelFormat::Rgb24).ok()
    }

    fn resolve(&self, location: &Location) -> Result<u32> {
        match location {
            Location::Page(p) => {
                self.check_page(*p)?;
                Ok(*p)
            }
            Location::Engine(uri) => match self.raw.resolve_uri(uri)? {
                Some(p) => Ok(p),
                None => Err(DocError::UnresolvedLocation(uri.clone())),
            },
        }
    }
}

fn rect(b: [f32; 4]) -> RectF {
    RectF::new(b[0], b[1], b[2], b[3])
}

fn quad_from(q: [f32; 8]) -> Quad {
    Quad {
        ul: PointF { x: q[0], y: q[1] },
        ur: PointF { x: q[2], y: q[3] },
        ll: PointF { x: q[4], y: q[5] },
        lr: PointF { x: q[6], y: q[7] },
    }
}

/// Flattens structured text. Only text blocks contribute characters. A newline
/// follows each line in `text`.
pub(crate) fn page_text_from(stext: &StextPage) -> PageText {
    let mut out = PageText::default();
    for block in stext.blocks.iter().filter(|b| b.kind == 0) {
        for line in span(&stext.lines, block.first_line, block.line_count) {
            let first_char = out.chars.len();
            for c in span(&stext.chars, line.first_char, line.char_count) {
                let ch = char::from_u32(c.unicode).unwrap_or('\u{fffd}');
                out.text.push(ch);
                out.chars.push(TextChar {
                    ch,
                    bbox: rect(c.bbox),
                });
            }
            out.lines.push(TextLine {
                bbox: rect(line.bbox),
                first_char,
                char_count: out.chars.len() - first_char,
            });
            out.text.push('\n');
        }
    }
    out
}

/// The `count` items from `start`, clipped to the slice.
fn span<T>(items: &[T], start: usize, count: usize) -> &[T] {
    let s = start.min(items.len());
    let e = start.saturating_add(count).min(items.len());
    &items[s..e]
}

/// Builds the user stylesheet for a layout. Empty settings produce no rule.
pub(crate) fn layout_css(layout: &Layout) -> String {
    let mut css = String::new();
    if let Some(Margins {
        top,
        right,
        bottom,
        left,
    }) = layout.margins
    {
        css.push_str(&format!(
            "@page {{ margin: {top}pt {right}pt {bottom}pt {left}pt; }}\n"
        ));
    }
    let mut body = Vec::new();
    if let Some(lh) = layout.line_height {
        body.push(format!("line-height: {lh};"));
    }
    if let Some(family) = &layout.font_family {
        body.push(format!("font-family: \"{}\";", escape_css_string(family)));
    }
    if !body.is_empty() {
        css.push_str(&format!("body {{ {} }}\n", body.join(" ")));
    }
    css.push_str(&layout.css_extra);
    css
}

fn escape_css_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' | '\r' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_mupdf_sys::{StextBlock, StextChar, StextLine};

    #[test]
    fn default_layout_writes_no_rules() {
        let layout = Layout::new(1264.0, 1680.0, 24.0);
        assert_eq!(layout_css(&layout), "");
    }

    #[test]
    fn layout_fields_become_css() {
        let layout = Layout {
            margins: Some(Margins {
                top: 10.0,
                right: 12.0,
                bottom: 14.0,
                left: 16.0,
            }),
            line_height: Some(1.4),
            font_family: Some("Liberation \"Serif\"".to_string()),
            css_extra: "p { text-indent: 0 }".to_string(),
            ..Layout::new(1264.0, 1680.0, 24.0)
        };
        let css = layout_css(&layout);
        assert!(css.contains("@page { margin: 10pt 12pt 14pt 16pt; }"));
        assert!(css.contains("body { line-height: 1.4; font-family: \"Liberation \\\"Serif\\\"\"; }"));
        assert!(css.ends_with("p { text-indent: 0 }"));
    }

    #[test]
    fn page_text_follows_blocks_and_lines() {
        let bb = [0.0, 0.0, 1.0, 1.0];
        let stext = StextPage {
            blocks: vec![
                StextBlock {
                    kind: 1,
                    bbox: bb,
                    first_line: 0,
                    line_count: 0,
                },
                StextBlock {
                    kind: 0,
                    bbox: bb,
                    first_line: 0,
                    line_count: 2,
                },
            ],
            lines: vec![
                StextLine {
                    bbox: bb,
                    first_char: 0,
                    char_count: 2,
                },
                StextLine {
                    bbox: bb,
                    first_char: 2,
                    char_count: 1,
                },
            ],
            chars: vec![
                StextChar {
                    unicode: 'a' as u32,
                    bbox: bb,
                },
                StextChar {
                    unicode: 'b' as u32,
                    bbox: bb,
                },
                StextChar {
                    unicode: 'c' as u32,
                    bbox: bb,
                },
            ],
        };
        let text = page_text_from(&stext);
        assert_eq!(text.text, "ab\nc\n");
        assert_eq!(text.chars.len(), 3);
        assert_eq!(text.lines[1].first_char, 2);
        assert_eq!(text.lines[1].char_count, 1);
    }

    #[test]
    fn quad_bounds_cover_all_corners() {
        let q = quad_from([0.0, 2.0, 5.0, 1.0, -1.0, 4.0, 3.0, 6.0]);
        let b = q.bounds();
        assert_eq!(b, RectF::new(-1.0, 1.0, 5.0, 6.0));
    }
}
