//! Fixed-layout PNG/JPEG comics in RAR archives. Archive members are never
//! extracted to disk. Pages come from one forward pass over the archive: the
//! reader keeps its position, so a later page continues the stream, and a
//! bounded cache holds the compressed bytes of recently read pages.

use std::cell::{Ref, RefCell};
use std::cmp::Ordering;
use std::collections::VecDeque;
use std::io;
use std::path::Path;

use kaia_archive_sys::{Entry, Reader, MAX_ENTRY_BYTES};
use kaia_gfx::image::{decode_rgb, scale_pixmap, DecodeLimits};
use kaia_gfx::{Color, PixelFormat, Pixmap, Rect, Size};

use crate::{
    DocError, Document, Layout, Link, Location, Metadata, PageSize, PageText, Quad, Result, TocEntry,
};

/// Compressed page bytes kept between requests. One archive entry is at most
/// MAX_ENTRY_BYTES, so any single page fits in an empty cache.
const PAGE_CACHE_BYTES: usize = MAX_ENTRY_BYTES as usize;

pub struct CbrDocument {
    /// Image entries in page order, with their archive positions.
    pages: Vec<(usize, Entry)>,
    state: RefCell<State>,
}

struct State {
    archive: Walker<Reader>,
    /// The page most recently decoded, so repeated renders skip the decoder.
    decoded: Option<(u32, Pixmap)>,
}

fn engine(error: impl std::fmt::Display) -> DocError {
    DocError::Engine(format!("CBR: {error}"))
}

impl CbrDocument {
    pub fn open(path: &Path) -> Result<Self> {
        let path = path.to_owned();
        let mut archive: Walker<Reader> =
            Walker::new(Box::new(move || Reader::open(&path)), PAGE_CACHE_BYTES);
        // Opening reads every entry once to validate the archive. The cache keeps
        // the last pages that fit, so the final page needs no second pass.
        let mut pages = archive.scan().map_err(engine)?;
        if pages.is_empty() {
            return Err(engine("archive contains no PNG or JPEG pages"));
        }
        // Stable sorting preserves archive order for names with equal numeric
        // values, such as 01.png and 1.png. MuPDF's CBZ comparison ignores case.
        pages.sort_by(|a, b| natural_cmp(&a.1.name, &b.1.name));
        Ok(Self {
            pages,
            state: RefCell::new(State {
                archive,
                decoded: None,
            }),
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
        let is_loaded = self
            .state
            .borrow()
            .decoded
            .as_ref()
            .is_some_and(|(cached, _)| *cached == page);
        if !is_loaded {
            let mut state = self.state.borrow_mut();
            let State { archive, decoded } = &mut *state;
            // Drop the old page before allocating the next one.
            decoded.take();
            archive.load(*index, expected).map_err(engine)?;
            let pixels = decode_rgb(archive.current(), DecodeLimits::default()).map_err(engine)?;
            *decoded = Some((page, pixels));
        }
        Ok(Ref::map(self.state.borrow(), |state| {
            &state.decoded.as_ref().expect("page loaded").1
        }))
    }

    fn render_inner(&self, page: u32, scale: f32, format: PixelFormat, clip: Option<Rect>) -> Result<Pixmap> {
        if !scale.is_finite() || scale <= 0.0 {
            return Err(DocError::InvalidArgument(format!("scale {scale}")));
        }
        let pixels = self.pixels(page)?;
        let width = (pixels.width() as f64 * scale as f64).round().max(1.0);
        let height = (pixels.height() as f64 * scale as f64).round().max(1.0);
        if width > 16384.0 || height > 16384.0 || (clip.is_none() && width * height > 16_777_216.0) {
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
            || u64::from(clip.w) * u64::from(clip.h) > 16_777_216
            || clip.x as u64 + clip.w as u64 > size.w as u64
            || clip.y as u64 + clip.h as u64 > size.h as u64
        {
            return Err(DocError::InvalidArgument("clip is outside the page".into()));
        }
        if clip != Rect::from_size(size) {
            return Ok(render_region(&pixels, size, clip, format));
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

/// Samples only the visible rectangle, with bilinear filtering.
fn render_region(source: &Pixmap, size: Size, clip: Rect, format: PixelFormat) -> Pixmap {
    let mut out = Pixmap::new(clip.w, clip.h, format);
    let channels = if format == PixelFormat::Gray8 { 1 } else { 3 };
    for y in 0..clip.h as usize {
        let sy = ((clip.y as f64 + y as f64 + 0.5) * source.height() as f64 / size.h as f64 - 0.5)
            .clamp(0.0, (source.height() - 1) as f64);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(source.height() as usize - 1);
        for x in 0..clip.w as usize {
            let sx = ((clip.x as f64 + x as f64 + 0.5) * source.width() as f64 / size.w as f64 - 0.5)
                .clamp(0.0, (source.width() - 1) as f64);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(source.width() as usize - 1);
            let sample =
                |row, col, channel| f64::from(source.data()[row * source.stride() + col * 3 + channel]);
            let rgb: [u8; 3] = std::array::from_fn(|c| {
                let top = sample(y0, x0, c) * (1.0 - sx.fract()) + sample(y0, x1, c) * sx.fract();
                let bottom = sample(y1, x0, c) * (1.0 - sx.fract()) + sample(y1, x1, c) * sx.fract();
                (top * (1.0 - sy.fract()) + bottom * sy.fract()).round() as u8
            });
            let dst = y * out.stride() + x * channels;
            if channels == 1 {
                out.data_mut()[dst] = Color::rgb(rgb[0], rgb[1], rgb[2]).luma();
            } else {
                out.data_mut()[dst..dst + 3].copy_from_slice(&rgb);
            }
        }
    }
    out
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

fn is_page(entry: &Entry) -> bool {
    entry.is_file && is_image(&entry.name)
}

/// Entries of one open archive in archive order. `next_entry` discards the
/// unread data of the previous entry, so skipping an entry costs the same as
/// reading it.
trait Source {
    fn next_entry(&mut self) -> io::Result<Option<Entry>>;
    fn read_entry(&mut self) -> io::Result<Vec<u8>>;
}

impl Source for Reader {
    fn next_entry(&mut self) -> io::Result<Option<Entry>> {
        Reader::next_entry(self)
    }

    fn read_entry(&mut self) -> io::Result<Vec<u8>> {
        Reader::read_entry(self)
    }
}

/// Most entries the cache holds. A comic has a few hundred pages, and the limit
/// keeps the linear lookups below short for archives of tiny images.
const PAGE_CACHE_ENTRIES: usize = 256;

/// Compressed image bytes of recently read entries, least recently used first.
struct PageCache {
    cap: usize,
    bytes: usize,
    pages: VecDeque<(usize, Vec<u8>)>,
}

impl PageCache {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            bytes: 0,
            pages: VecDeque::new(),
        }
    }

    fn contains(&self, index: usize) -> bool {
        self.pages.iter().any(|(cached, _)| *cached == index)
    }

    /// Moves a cached entry to the most recently used slot. Returns false when
    /// the entry is not cached.
    fn touch(&mut self, index: usize) -> bool {
        match self.pages.iter().position(|(cached, _)| *cached == index) {
            Some(position) => {
                let page = self.pages.remove(position).expect("position from iter");
                self.pages.push_back(page);
                true
            }
            None => false,
        }
    }

    /// Evicts least recently used entries until one more entry of `size` bytes
    /// fits under the byte cap and the entry limit.
    fn make_room(&mut self, size: usize) {
        while self.pages.len() == PAGE_CACHE_ENTRIES || self.bytes + size > self.cap {
            let (_, evicted) = self.pages.pop_front().expect("entry fits the cap");
            self.bytes -= evicted.len();
        }
    }

    fn insert(&mut self, index: usize, data: Vec<u8>) {
        self.bytes += data.len();
        self.pages.push_back((index, data));
    }

    /// The most recently used entry. After `Walker::load`, that is the loaded entry.
    fn latest(&self) -> &[u8] {
        &self.pages.back().expect("an entry was loaded").1
    }
}

/// Reads archive entries in order from one open source. A solid RAR stores all
/// entries in one compressed stream, so reaching entry N decodes every entry
/// before it. The walker keeps the source open and continues it for the next
/// page. A request for an earlier entry opens the archive again. libarchive does
/// not report whether a RAR is solid, so every archive takes this path.
struct Walker<S: Source> {
    open: Box<dyn FnMut() -> io::Result<S>>,
    /// The open source, and the archive position of the entry it returns next.
    stream: Option<(S, usize)>,
    cache: PageCache,
}

fn changed() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "archive changed after opening")
}

impl<S: Source> Walker<S> {
    fn new(open: Box<dyn FnMut() -> io::Result<S>>, cap: usize) -> Self {
        Self {
            open,
            stream: None,
            cache: PageCache::new(cap),
        }
    }

    /// Reads every entry once. Returns the image entries with their archive
    /// positions and caches the last images that fit.
    fn scan(&mut self) -> io::Result<Vec<(usize, Entry)>> {
        let mut source = (self.open)()?;
        let mut pages = Vec::new();
        let mut index = 0;
        while let Some(entry) = source.next_entry()? {
            if is_page(&entry) {
                cache_entry(&mut self.cache, &mut source, index, entry.size)?;
                pages.push((index, entry));
            }
            index += 1;
        }
        self.stream = Some((source, index));
        Ok(pages)
    }

    /// Makes the entry at `index` the most recently used one, reading it if it is
    /// not cached. `expected` must match the entry found at `index`.
    fn load(&mut self, index: usize, expected: &Entry) -> io::Result<()> {
        if self.cache.touch(index) {
            return Ok(());
        }
        let result = self.walk_to(index, expected);
        if result.is_err() {
            // A failed reader stays failed. The next request opens the archive again.
            self.stream = None;
        }
        result
    }

    /// The bytes of the most recently loaded entry.
    fn current(&self) -> &[u8] {
        self.cache.latest()
    }

    fn walk_to(&mut self, index: usize, expected: &Entry) -> io::Result<()> {
        if self.stream.as_ref().is_none_or(|(_, next)| *next > index) {
            self.stream = Some(((self.open)()?, 0));
        }
        let (source, next) = self.stream.as_mut().expect("stream was opened");
        loop {
            let entry = source.next_entry()?.ok_or_else(changed)?;
            let current = *next;
            *next += 1;
            if current == index {
                // Check the entry before caching it, so a mismatch never leaves bad bytes behind.
                if entry != *expected {
                    return Err(changed());
                }
                return cache_entry(&mut self.cache, source, current, entry.size);
            }
            // Images passed on the way stay cached for the pages after them.
            if is_page(&entry) && !self.cache.contains(current) {
                cache_entry(&mut self.cache, source, current, entry.size)?;
            }
        }
    }
}

/// Reads the current entry into the cache. Room is made before the read, so the
/// cache never grows past its cap while the entry is read.
fn cache_entry<S: Source>(cache: &mut PageCache, source: &mut S, index: usize, size: u64) -> io::Result<()> {
    let size = usize::try_from(size)
        .ok()
        .filter(|size| *size <= cache.cap)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "entry exceeds the page cache"))?;
    cache.make_room(size);
    let data = source.read_entry()?;
    if data.len() != size {
        return Err(changed());
    }
    cache.insert(index, data);
    Ok(())
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

    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Counters {
        opened: Cell<usize>,
        /// Bytes passed through the decoder, whether read or skipped.
        decoded: Cell<usize>,
    }

    /// Stands in for a solid RAR stream. Every entry costs its size to pass
    /// through, whether `read_entry` returns it or `next_entry` skips it.
    struct FakeSource {
        entries: Rc<Vec<(Entry, Vec<u8>)>>,
        counters: Rc<Counters>,
        next: usize,
        pending: usize,
    }

    impl FakeSource {
        fn decode_pending(&mut self) {
            self.counters
                .decoded
                .set(self.counters.decoded.get() + self.pending);
            self.pending = 0;
        }
    }

    impl Source for FakeSource {
        fn next_entry(&mut self) -> io::Result<Option<Entry>> {
            self.decode_pending();
            let Some((entry, _)) = self.entries.get(self.next) else {
                return Ok(None);
            };
            self.next += 1;
            self.pending = entry.size as usize;
            Ok(Some(entry.clone()))
        }

        fn read_entry(&mut self) -> io::Result<Vec<u8>> {
            self.decode_pending();
            Ok(self.entries[self.next - 1].1.clone())
        }
    }

    fn fake_walker(entries: Vec<(Entry, Vec<u8>)>, cap: usize) -> (Walker<FakeSource>, Rc<Counters>) {
        let entries = Rc::new(entries);
        let counters = Rc::new(Counters::default());
        let open = {
            let (entries, counters) = (entries.clone(), counters.clone());
            Box::new(move || {
                counters.opened.set(counters.opened.get() + 1);
                Ok(FakeSource {
                    entries: entries.clone(),
                    counters: counters.clone(),
                    next: 0,
                    pending: 0,
                })
            })
        };
        (Walker::new(open, cap), counters)
    }

    fn entry(name: &str, data: Vec<u8>) -> (Entry, Vec<u8>) {
        let entry = Entry {
            name: name.to_owned(),
            size: data.len() as u64,
            is_file: true,
        };
        (entry, data)
    }

    /// `pages` images of `size` bytes, with one text file in the middle. Each page
    /// is filled with its page number, so a wrong page shows in the bytes.
    fn archive(pages: usize, size: usize) -> Vec<(Entry, Vec<u8>)> {
        let mut entries = Vec::new();
        for page in 0..pages {
            if page == pages / 2 {
                entries.push(entry("readme.txt", vec![b'x'; 10]));
            }
            entries.push(entry(&format!("page{page:03}.png"), vec![page as u8; size]));
        }
        entries
    }

    fn total_bytes(entries: &[(Entry, Vec<u8>)]) -> usize {
        entries.iter().map(|(entry, _)| entry.size as usize).sum()
    }

    #[test]
    fn sequential_pages_after_a_scan_decode_each_entry_once() {
        let entries = archive(200, 1024);
        let total = total_bytes(&entries);
        let (mut walker, counters) = fake_walker(entries.clone(), 16 * 1024);
        let pages = walker.scan().unwrap();
        assert_eq!(pages.len(), 200);
        assert_eq!(counters.decoded.get(), total);

        for (page, (index, expected)) in pages.iter().enumerate() {
            walker.load(*index, expected).unwrap();
            assert_eq!(walker.current(), entries[*index].1.as_slice(), "page {page}");
            assert!(walker.cache.bytes <= walker.cache.cap);
        }
        // Page 0 reopens the archive. Every later page continues the same stream.
        assert_eq!(counters.opened.get(), 2);
        assert_eq!(counters.decoded.get() - total, total);
    }

    #[test]
    fn cache_stays_under_its_cap_and_evicts_the_least_recently_used_page() {
        let entries = archive(10, 1024);
        let (mut walker, _) = fake_walker(entries.clone(), 4 * 1024);
        let pages = walker.scan().unwrap();
        // The scan leaves the last four pages cached.
        assert_eq!(walker.cache.pages.len(), 4);
        assert!(walker.cache.bytes <= walker.cache.cap);
        let (last_index, last) = pages[9].clone();
        let (touched_index, touched) = pages[6].clone();
        let (evicted_index, _) = pages[7].clone();
        walker.load(touched_index, &touched).unwrap();
        walker.load(last_index, &last).unwrap();
        assert_eq!(walker.cache.pages.back().unwrap().0, last_index);

        // Page 0 is not cached, so it is read. The least recently used page goes.
        let (first_index, first) = pages[0].clone();
        walker.load(first_index, &first).unwrap();
        assert!(walker.cache.bytes <= walker.cache.cap);
        assert!(!walker.cache.contains(evicted_index));
        assert!(walker.cache.contains(touched_index));
        assert_eq!(walker.current(), entries[first_index].1.as_slice());
    }

    #[test]
    fn backward_jump_reopens_and_reads_only_the_entries_before_the_page() {
        let entries = archive(200, 1024);
        let (mut walker, counters) = fake_walker(entries.clone(), 16 * 1024);
        let pages = walker.scan().unwrap();
        let (late_index, late) = pages[150].clone();
        walker.load(late_index, &late).unwrap();
        let opened = counters.opened.get();
        let decoded = counters.decoded.get();

        // Page 3 sits before the stream position, so the archive opens again.
        let (index, expected) = pages[3].clone();
        walker.load(index, &expected).unwrap();
        assert_eq!(counters.opened.get(), opened + 1);
        assert_eq!(counters.decoded.get() - decoded, 4 * 1024);
        assert_eq!(walker.current(), entries[index].1.as_slice());
    }

    #[test]
    fn mismatched_entry_fails_without_caching_bad_bytes() {
        // Two 8-byte pages fill the cache, so page 0 is not cached and gets read.
        let entries = archive(4, 8);
        let (mut walker, _) = fake_walker(entries.clone(), 16);
        let pages = walker.scan().unwrap();
        let (index, expected) = pages[0].clone();
        let wrong = pages[1].1.clone();
        assert!(walker.load(index, &wrong).is_err());
        assert!(!walker.cache.contains(index));
        walker.load(index, &expected).unwrap();
        assert_eq!(walker.current(), entries[index].1.as_slice());
    }

    #[test]
    fn page_larger_than_the_cap_fails_the_scan() {
        let entries = archive(2, 2048);
        let (mut walker, _) = fake_walker(entries, 1024);
        assert!(walker.scan().is_err());
    }

    #[test]
    fn cache_holds_at_most_the_entry_limit() {
        let entries = archive(PAGE_CACHE_ENTRIES + 44, 1);
        let (mut walker, _) = fake_walker(entries.clone(), 1024 * 1024);
        let pages = walker.scan().unwrap();
        assert_eq!(walker.cache.pages.len(), PAGE_CACHE_ENTRIES);
        let (index, expected) = pages[0].clone();
        walker.load(index, &expected).unwrap();
        assert_eq!(walker.cache.pages.len(), PAGE_CACHE_ENTRIES);
        assert_eq!(walker.current(), entries[index].1.as_slice());
    }
}
