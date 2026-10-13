//! Relative URL resolution, URI templates and percent coding.

use url::Url;

use crate::error::{Error, Result};

/// Parses a catalog or feed address. Only absolute URLs are accepted.
pub(crate) fn parse_url(text: &str) -> Result<Url> {
    let url = Url::parse(text.trim()).map_err(|_| Error::InvalidUrl(text.to_owned()))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        other => Err(Error::UnsupportedScheme(other.to_owned())),
    }
}

/// Resolves `href` against `base`. A `{...}` template in `href` (for example
/// `{searchTerms}` or `{pageNumber}`) stays as written.
pub(crate) fn resolve(base: &Url, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    let (masked, templates) = mask_templates(href);
    let joined = base.join(&masked).ok()?;
    Some(unmask_templates(joined.as_str(), &templates))
}

const MASK: &str = "kaiatpl";

/// Replaces each `{...}` with a plain token, so the URL parser does not
/// percent-encode the braces.
fn mask_templates(text: &str) -> (String, Vec<String>) {
    let mut out = String::with_capacity(text.len());
    let mut templates = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        out.push_str(&rest[..open]);
        templates.push(rest[open..open + close + 1].to_owned());
        out.push_str(&format!("{MASK}{}x", templates.len() - 1));
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    (out, templates)
}

fn unmask_templates(text: &str, templates: &[String]) -> String {
    let mut out = text.to_owned();
    for (index, template) in templates.iter().enumerate() {
        out = out.replace(&format!("{MASK}{index}x"), template);
    }
    out
}

/// Decodes `%XX` sequences. `+` stays as written, as in KOReader's `urlDecode`.
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Encodes a query value. Only ASCII letters, digits and `-._~` stay as they
/// are, as in KOReader's `urlEncode`.
pub(crate) fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(byte));
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The scheme, host and port of a URL. Credentials go only to the origin of
/// the catalog they belong to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Origin {
    scheme: String,
    host: String,
    port: u16,
}

impl Origin {
    pub(crate) fn of(url: &Url) -> Option<Self> {
        Some(Self {
            scheme: url.scheme().to_owned(),
            host: url.host_str()?.to_owned(),
            port: url.port_or_known_default()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://example.org/opds/root.xml").expect("valid base")
    }

    #[test]
    fn resolves_relative_and_absolute_links() {
        assert_eq!(
            resolve(&base(), "new/books.xml").as_deref(),
            Some("https://example.org/opds/new/books.xml")
        );
        assert_eq!(
            resolve(&base(), "/ebooks/1.epub").as_deref(),
            Some("https://example.org/ebooks/1.epub")
        );
        assert_eq!(
            resolve(&base(), "http://other.example/x").as_deref(),
            Some("http://other.example/x")
        );
    }

    #[test]
    fn keeps_templates_unencoded() {
        assert_eq!(
            resolve(&base(), "/search/{searchTerms}").as_deref(),
            Some("https://example.org/search/{searchTerms}")
        );
        assert_eq!(
            resolve(&base(), "/pages?n={pageNumber}&w={maxWidth}").as_deref(),
            Some("https://example.org/pages?n={pageNumber}&w={maxWidth}")
        );
    }

    #[test]
    fn empty_href_resolves_to_none() {
        assert_eq!(resolve(&base(), "  "), None);
    }

    #[test]
    fn rejects_other_schemes() {
        assert_eq!(
            parse_url("ftp://example.org/x"),
            Err(Error::UnsupportedScheme("ftp".to_owned()))
        );
        assert!(matches!(parse_url("not a url"), Err(Error::InvalidUrl(_))));
    }

    #[test]
    fn percent_coding_matches_koreader() {
        assert_eq!(encode_component("a b&c~d.e_f-g"), "a%20b%26c~d.e_f-g");
        assert_eq!(percent_decode("Mark%20Twain%2Bx"), "Mark Twain+x");
        assert_eq!(percent_decode("a%20b+c"), "a b+c");
        assert_eq!(percent_decode("bad%zz%2"), "bad%zz%2");
    }
}
