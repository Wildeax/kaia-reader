//! Word tables for `.idx` and `.syn` files, kept in memory and searched by binary search.

use std::cmp::Ordering;

/// StarDict word order: ASCII case-insensitive first, then plain byte order to break ties.
///
/// Two words compare equal only when their bytes are identical, so every word has one
/// position in a sorted table.
pub(crate) fn stardict_cmp(a: &[u8], b: &[u8]) -> Ordering {
    ascii_fold_cmp(a, b).then_with(|| a.cmp(b))
}

/// ASCII case-insensitive comparison, the primary key of [`stardict_cmp`].
pub(crate) fn ascii_fold_cmp(a: &[u8], b: &[u8]) -> Ordering {
    a.iter()
        .map(u8::to_ascii_lowercase)
        .cmp(b.iter().map(u8::to_ascii_lowercase))
}

/// One record: where its word sits in the raw file bytes, and its payload.
#[derive(Debug)]
pub(crate) struct Item<T> {
    pub(crate) start: u32,
    pub(crate) len: u32,
    pub(crate) value: T,
}

/// Records in file order, plus the permutation that sorts them by word.
///
/// Files are normally sorted already. Sorting on load keeps lookups correct for files
/// that are not, and file order stays available because `.syn` files refer to it.
#[derive(Debug)]
pub(crate) struct WordTable<T> {
    raw: Vec<u8>,
    items: Vec<Item<T>>,
    order: Vec<u32>,
}

impl<T> WordTable<T> {
    pub(crate) fn new(raw: Vec<u8>, items: Vec<Item<T>>) -> Self {
        let mut order: Vec<u32> = (0..items.len() as u32).collect();
        let table = WordTable {
            raw,
            items,
            order: Vec::new(),
        };
        order.sort_by(|&a, &b| stardict_cmp(table.word(a as usize), table.word(b as usize)));
        WordTable { order, ..table }
    }

    pub(crate) fn len(&self) -> usize {
        self.items.len()
    }

    pub(crate) fn item(&self, index: usize) -> &Item<T> {
        &self.items[index]
    }

    pub(crate) fn values(&self) -> impl Iterator<Item = &T> {
        self.items.iter().map(|item| &item.value)
    }

    pub(crate) fn word(&self, index: usize) -> &[u8] {
        let item = &self.items[index];
        let start = item.start as usize;
        &self.raw[start..start + item.len as usize]
    }

    /// File-order indexes of the records whose word equals `key` exactly.
    pub(crate) fn exact(&self, key: &[u8]) -> impl Iterator<Item = usize> + '_ {
        let lo = self.partition(|w| stardict_cmp(w, key) == Ordering::Less);
        let hi = self.partition(|w| stardict_cmp(w, key) != Ordering::Greater);
        self.order[lo..hi].iter().map(|&i| i as usize)
    }

    /// File-order indexes of the records whose word equals `key` ignoring ASCII case.
    pub(crate) fn ascii_fold(&self, key: &[u8]) -> impl Iterator<Item = usize> + '_ {
        let lo = self.partition(|w| ascii_fold_cmp(w, key) == Ordering::Less);
        let hi = self.partition(|w| ascii_fold_cmp(w, key) != Ordering::Greater);
        self.order[lo..hi].iter().map(|&i| i as usize)
    }

    /// First sorted position whose word does not satisfy `before`.
    fn partition(&self, before: impl Fn(&[u8]) -> bool) -> usize {
        let (mut lo, mut hi) = (0, self.order.len());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if before(self.word(self.order[mid] as usize)) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(words: &[&str]) -> WordTable<usize> {
        let mut raw = Vec::new();
        let mut items = Vec::new();
        for (i, w) in words.iter().enumerate() {
            items.push(Item {
                start: raw.len() as u32,
                len: w.len() as u32,
                value: i,
            });
            raw.extend_from_slice(w.as_bytes());
        }
        WordTable::new(raw, items)
    }

    fn words_of<T>(t: &WordTable<T>, it: impl Iterator<Item = usize>) -> Vec<String> {
        it.map(|i| String::from_utf8(t.word(i).to_vec()).unwrap())
            .collect()
    }

    #[test]
    fn stardict_order_is_case_insensitive_then_bytewise() {
        let mut words = vec!["banana", "APPLE", "Apple", "apple", "apple2", "Zebra", "ant"];
        words.sort_by(|a, b| stardict_cmp(a.as_bytes(), b.as_bytes()));
        assert_eq!(
            words,
            ["ant", "APPLE", "Apple", "apple", "apple2", "banana", "Zebra"]
        );
    }

    #[test]
    fn exact_finds_only_identical_bytes() {
        let t = table(&["Apple", "apple", "banana", "apple"]);
        assert_eq!(words_of(&t, t.exact(b"apple")), ["apple", "apple"]);
        assert_eq!(words_of(&t, t.exact(b"Apple")), ["Apple"]);
        assert_eq!(words_of(&t, t.exact(b"APPLE")), Vec::<String>::new());
        assert_eq!(words_of(&t, t.exact(b"cherry")), Vec::<String>::new());
    }

    #[test]
    fn ascii_fold_finds_every_case_variant() {
        let t = table(&["Apple", "apple", "banana", "APPLE", "applet"]);
        let mut found = words_of(&t, t.ascii_fold(b"ApPlE"));
        found.sort();
        assert_eq!(found, ["APPLE", "Apple", "apple"]);
        assert!(words_of(&t, t.ascii_fold(b"app")).is_empty());
    }

    #[test]
    fn unsorted_input_is_sorted_and_keeps_file_order() {
        let t = table(&["zeta", "alpha", "mid"]);
        assert_eq!(t.word(0), b"zeta");
        assert_eq!(words_of(&t, t.exact(b"alpha")), ["alpha"]);
        assert_eq!(t.item(2).value, 2);
    }
}
