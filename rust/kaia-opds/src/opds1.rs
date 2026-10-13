//! OPDS 1.x (Atom) feeds.

use url::Url;

use crate::error::{Error, Result};
use crate::files;
use crate::model::{Acquisition, Entry, Facet, Feed, FeedKind, Link, Pse};
use crate::rel;
use crate::resolve::resolve;
use crate::text::{collapse, plain_text};
use crate::xml::{parse_root, Node};

/// Parses an Atom feed. `base` is the URL the feed came from.
pub(crate) fn parse(text: &str, base: &Url) -> Result<Feed> {
    let root = parse_root(text)?;
    if root.local() != "feed" {
        return Err(Error::Parse("not an Atom feed".to_owned()));
    }
    let mut feed = Feed {
        title: title_of(&root).unwrap_or_default(),
        url: base.to_string(),
        kind: FeedKind::Navigation,
        entries: Vec::new(),
        next: None,
        prev: None,
        search_template: None,
        search_description: None,
        facets: Vec::new(),
    };
    for link in root.children_named("link") {
        apply_feed_link(&mut feed, link, base);
    }
    if feed.search_description.is_some() {
        // The description document gives the template, so a template in the
        // feed is not used.
        feed.search_template = None;
    }
    feed.entries = root
        .children_named("entry")
        .map(|node| entry(node, base))
        .collect();
    feed.kind = if feed
        .entries
        .iter()
        .any(|e| !e.acquisitions.is_empty() || e.pse.is_some())
    {
        FeedKind::Acquisition
    } else {
        FeedKind::Navigation
    };
    Ok(feed)
}

fn title_of(node: &Node) -> Option<String> {
    let title = collapse(&node.child("title")?.text_content());
    (!title.is_empty()).then_some(title)
}

/// Feed-level links. Like KOReader, only links with a `type` are read.
fn apply_feed_link(feed: &mut Feed, link: &Node, base: &Url) {
    let Some(ty) = link.attr("type") else {
        return;
    };
    let rel = link.attr("rel");
    let href = link.attr("href").and_then(|h| resolve(base, h));
    if ty.contains(rel::OPENSEARCH) {
        feed.search_description = href;
        return;
    }
    if rel == Some(rel::FACET) {
        if let Some(href) = href {
            feed.facets.push(Facet {
                group: link.prefixed_attr("facetGroup").unwrap_or("Filters").to_owned(),
                title: link.attr("title").unwrap_or("").to_owned(),
                url: href,
                count: link.prefixed_attr("count").and_then(|c| c.parse().ok()),
                active: link.prefixed_attr("activeFacet") == Some("true"),
            });
        }
        return;
    }
    if !ty.contains(rel::ATOM) {
        return;
    }
    match (rel, href) {
        (Some(rel::NEXT), Some(href)) => feed.next = Some(href),
        (Some(rel::PREVIOUS | rel::PREV), Some(href)) => feed.prev = Some(href),
        // Calibre-style search: an Atom link whose relation contains "search".
        (Some(rel), Some(href)) if rel.contains("search") => feed.search_template = Some(href),
        _ => {}
    }
}

fn entry(node: &Node, base: &Url) -> Entry {
    let title = title_of(node).unwrap_or_else(|| "Unknown".to_owned());
    let authors: Vec<String> = node
        .children_named("author")
        .filter_map(|author| author.child("name"))
        .map(|name| collapse(&name.text_content()))
        .filter(|name| !name.is_empty())
        .collect();
    let author_text = if authors.is_empty() {
        "Unknown Author".to_owned()
    } else {
        authors.join(", ")
    };
    let book_name = format!("{author_text} - {title}");
    let summary = node
        .child("content")
        .or_else(|| node.child("summary"))
        .map(|body| plain_text(&body.text_content()))
        .filter(|text| !text.is_empty());
    let updated = node
        .child("updated")
        .map(|u| collapse(&u.text_content()))
        .filter(|u| !u.is_empty());

    let mut out = Entry {
        title,
        authors,
        summary,
        updated,
        count: None,
        links: Vec::new(),
        navigation: None,
        acquisitions: Vec::new(),
        thumbnail: None,
        image: None,
        pse: None,
    };
    for link in node.children_named("link") {
        apply_entry_link(&mut out, link, base, &book_name);
    }
    out
}

fn apply_entry_link(out: &mut Entry, link: &Node, base: &Url, book_name: &str) {
    let Some(href) = link.attr("href").and_then(|h| resolve(base, h)) else {
        return;
    };
    let rel = link.attr("rel");
    let mime = link.attr("type").map(str::to_owned);
    let title = link.attr("title").map(str::to_owned);
    out.links.push(Link {
        rel: rel.map(str::to_owned),
        href: href.clone(),
        mime: mime.clone(),
        title: title.clone(),
    });

    if mime.as_deref().is_some_and(|m| m.contains(rel::ATOM)) && rel.is_none_or(rel::is_catalog) {
        out.navigation = Some(href.clone());
    }
    if rel.is_none() && title.is_none() {
        return;
    }

    let acquisition = |href: String| Acquisition {
        url: href,
        mime: mime.clone(),
        title: title.clone(),
        size: link.attr("length").and_then(|l| l.parse().ok()),
        indirect: false,
        book_name: book_name.to_owned(),
    };
    match rel {
        Some(rel) if rel == rel::BORROW => {}
        Some(rel) if rel::is_acquisition(rel) => out.acquisitions.push(acquisition(href.clone())),
        Some(rel) if rel == rel::PSE_STREAM => {
            if out.pse.is_none() {
                out.pse = pse_of(link, &href, &mime, &title);
            }
        }
        Some(rel) if rel::is_thumbnail(rel) => out.thumbnail = Some(href.clone()),
        Some(rel) if rel::is_image(rel) => out.image = Some(href.clone()),
        other => {
            let supported = mime.as_deref().is_some_and(files::is_supported_mime);
            if other != Some(rel::ALTERNATE) && supported {
                out.acquisitions.push(acquisition(href.clone()));
            }
        }
    }

    // Some catalogs label a PDF only by its title, or name it application/pdf
    // without a file extension. KOReader adds `.pdf` to the link so the file
    // gets the right type.
    let labelled_pdf = title.as_deref() == Some("pdf")
        || (mime.as_deref() == Some("application/pdf") && rel != Some(rel::SUBSECTION));
    if labelled_pdf {
        if let Some(pdf) = with_pdf_extension(&href) {
            out.acquisitions.push(Acquisition {
                url: pdf,
                mime: Some("application/pdf".to_owned()),
                title: title.clone(),
                size: None,
                indirect: false,
                book_name: book_name.to_owned(),
            });
        }
    }
}

fn pse_of(link: &Node, href: &str, mime: &Option<String>, title: &Option<String>) -> Option<Pse> {
    let count = link.prefixed_attr("count")?.parse::<u32>().ok()?;
    let last_read = link
        .prefixed_attr("lastRead")
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&page| page > 0);
    Some(Pse {
        template: href.to_owned(),
        count,
        last_read,
        mime: mime.clone(),
        title: title.clone(),
    })
}

/// The same URL with `.pdf` added to the path, or `None` when the path
/// already ends in `/pdf/` or has a `pdf` extension.
fn with_pdf_extension(href: &str) -> Option<String> {
    let mut url = Url::parse(href).ok()?;
    let path = url.path().to_owned();
    if path.ends_with("/pdf/") {
        return None;
    }
    if path.rsplit_once('.').map(|(_, ext)| ext) == Some("pdf") {
        return None;
    }
    url.set_path(&format!("{path}.pdf"));
    Some(url.to_string())
}
