//! OPDS 2.0 (JSON) feeds, and the indirect acquisition documents they link.

use serde_json::Value;
use url::Url;

use crate::error::{Error, Result};
use crate::model::{Acquisition, Entry, Facet, Feed, FeedKind, Link};
use crate::rel;
use crate::resolve::resolve;
use crate::text::{collapse, plain_text};

/// Parses an OPDS 2.0 feed. `base` is the URL the feed came from.
pub(crate) fn parse(text: &str, base: &Url) -> Result<Feed> {
    let doc: Value = serde_json::from_str(text).map_err(|e| Error::Parse(e.to_string()))?;
    let mut feed = Feed {
        title: string_at(&doc, &["metadata", "title"]).unwrap_or_default(),
        url: base.to_string(),
        kind: FeedKind::Navigation,
        entries: Vec::new(),
        next: None,
        prev: None,
        search_template: None,
        search_description: None,
        facets: Vec::new(),
    };

    for link in array(&doc, "links") {
        let Some(relation) = link_rel(link) else {
            continue;
        };
        match relation.as_str() {
            rel::NEXT => feed.next = link_href(link, base),
            rel::PREVIOUS | rel::PREV => feed.prev = link_href(link, base),
            "search" => feed.search_template = search_template(link, base),
            _ => {}
        }
    }

    for facet in array(&doc, "facets") {
        let group = string_at(facet, &["metadata", "title"]).unwrap_or_default();
        for link in array(facet, "links") {
            let Some(href) = link_href(link, base) else {
                continue;
            };
            feed.facets.push(Facet {
                group: group.clone(),
                title: str_field(link, "title").unwrap_or_default().to_owned(),
                count: link.pointer("/properties/numberOfItems").and_then(Value::as_u64),
                active: link_rel(link).as_deref() == Some(rel::SELF) || href == base.as_str(),
                url: href,
            });
        }
    }

    // Navigation first, then collections (groups), then publications, as the
    // plugin orders them.
    let mut navigation = Vec::new();
    let mut collections = Vec::new();
    let mut publications = Vec::new();
    add_navigation(
        &mut navigation,
        &mut feed.facets,
        array(&doc, "navigation"),
        base,
        None,
    );
    for group in array(&doc, "groups") {
        let title = string_at(group, &["metadata", "title"]);
        if let Some(navs) = group.get("navigation").and_then(Value::as_array) {
            add_navigation(&mut navigation, &mut feed.facets, navs, base, title.as_deref());
        } else if let (Some(title), Some(href)) = (
            title,
            array(group, "links").first().and_then(|l| link_href(l, base)),
        ) {
            let count = group
                .pointer("/metadata/numberOfItems")
                .and_then(Value::as_u64)
                .filter(|&n| n != 0);
            collections.push(Entry {
                title,
                count,
                navigation: Some(href),
                ..bare_entry()
            });
        }
    }
    for publication in array(&doc, "publications") {
        publications.push(publication_entry(publication, base));
    }

    feed.entries = navigation;
    feed.entries.extend(collections);
    feed.entries.extend(publications);
    feed.kind = if feed.entries.iter().any(|e| !e.acquisitions.is_empty()) {
        FeedKind::Acquisition
    } else {
        FeedKind::Navigation
    };
    Ok(feed)
}

/// The acquisitions that an indirect acquisition document lists. Only the
/// download relations count, as in KOReader's `getItemFromPublication`.
pub(crate) fn indirect_acquisitions(text: &str, base: &Url, book_name: &str) -> Result<Vec<Acquisition>> {
    let doc: Value = serde_json::from_str(text).map_err(|e| Error::Parse(e.to_string()))?;
    Ok(array(&doc, "links")
        .iter()
        .filter(|link| link_rel(link).is_some_and(|r| rel::is_download(&r)))
        .filter_map(|link| {
            Some(Acquisition {
                url: link_href(link, base)?,
                mime: str_field(link, "type").map(str::to_owned),
                title: str_field(link, "title").map(str::to_owned),
                size: None,
                indirect: false,
                book_name: book_name.to_owned(),
            })
        })
        .collect())
}

fn bare_entry() -> Entry {
    Entry {
        title: String::new(),
        authors: Vec::new(),
        summary: None,
        updated: None,
        count: None,
        links: Vec::new(),
        navigation: None,
        acquisitions: Vec::new(),
        thumbnail: None,
        image: None,
        pse: None,
    }
}

fn add_navigation(
    out: &mut Vec<Entry>,
    facets: &mut Vec<Facet>,
    navigation: &[Value],
    base: &Url,
    group_title: Option<&str>,
) {
    for nav in navigation {
        let (Some(title), Some(href)) = (str_field(nav, "title"), link_href(nav, base)) else {
            continue;
        };
        let relation = link_rel(nav);
        if relation.as_deref() == Some(rel::SELF) {
            continue;
        }
        if let (Some(group), Some(rel::FACET)) = (group_title, relation.as_deref()) {
            facets.push(Facet {
                group: group.to_owned(),
                title: title.to_owned(),
                count: nav.pointer("/properties/numberOfItems").and_then(Value::as_u64),
                active: href == base.as_str(),
                url: href,
            });
            continue;
        }
        out.push(Entry {
            title: match group_title {
                Some(group) => format!("{group} - {title}"),
                None => title.to_owned(),
            },
            count: nav.pointer("/properties/numberOfItems").and_then(Value::as_u64),
            navigation: Some(href),
            ..bare_entry()
        });
    }
}

fn publication_entry(publication: &Value, base: &Url) -> Entry {
    let metadata = publication.get("metadata").unwrap_or(&Value::Null);
    let title = string_at(metadata, &["title"]).unwrap_or_else(|| "Unknown".to_owned());
    let authors: Vec<String> = metadata
        .get("author")
        .map(names)
        .unwrap_or_default()
        .into_iter()
        .map(|name| collapse(&name))
        .filter(|name| !name.is_empty())
        .collect();
    let author_text = if authors.is_empty() {
        "Unknown Author".to_owned()
    } else {
        authors.join(", ")
    };
    let book_name = format!("{author_text} - {title}");

    let mut entry = Entry {
        title,
        authors,
        summary: string_at(metadata, &["description"]).map(|d| plain_text(&d)),
        updated: string_at(metadata, &["modified"]),
        ..bare_entry()
    };

    for link in array(publication, "links") {
        let Some(href) = link_href(link, base) else {
            continue;
        };
        let relation = link_rel(link);
        entry.links.push(Link {
            rel: relation.clone(),
            href: href.clone(),
            mime: str_field(link, "type").map(str::to_owned),
            title: str_field(link, "title").map(str::to_owned),
        });
        let Some(relation) = relation else {
            continue;
        };
        if relation == rel::SELF || relation == rel::BORROW || !rel::is_acquisition(&relation) {
            continue;
        }
        let indirect = link
            .pointer("/properties/indirectAcquisition/0/type")
            .and_then(Value::as_str);
        entry.acquisitions.push(Acquisition {
            url: href,
            mime: indirect
                .map(str::to_owned)
                .or_else(|| str_field(link, "type").map(str::to_owned)),
            title: str_field(link, "title").map(str::to_owned),
            size: None,
            indirect: indirect.is_some(),
            book_name: book_name.clone(),
        });
    }

    let images = array(publication, "images");
    let mut thumbnail = None;
    let mut image = None;
    for link in images {
        let Some(href) = link_href(link, base) else {
            continue;
        };
        match link_rel(link).as_deref() {
            Some(r) if rel::is_thumbnail(r) => thumbnail = Some(href),
            Some(r) if rel::is_image(r) => image = Some(href),
            _ => {}
        }
    }
    // Without a thumbnail relation, the first image stands in for one.
    if thumbnail.is_none() {
        thumbnail = images.first().and_then(|link| link_href(link, base));
    }
    entry.image = image.or_else(|| thumbnail.clone());
    entry.thumbnail = thumbnail;
    entry
}

/// Turns an OPDS 2.0 search link into a template with `{searchTerms}`.
fn search_template(link: &Value, base: &Url) -> Option<String> {
    let raw = str_field(link, "href")?;
    let Some(start) = raw.find('{') else {
        return resolve(base, raw);
    };
    let head = &raw[..start];
    let template = if head.contains("query") {
        format!("{head}{{searchTerms}}")
    } else if head.contains('?') {
        format!("{head}&query={{searchTerms}}")
    } else {
        format!("{head}?query={{searchTerms}}")
    };
    resolve(base, &template)
}

/// The first relation of a link. The relation may be a string or a list.
fn link_rel(link: &Value) -> Option<String> {
    match link.get("rel")? {
        Value::String(s) => Some(s.clone()),
        Value::Array(list) => list.first().and_then(Value::as_str).map(str::to_owned),
        _ => None,
    }
}

fn link_href(link: &Value, base: &Url) -> Option<String> {
    str_field(link, "href").and_then(|href| resolve(base, href))
}

fn str_field<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn string_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().map(collapse).filter(|s| !s.is_empty())
}

fn array<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// Names from an author value: a string, an object with `name`, or a list of
/// either.
fn names(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => vec![s.clone()],
        Value::Object(_) => value.get("name").map(names).unwrap_or_default(),
        Value::Array(list) => list.iter().flat_map(names).collect(),
        _ => Vec::new(),
    }
}
