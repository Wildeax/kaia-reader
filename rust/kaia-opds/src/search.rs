//! OpenSearch descriptions and search templates.

use url::Url;

use crate::error::{Error, Result};
use crate::rel;
use crate::resolve::{encode_component, resolve};
use crate::xml::parse_root;

/// Finds the Atom search template in an OpenSearch description.
pub(crate) fn parse_description(text: &str, base: &Url) -> Result<Option<String>> {
    let root = parse_root(text)?;
    if root.local() != "OpenSearchDescription" {
        return Err(Error::Parse("not an OpenSearch description".to_owned()));
    }
    for url in root.children_named("Url") {
        let is_atom = url.attr("type").is_some_and(|ty| ty.contains(rel::ATOM));
        if let (true, Some(template)) = (is_atom, url.attr("template")) {
            return Ok(resolve(base, template));
        }
    }
    Ok(None)
}

/// Fills a search template. `{searchTerms}` becomes the encoded terms, and
/// optional parameters (`{name?}`) are removed. Other placeholders stay.
pub(crate) fn expand(template: &str, terms: &str) -> String {
    let encoded = encode_component(terms);
    let mut out = String::with_capacity(template.len() + encoded.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(len) = rest[open..].find('}') else {
            break;
        };
        out.push_str(&rest[..open]);
        let name = &rest[open + 1..open + len];
        if name == "searchTerms" {
            out.push_str(&encoded);
        } else if !name.ends_with('?') {
            out.push_str(&rest[open..open + len + 1]);
        }
        rest = &rest[open + len + 1..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_terms_and_drops_optional_parameters() {
        assert_eq!(
            expand(
                "https://x.org/s?q={searchTerms}&count={count?}&p={pageNumber}",
                "Mark Twain & co"
            ),
            "https://x.org/s?q=Mark%20Twain%20%26%20co&count=&p={pageNumber}"
        );
    }

    #[test]
    fn reads_the_atom_template_from_a_description() {
        let base = Url::parse("https://x.org/opds/osd").expect("url");
        let xml = r#"<?xml version="1.0"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
  <ShortName>Books</ShortName>
  <Url type="text/html" template="/html?q={searchTerms}"/>
  <Url type="application/atom+xml;profile=opds-catalog" template="/search?q={searchTerms}"/>
</OpenSearchDescription>"#;
        assert_eq!(
            parse_description(xml, &base).expect("parses"),
            Some("https://x.org/search?q={searchTerms}".to_owned())
        );
        assert_eq!(
            parse_description("<feed/>", &base),
            Err(Error::Parse("not an OpenSearch description".to_owned()))
        );
    }
}
