//! A small element tree built on `quick-xml`.
//!
//! Feeds are read into this tree once. The OPDS code then looks elements up
//! by local name, so the namespace prefixes that feeds use (`opds:`, `thr:`,
//! `pse:`, `dc:`) do not matter. Text and child elements stay in document
//! order, so mixed content reads correctly.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::error::{Error, Result};
use crate::text::is_block;

/// Deepest element nesting the parser accepts. Real feeds nest far less.
const MAX_DEPTH: usize = 128;

/// One piece of an element's content.
#[derive(Debug)]
pub(crate) enum Child {
    Text(String),
    Element(Node),
}

/// One element.
#[derive(Debug, Default)]
pub(crate) struct Node {
    /// The name as written, with its prefix if it has one.
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Child>,
}

impl Node {
    /// The name without the prefix.
    pub fn local(&self) -> &str {
        local_name(&self.name)
    }

    /// The child elements, in document order.
    pub fn elements(&self) -> impl Iterator<Item = &Node> {
        self.children.iter().filter_map(|child| match child {
            Child::Element(node) => Some(node),
            Child::Text(_) => None,
        })
    }

    /// The first child element with this local name.
    pub fn child(&self, local: &str) -> Option<&Node> {
        self.elements().find(|c| c.local() == local)
    }

    /// All child elements with this local name, in document order.
    pub fn children_named<'a>(&'a self, local: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.elements().filter(move |c| c.local() == local)
    }

    /// An attribute written without a prefix, for example `href`.
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// An attribute written with a prefix, matched by its local name, for
    /// example `opds:facetGroup` or `pse:count`.
    pub fn prefixed_attr(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(key, _)| key.contains(':') && local_name(key) == local)
            .map(|(_, value)| value.as_str())
    }

    /// The text of this element and its descendants, in document order. Block
    /// elements (paragraphs, list items, line breaks) get a space on each side
    /// so their words do not run together. Inline elements add nothing.
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        for child in &self.children {
            match child {
                Child::Text(text) => out.push_str(text),
                Child::Element(node) => {
                    let block = is_block(node.local());
                    if block {
                        out.push(' ');
                    }
                    out.push_str(&node.text_content());
                    if block {
                        out.push(' ');
                    }
                }
            }
        }
        out
    }
}

/// The part of a qualified name after the colon.
pub(crate) fn local_name(qualified: &str) -> &str {
    qualified.rsplit(':').next().unwrap_or(qualified)
}

fn parse_error(message: impl std::fmt::Display) -> Error {
    Error::Parse(message.to_string())
}

/// Parses a document and returns its root element.
pub(crate) fn parse_root(text: &str) -> Result<Node> {
    let mut reader = Reader::from_str(text);
    reader.config_mut().check_end_names = true;
    // The bottom entry holds the document itself and is never closed.
    let mut stack: Vec<Node> = vec![Node::default()];
    loop {
        match reader.read_event().map_err(parse_error)? {
            Event::Start(start) => {
                if stack.len() > MAX_DEPTH {
                    return Err(parse_error("elements are nested too deeply"));
                }
                stack.push(element(&start)?);
            }
            Event::Empty(start) => {
                let node = element(&start)?;
                push_child(&mut stack, Child::Element(node))?;
            }
            Event::End(end) => {
                if stack.len() < 2 {
                    return Err(parse_error("closing tag without an opening tag"));
                }
                let node = stack
                    .pop()
                    .ok_or_else(|| parse_error("unexpected end of document"))?;
                if node.name.as_bytes() != end.name().as_ref() {
                    let closing = String::from_utf8_lossy(end.name().as_ref()).into_owned();
                    return Err(parse_error(format!(
                        "closing tag </{closing}> does not match <{}>",
                        node.name
                    )));
                }
                push_child(&mut stack, Child::Element(node))?;
            }
            Event::Text(text) => {
                let value = text.decode().map_err(parse_error)?;
                push_text(&mut stack, &value)?;
            }
            Event::CData(data) => {
                let value = data.decode().map_err(parse_error)?;
                push_text(&mut stack, &value)?;
            }
            Event::GeneralRef(reference) => {
                let value = reference_text(&reference)?;
                push_text(&mut stack, &value)?;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if stack.len() != 1 {
        let open = stack.last().map_or("", |node| node.name.as_str());
        return Err(parse_error(format!("element <{open}> is not closed")));
    }
    let document = stack.pop().ok_or_else(|| parse_error("empty document"))?;
    document
        .children
        .into_iter()
        .find_map(|child| match child {
            Child::Element(node) => Some(node),
            Child::Text(_) => None,
        })
        .ok_or_else(|| parse_error("no root element"))
}

fn current(stack: &mut [Node]) -> Result<&mut Node> {
    stack
        .last_mut()
        .ok_or_else(|| parse_error("unexpected end of document"))
}

fn push_child(stack: &mut [Node], child: Child) -> Result<()> {
    current(stack)?.children.push(child);
    Ok(())
}

/// Adds text to the open element. Text that arrives in pieces, such as text
/// around an entity reference, becomes one piece.
fn push_text(stack: &mut [Node], text: &str) -> Result<()> {
    let node = current(stack)?;
    match node.children.last_mut() {
        Some(Child::Text(existing)) => existing.push_str(text),
        _ => node.children.push(Child::Text(text.to_owned())),
    }
    Ok(())
}

fn element(start: &BytesStart<'_>) -> Result<Node> {
    let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
    let mut attrs = Vec::new();
    for attr in start.attributes() {
        let attr = attr.map_err(parse_error)?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        let raw = String::from_utf8_lossy(&attr.value);
        let value = quick_xml::escape::unescape(&raw)
            .map_err(parse_error)?
            .into_owned();
        attrs.push((key, value));
    }
    Ok(Node {
        name,
        attrs,
        children: Vec::new(),
    })
}

/// The character for `&name;` or `&#...;`. Unknown names stay as written.
fn reference_text(reference: &quick_xml::events::BytesRef<'_>) -> Result<String> {
    if reference.is_char_ref() {
        let ch = reference.resolve_char_ref().map_err(parse_error)?;
        return Ok(ch.map(String::from).unwrap_or_default());
    }
    let name = reference.decode().map_err(parse_error)?;
    let value = match name.as_ref() {
        "amp" => "&".to_owned(),
        "lt" => "<".to_owned(),
        "gt" => ">".to_owned(),
        "quot" => "\"".to_owned(),
        "apos" => "'".to_owned(),
        other => format!("&{other};"),
    };
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_tree_with_prefixes_and_references() {
        let root = parse_root(
            r#"<feed xmlns:opds="x"><title>A &amp; B &#233;</title><link opds:facetGroup="F" rel="r"/><entry><![CDATA[raw <b>]]></entry></feed>"#,
        )
        .expect("well formed");
        assert_eq!(root.local(), "feed");
        assert_eq!(
            root.child("title").map(Node::text_content).as_deref(),
            Some("A & B \u{e9}")
        );
        let link = root.child("link").expect("link");
        assert_eq!(link.prefixed_attr("facetGroup"), Some("F"));
        assert_eq!(link.attr("rel"), Some("r"));
        assert_eq!(
            root.child("entry").map(Node::text_content).as_deref(),
            Some("raw <b>")
        );
    }

    #[test]
    fn keeps_mixed_content_in_order() {
        let root = parse_root("<c>Most <i>downloaded</i> books<p>next</p>end</c>").expect("well formed");
        assert_eq!(root.text_content(), "Most downloaded books next end");
    }

    #[test]
    fn rejects_mismatched_and_unclosed_tags() {
        assert!(matches!(parse_root("<feed><entry></feed>"), Err(Error::Parse(_))));
        assert!(matches!(parse_root("<feed><entry>"), Err(Error::Parse(_))));
        assert!(matches!(parse_root("just text"), Err(Error::Parse(_))));
        assert!(matches!(parse_root(""), Err(Error::Parse(_))));
    }

    #[test]
    fn rejects_very_deep_nesting_without_overflowing() {
        let deep = format!("{}{}", "<a>".repeat(5000), "</a>".repeat(5000));
        assert!(matches!(parse_root(&deep), Err(Error::Parse(_))));
    }
}
