//! The `.ifo` metadata file.

use crate::error::Error;

const MAGIC: &str = "StarDict's dict ifo file";

/// Metadata from a `.ifo` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// Display name. Required; sdcv skips dictionaries without one.
    pub bookname: String,
    pub author: String,
    pub description: String,
    pub version: String,
    /// Number of words in the `.idx` file, as the file header states.
    pub word_count: u64,
    /// Number of synonyms in the `.syn` file, as the file header states.
    pub syn_word_count: u64,
    /// Uncompressed size of the `.idx` file, as the file header states.
    pub idx_file_size: u64,
    /// Width of the offset field in the `.idx` file: 32 or 64.
    pub idx_offset_bits: u8,
    /// Type letters from `sametypesequence`, such as `m` or `tm`.
    pub same_type_sequence: Option<String>,
}

pub(crate) fn parse(text: &str) -> Result<Info, Error> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some(MAGIC) {
        return Err(Error::format("first line is not the StarDict ifo header"));
    }
    let mut info = Info {
        bookname: String::new(),
        author: String::new(),
        description: String::new(),
        version: String::new(),
        word_count: 0,
        syn_word_count: 0,
        idx_file_size: 0,
        idx_offset_bits: 32,
        same_type_sequence: None,
    };
    for line in lines {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim_end_matches('\r');
        match key.trim() {
            "bookname" => info.bookname = value.to_owned(),
            "author" => info.author = value.to_owned(),
            "description" => info.description = value.to_owned(),
            "version" => info.version = value.to_owned(),
            "wordcount" => info.word_count = number("wordcount", value)?,
            "synwordcount" => info.syn_word_count = number("synwordcount", value)?,
            "idxfilesize" => info.idx_file_size = number("idxfilesize", value)?,
            "idxoffsetbits" => {
                info.idx_offset_bits = match value.trim() {
                    "32" => 32,
                    "64" => 64,
                    other => return Err(Error::format(format!("idxoffsetbits is {other:?}, not 32 or 64"))),
                };
            }
            "sametypesequence" => {
                let seq = value.trim();
                if seq.is_empty() || !seq.bytes().all(|b| b.is_ascii_alphabetic()) {
                    return Err(Error::format(format!(
                        "sametypesequence {seq:?} is not a list of type letters"
                    )));
                }
                info.same_type_sequence = Some(seq.to_owned());
            }
            _ => {}
        }
    }
    if info.bookname.trim().is_empty() {
        return Err(Error::format("ifo has no bookname"));
    }
    Ok(info)
}

fn number(key: &str, value: &str) -> Result<u64, Error> {
    value
        .trim()
        .parse()
        .map_err(|_| Error::format(format!("{key} is {:?}, not a number", value.trim())))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "StarDict's dict ifo file\n";

    #[test]
    fn parses_required_and_optional_fields() {
        let text = format!(
            "{HEADER}version=2.4.2\nbookname=Test Dict\nwordcount=3\nsynwordcount=1\n\
             idxfilesize=42\nidxoffsetbits=64\nsametypesequence=tm\nauthor=Ann\n\
             description=Line one<br>line two\n"
        );
        let info = parse(&text).unwrap();
        assert_eq!(info.bookname, "Test Dict");
        assert_eq!(info.version, "2.4.2");
        assert_eq!(info.word_count, 3);
        assert_eq!(info.syn_word_count, 1);
        assert_eq!(info.idx_file_size, 42);
        assert_eq!(info.idx_offset_bits, 64);
        assert_eq!(info.same_type_sequence.as_deref(), Some("tm"));
        assert_eq!(info.author, "Ann");
        assert_eq!(info.description, "Line one<br>line two");
    }

    #[test]
    fn offset_bits_default_to_32_and_crlf_is_accepted() {
        let info = parse(&format!("{HEADER}bookname=Crlf\r\nversion=2.4.2\r\n")).unwrap();
        assert_eq!(info.idx_offset_bits, 32);
        assert_eq!(info.bookname, "Crlf");
        assert_eq!(info.same_type_sequence, None);
    }

    #[test]
    fn rejects_wrong_header() {
        assert!(parse("bookname=No header\n").is_err());
        assert!(parse("StarDict's dict ifo file 2\nbookname=X\n").is_err());
    }

    #[test]
    fn rejects_missing_bookname() {
        assert!(parse(&format!("{HEADER}version=2.4.2\n")).is_err());
        assert!(parse(&format!("{HEADER}bookname=   \n")).is_err());
    }

    #[test]
    fn rejects_bad_offset_bits_and_type_sequence() {
        assert!(parse(&format!("{HEADER}bookname=X\nidxoffsetbits=16\n")).is_err());
        assert!(parse(&format!("{HEADER}bookname=X\nsametypesequence=1\n")).is_err());
        assert!(parse(&format!("{HEADER}bookname=X\nwordcount=many\n")).is_err());
    }
}
