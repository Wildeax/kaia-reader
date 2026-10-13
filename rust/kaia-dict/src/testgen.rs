//! Writes StarDict files for tests. The product never writes dictionaries.
//!
//! The writer sorts words with its own rule instead of the crate's comparator, so a bug in
//! the comparator shows up as a failed lookup rather than matching itself.

use std::cmp::Ordering;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use flate2::{Compress, Compression, Crc, FlushCompress};

/// A word and its fields. Each field is a type letter and its bytes.
pub(crate) type GenEntry = (String, Vec<(u8, Vec<u8>)>);

/// Description of one dictionary to write.
pub(crate) struct Gen {
    pub name: String,
    pub entries: Vec<GenEntry>,
    /// `idxoffsetbits=64`.
    pub idx64: bool,
    /// Write `.idx.gz` instead of `.idx`.
    pub gzip_idx: bool,
    /// Write `.dict.dz` with this uncompressed chunk length, instead of `.dict`.
    pub dictzip_chunk: Option<usize>,
    /// `sametypesequence`. Fields then store no type letter.
    pub same_type: Option<String>,
    /// `(synonym, target headword)` pairs for the `.syn` file.
    pub syn: Vec<(String, String)>,
}

impl Gen {
    /// One plain-text (`m`) field per word.
    pub(crate) fn plain(name: &str, words: &[(&str, &str)]) -> Gen {
        Gen {
            name: name.to_owned(),
            entries: words
                .iter()
                .map(|(w, d)| ((*w).to_owned(), vec![(b'm', d.as_bytes().to_vec())]))
                .collect(),
            idx64: false,
            gzip_idx: false,
            dictzip_chunk: None,
            same_type: None,
            syn: Vec::new(),
        }
    }

    /// Writes the files into `dir` and returns the `.ifo` path.
    pub(crate) fn write(&self, dir: &Path) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let mut sorted: Vec<&GenEntry> = self.entries.iter().collect();
        sorted.sort_by(|a, b| order(a.0.as_bytes(), b.0.as_bytes()));

        let mut dict = Vec::new();
        let mut idx = Vec::new();
        for (word, fields) in &sorted {
            let start = dict.len();
            for (letter, body) in fields {
                if self.same_type.is_none() {
                    dict.push(*letter);
                }
                if letter.is_ascii_uppercase() {
                    dict.extend((body.len() as u32).to_be_bytes());
                    dict.extend(body);
                } else {
                    dict.extend(body);
                    dict.push(0);
                }
            }
            let size = dict.len() - start;
            idx.extend(word.as_bytes());
            idx.push(0);
            if self.idx64 {
                idx.extend((start as u64).to_be_bytes());
            } else {
                idx.extend((start as u32).to_be_bytes());
            }
            idx.extend((size as u32).to_be_bytes());
        }

        let base = dir.join(&self.name);
        let ifo = base.with_extension("ifo");
        match self.dictzip_chunk {
            Some(chunk) => fs::write(base.with_extension("dict.dz"), dictzip(&dict, chunk)).unwrap(),
            None => fs::write(base.with_extension("dict"), &dict).unwrap(),
        }
        if self.gzip_idx {
            let mut enc = GzEncoder::new(Vec::new(), Compression::default());
            enc.write_all(&idx).unwrap();
            fs::write(base.with_extension("idx.gz"), enc.finish().unwrap()).unwrap();
        } else {
            fs::write(base.with_extension("idx"), &idx).unwrap();
        }

        if !self.syn.is_empty() {
            let mut syn: Vec<(&str, u32)> = self
                .syn
                .iter()
                .map(|(from, to)| {
                    let target = sorted
                        .iter()
                        .position(|e| e.0 == *to)
                        .unwrap_or_else(|| panic!("syn target {to:?} is not a headword"));
                    (from.as_str(), target as u32)
                })
                .collect();
            syn.sort_by(|a, b| order(a.0.as_bytes(), b.0.as_bytes()));
            let mut bytes = Vec::new();
            for (word, target) in syn {
                bytes.extend(word.as_bytes());
                bytes.push(0);
                bytes.extend(target.to_be_bytes());
            }
            fs::write(base.with_extension("syn"), bytes).unwrap();
        }

        let mut meta = String::from("StarDict's dict ifo file\nversion=2.4.2\n");
        meta.push_str(&format!("bookname={}\n", self.name));
        meta.push_str(&format!("wordcount={}\n", self.entries.len()));
        meta.push_str(&format!("idxfilesize={}\n", idx.len()));
        if !self.syn.is_empty() {
            meta.push_str(&format!("synwordcount={}\n", self.syn.len()));
        }
        if self.idx64 {
            meta.push_str("idxoffsetbits=64\n");
        }
        if let Some(seq) = &self.same_type {
            meta.push_str(&format!("sametypesequence={seq}\n"));
        }
        meta.push_str("author=Generated\ndescription=Test dictionary\n");
        fs::write(&ifo, meta).unwrap();
        ifo
    }
}

/// StarDict order written independently of the crate: ASCII case-insensitive, then bytes.
fn order(a: &[u8], b: &[u8]) -> Ordering {
    a.iter()
        .map(|c| c.to_ascii_lowercase())
        .cmp(b.iter().map(|c| c.to_ascii_lowercase()))
        .then(a.cmp(b))
}

/// One dictzip chunk, as dictzip writes it: a raw deflate stream ended by a full flush
/// (`00 00 ff ff`) with no final block. A full flush also resets the window, so each chunk
/// decodes on its own.
fn deflate_chunk(data: &[u8]) -> Vec<u8> {
    let mut comp = Compress::new(Compression::default(), false);
    let mut out = Vec::new();
    loop {
        out.reserve(4096);
        let pos = comp.total_in() as usize;
        comp.compress_vec(&data[pos..], &mut out, FlushCompress::Full)
            .unwrap();
        let input_done = comp.total_in() as usize == data.len();
        // Spare room left in the buffer means the flush finished.
        if input_done && out.capacity() > out.len() {
            return out;
        }
    }
}

/// A dictzip file, laid out as dictzip writes it: gzip header with an `RA` extra field,
/// one full-flush deflate chunk per piece of data, an empty final block (`03 00`), then the
/// gzip trailer.
pub(crate) fn dictzip(data: &[u8], chunk: usize) -> Vec<u8> {
    assert!(chunk > 0 && chunk <= u16::MAX as usize);
    let parts: Vec<Vec<u8>> = data.chunks(chunk).map(deflate_chunk).collect();
    let mut ra = Vec::new();
    ra.extend(1u16.to_le_bytes());
    ra.extend((chunk as u16).to_le_bytes());
    ra.extend((parts.len() as u16).to_le_bytes());
    for part in &parts {
        ra.extend((part.len() as u16).to_le_bytes());
    }
    const FEXTRA: u8 = 0x04;
    const FNAME: u8 = 0x08;
    let mut out = vec![0x1f, 0x8b, 8, FEXTRA | FNAME, 0, 0, 0, 0, 0, 3];
    out.extend((ra.len() as u16 + 4).to_le_bytes());
    out.extend(b"RA");
    out.extend((ra.len() as u16).to_le_bytes());
    out.extend(ra);
    out.extend(b"stardict.dict\0");
    for part in parts {
        out.extend(part);
    }
    out.extend([0x03, 0x00]);
    let mut crc = Crc::new();
    crc.update(data);
    out.extend(crc.sum().to_le_bytes());
    out.extend((data.len() as u32).to_le_bytes());
    out
}

/// A fresh, empty scratch directory for one test.
pub(crate) fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("kaia-dict-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// `count` distinct words with mixed case, for lookup tests.
pub(crate) fn sample_words(count: usize) -> Vec<String> {
    (0..count)
        .map(|i| {
            if i % 7 == 0 {
                format!("Word{i:06}")
            } else {
                format!("word{i:06}")
            }
        })
        .collect()
}
