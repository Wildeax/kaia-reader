//! Fixed-layout PNG/JPEG comics in RAR archives. Only the current decoded page
//! is cached; archive members are never extracted to disk.

use std::cell::{Ref, RefCell};
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use kaia_archive_sys::{Entry, Reader};
use kaia_gfx::image::{decode_rgb, scale_pixmap, DecodeLimits};
use kaia_gfx::{Color, PixelFormat, Pixmap, Rect, Size};

use crate::{
    DocError, Document, Layout, Link, Location, Metadata, PageSize, PageText, Quad, Result, TocEntry,
};

pub struct CbrDocument {
    path: PathBuf,
    pages: Vec<(usize, Entry)>,
    cache: RefCell<Option<(u32, Pixmap)>>,
}

fn engine(error: impl std::fmt::Display) -> DocError {
    DocError::Engine(format!("CBR: {error}"))
}

impl CbrDocument {
    pub fn open(path: &Path) -> Result<Self> {
        let mut reader = Reader::open(path).map_err(engine)?;
        let mut pages = Vec::new();
        let mut index = 0;
        while let Some(entry) = reader.next_entry().map_err(engine)? {
            if entry.is_file && is_image(&entry.name) {
                pages.push((index, entry));
            }
            index += 1;
        }
        if pages.is_empty() {
            return Err(engine("archive contains no PNG or JPEG pages"));
        }
        // Stable sorting preserves archive order for names with equal numeric
        // values, such as 01.png and 1.png. MuPDF's CBZ comparison ignores case.
        pages.sort_by(|a, b| natural_cmp(&a.1.name, &b.1.name));
        Ok(Self {
            path: path.to_owned(),
            pages,
            cache: RefCell::new(None),
        })
    }

    fn check(&self, page: u32) -> Result<&(usize, Entry)> {
        self.pages.get(page as usize).ok_or(DocError::PageOutOfRange {
            page,
            count: self.pages.len(),
        })
    }

    fn pixels(&self, page: u32) -> Result<Ref<'_, Pixmap>> {
        let (index, expected) = self.check(page)?;
        if self.cache.borrow().as_ref().is_none_or(|cached| cached.0 != page) {
            // Drop the old page before allocating the next one.
            self.cache.borrow_mut().take();
            let mut reader = Reader::open(&self.path).map_err(engine)?;
            for current in 0..=*index {
                let entry = reader
                    .next_entry()
                    .map_err(engine)?
                    .ok_or_else(|| engine("archive changed after opening"))?;
                if current == *index {
                    if &entry != expected {
                        return Err(engine("archive changed after opening"));
                    }
                    let data = reader.read_entry().map_err(engine)?;
                    let pixels = decode_rgb(&data, DecodeLimits::default()).map_err(engine)?;
                    *self.cache.borrow_mut() = Some((page, pixels));
                }
            }
        }
        Ok(Ref::map(self.cache.borrow(), |cache| {
            &cache.as_ref().expect("page loaded").1
        }))
    }

    fn render_inner(&self, page: u32, scale: f32, format: PixelFormat, clip: Option<Rect>) -> Result<Pixmap> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(DocError::InvalidArgument(format!("scale {scale}")));
        }
        let pixels = self.pixels(page)?;
        let width = (pixels.width() as f64 * scale as f64).round().max(1.0);
        let height = (pixels.height() as f64 * scale as f64).round().max(1.0);
        // Bound the intermediate resize too, even when the requested clip is small.
        if width > 16384.0 || height > 16384.0 || width * height > 16_777_216.0 {
            return Err(DocError::InvalidArgument(
                "CBR render exceeds 16 megapixels or 16384 pixels per side".into(),
            ));
        }
        let size = Size::new(width as u32, height as u32);
        let clip = clip.unwrap_or(Rect::from_size(size));
        if clip.x < 0
            || clip.y < 0
            || clip.w == 0
            || clip.h == 0
            || clip.x as u64 + clip.w as u64 > size.w as u64
            || clip.y as u64 + clip.h as u64 > size.h as u64
        {
            return Err(DocError::InvalidArgument("clip is outside the page".into()));
        }
        let scaled = scale_pixmap(&pixels, size);
        let mut result = Pixmap::new(clip.w, clip.h, format);
        for row in 0..clip.h as usize {
            let start = (row + clip.y as usize) * scaled.stride() + clip.x as usize * 3;
            let source = &scaled.data()[start..start + clip.w as usize * 3];
            let stride = result.stride();
            let target = &mut result.data_mut()[row * stride..(row + 1) * stride];
            match format {
                PixelFormat::Rgb24 => target.copy_from_slice(source),
                PixelFormat::Gray8 => {
                    for (dst, rgb) in target.iter_mut().zip(source.as_chunks::<3>().0.iter()) {
                        *dst = Color::rgb(rgb[0], rgb[1], rgb[2]).luma();
                    }
                }
            }
        }
        Ok(result)
    }
}

impl Document for CbrDocument {
    fn page_count(&self) -> usize {
        self.pages.len()
    }
    fn page_size(&self, page: u32) -> Result<PageSize> {
        let pixels = self.pixels(page)?;
        Ok(PageSize {
            width: pixels.width() as f32,
            height: pixels.height() as f32,
        })
    }
    fn render(&self, page: u32, scale: f32, format: PixelFormat) -> Result<Pixmap> {
        self.render_inner(page, scale, format, None)
    }
    fn render_clip(&self, page: u32, scale: f32, format: PixelFormat, clip: Rect) -> Result<Pixmap> {
        self.render_inner(page, scale, format, Some(clip))
    }
    fn toc(&self) -> Vec<TocEntry> {
        Vec::new()
    }
    fn metadata(&self) -> Metadata {
        Metadata::default()
    }
    fn page_text(&self, page: u32) -> Result<PageText> {
        self.check(page)?;
        Ok(PageText::default())
    }
    fn search(&self, page: u32, _needle: &str) -> Result<Vec<Quad>> {
        self.check(page)?;
        Ok(Vec::new())
    }
    fn links(&self, page: u32) -> Result<Vec<Link>> {
        self.check(page)?;
        Ok(Vec::new())
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
        let size = self.page_size(0).ok()?;
        let scale = (max_size.w as f32 / size.width).min(max_size.h as f32 / size.height);
        self.render(0, scale, PixelFormat::Rgb24).ok()
    }
    fn resolve(&self, location: &Location) -> Result<u32> {
        match location {
            Location::Page(page) => {
                self.check(*page)?;
                Ok(*page)
            }
            Location::Engine(value) => Err(DocError::UnresolvedLocation(value.clone())),
        }
    }
}

fn is_image(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg"))
}

/// MuPDF's CBZ ordering, comparing arbitrarily long digit runs without integer
/// overflow. ASCII case and leading zeroes do not affect the comparison.
fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    while !a.is_empty() && !b.is_empty() {
        let order = if a[0].is_ascii_digit() && b[0].is_ascii_digit() {
            let an = a.iter().take_while(|ch| ch.is_ascii_digit()).count();
            let bn = b.iter().take_while(|ch| ch.is_ascii_digit()).count();
            let aa = &a[..an];
            let bb = &b[..bn];
            let aa = &aa[aa.iter().take_while(|ch| **ch == b'0').count()..];
            let bb = &bb[bb.iter().take_while(|ch| **ch == b'0').count()..];
            let order = aa.len().cmp(&bb.len()).then_with(|| aa.cmp(bb));
            a = &a[an..];
            b = &b[bn..];
            order
        } else {
            let order = a[0].to_ascii_uppercase().cmp(&b[0].to_ascii_uppercase());
            a = &a[1..];
            b = &b[1..];
            order
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn natural_order_matches_cbz_without_integer_overflow() {
        let mut names = [
            "page10.png",
            "page002.PNG",
            "page1.png",
            "page999999999999999999999.png",
            "page11.png",
        ];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            names,
            [
                "page1.png",
                "page002.PNG",
                "page10.png",
                "page11.png",
                "page999999999999999999999.png"
            ]
        );
        assert_eq!(natural_cmp("Page01.PNG", "page1.png"), Ordering::Equal);
    }
}
