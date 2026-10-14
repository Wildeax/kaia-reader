//! Read-only Kobo dicthtml lookup. Independent implementation of the format at
//! https://pgaskin.net/dictutil/dicthtml/format.html and prefixes.html.
//! No dictutil source code is included. The marisa autocomplete index is unused.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use flate2::read::MultiGzDecoder;
use regex::Regex;
use zip::ZipArchive;

use crate::{Definition, DefinitionKind, Entry, Error, Match};

const MAX_SHARD: u64 = 32 * 1024 * 1024;
const PROTECTED: &str = "Encrypted or unsupported Kobo dictionary. Skipped; Kaia cannot decrypt it.";
static CYRILLIC: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Cyrillic}").unwrap());
static LETTERS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{L}{2}$").unwrap());

/// Non-Japanese dicthtml filename prefix. Count code points before trimming.
pub fn prefix(word: &str) -> String {
    let lower: String = word
        .chars()
        .take_while(|c| *c != '\0')
        .take(2)
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect();
    let mut value = lower.trim().to_owned();
    if value.is_empty() {
        return "11".into();
    }
    if CYRILLIC.is_match(&value) {
        return value;
    }
    if value.chars().count() == 1 {
        value.push('a');
    }
    if LETTERS.is_match(&value) {
        value
    } else {
        "11".into()
    }
}

#[derive(Debug)]
pub struct KoboDictionary {
    path: PathBuf,
    name: String,
}

impl KoboDictionary {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let mut zip = archive(path)?;
        let mut shards = 0;
        for i in 0..zip.len() {
            let mut file = zip.by_index(i).map_err(zip_error)?;
            if !file.name().map_err(zip_error)?.ends_with(".html") {
                continue;
            }
            shards += 1;
            // Payload encryption is independent of ZIP's encryption flag. There
            // is no documented AES magic. Reject unknown payload signatures.
            let mut magic = [0; 2];
            file.read_exact(&mut magic)?;
            let reader = std::io::Cursor::new(magic).chain(file);
            let mut decoded: Box<dyn Read + '_> = if magic == [0x1f, 0x8b] {
                Box::new(MultiGzDecoder::new(reader))
            } else {
                Box::new(reader)
            };
            let mut header = Vec::new();
            decoded.by_ref().take(256).read_to_end(&mut header)?;
            if !readable_header(&header) {
                return Err(Error::format(PROTECTED).at(path));
            }
        }
        if shards == 0 {
            return Err(Error::format("No dicthtml shards").at(path));
        }
        let locale = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("dicthtml")
            .strip_prefix("dicthtml-")
            .unwrap_or("en");
        Ok(Self {
            path: path.to_owned(),
            name: format!("Kobo {locale}"),
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn lookup(&self, word: &str) -> Result<Vec<Entry>, Error> {
        let word = word.trim();
        if word.is_empty() {
            return Ok(Vec::new());
        }
        let mut zip = archive(&self.path)?;
        let filename = format!("{}.html", prefix(word));
        let file = match zip.by_name(&filename) {
            Ok(file) => file,
            Err(zip::result::ZipError::FileNotFound) => return Ok(Vec::new()),
            Err(error) => return Err(zip_error(error)),
        };
        let mut raw = Vec::new();
        file.take(MAX_SHARD + 1).read_to_end(&mut raw)?;
        if raw.len() as u64 > MAX_SHARD {
            return Err(Error::format("Kobo shard is too large"));
        }
        let bytes = if raw.starts_with(&[0x1f, 0x8b]) {
            let mut bytes = Vec::new();
            MultiGzDecoder::new(raw.as_slice())
                .take(MAX_SHARD + 1)
                .read_to_end(&mut bytes)?;
            bytes
        } else {
            raw
        };
        if bytes.len() as u64 > MAX_SHARD {
            return Err(Error::format("Kobo shard is too large"));
        }
        if !readable_header(&bytes) {
            return Err(Error::format(PROTECTED).at(&self.path));
        }
        let html = std::str::from_utf8(&bytes).map_err(|_| Error::format("Invalid dicthtml UTF-8"))?;
        let query = word.to_lowercase();
        let mut entries = Vec::new();
        for part in html.split("<w>").skip(1) {
            let Some((entry, _)) = part.split_once("</w>") else {
                continue;
            };
            let Some(headword) = attribute(entry, "<a name=\"") else {
                continue;
            };
            let headword = crate::strip_html(headword);
            let exact = headword == word;
            let variant = entry
                .split("<variant name=\"")
                .skip(1)
                .filter_map(|s| s.split_once('"').map(|(name, _)| name))
                .any(|name| crate::strip_html(name).to_lowercase() == query);
            if exact || headword.to_lowercase() == query || variant {
                entries.push(Entry {
                    dict_name: self.name.clone(),
                    headword,
                    definition: Definition {
                        kind: DefinitionKind::Html,
                        text: entry.to_owned(),
                    },
                    matched: if exact {
                        Match::Exact
                    } else if variant {
                        Match::Synonym
                    } else {
                        Match::CaseInsensitive
                    },
                });
            }
        }
        Ok(entries)
    }
}

fn attribute<'a>(html: &'a str, tag: &str) -> Option<&'a str> {
    html.split_once(tag)?.1.split_once('"').map(|(value, _)| value)
}

fn readable_header(bytes: &[u8]) -> bool {
    // A prefix can end midway through UTF-8. The HTML signature is ASCII.
    let text = String::from_utf8_lossy(bytes);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    text.starts_with("<html") || text.starts_with("<?xml") || text.starts_with("<!DOCTYPE html")
}

fn archive(path: &Path) -> Result<ZipArchive<File>, Error> {
    ZipArchive::new(File::open(path)?).map_err(|e| zip_error(e).at(path))
}

fn zip_error(error: zip::result::ZipError) -> Error {
    Error::format(format!("Cannot read Kobo ZIP, possibly encrypted: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture(gzip: bool, protected: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let mut zip = zip::ZipWriter::new(File::create(dir.path().join("dicthtml-en-es.zip")).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("words", options).unwrap();
        zip.write_all(b"synthetic unused marisa index").unwrap();
        zip.start_file("te.html", options).unwrap();
        let html = b"<html><w><a name=\"test\" /><var><variant name=\"tests\"/></var><p>A &amp; B</p></w><w><a name=\"test\" /><p>Second</p></w><w><a name=\"team\" />Wrong</w></html>";
        if protected {
            zip.write_all(&[0xa5; 32]).unwrap();
        } else if gzip {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gz.write_all(html).unwrap();
            zip.write_all(&gz.finish().unwrap()).unwrap();
        } else {
            zip.write_all(html).unwrap();
        }
        zip.finish().unwrap();
        dir
    }

    #[test]
    fn documented_prefixes() {
        for (word, expected) in [
            ("test", "te"),
            ("a", "aa"),
            ("Èe", "èe"),
            ("àççèñts", "àç"),
            ("", "11"),
            (" x", "xa"),
            (" 123", "11"),
            ("x 23", "xa"),
            ("д ", "д"),
            ("дaд", "дa"),
            ("未", "未a"),
            ("a\0b", "aa"),
            ("1a", "11"),
            ("İx", "ix"),
        ] {
            assert_eq!(prefix(word), expected, "{word}");
        }
    }

    #[test]
    fn plain_and_gzip_lookup_without_marisa_are_read_only() {
        for gzip in [false, true] {
            let dir = fixture(gzip, false);
            let path = dir.path().join("dicthtml-en-es.zip");
            let before = std::fs::read(&path).unwrap();
            let dictionary = KoboDictionary::open(&path).unwrap();
            assert_eq!(dictionary.name(), "Kobo en-es");
            let entries = dictionary.lookup("TEST").unwrap();
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].definition.plain_text(), "A & B");
            assert_eq!(dictionary.lookup("tests").unwrap().len(), 1);
            assert!(dictionary.lookup("missing").unwrap().is_empty());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }

    #[test]
    fn kobo_matches_suppress_stardict_fuzzy_guesses() {
        let dir = fixture(false, false);
        crate::testgen::Gen::plain("StarDict", &[("tent", "A shelter")]).write(dir.path());
        let mut library = crate::Library::open_dir(dir.path()).unwrap();
        let before = library.lookup("test");
        assert!(before.iter().any(|entry| entry.matched == Match::Fuzzy));
        library.add_kobo_dir(dir.path());
        let found = library.lookup("test");
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|entry| entry.dict_name == "Kobo en-es"));
        let options = crate::LookupOptions {
            disabled: ["Kobo en-es".into()].into(),
            ..Default::default()
        };
        assert_eq!(library.lookup_with("test", &options), before);
    }

    #[test]
    fn zip_encryption_flag_is_skipped() {
        let dir = fixture(false, false);
        let path = dir.path().join("dicthtml-en-es.zip");
        let mut bytes = std::fs::read(&path).unwrap();
        for offset in 0..bytes.len().saturating_sub(10) {
            if bytes[offset..].starts_with(b"PK\x03\x04") {
                bytes[offset + 6] |= 1;
            }
            if bytes[offset..].starts_with(b"PK\x01\x02") {
                bytes[offset + 8] |= 1;
            }
        }
        std::fs::write(&path, bytes).unwrap();
        assert!(KoboDictionary::open(path)
            .unwrap_err()
            .to_string()
            .contains("encrypted"));
    }

    #[test]
    fn encrypted_payload_marker_is_skipped_with_note() {
        let dir = fixture(false, true);
        let error = KoboDictionary::open(dir.path().join("dicthtml-en-es.zip")).unwrap_err();
        assert!(error
            .to_string()
            .contains("Encrypted or unsupported Kobo dictionary"));
    }
}
