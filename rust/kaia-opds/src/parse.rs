//! Format detection and the parse entry points.

use url::Url;

use crate::error::{Error, Result};
use crate::model::{Acquisition, Feed};
use crate::resolve::parse_url;
use crate::{opds1, opds2, search};

/// Parses a catalog body. OPDS 1.x (Atom, starts with `<`) and OPDS 2.0
/// (JSON, starts with `{`) are both accepted. `base` is the URL the body
/// came from, used to resolve relative links. No network access happens here.
pub fn parse_feed(body: &[u8], base: &str) -> Result<Feed> {
    let base = parse_url(base)?;
    let text = String::from_utf8_lossy(body);
    let text = text.trim_start_matches('\u{feff}').trim_start();
    match text.as_bytes().first() {
        Some(b'<') => opds1::parse(text, &base),
        Some(b'{') => opds2::parse(text, &base),
        _ => Err(Error::Parse("not an OPDS feed".to_owned())),
    }
}

/// Reads an OpenSearch description document and returns its Atom search
/// template, with `{searchTerms}` left in place and relative URLs resolved.
/// `None` when the document has no Atom template.
pub fn parse_search_description(body: &[u8], base: &str) -> Result<Option<String>> {
    let base = parse_url(base)?;
    search::parse_description(&String::from_utf8_lossy(body), &base)
}

/// Reads the links of an OPDS 2.0 indirect acquisition document. The result
/// has the acquisitions that the document names, with `book_name` copied over.
pub(crate) fn parse_indirect(body: &[u8], base: &Url, book_name: &str) -> Result<Vec<Acquisition>> {
    opds2::indirect_acquisitions(&String::from_utf8_lossy(body), base, book_name)
}
