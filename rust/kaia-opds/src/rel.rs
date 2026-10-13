//! Link relations and media types that OPDS feeds use, as KOReader's
//! `opdsbrowser.lua` matches them.

/// Media type of an Atom feed. Matched as a substring, so parameters such as
/// `;profile=opds-catalog` are allowed.
pub(crate) const ATOM: &str = "application/atom+xml";
/// Media type of an OpenSearch description document.
pub(crate) const OPENSEARCH: &str = "application/opensearchdescription+xml";

pub(crate) const NEXT: &str = "next";
pub(crate) const PREVIOUS: &str = "previous";
pub(crate) const PREV: &str = "prev";
pub(crate) const SELF: &str = "self";
pub(crate) const ALTERNATE: &str = "alternate";
pub(crate) const SUBSECTION: &str = "subsection";
pub(crate) const FACET: &str = "http://opds-spec.org/facet";
pub(crate) const BORROW: &str = "http://opds-spec.org/acquisition/borrow";
pub(crate) const PSE_STREAM: &str = "http://vaemendis.net/opds-pse/stream";

const ACQUISITION_PREFIX: &str = "http://opds-spec.org/acquisition";

/// Relations that open a sub-catalog.
const CATALOG: &[&str] = &[
    "subsection",
    "http://opds-spec.org/subsection",
    "http://opds-spec.org/crawlable",
    "http://opds-spec.org/sort/popular",
    "http://opds-spec.org/sort/new",
];

/// Relations that name a thumbnail image. The last one in a feed wins.
const THUMBNAIL: &[&str] = &[
    "http://opds-spec.org/image/thumbnail",
    "http://opds-spec.org/thumbnail",
    "x-stanza-cover-image-thumbnail",
];

/// Relations that name a cover image.
const IMAGE: &[&str] = &[
    "http://opds-spec.org/image",
    "http://opds-spec.org/cover",
    "x-stanza-cover-image",
];

/// Relations that name a file to download in OPDS 2.0 indirect acquisitions.
const DOWNLOAD: &[&str] = &[
    "download",
    "publication",
    "http://opds-spec.org/acquisition",
    "http://opds-spec.org/acquisition/open-access",
];

/// True for the acquisition relations (`http://opds-spec.org/acquisition...`),
/// including `open-access` and `buy`.
pub(crate) fn is_acquisition(rel: &str) -> bool {
    rel.starts_with(ACQUISITION_PREFIX)
}

pub(crate) fn is_catalog(rel: &str) -> bool {
    CATALOG.contains(&rel)
}

pub(crate) fn is_thumbnail(rel: &str) -> bool {
    THUMBNAIL.contains(&rel)
}

pub(crate) fn is_image(rel: &str) -> bool {
    IMAGE.contains(&rel)
}

pub(crate) fn is_download(rel: &str) -> bool {
    DOWNLOAD.contains(&rel)
}
