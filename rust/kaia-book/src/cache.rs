//! The rendered page cache. It is shared by the UI thread and the engine
//! thread, so the engine can prerender the next page while the UI shows the
//! current one. The bound is in bytes of pixel data.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use kaia_gfx::{PixelFormat, Pixmap, Size};

use crate::view::ZoomMode;

/// What a rendered page depends on. `layout` changes with each relayout, so
/// pages rendered before a relayout never match after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RenderKey {
    pub layout: u64,
    pub page: u32,
    pub viewport: Size,
    pub zoom: ZoomMode,
    pub format: PixelFormat,
}

/// Counters of the page cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CacheStats {
    /// Lookups that found a page.
    pub hits: u64,
    /// Lookups that did not find a page.
    pub misses: u64,
    /// Pixel bytes held now.
    pub bytes: usize,
    /// Pages held now.
    pub entries: usize,
    /// The bound in bytes.
    pub capacity: usize,
}

struct Entry {
    pixmap: Arc<Pixmap>,
    tick: u64,
}

/// A least-recently-used cache of rendered pages, bounded by bytes.
pub(crate) struct PageCache {
    capacity: usize,
    bytes: usize,
    entries: HashMap<RenderKey, Entry>,
    /// Ticks to keys, oldest first. Each entry has exactly one tick.
    order: BTreeMap<u64, RenderKey>,
    tick: u64,
    hits: u64,
    misses: u64,
}

impl PageCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bytes: 0,
            entries: HashMap::new(),
            order: BTreeMap::new(),
            tick: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// Looks up a page and marks it as recently used. Counts a hit or a miss.
    pub fn get(&mut self, key: &RenderKey) -> Option<Arc<Pixmap>> {
        let Some(entry) = self.entries.get_mut(key) else {
            self.misses += 1;
            return None;
        };
        self.hits += 1;
        self.tick += 1;
        self.order.remove(&entry.tick);
        entry.tick = self.tick;
        self.order.insert(self.tick, *key);
        Some(entry.pixmap.clone())
    }

    /// True when the page is held. Does not count as a lookup.
    pub fn contains(&self, key: &RenderKey) -> bool {
        self.entries.contains_key(key)
    }

    /// Stores a page, evicting the least recently used pages until it fits.
    /// A page larger than the whole cache is returned but not stored.
    pub fn insert(&mut self, key: RenderKey, pixmap: Pixmap) -> Arc<Pixmap> {
        let pixmap = Arc::new(pixmap);
        let size = pixmap.data().len();
        if size > self.capacity {
            return pixmap;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.order.remove(&old.tick);
            self.bytes -= old.pixmap.data().len();
        }
        while self.bytes + size > self.capacity {
            let Some((&tick, &victim)) = self.order.iter().next() else {
                break;
            };
            self.order.remove(&tick);
            if let Some(old) = self.entries.remove(&victim) {
                self.bytes -= old.pixmap.data().len();
            }
        }
        self.tick += 1;
        self.order.insert(self.tick, key);
        self.entries.insert(
            key,
            Entry {
                pixmap: pixmap.clone(),
                tick: self.tick,
            },
        );
        self.bytes += size;
        pixmap
    }

    /// Looks up a page without counting it as a hit or a miss.
    pub fn peek(&self, key: &RenderKey) -> Option<Arc<Pixmap>> {
        self.entries.get(key).map(|e| e.pixmap.clone())
    }

    pub fn stats(&self) -> CacheStats {
        CacheStats {
            hits: self.hits,
            misses: self.misses,
            bytes: self.bytes,
            entries: self.entries.len(),
            capacity: self.capacity,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(page: u32) -> RenderKey {
        RenderKey {
            layout: 0,
            page,
            viewport: Size { w: 4, h: 4 },
            zoom: ZoomMode::FitPage,
            format: PixelFormat::Gray8,
        }
    }

    #[test]
    fn evicts_least_recently_used_by_bytes() {
        // Each 4x4 gray page is 16 bytes. The cache holds three of them.
        let mut cache = PageCache::new(48);
        for p in 0..3 {
            cache.insert(key(p), Pixmap::new(4, 4, PixelFormat::Gray8));
        }
        assert!(cache.get(&key(0)).is_some());
        cache.insert(key(3), Pixmap::new(4, 4, PixelFormat::Gray8));
        assert!(cache.contains(&key(0)));
        assert!(!cache.contains(&key(1)));
        let s = cache.stats();
        assert_eq!((s.entries, s.bytes, s.hits, s.misses), (3, 48, 1, 0));
    }

    #[test]
    fn oversize_page_is_not_stored() {
        let mut cache = PageCache::new(8);
        let pix = cache.insert(key(0), Pixmap::new(4, 4, PixelFormat::Gray8));
        assert_eq!(pix.width(), 4);
        assert_eq!(cache.stats().entries, 0);
    }
}
