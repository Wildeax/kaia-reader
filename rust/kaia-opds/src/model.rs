//! Parsed catalog data. All URLs in these types are absolute.

use crate::files;

/// One OPDS feed: a navigation list, an acquisition list, or a mix of both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    pub title: String,
    /// The URL the feed was read from, after redirects. Relative links were
    /// resolved against it.
    pub url: String,
    pub kind: FeedKind,
    pub entries: Vec<Entry>,
    /// The next page of this feed, from a `next` link.
    pub next: Option<String>,
    /// The previous page of this feed, from a `previous` or `prev` link.
    pub prev: Option<String>,
    /// A search URL with a `{searchTerms}` placeholder. Set by Calibre-style
    /// feeds that link a template directly.
    pub search_template: Option<String>,
    /// The URL of an OpenSearch description document. Call
    /// `Client::search` to fetch it and find the template.
    pub search_description: Option<String>,
    pub facets: Vec<Facet>,
}

/// What a feed lists. A feed with at least one downloadable entry is an
/// acquisition feed. Any other feed is a navigation feed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedKind {
    Navigation,
    Acquisition,
}

/// One filter that a feed offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facet {
    /// The group the filter belongs to, for example "Formats". `Filters` when
    /// the feed sets no group.
    pub group: String,
    pub title: String,
    pub url: String,
    pub count: Option<u64>,
    /// True when the feed is currently filtered by this facet.
    pub active: bool,
}

/// One item in a feed: a sub-catalog, a book, or both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub title: String,
    pub authors: Vec<String>,
    /// Plain text. Tags are removed from HTML content.
    pub summary: Option<String>,
    pub updated: Option<String>,
    /// Item count from the feed, for example the number of books in a group.
    pub count: Option<u64>,
    /// All links of the entry that have an absolute URL.
    pub links: Vec<Link>,
    /// The sub-feed this entry opens, if it is a navigation item.
    pub navigation: Option<String>,
    /// Every acquisition link, including types the reader cannot open. Use
    /// [`Entry::downloads`] to get the ones with a supported file type.
    pub acquisitions: Vec<Acquisition>,
    pub thumbnail: Option<String>,
    pub image: Option<String>,
    /// Page streaming (OPDS-PSE), for comics and other paged documents.
    pub pse: Option<Pse>,
}

impl Entry {
    /// Acquisitions with a file type the reader can open.
    pub fn downloads(&self) -> impl Iterator<Item = &Acquisition> {
        self.acquisitions.iter().filter(|a| a.file_type().is_some())
    }
}

/// A link inside an entry or a feed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub rel: Option<String>,
    pub href: String,
    pub mime: Option<String>,
    pub title: Option<String>,
}

/// A file offered for download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Acquisition {
    pub url: String,
    pub mime: Option<String>,
    /// The label the server gives the format, for example "EPUB (no images)".
    pub title: Option<String>,
    /// Size from the `length` attribute, when the server sends it.
    pub size: Option<u64>,
    /// True for an OPDS 2.0 indirect acquisition. Its URL is a feed, not a
    /// file. Resolve it with `Client::resolve_indirect` first.
    pub indirect: bool,
    /// "Author - Title" as the entry names it. Used for the file name.
    pub book_name: String,
}

impl Acquisition {
    /// The extension the reader uses for this file, or `None` when the reader
    /// cannot open it. The extension in the URL wins over the MIME type.
    pub fn file_type(&self) -> Option<&'static str> {
        files::file_type(&self.url, self.mime.as_deref())
    }

    /// The file name the reader suggests, for example `Jane Doe - Book.epub`.
    /// `None` when the file type is not supported.
    pub fn file_name(&self) -> Option<String> {
        self.file_type()
            .map(|ext| files::book_file_name(&self.book_name, ext))
    }
}

/// OPDS-PSE page streaming. A page is one image at a URL built from the
/// template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pse {
    /// Absolute URL with `{pageNumber}` and `{maxWidth}` placeholders.
    pub template: String,
    /// Number of pages in the document.
    pub count: u32,
    /// The last page the server recorded for this reader, 1-based as the
    /// server sends it. `None` when zero or not set.
    pub last_read: Option<u32>,
    pub mime: Option<String>,
    pub title: Option<String>,
}

impl Pse {
    /// The URL of one page. `index` is 0-based, as the server counts pages.
    /// `max_width` is the largest width the reader can show, in pixels.
    pub fn page_url(&self, index: u32, max_width: u32) -> String {
        self.template
            .replace("{pageNumber}", &index.to_string())
            .replace("{maxWidth}", &max_width.to_string())
    }
}
