//! Parsing of feed fixtures. No network access.

use kaia_opds::{parse_feed, parse_search_description, Error, FeedKind};

const GUTENBERG: &[u8] = include_bytes!("fixtures/gutenberg_nav.xml");
const ACQUISITION: &[u8] = include_bytes!("fixtures/acquisition.xml");
const ACQUISITION_PAGE2: &[u8] = include_bytes!("fixtures/acquisition_page2.xml");
const OPENSEARCH: &[u8] = include_bytes!("fixtures/opensearch.xml");
const RELATIVE: &[u8] = include_bytes!("fixtures/relative.xml");
const CALIBRE: &[u8] = include_bytes!("fixtures/calibre.xml");
const PSE: &[u8] = include_bytes!("fixtures/pse.xml");
const MALFORMED: &[u8] = include_bytes!("fixtures/malformed.xml");
const OPDS2: &[u8] = include_bytes!("fixtures/opds2_feed.json");

const GUTENBERG_URL: &str = "https://m.gutenberg.org/ebooks.opds/?format=opds";

#[test]
fn navigation_feed_lists_sub_catalogs_in_order() {
    let feed = parse_feed(GUTENBERG, GUTENBERG_URL).expect("parses");
    assert_eq!(feed.title, "Project Gutenberg");
    assert_eq!(feed.kind, FeedKind::Navigation);
    assert_eq!(
        feed.next.as_deref(),
        Some("https://m.gutenberg.org/ebooks.opds/?format=opds&start_index=26")
    );
    assert_eq!(feed.prev, None);
    assert_eq!(
        feed.search_description.as_deref(),
        Some("https://www.gutenberg.org/catalog/osd-books.xml")
    );
    assert_eq!(feed.search_template, None);

    let titles: Vec<&str> = feed.entries.iter().map(|e| e.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Search for books", "Bookshelves", "Popular", "Recent additions"]
    );

    let bookshelves = &feed.entries[1];
    assert_eq!(
        bookshelves.navigation.as_deref(),
        Some("https://m.gutenberg.org/ebooks/bookshelves.opds")
    );
    assert!(bookshelves.acquisitions.is_empty());
    assert_eq!(bookshelves.summary.as_deref(), Some("Books grouped by shelf"));

    // XHTML content is flattened to plain text.
    assert_eq!(feed.entries[2].summary.as_deref(), Some("Most downloaded books"));
    assert_eq!(
        feed.entries[3].navigation.as_deref(),
        Some("https://m.gutenberg.org/ebooks/recent.opds")
    );
}

#[test]
fn acquisition_feed_offers_each_format_with_its_file_type() {
    let feed = parse_feed(ACQUISITION, "https://m.gutenberg.org/books/new").expect("parses");
    assert_eq!(feed.kind, FeedKind::Acquisition);
    assert_eq!(
        feed.next.as_deref(),
        Some("https://m.gutenberg.org/books/new?page=2")
    );
    assert_eq!(
        feed.prev.as_deref(),
        Some("https://m.gutenberg.org/books/new?page=0")
    );

    let facets: Vec<(&str, &str, Option<u64>, bool)> = feed
        .facets
        .iter()
        .map(|f| (f.group.as_str(), f.title.as_str(), f.count, f.active))
        .collect();
    assert_eq!(
        facets,
        [
            ("Languages", "English", Some(120), true),
            ("Languages", "French", Some(8), false),
        ]
    );
    assert_eq!(feed.facets[0].url, "https://m.gutenberg.org/books/new?lang=en");

    let austen = &feed.entries[0];
    assert_eq!(austen.title, "Pride and Prejudice");
    assert_eq!(austen.authors, ["Jane Austen"]);
    assert_eq!(austen.summary.as_deref(), Some("A novel about \"manners\"."));
    assert_eq!(austen.updated.as_deref(), Some("2026-09-01T00:00:00Z"));
    assert_eq!(
        austen.thumbnail.as_deref(),
        Some("https://m.gutenberg.org/cache/epub/1342/cover.small.jpg")
    );
    assert_eq!(
        austen.image.as_deref(),
        Some("https://m.gutenberg.org/cache/epub/1342/cover.jpg")
    );

    // The `.epub.noimages` link has an unknown extension, so the MIME type decides.
    let formats: Vec<(Option<&str>, Option<&str>)> = austen
        .downloads()
        .map(|a| (a.file_type(), a.title.as_deref()))
        .collect();
    assert_eq!(
        formats,
        [
            (Some("epub"), Some("EPUB (no images)")),
            (Some("mobi"), Some("Kindle")),
            (Some("txt"), Some("Plain text")),
        ]
    );
    // The unsupported binary stays in the list but is not offered.
    assert_eq!(austen.acquisitions.len(), 4);
    assert_eq!(austen.acquisitions[0].size, Some(312_000));
    assert_eq!(
        austen.acquisitions[0].file_name().as_deref(),
        Some("Jane Austen - Pride and Prejudice.epub")
    );
    assert_eq!(austen.acquisitions[3].file_type(), None);
    assert_eq!(austen.acquisitions[3].file_name(), None);
    // The alternate HTML link is kept as a link, not as a file.
    assert!(austen.links.iter().any(|l| l.rel.as_deref() == Some("alternate")));
}

#[test]
fn entries_without_authors_and_pdf_titles_follow_the_plugin() {
    let feed = parse_feed(ACQUISITION, "https://m.gutenberg.org/books/new").expect("parses");

    let ulysses = &feed.entries[1];
    assert!(ulysses.authors.is_empty());
    assert_eq!(ulysses.summary.as_deref(), Some("Dublin, 1904"));
    // Labelled `pdf` by title with no extension: a `.pdf` copy of the link is added.
    let pdf = ulysses
        .acquisitions
        .iter()
        .find(|a| a.url.ends_with(".pdf"))
        .expect("pdf link added");
    assert_eq!(pdf.url, "https://m.gutenberg.org/files/4300.pdf");
    assert_eq!(pdf.file_type(), Some("pdf"));
    assert_eq!(pdf.file_name().as_deref(), Some("Unknown Author - Ulysses.pdf"));
    assert_eq!(ulysses.downloads().count(), 2);

    let pamphlet = &feed.entries[2];
    // The borrow link is not a sub-catalog and not a file.
    assert_eq!(pamphlet.navigation, None);
    assert_eq!(pamphlet.acquisitions.len(), 1);
    assert_eq!(pamphlet.acquisitions[0].file_type(), Some("zip"));
}

#[test]
fn next_page_has_its_own_entries() {
    let feed = parse_feed(ACQUISITION_PAGE2, "https://m.gutenberg.org/books/new?page=2").expect("parses");
    assert_eq!(feed.title, "New Books, page 2");
    assert_eq!(feed.next, None);
    assert_eq!(feed.prev.as_deref(), Some("https://m.gutenberg.org/books/new"));
    assert_eq!(feed.entries.len(), 1);
    assert_eq!(feed.entries[0].authors, ["Mary Shelley"]);
}

#[test]
fn relative_links_resolve_against_the_feed_url() {
    let feed = parse_feed(RELATIVE, "https://example.org/opds/catalog/root.xml").expect("parses");
    assert_eq!(
        feed.next.as_deref(),
        Some("https://example.org/opds/catalog/root.xml?page=2")
    );
    assert_eq!(
        feed.prev.as_deref(),
        Some("https://example.org/opds/shelf/new.xml")
    );

    let walden = &feed.entries[0];
    assert_eq!(
        walden.thumbnail.as_deref(),
        Some("https://cdn.example.net/cover.jpg")
    );
    assert_eq!(
        walden.image.as_deref(),
        Some("https://example.org/opds/catalog/covers/walden.jpg")
    );
    assert_eq!(
        walden.navigation.as_deref(),
        Some("https://example.org/opds/catalog/walden/chapters.xml")
    );
    assert_eq!(
        walden.acquisitions[0].url,
        "https://example.org/opds/files/walden.epub"
    );
    assert_eq!(walden.acquisitions[0].file_type(), Some("epub"));
    assert_eq!(feed.kind, FeedKind::Acquisition);
}

#[test]
fn calibre_feed_has_a_search_template_facets_and_sizes() {
    let feed = parse_feed(CALIBRE, "http://192.168.1.20:8080/opds").expect("parses");
    assert_eq!(
        feed.search_template.as_deref(),
        Some("http://192.168.1.20:8080/search/{searchTerms}")
    );
    assert_eq!(feed.search_description, None);
    assert_eq!(feed.facets.len(), 2);
    assert_eq!(feed.facets[0].group, "Formats");
    assert_eq!(feed.facets[0].count, Some(42));
    assert!(feed.facets[1].active);

    let dune = &feed.entries[0];
    assert_eq!(
        dune.acquisitions[0].url,
        "http://192.168.1.20:8080/download/7/epub/"
    );
    assert_eq!(dune.acquisitions[0].size, Some(1_048_576));
    // The download path has no extension, so the MIME type gives `epub`.
    assert_eq!(dune.acquisitions[0].file_type(), Some("epub"));
    assert_eq!(
        dune.thumbnail.as_deref(),
        Some("http://192.168.1.20:8080/get/thumb/7")
    );
    assert_eq!(
        dune.image.as_deref(),
        Some("http://192.168.1.20:8080/get/cover/7")
    );
}

#[test]
fn pse_entries_carry_the_page_template_and_last_page() {
    let feed = parse_feed(PSE, "https://comics.example/opds/").expect("parses");
    let watchmen = &feed.entries[0];
    let pse = watchmen.pse.as_ref().expect("pse link");
    assert_eq!(pse.count, 12);
    assert_eq!(pse.last_read, Some(5));
    assert_eq!(pse.mime.as_deref(), Some("image/jpeg"));
    assert_eq!(
        pse.page_url(0, 800),
        "https://comics.example/pse/7/0?maxWidth=800"
    );
    assert_eq!(
        pse.page_url(11, 600),
        "https://comics.example/pse/7/11?maxWidth=600"
    );
    assert_eq!(watchmen.downloads().count(), 1);
    assert_eq!(watchmen.acquisitions[0].file_type(), Some("cbz"));

    // A stream link without pse:count gives no page stream, as in the plugin.
    assert!(feed.entries[1].pse.is_none());
    assert_eq!(feed.kind, FeedKind::Acquisition);
}

#[test]
fn opensearch_description_gives_the_atom_template() {
    let template = parse_search_description(OPENSEARCH, "https://www.gutenberg.org/catalog/osd-books.xml")
        .expect("parses");
    assert_eq!(
        template.as_deref(),
        Some("https://www.gutenberg.org/ebooks/search.opds/?query={searchTerms}")
    );
}

#[test]
fn opds2_feed_is_read_in_the_plugin_order() {
    let feed = parse_feed(OPDS2, "https://standardebooks.org/opds/feed.json").expect("parses");
    assert_eq!(feed.title, "Standard Ebooks");
    assert_eq!(feed.kind, FeedKind::Acquisition);
    assert_eq!(
        feed.next.as_deref(),
        Some("https://standardebooks.org/opds/feed.json?page=2")
    );
    assert_eq!(
        feed.search_template.as_deref(),
        Some("https://standardebooks.org/opds/search?query={searchTerms}")
    );
    assert_eq!(feed.facets.len(), 2);
    assert_eq!(feed.facets[0].group, "Sort");
    assert!(feed.facets[0].active, "the self link marks the active sort");
    assert_eq!(
        feed.facets[1].url,
        "https://standardebooks.org/opds/feed.json?sort=new"
    );

    let titles: Vec<&str> = feed.entries.iter().map(|e| e.title.as_str()).collect();
    // Navigation entries, group navigation included, come before collections.
    assert_eq!(
        titles,
        ["Subjects", "Authors - A", "Featured", "Pride and Prejudice"]
    );
    assert_eq!(
        feed.entries[0].navigation.as_deref(),
        Some("https://standardebooks.org/opds/subjects.json")
    );
    assert_eq!(
        feed.entries[1].navigation.as_deref(),
        Some("https://standardebooks.org/opds/authors/a.json")
    );
    assert_eq!(feed.entries[2].count, Some(2));
    assert_eq!(
        feed.entries[2].navigation.as_deref(),
        Some("https://standardebooks.org/opds/featured.json")
    );

    let austen = &feed.entries[3];
    assert_eq!(austen.authors, ["Jane Austen"]);
    assert_eq!(austen.summary.as_deref(), Some("A novel about manners."));
    assert_eq!(austen.updated.as_deref(), Some("2026-09-01T00:00:00Z"));
    assert_eq!(
        austen.thumbnail.as_deref(),
        Some("https://standardebooks.org/img/1342-thumb.jpg")
    );
    assert_eq!(
        austen.image.as_deref(),
        Some("https://standardebooks.org/img/1342.jpg")
    );
    assert_eq!(austen.acquisitions.len(), 2);
    assert!(!austen.acquisitions[0].indirect);
    assert_eq!(austen.acquisitions[0].file_type(), Some("epub"));
    // The second link points to a document that lists the files.
    assert!(austen.acquisitions[1].indirect);
    assert_eq!(austen.acquisitions[1].file_type(), Some("epub"));
    assert_eq!(austen.downloads().count(), 2);
}

#[test]
fn malformed_feed_is_an_error_not_a_panic() {
    assert!(matches!(
        parse_feed(MALFORMED, GUTENBERG_URL),
        Err(Error::Parse(_))
    ));
    assert!(matches!(parse_feed(b"", GUTENBERG_URL), Err(Error::Parse(_))));
    assert!(matches!(
        parse_feed(b"   \n", GUTENBERG_URL),
        Err(Error::Parse(_))
    ));
    assert!(matches!(
        parse_feed(b"\xff\xfe<\x00", GUTENBERG_URL),
        Err(Error::Parse(_))
    ));
    assert!(matches!(
        parse_feed(b"<html><body>Login</body></html>", GUTENBERG_URL),
        Err(Error::Parse(_))
    ));
    assert!(matches!(
        parse_feed(b"{\"metadata\": ", GUTENBERG_URL),
        Err(Error::Parse(_))
    ));
    assert!(matches!(
        parse_feed(b"hello", GUTENBERG_URL),
        Err(Error::Parse(_))
    ));
}

#[test]
fn bad_base_addresses_are_rejected() {
    assert!(matches!(
        parse_feed(GUTENBERG, "ftp://example.org/x"),
        Err(Error::UnsupportedScheme(_))
    ));
    assert!(matches!(
        parse_feed(GUTENBERG, "not a url"),
        Err(Error::InvalidUrl(_))
    ));
}
