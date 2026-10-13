//! Fixed layout DjVu documents through DjVuLibre.

use std::path::Path;

use kaia_djvu_sys::{Document as Raw, Sexp};
use kaia_gfx::{PixelFormat, Pixmap, Rect, Size};

use crate::document::Document;
use crate::error::{DocError, Result};
use crate::types::{
    Layout, Link, LinkTarget, Location, Metadata, PageSize, PageText, PointF, Quad, RectF, TextChar,
    TextLine, TocEntry,
};

pub struct DjvuDocument {
    raw: Raw,
}
impl DjvuDocument {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            raw: Raw::open(path).map_err(engine)?,
        })
    }
    fn info(&self, page: u32) -> Result<kaia_djvu_sys::Info> {
        self.check(page)?;
        self.raw.page_info(page).map_err(engine)
    }
    fn check(&self, page: u32) -> Result<()> {
        if page >= self.raw.page_count() {
            Err(DocError::PageOutOfRange {
                page,
                count: self.raw.page_count() as usize,
            })
        } else {
            Ok(())
        }
    }
    fn render_inner(&self, page: u32, scale: f32, format: PixelFormat, clip: Option<Rect>) -> Result<Pixmap> {
        let info = self.info(page)?;
        if !scale.is_finite() || scale <= 0.0 {
            return Err(DocError::InvalidArgument(format!("scale {scale}")));
        }
        let factor = scale * 72.0 / info.dpi as f32;
        let width = (info.width as f32 * factor).round();
        let height = (info.height as f32 * factor).round();
        if !width.is_finite()
            || !height.is_finite()
            || width < 1.0
            || height < 1.0
            || width > i32::MAX as f32
            || height > i32::MAX as f32
        {
            return Err(DocError::InvalidArgument("render dimensions are invalid".into()));
        }
        let (width, height) = (width as u32, height as u32);
        let area = if let Some(rect) = clip {
            if rect.x < 0 || rect.y < 0 || rect.w == 0 || rect.h == 0 {
                return Err(DocError::InvalidArgument("clip is outside the page".into()));
            }
            [rect.x as u32, rect.y as u32, rect.w, rect.h]
        } else {
            [0, 0, width, height]
        };
        let bytes = self
            .raw
            .render(page, width, height, format == PixelFormat::Gray8, area)
            .map_err(engine)?;
        Pixmap::from_data(area[2], area[3], format, bytes)
            .ok_or_else(|| DocError::Engine("DjVu pixel buffer size mismatch".into()))
    }
}
impl Document for DjvuDocument {
    fn page_count(&self) -> usize {
        self.raw.page_count() as usize
    }
    fn page_size(&self, page: u32) -> Result<PageSize> {
        let info = self.info(page)?;
        Ok(PageSize {
            width: info.width as f32 * 72.0 / info.dpi as f32,
            height: info.height as f32 * 72.0 / info.dpi as f32,
        })
    }
    fn render(&self, page: u32, scale: f32, format: PixelFormat) -> Result<Pixmap> {
        self.render_inner(page, scale, format, None)
    }
    fn render_clip(&self, page: u32, scale: f32, format: PixelFormat, clip: Rect) -> Result<Pixmap> {
        self.render_inner(page, scale, format, Some(clip))
    }
    fn toc(&self) -> Vec<TocEntry> {
        let mut entries = Vec::new();
        if let Ok(outline) = self.raw.outline() {
            walk_outline(&outline, 0, &mut entries);
        }
        entries
    }
    fn metadata(&self) -> Metadata {
        let mut metadata = Metadata::default();
        if let Ok(pairs) = self.raw.metadata() {
            for (key, value) in pairs {
                match key.to_ascii_lowercase().as_str() {
                    "title" => metadata.title = Some(value),
                    "author" => metadata.author = Some(value),
                    _ => {}
                }
            }
        }
        metadata
    }
    fn page_text(&self, page: u32) -> Result<PageText> {
        let info = self.info(page)?;
        let expr = self.raw.page_text(page).map_err(engine)?;
        let mut out = PageText::default();
        collect_text(&expr, info, &mut out);
        Ok(out)
    }
    fn search(&self, page: u32, needle: &str) -> Result<Vec<Quad>> {
        let text = self.page_text(page)?;
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let mut hits = Vec::new();
        for (offset, _) in text.text.match_indices(needle) {
            let first = text.text[..offset].chars().filter(|ch| *ch != '\n').count();
            let count = needle.chars().filter(|ch| *ch != '\n').count();
            if count == 0 || first + count > text.chars.len() {
                continue;
            }
            let b = text.chars[first..first + count].iter().fold(
                RectF::new(f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
                |acc, ch| {
                    RectF::new(
                        acc.x0.min(ch.bbox.x0),
                        acc.y0.min(ch.bbox.y0),
                        acc.x1.max(ch.bbox.x1),
                        acc.y1.max(ch.bbox.y1),
                    )
                },
            );
            hits.push(quad(b));
        }
        Ok(hits)
    }
    fn links(&self, page: u32) -> Result<Vec<Link>> {
        let info = self.info(page)?;
        let mut links = Vec::new();
        for area in self.raw.page_links(page).map_err(engine)? {
            if let Sexp::List(items) = area {
                if items.len() < 4 || symbol(&items[0]) != Some("maparea") {
                    continue;
                }
                let target = match &items[1] {
                    Sexp::String(uri) => link_target(uri),
                    Sexp::List(dest) => dest
                        .iter()
                        .find_map(string)
                        .map(link_target)
                        .unwrap_or_else(|| LinkTarget::Uri(String::new())),
                    _ => continue,
                };
                if let Some(bbox) = link_rect(&items[3], info) {
                    links.push(Link { bbox, target });
                }
            }
        }
        Ok(links)
    }
    fn is_reflowable(&self) -> bool {
        false
    }
    fn set_layout(&mut self, _layout: &Layout) -> Result<()> {
        Err(DocError::NotReflowable)
    }
    fn cover(&self, max_size: Size) -> Option<Pixmap> {
        if max_size.w == 0 || max_size.h == 0 {
            return None;
        }
        let page = self.page_size(0).ok()?;
        let scale = (max_size.w as f32 / page.width).min(max_size.h as f32 / page.height);
        self.render(0, scale, PixelFormat::Rgb24).ok()
    }
    fn resolve(&self, location: &Location) -> Result<u32> {
        match location {
            Location::Page(page) => {
                self.check(*page)?;
                Ok(*page)
            }
            Location::Engine(uri) => match link_target(uri) {
                LinkTarget::Page(page) => {
                    self.check(page)?;
                    Ok(page)
                }
                _ => Err(DocError::UnresolvedLocation(uri.clone())),
            },
        }
    }
}
fn engine(error: kaia_djvu_sys::Error) -> DocError {
    DocError::Engine(error.to_string())
}
fn symbol(expr: &Sexp) -> Option<&str> {
    if let Sexp::Symbol(s) = expr {
        Some(s)
    } else {
        None
    }
}
fn string(expr: &Sexp) -> Option<&str> {
    if let Sexp::String(s) = expr {
        Some(s)
    } else {
        None
    }
}
fn number(expr: &Sexp) -> Option<f32> {
    if let Sexp::Number(n) = expr {
        Some(*n as f32)
    } else {
        None
    }
}
fn bbox(items: &[Sexp], info: kaia_djvu_sys::Info) -> Option<RectF> {
    if items.len() < 5 {
        return None;
    }
    let scale = 72.0 / info.dpi as f32;
    Some(RectF::new(
        number(&items[1])? * scale,
        (info.height as f32 - number(&items[4])?) * scale,
        number(&items[3])? * scale,
        (info.height as f32 - number(&items[2])?) * scale,
    ))
}
fn collect_text(expr: &Sexp, info: kaia_djvu_sys::Info, out: &mut PageText) {
    let Sexp::List(items) = expr else {
        return;
    };
    if let Some(kind) = items.first().and_then(symbol) {
        if kind == "word" {
            if let Some(box_) = bbox(items, info) {
                if let Some(value) = items.get(5).and_then(string) {
                    if !out.text.is_empty() && !out.text.ends_with(['\n', ' ']) {
                        out.text.push(' ');
                        out.chars.push(TextChar { ch: ' ', bbox: box_ });
                    }
                    let count = value.chars().count();
                    for (index, ch) in value.chars().enumerate() {
                        let x0 = box_.x0 + box_.width() * index as f32 / count as f32;
                        let x1 = box_.x0 + box_.width() * (index + 1) as f32 / count as f32;
                        out.chars.push(TextChar {
                            ch,
                            bbox: RectF::new(x0, box_.y0, x1, box_.y1),
                        });
                    }
                    out.text.push_str(value);
                }
            }
            return;
        }
        if kind == "line" {
            if !out.text.is_empty() && !out.text.ends_with('\n') {
                out.text.push('\n');
            }
            let first_char = out.chars.len();
            for child in items.iter().skip(5) {
                collect_text(child, info, out);
            }
            if out.chars.len() > first_char {
                if let Some(box_) = bbox(items, info) {
                    out.lines.push(TextLine {
                        bbox: box_,
                        first_char,
                        char_count: out.chars.len() - first_char,
                    });
                }
            }
            return;
        }
    }
    for child in items.iter().skip(5) {
        collect_text(child, info, out);
    }
}
fn walk_outline(expr: &Sexp, level: u32, out: &mut Vec<TocEntry>) {
    let Sexp::List(items) = expr else {
        return;
    };
    if level == 0 {
        for item in items.iter().skip(1) {
            walk_outline(item, 1, out);
        }
    } else {
        if items.len() < 2 {
            return;
        }
        let title = string(&items[0]).unwrap_or("").to_string();
        let page = string(&items[1]).and_then(|uri| match link_target(uri) {
            LinkTarget::Page(page) => Some(page),
            _ => None,
        });
        out.push(TocEntry {
            level,
            title,
            page,
            xpointer: None,
        });
        for item in items.iter().skip(2) {
            walk_outline(item, level + 1, out);
        }
    }
}
fn link_target(uri: &str) -> LinkTarget {
    if let Some(number) = uri
        .strip_prefix('#')
        .and_then(|s| {
            s.trim_start_matches(|ch: char| !ch.is_ascii_digit())
                .parse::<u32>()
                .ok()
        })
        .and_then(|n| n.checked_sub(1))
    {
        LinkTarget::Page(number)
    } else {
        LinkTarget::Uri(uri.to_string())
    }
}
fn link_rect(expr: &Sexp, info: kaia_djvu_sys::Info) -> Option<RectF> {
    let Sexp::List(items) = expr else {
        return None;
    };
    if items.len() < 5 || symbol(&items[0]) != Some("rect") {
        return None;
    }
    let scale = 72.0 / info.dpi as f32;
    let x = number(&items[1])?;
    let y = number(&items[2])?;
    let w = number(&items[3])?;
    let h = number(&items[4])?;
    Some(RectF::new(
        x * scale,
        (info.height as f32 - y - h) * scale,
        (x + w) * scale,
        (info.height as f32 - y) * scale,
    ))
}
fn quad(b: RectF) -> Quad {
    Quad {
        ul: PointF { x: b.x0, y: b.y0 },
        ur: PointF { x: b.x1, y: b.y0 },
        ll: PointF { x: b.x0, y: b.y1 },
        lr: PointF { x: b.x1, y: b.y1 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_gfx::png::write_pixmap;

    fn sample() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/djvu3spec.djvu")
    }
    #[test]
    fn sample_opens_and_renders() {
        let doc = crate::open(&sample()).unwrap();
        assert_eq!(doc.page_count(), 71);
        let size = doc.page_size(0).unwrap();
        assert!(size.width > 0.0 && size.height > 0.0);
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/kaia-djvu-test");
        std::fs::create_dir_all(&dir).unwrap();
        for (format, name) in [
            (PixelFormat::Gray8, "page1-gray.png"),
            (PixelFormat::Rgb24, "page1-rgb.png"),
        ] {
            let image = doc.render(0, 0.5, format).unwrap();
            assert!(image.data().iter().any(|byte| *byte < 240));
            write_pixmap(&image, &dir.join(name)).unwrap();
            let clip = doc
                .render_clip(0, 0.5, format, Rect::new(0, 0, 100, 100))
                .unwrap();
            assert_eq!((clip.width(), clip.height()), (100, 100));
        }
        let toc = doc.toc();
        assert!(toc
            .iter()
            .any(|entry| entry.title == "1 Introduction" && entry.page == Some(0)));
        let _metadata = doc.metadata();
        let _links = doc.links(0).unwrap();
        let text = doc.page_text(0).unwrap();
        assert!(text.text.contains("Lizardtech DjVu Reference"));
        let word = text.text.split_whitespace().find(|word| word.len() > 3).unwrap();
        let hits = doc.search(0, word).unwrap();
        assert!(!hits.is_empty());
        let page = RectF::new(0.0, 0.0, size.width, size.height);
        assert!(page.contains_rect(&hits[0].bounds()));
    }
    #[test]
    fn corrupt_file_returns_error() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/kaia-djvu-test/corrupt.djvu");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"AT&TFORMbad data").unwrap();
        assert!(DjvuDocument::open(&path).is_err());
    }
}
