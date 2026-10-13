//! Shared sticker storage and placement. Document reads run on the reader worker.
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kaia_book::{BookSession, Pos, Selection, TextLayout};
use kaia_doc::{cre::CreDocument, Document, PageText, RectF};
use kaia_gfx::{image, AlphaPixmap, Point, Rect, Size};
use kaia_storage::{Anchor, DocumentId, Offset, Placement, Sticker, StickerState, Store, TextQuote};

pub const BUNDLED: [(&str, &str); 4] = [
    ("star", "Star"),
    ("heart", "Heart"),
    ("question", "Question"),
    ("check", "Check"),
];

pub(super) fn bundled_path(services: &crate::services::Services, name: &str) -> Option<PathBuf> {
    BUNDLED.iter().any(|(id, _)| *id == name).then(|| {
        crate::services::resources_dir(&services.platform)
            .join("kaia/stickers")
            .join(format!("{name}.png"))
    })
}

pub(super) fn store(session: &BookSession) -> Result<Store, String> {
    let dir = session
        .state_path()
        .parent()
        .and_then(Path::parent)
        .ok_or("No data folder.")?;
    Store::open(dir).map_err(|e| e.to_string())
}

pub(super) fn identity(session: &BookSession) -> DocumentId {
    let doc = session.identity();
    DocumentId {
        partial_md5: doc.partial_md5.clone(),
        file_size: doc.file_size,
        title: session.metadata().title.clone(),
    }
}

#[derive(Clone)]
pub struct Drawing {
    pub id: String,
    pub rect: Rect,
    pub anchor_line: Rect,
    pub image: Option<Arc<AlphaPixmap>>,
    pub text: Option<String>,
    pub marker: bool,
    pub faded: bool,
}

impl Drawing {
    pub(super) fn hit_rect(&self, minimum: u32) -> Rect {
        let w = self.rect.w.max(minimum);
        let h = self.rect.h.max(minimum);
        Rect::new(
            self.rect.x - (w - self.rect.w) as i32 / 2,
            self.rect.y - (h - self.rect.h) as i32 / 2,
            w,
            h,
        )
    }

    pub(super) fn paint(&self, cx: &mut kaia_ui::widget::PaintCx) {
        use kaia_ui::Tone;
        if let Some(image) = &self.image {
            cx.blit_alpha(image, Point::new(self.rect.x, self.rect.y));
        } else {
            let ink = cx.color(Tone::Ink);
            let paper = cx.color(Tone::SurfaceAlt);
            cx.fill_rect(self.rect, paper);
            cx.border(self.rect, 4, 1, ink);
            if self.marker {
                for row in 1..=3 {
                    cx.fill_rect(
                        Rect::new(
                            self.rect.x + 4,
                            self.rect.y + row * self.rect.h as i32 / 4,
                            self.rect.w.saturating_sub(8),
                            2,
                        ),
                        ink,
                    );
                }
            } else {
                let style = cx.theme.style(&cx.theme.font.caption, Tone::Ink);
                let text = cx.fonts.layout(
                    self.text.as_deref().unwrap_or("Sticker"),
                    &style,
                    self.rect.w.saturating_sub(8),
                    Some(3),
                    true,
                );
                cx.text(&text, Point::new(self.rect.x + 4, self.rect.y + 4));
            }
        }
    }
}

struct TextPage {
    text: String,
    positions: Vec<RectF>,
    column_right: f32,
    lines: Vec<RectF>,
}

impl TextPage {
    fn new(page: PageText) -> Self {
        let mut text = String::new();
        let mut positions = Vec::new();
        let mut space = false;
        for line in &page.lines {
            if !text.is_empty() && !space {
                text.push(' ');
                positions.push(line.bbox);
                space = true;
            }
            for ch in &page.chars[line.first_char..line.first_char + line.char_count] {
                if ch.ch.is_whitespace() {
                    if !space && !text.is_empty() {
                        text.push(' ');
                        positions.push(ch.bbox);
                    }
                    space = true;
                } else {
                    text.push(ch.ch);
                    positions.push(ch.bbox);
                    space = false;
                }
            }
        }
        if text.ends_with(' ') {
            text.pop();
            positions.pop();
        }
        Self {
            text,
            positions,
            column_right: page.lines.iter().map(|l| l.bbox.x1).fold(0.0, f32::max),
            lines: page.lines.iter().map(|l| l.bbox).collect(),
        }
    }
}

struct TextCache {
    path: PathBuf,
    modified: Option<std::time::SystemTime>,
    layout: TextLayout,
    viewport: Size,
    doc: CreDocument,
    pages: HashMap<u32, TextPage>,
    located: HashMap<String, (Anchor, Option<(u32, RectF)>)>,
}
thread_local! {
    static TEXT: RefCell<Option<TextCache>> = const { RefCell::new(None) };
}

impl TextCache {
    fn new(session: &BookSession, viewport: Size) -> Result<Self, String> {
        let mut doc = CreDocument::open(session.path()).map_err(|e| e.to_string())?;
        let l = session.layout();
        doc.set_hyphenation(l.hyphenation.then_some(kaia_book::layout::HYPHENATION_DICT))
            .map_err(|e| e.to_string())?;
        doc.set_embedded_styles(l.embedded_styles)
            .map_err(|e| e.to_string())?;
        doc.set_layout(&kaia_doc::Layout {
            width: viewport.w as f32,
            height: viewport.h as f32,
            em: l.font_size,
            margins: Some(l.margins),
            line_height: Some(l.line_spacing),
            font_family: l.font_family.clone(),
            css_extra: if l.justify {
                "body { text-align: justify !important; }\n".into()
            } else {
                String::new()
            },
        })
        .map_err(|e| e.to_string())?;
        Ok(Self {
            path: session.path().into(),
            modified: std::fs::metadata(session.path())
                .ok()
                .and_then(|m| m.modified().ok()),
            layout: l.clone(),
            viewport,
            doc,
            pages: HashMap::new(),
            located: HashMap::new(),
        })
    }

    fn page(&mut self, page: u32) -> Result<&TextPage, String> {
        if !self.pages.contains_key(&page) {
            self.pages.insert(
                page,
                TextPage::new(self.doc.page_text(page).map_err(|e| e.to_string())?),
            );
        }
        Ok(&self.pages[&page])
    }

    fn selection_anchor(&mut self, selection: &Selection) -> Result<Anchor, String> {
        let Pos::Xpointer(locator) = &selection.pos0 else {
            return Err("This text engine has no sticker positions.".into());
        };
        let exact: String = normalize(&selection.text).chars().take(1000).collect();
        let page = self.page(selection.page)?;
        let target = selection
            .page_rects
            .first()
            .ok_or("The selection has no text box.")?;
        let chars: Vec<char> = page.text.chars().collect();
        let n = exact.chars().count();
        let candidates = chars
            .windows(n.max(1))
            .enumerate()
            .filter(|(_, c)| c.iter().collect::<String>() == exact);
        let start = candidates
            .min_by(|(a, _), (b, _)| {
                distance(page.positions[*a], *target).total_cmp(&distance(page.positions[*b], *target))
            })
            .map(|(i, _)| i)
            .ok_or("The selected text could not be located.")?;
        let quote = TextQuote {
            exact,
            prefix: chars[start.saturating_sub(32)..start].iter().collect(),
            suffix: chars[start + n..(start + n + 32).min(chars.len())]
                .iter()
                .collect(),
        };
        Ok(Anchor::Reflowable {
            locator: locator.clone(),
            quote,
            offset: Offset { dx: 0.0, dy: 0.0 },
            placement: Placement::Margin,
        })
    }

    fn locate(&mut self, sticker: &Sticker) -> Result<Option<(u32, RectF)>, String> {
        let id = sticker.id.as_deref().unwrap_or("");
        if let Some((anchor, found)) = self.located.get(id) {
            if anchor == &sticker.anchor {
                return Ok(*found);
            }
        }
        let Anchor::Reflowable { locator, quote, .. } = &sticker.anchor else {
            return Ok(None);
        };
        let exact = normalize(&quote.exact);
        let mut found = None;
        if let Ok(Some(page)) = self.doc.xpointer_page(locator) {
            if let Ok(boxes) = self.doc.char_boxes(page, locator, None) {
                let tuples: Vec<_> = boxes
                    .iter()
                    .map(|b| {
                        (
                            b.ch,
                            RectF::new(b.rect[0], b.rect[1], b.rect[2], b.rect[3]),
                            b.line,
                        )
                    })
                    .collect();
                let text = kaia_book::text::page_text_from_boxes(&tuples).text;
                if normalize(&text).starts_with(&exact) {
                    if let Some(b) = boxes.first() {
                        found = Some((page, RectF::new(b.rect[0], b.rect[1], b.rect[2], b.rect[3])));
                    }
                }
            }
        }
        if found.is_none() {
            // Join page text so a quote may cross a page after a font change.
            let mut text = String::new();
            let mut positions = Vec::new();
            for p in 0..self.doc.page_count() as u32 {
                let page = self.page(p)?;
                if !text.is_empty() {
                    text.push(' ');
                    positions.push(None);
                }
                text.push_str(&page.text);
                positions.extend(page.positions.iter().map(|r| Some((p, *r))));
            }
            // Lua stores trimmed context. Its full-context search adds spaces
            // between the prefix, quote and suffix before trying a bare quote.
            let prefix = normalize(&quote.prefix);
            let suffix = normalize(&quote.suffix);
            let full = normalize(&format!("{prefix} {exact} {suffix}"));
            let full_match = kaia_storage::quote_locate(&text, &full, "", "").map(|i| {
                i + if prefix.is_empty() {
                    0
                } else {
                    prefix.chars().count() + 1
                }
            });
            let index = full_match
                .or_else(|| kaia_storage::quote_locate(&text, &exact, &quote.prefix, &quote.suffix));
            if let Some(index) = index {
                found = positions.get(index).copied().flatten();
            }
        }
        self.located.insert(id.into(), (sticker.anchor.clone(), found));
        Ok(found)
    }
}
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn distance(a: RectF, b: RectF) -> f32 {
    (a.x0 - b.x0).abs() + (a.y0 - b.y0).abs() * 10.0
}

pub(super) fn frame(
    session: &mut BookSession,
    selection: Option<&Selection>,
    viewport: Size,
) -> Result<(Vec<Drawing>, Option<Anchor>), String> {
    let mut store = store(session)?;
    let doc = identity(session);
    store
        .document_seen(&doc.partial_md5, doc.file_size, doc.title.as_deref())
        .map_err(|e| e.to_string())?;
    let stickers = store.list_stickers(&doc.partial_md5).map_err(|e| e.to_string())?;
    let transform = session.page_transform(viewport).map_err(|e| e.to_string())?;
    let mut anchor = None;
    let mut drawings = Vec::new();
    TEXT.with(|cell| -> Result<(), String> {
        let mut cache = cell.borrow_mut();
        let needs_text = session.is_reflowable()
            && (selection.is_some() || stickers.iter().any(|s| s.state == StickerState::Attached));
        if needs_text {
            if !session.uses_cre() {
                return Err("Stickers need CREngine for this book.".into());
            }
            if cache.as_ref().is_none_or(|c| {
                c.path != session.path()
                    || c.modified
                        != std::fs::metadata(session.path())
                            .ok()
                            .and_then(|m| m.modified().ok())
                    || c.layout != *session.layout()
                    || c.viewport != viewport
            }) {
                *cache = Some(TextCache::new(session, viewport)?);
            }
        }
        if let Some(sel) = selection {
            anchor = Some(if session.is_reflowable() {
                cache.as_mut().unwrap().selection_anchor(sel)?
            } else {
                let r = sel.page_rects.first().ok_or("The selection has no text box.")?;
                Anchor::Fixed {
                    page: sel.page + 1,
                    x: (r.x0 / transform.page.width).clamp(0.0, 1.0),
                    y: (r.y0 / transform.page.height).clamp(0.0, 1.0),
                }
            });
        }
        for s in stickers.iter().filter(|s| s.state == StickerState::Attached) {
            let (line, column, page_size) = match &s.anchor {
                Anchor::Reflowable { .. } => {
                    let c = cache.as_mut().ok_or("No text document.")?;
                    let Some((p, rect)) = c.locate(s)? else {
                        store
                            .set_state(s.id.as_deref().unwrap_or(""), StickerState::NeedsReview)
                            .map_err(|e| e.to_string())?;
                        continue;
                    };
                    if p != session.current().page {
                        continue;
                    }
                    let page = c.page(p)?;
                    let mid = (rect.y0 + rect.y1) / 2.0;
                    let line = page
                        .lines
                        .iter()
                        .find(|r| r.y0 <= mid && r.y1 >= mid)
                        .copied()
                        .unwrap_or(rect);
                    (transform.to_screen(line), page.column_right, viewport)
                }
                Anchor::Fixed { page, x, y } if *page == session.current().page + 1 => {
                    let origin = Point::new(
                        (viewport.w as i32 - transform.image.w as i32) / 2,
                        (viewport.h as i32 - transform.image.h as i32) / 2,
                    );
                    (
                        Rect::new(
                            origin.x + (*x * transform.image.w as f32) as i32,
                            origin.y + (*y * transform.image.h as f32) as i32,
                            1,
                            1,
                        ),
                        0.0,
                        transform.image,
                    )
                }
                Anchor::Fixed { page, .. } if *page > session.page_count() as u32 => {
                    store
                        .set_state(s.id.as_deref().unwrap_or(""), StickerState::NeedsReview)
                        .map_err(|e| e.to_string())?;
                    continue;
                }
                _ => continue,
            };
            let drawing = drawing(&store, s, line, column, page_size, viewport)?;
            if !drawing.rect.intersect(&Rect::from_size(viewport)).is_empty() {
                drawings.push(drawing);
            }
        }
        Ok(())
    })?;
    Ok((drawings, anchor))
}

fn drawing(
    store: &Store,
    sticker: &Sticker,
    line: Rect,
    right: f32,
    page: Size,
    viewport: Size,
) -> Result<Drawing, String> {
    let scale = viewport.w.min(viewport.h) as f32 / 600.0;
    let gap = (4.0 * scale).ceil();
    let full_w = (sticker.size.w * page.w as f32).max(1.0);
    let full_h = (sticker.size.h * page.h as f32).max(1.0);
    let mut w = full_w;
    let mut h = full_h;
    let mut x = line.x as f32;
    let mut y = line.y as f32;
    let mut faded = false;
    let mut marker = false;
    if let Anchor::Reflowable {
        offset, placement, ..
    } = sticker.anchor
    {
        let avail = (viewport.w as f32 - right - 2.0 * gap).max(0.0);
        if sticker.asset.is_some() && placement == Placement::Inline {
            x += offset.dx * line.h as f32;
            y += (1.0 + offset.dy) * line.h as f32 - h;
        } else {
            if sticker.asset.is_none() && avail < 80.0 * scale {
                marker = true;
                w = avail.clamp(16.0 * scale, 28.0 * scale);
                h = w;
                x = right + gap;
            } else if sticker.asset.is_some() && avail < 22.0 * scale {
                faded = true;
                x = line.x as f32 + line.w as f32 - w;
            } else {
                w = full_w.min(avail);
                h = if sticker.asset.is_some() {
                    full_h * w / full_w
                } else {
                    90.0 * scale
                };
                x = right + gap;
            }
            y += line.h as f32 / 2.0 - h / 2.0 + offset.dy * line.h as f32;
        }
    } else if sticker.asset.is_none() {
        marker = true;
        w = 22.0 * scale;
        h = w;
        if x >= w + gap {
            x -= w + gap;
        } else {
            y -= h + gap;
        }
    }
    let mut rect = Rect::new(
        x as i32,
        y as i32,
        w.round().max(1.0) as u32,
        h.round().max(1.0) as u32,
    );
    if matches!(sticker.anchor, Anchor::Reflowable { .. }) {
        rect.x = rect.x.clamp(0, viewport.w.saturating_sub(rect.w) as i32);
        rect.y = rect.y.clamp(0, viewport.h.saturating_sub(rect.h) as i32);
    }
    let image = sticker
        .asset
        .as_ref()
        .map(|sha| asset_image(store, sha, rect.size(), sticker.rotation, faded))
        .transpose()?;
    if let Some(image) = &image {
        rect.x += (rect.w as i32 - image.width() as i32) / 2;
        rect.y += (rect.h as i32 - image.height() as i32) / 2;
        rect.w = image.width();
        rect.h = image.height();
    }
    Ok(Drawing {
        id: sticker.id.clone().unwrap_or_default(),
        rect,
        anchor_line: line,
        image,
        text: sticker.text.clone(),
        marker,
        faded,
    })
}

#[derive(Hash, PartialEq, Eq)]
struct ImageKey {
    store: PathBuf,
    sha: String,
    width: u32,
    height: u32,
    rotation: i16,
    faded: bool,
}
thread_local! {
    static IMAGES: RefCell<HashMap<ImageKey, Arc<AlphaPixmap>>> = RefCell::new(HashMap::new());
}
fn asset_image(
    store: &Store,
    sha: &str,
    size: Size,
    rotation: i16,
    faded: bool,
) -> Result<Arc<AlphaPixmap>, String> {
    let key = ImageKey {
        store: store.dir().into(),
        sha: sha.into(),
        width: size.w,
        height: size.h,
        rotation,
        faded,
    };
    IMAGES.with(|cell| {
        let mut images = cell.borrow_mut();
        if let Some(image) = images.get(&key) {
            return Ok(image.clone());
        }
        let bytes =
            std::fs::read(store.asset_path(sha).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let image = image::decode_rgba(
            &bytes,
            image::DecodeLimits {
                max_width: 2048,
                max_height: 2048,
                max_pixels: 2048 * 2048,
            },
        )
        .map_err(|e| e.to_string())?;
        let mut image = rotate(image::scale_rgba(&image, size), rotation);
        if faded {
            for p in image.data_mut().as_chunks_mut::<4>().0 {
                p[3] = (u16::from(p[3]) * 102 / 255) as u8;
            }
        }
        let image = Arc::new(image);
        if images.len() >= 32 {
            images.clear();
        }
        images.insert(key, image.clone());
        Ok(image)
    })
}

fn rotate(image: AlphaPixmap, degrees: i16) -> AlphaPixmap {
    if degrees == 0 {
        return image;
    }
    let (sin, cos) = (f32::from(degrees).to_radians()).sin_cos();
    let (w, h) = (image.width() as f32, image.height() as f32);
    let mut out = AlphaPixmap::new(
        (w * cos.abs() + h * sin.abs()).round() as u32,
        (w * sin.abs() + h * cos.abs()).round() as u32,
    );
    for y in 0..out.height() {
        for x in 0..out.width() {
            let dx = x as f32 + 0.5 - out.width() as f32 / 2.0;
            let dy = y as f32 + 0.5 - out.height() as f32 / 2.0;
            let sx = (cos * dx + sin * dy + w / 2.0).floor() as i32;
            let sy = (-sin * dx + cos * dy + h / 2.0).floor() as i32;
            if let Some(pixel) = image.get(sx, sy) {
                out.put(x as i32, y as i32, pixel);
            }
        }
    }
    out
}
