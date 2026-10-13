//! One StarDict dictionary: its files, its in-memory index, and definition parsing.

use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use flate2::read::MultiGzDecoder;

use crate::dictzip::{DataFile, DictZip};
use crate::error::Error;
use crate::ifo::{self, Info};
use crate::index::{Item, WordTable};
use crate::lookup::{self, Definition, DefinitionKind, Entry, LookupOptions};

/// One `.idx` record: where the definition sits in the data file.
#[derive(Debug)]
pub(crate) struct Rec {
    pub(crate) offset: u64,
    pub(crate) size: u32,
}

/// A StarDict dictionary opened from its `.ifo` file.
///
/// The `.idx` index and the optional `.syn` file are read into memory when the dictionary
/// opens. Definitions are read from the `.dict` or `.dict.dz` file on each lookup.
#[derive(Debug)]
pub struct Dictionary {
    path: PathBuf,
    info: Info,
    pub(crate) index: WordTable<Rec>,
    /// Synonym word, with the index of its target record in file order.
    pub(crate) syn: Option<WordTable<u32>>,
    pub(crate) data: DataFile,
}

impl Dictionary {
    /// Opens the dictionary described by `ifo_path`.
    ///
    /// The `.idx` (or `.idx.gz`), the `.dict` (or `.dict.dz`) and the optional `.syn` file
    /// must sit next to the `.ifo` file under the same base name. Every index record is
    /// checked against the data file, so a truncated or mismatched dictionary fails here.
    pub fn open(ifo_path: impl AsRef<Path>) -> Result<Self, Error> {
        let ifo_path = ifo_path.as_ref();
        let base = ifo_path.with_extension("");
        let ifo_bytes = read_file(ifo_path)?.ok_or_else(|| Error::Missing {
            path: ifo_path.to_path_buf(),
        })?;
        let info = ifo::parse(&String::from_utf8_lossy(&ifo_bytes)).map_err(|e| e.at(ifo_path))?;

        let index_bytes = read_index(&base)?;
        if info.idx_file_size != 0 && info.idx_file_size != index_bytes.len() as u64 {
            return Err(Error::format("idxfilesize does not match the index").at(&base));
        }
        let index = parse_index(index_bytes, info.idx_offset_bits)?;
        let data = open_data(&base)?;

        for rec in index.values() {
            if rec
                .offset
                .checked_add(u64::from(rec.size))
                .is_none_or(|end| end > data.len())
            {
                return Err(Error::format(format!(
                    "index points past the end of the data file ({} bytes)",
                    data.len()
                ))
                .at(&with_suffix(&base, ".dict")));
            }
        }

        let syn = read_syn(&base, index.len())?;
        Ok(Dictionary {
            path: ifo_path.to_path_buf(),
            info,
            index,
            syn,
            data,
        })
    }

    /// Metadata from the `.ifo` file.
    pub fn info(&self) -> &Info {
        &self.info
    }

    /// The display name, from `bookname`.
    pub fn name(&self) -> &str {
        &self.info.bookname
    }

    /// Path of the `.ifo` file this dictionary was opened from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Number of words in the index.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// True when the index has no words.
    pub fn is_empty(&self) -> bool {
        self.index.len() == 0
    }

    /// Looks up `word` with the default options.
    pub fn lookup(&self, word: &str) -> Vec<Entry> {
        self.lookup_with(word, &LookupOptions::default())
    }

    /// Looks up `word` in this dictionary only.
    ///
    /// An index record whose definition cannot be read is skipped, so a damaged entry
    /// shows as a missing one. [`Dictionary::open`] already checks that every record lies
    /// inside the data file.
    pub fn lookup_with(&self, word: &str, opts: &LookupOptions) -> Vec<Entry> {
        lookup::run(&[self], word, opts)
    }

    /// Builds the entry for index record `rec`, or `None` when the record has no text.
    pub(crate) fn entry(&self, rec: usize, matched: lookup::Match) -> Option<Entry> {
        let definition = self.read_definition(&self.index.item(rec).value).ok()??;
        Some(Entry {
            dict_name: self.info.bookname.clone(),
            headword: String::from_utf8_lossy(self.index.word(rec)).into_owned(),
            definition,
            matched,
        })
    }

    fn read_definition(&self, rec: &Rec) -> Result<Option<Definition>, Error> {
        let bytes = self.data.read(rec.offset, rec.size as usize)?;
        parse_fields(&bytes, self.info.same_type_sequence.as_deref())
    }
}

/// Splits one entry's data into its fields and joins the text fields.
///
/// With `sametypesequence`, the type letters come from the sequence and are not stored
/// in the data. Without it, each field starts with its own type letter. Lowercase
/// letters mark NUL-terminated text. Uppercase letters mark binary data with a 4-byte
/// big-endian length, which is skipped. Type `r` (resource list) is skipped too.
///
/// The entry's kind is the kind of its first text field. Text fields are joined
/// with a line break, as in sdcv.
fn parse_fields(data: &[u8], same_type: Option<&str>) -> Result<Option<Definition>, Error> {
    let mut parts: Vec<String> = Vec::new();
    let mut kind: Option<DefinitionKind> = None;
    let mut pos = 0;
    let mut field = 0;
    while pos < data.len() {
        let letter = match same_type {
            Some(seq) => match seq.as_bytes().get(field) {
                Some(&letter) => letter,
                None => break,
            },
            None => {
                let letter = data[pos];
                pos += 1;
                letter
            }
        };
        field += 1;
        if letter.is_ascii_uppercase() {
            if same_type.is_some_and(|seq| field == seq.len()) {
                pos = data.len();
                continue;
            }
            let len_bytes = data
                .get(pos..pos + 4)
                .ok_or_else(|| Error::format("binary field length is truncated"))?;
            let len = u32::from_be_bytes([len_bytes[0], len_bytes[1], len_bytes[2], len_bytes[3]]) as usize;
            pos += 4;
            pos = pos
                .checked_add(len)
                .filter(|end| *end <= data.len())
                .ok_or_else(|| Error::format("binary field runs past the end of the entry"))?;
        } else if letter.is_ascii_lowercase() {
            let rest = &data[pos..];
            let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
            let text = &rest[..end];
            pos += (end + 1).min(rest.len());
            if let Some(field_kind) = text_kind(letter) {
                kind.get_or_insert(field_kind);
                let value = String::from_utf8_lossy(text);
                parts.push(match letter {
                    b't' | b'y' => format!("[{value}]"),
                    b'x' => format!("[{}]", crate::html::strip_inline(&value, true)),
                    _ => value.into_owned(),
                });
            }
        } else {
            return Err(Error::format(format!("unknown field type {:?}", letter as char)));
        }
    }
    Ok(kind.map(|kind| Definition {
        kind,
        text: parts.join("\n"),
    }))
}

/// Maps a lowercase type letter to a text kind. Returns `None` for types that are not shown.
fn text_kind(letter: u8) -> Option<DefinitionKind> {
    Some(match letter {
        b'm' => DefinitionKind::Plain,
        b'h' => DefinitionKind::Html,
        b'x' => DefinitionKind::Xdxf,
        b'g' => DefinitionKind::Pango,
        b't' | b'y' => DefinitionKind::Phonetic,
        b'r' => return None,
        other => DefinitionKind::Other(other as char),
    })
}

fn with_suffix(base: &Path, suffix: &str) -> PathBuf {
    let mut name = base.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Reads a file. Returns `Ok(None)` when it does not exist.
fn read_file(path: &Path) -> Result<Option<Vec<u8>>, Error> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::from(e).at(path)),
    }
}

/// Reads the index, from `.idx` if present, otherwise from `.idx.gz`.
fn read_index(base: &Path) -> Result<Vec<u8>, Error> {
    let plain = with_suffix(base, ".idx");
    let gz = with_suffix(base, ".idx.gz");
    if !gz.is_file() {
        return read_file(&plain)?.ok_or(Error::Missing { path: plain });
    }
    let file = match File::open(&gz) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(Error::Missing { path: plain }),
        Err(e) => return Err(Error::from(e).at(&gz)),
    };
    let mut out = Vec::new();
    MultiGzDecoder::new(file)
        .read_to_end(&mut out)
        .map_err(|_| Error::format("index is not valid gzip").at(&gz))?;
    Ok(out)
}

/// Opens the data file, from `.dict` if present, otherwise from `.dict.dz`.
fn open_data(base: &Path) -> Result<DataFile, Error> {
    let plain = with_suffix(base, ".dict");
    let dz = with_suffix(base, ".dict.dz");
    if dz.is_file() {
        return Ok(DataFile::DictZip(DictZip::open(&dz)?));
    }
    if plain.is_file() {
        return DataFile::open_plain(&plain);
    }
    Err(Error::Missing { path: plain })
}

/// Reads the optional `.syn` file. Its targets must be valid index positions.
fn read_syn(base: &Path, word_count: usize) -> Result<Option<WordTable<u32>>, Error> {
    let path = with_suffix(base, ".syn");
    let Some(raw) = read_file(&path)? else {
        return Ok(None);
    };
    let table = scan(&raw, 4)
        .map_err(|e| e.at(&path))?
        .into_iter()
        .map(|(start, len, trailer)| {
            Ok(Item {
                start: to_u32(start)?,
                len: to_u32(len)?,
                value: be_uint(&raw[trailer..trailer + 4]) as u32,
            })
        })
        .collect::<Result<Vec<Item<u32>>, Error>>()
        .map_err(|e| e.at(&path))?;
    if table.iter().any(|item| item.value as usize >= word_count) {
        return Err(Error::format("synonym points past the end of the index").at(&path));
    }
    Ok(Some(WordTable::new(raw, table)))
}

fn parse_index(raw: Vec<u8>, offset_bits: u8) -> Result<WordTable<Rec>, Error> {
    let offset_len = if offset_bits == 64 { 8 } else { 4 };
    let items = scan(&raw, offset_len + 4)?
        .into_iter()
        .map(|(start, len, trailer)| {
            let offset_bytes = &raw[trailer..trailer + offset_len];
            let size_bytes = &raw[trailer + offset_len..trailer + offset_len + 4];
            Ok(Item {
                start: to_u32(start)?,
                len: to_u32(len)?,
                value: Rec {
                    offset: be_uint(offset_bytes),
                    size: be_uint(size_bytes) as u32,
                },
            })
        })
        .collect::<Result<Vec<Item<Rec>>, Error>>()?;
    Ok(WordTable::new(raw, items))
}

/// Finds each record as `(word start, word length, trailer start)`. A record is a
/// NUL-terminated word followed by `trailer_len` bytes.
fn scan(raw: &[u8], trailer_len: usize) -> Result<Vec<(usize, usize, usize)>, Error> {
    let mut records = Vec::new();
    let mut pos = 0;
    while pos < raw.len() {
        let nul = raw[pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::format("a word in the index is not terminated"))?;
        let trailer = pos + nul + 1;
        if trailer + trailer_len > raw.len() {
            return Err(Error::format("the index ends in the middle of an entry"));
        }
        records.push((pos, nul, trailer));
        pos = trailer + trailer_len;
    }
    Ok(records)
}

fn be_uint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |acc, &b| (acc << 8) | u64::from(b))
}

fn to_u32(value: usize) -> Result<u32, Error> {
    u32::try_from(value).map_err(|_| Error::format("file is too large for this reader"))
}
