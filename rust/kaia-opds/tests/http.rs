//! Client tests against a server on 127.0.0.1. No test reaches the internet.

mod support;

use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use kaia_opds::{parse_feed, Acquisition, Catalog, Client, Error, FeedKind};
use support::{body, fixed, Handler, Reply, Server};

const ACQUISITION: &[u8] = include_bytes!("fixtures/acquisition.xml");
const ACQUISITION_PAGE2: &[u8] = include_bytes!("fixtures/acquisition_page2.xml");
const CALIBRE: &[u8] = include_bytes!("fixtures/calibre.xml");
const PSE: &[u8] = include_bytes!("fixtures/pse.xml");
const INDIRECT: &[u8] = include_bytes!("fixtures/indirect.json");

/// Basic credentials for the tests: reader / secret.
const BASIC: &str = "Basic cmVhZGVyOnNlY3JldA==";

/// A temporary directory that is removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("kaia-opds-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn file_names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn catalog(url: String) -> Catalog {
    Catalog {
        title: "Test".to_owned(),
        url,
        username: Some("reader".to_owned()),
        password: Some("secret".to_owned()),
    }
}

/// A feed with one search link, parsed from a small Atom document.
fn feed_with_search_link(base: &str, href: &str) -> kaia_opds::Feed {
    let xml = format!(
        r#"<feed xmlns="http://www.w3.org/2005/Atom"><title>S</title><link rel="search" type="application/atom+xml" href="{href}"/></feed>"#
    );
    parse_feed(xml.as_bytes(), base).expect("feed parses")
}

#[test]
fn fetch_sends_the_opds_headers_and_resolves_links() {
    let server = Server::start(vec![("/books/new", body(ACQUISITION))]);
    let url = server.url("/books/new");
    let feed = Client::anonymous().fetch(&url).expect("fetch");

    assert_eq!(feed.title, "New Books");
    assert_eq!(feed.kind, FeedKind::Acquisition);
    assert_eq!(feed.url, url);
    assert_eq!(feed.next, Some(server.url("/books/new?page=2")));

    let request = &server.requests_to("/books/new")[0];
    assert_eq!(request.method, "GET");
    assert_eq!(request.header("accept-encoding"), Some("identity"));
    assert!(request
        .header("accept")
        .is_some_and(|a| a.contains("application/atom+xml")));
    assert_eq!(request.authorization, None);
}

#[test]
fn pagination_follows_next_and_previous_links() {
    let server = Server::start(vec![(
        "/books/new",
        Box::new(|request: &support::Seen| {
            if request.target.contains("page=2") {
                Reply::ok(ACQUISITION_PAGE2)
            } else {
                Reply::ok(ACQUISITION)
            }
        }) as Handler,
    )]);
    let client = Client::anonymous();
    let first = client.fetch(&server.url("/books/new")).expect("first page");
    let second = client
        .fetch(first.next.as_deref().expect("next link"))
        .expect("second page");
    assert_eq!(second.title, "New Books, page 2");
    assert_eq!(second.entries[0].title, "Frankenstein");
    let back = client
        .fetch(second.prev.as_deref().expect("previous link"))
        .expect("back to the first page");
    assert_eq!(back.title, "New Books");
}

#[test]
fn basic_auth_is_sent_only_with_the_catalog_credentials() {
    let server = Server::start(vec![(
        "/opds/private.xml",
        Box::new(|request: &support::Seen| {
            if request.authorization.as_deref() == Some(BASIC) {
                Reply::ok(CALIBRE)
            } else {
                Reply::status(401).with_header("WWW-Authenticate", "Basic realm=\"calibre\"")
            }
        }) as Handler,
    )]);
    let url = server.url("/opds/private.xml");

    // Without credentials the server answers 401, which the client reports.
    assert_eq!(Client::anonymous().fetch(&url), Err(Error::Unauthorized));

    // With the catalog's credentials the same request succeeds.
    let client = Client::for_catalog(&catalog(server.url("/opds/"))).expect("client");
    let feed = client.fetch(&url).expect("authenticated fetch");
    assert_eq!(feed.title, "Calibre Library");

    // A wrong password is refused again.
    let wrong = Catalog {
        password: Some("nope".to_owned()),
        ..catalog(server.url("/opds/"))
    };
    let wrong_client = Client::for_catalog(&wrong).expect("client");
    assert_eq!(wrong_client.fetch(&url), Err(Error::Unauthorized));
}

#[test]
fn same_origin_redirects_keep_the_credentials() {
    let server = Server::start(vec![
        ("/old", fixed_redirect_to("/new")),
        ("/new", body(ACQUISITION)),
    ]);
    let client = Client::for_catalog(&catalog(server.url("/"))).expect("client");
    let feed = client.fetch(&server.url("/old")).expect("follows the redirect");
    assert_eq!(feed.url, server.url("/new"));
    let seen = &server.requests_to("/new")[0];
    assert_eq!(seen.authorization.as_deref(), Some(BASIC));
}

fn fixed_redirect_to(path: &'static str) -> Handler {
    Box::new(move |_: &support::Seen| Reply::redirect(302, path))
}

#[test]
fn credentials_do_not_follow_a_redirect_to_another_origin() {
    let other = Server::start(vec![("/feed", body(ACQUISITION))]);
    let other_feed = other.url("/feed");
    let server = Server::start(vec![(
        "/go",
        Box::new(move |_: &support::Seen| Reply::redirect(301, &other_feed)) as Handler,
    )]);
    let client = Client::for_catalog(&catalog(server.url("/"))).expect("client");
    let feed = client.fetch(&server.url("/go")).expect("follows the redirect");
    assert_eq!(feed.entries.len(), 3);
    let seen = &other.requests_to("/feed")[0];
    assert_eq!(
        seen.authorization, None,
        "credentials stay with the catalog origin"
    );
}

#[test]
fn redirect_loops_stop() {
    let server = Server::start(vec![("/loop", fixed_redirect_to("/loop"))]);
    assert_eq!(
        Client::anonymous().fetch(&server.url("/loop")),
        Err(Error::TooManyRedirects)
    );
}

#[test]
fn http_errors_map_to_their_variants() {
    let server = Server::start(vec![
        ("/missing", fixed(|| Reply::status(404))),
        ("/denied", fixed(|| Reply::status(403))),
        ("/broken", fixed(|| Reply::status(500))),
        ("/uncompressed", fixed(|| Reply::status(406))),
        ("/empty", fixed(|| Reply::ok(Vec::new()))),
        ("/not-a-feed", body(b"<html>Login</html>")),
    ]);
    let client = Client::anonymous();
    assert_eq!(client.fetch(&server.url("/missing")), Err(Error::NotFound));
    assert_eq!(client.fetch(&server.url("/denied")), Err(Error::Forbidden));
    assert_eq!(client.fetch(&server.url("/broken")), Err(Error::Status(500)));
    assert_eq!(
        client.fetch(&server.url("/uncompressed")),
        Err(Error::NotAcceptable)
    );
    assert!(matches!(
        client.fetch(&server.url("/empty")),
        Err(Error::Parse(_))
    ));
    assert!(matches!(
        client.fetch(&server.url("/not-a-feed")),
        Err(Error::Parse(_))
    ));
}

#[test]
fn only_http_and_https_are_read() {
    assert!(matches!(
        Client::anonymous().fetch("ftp://example.org/feed"),
        Err(Error::UnsupportedScheme(_))
    ));
    assert!(matches!(
        Client::anonymous().fetch("nonsense"),
        Err(Error::InvalidUrl(_))
    ));
}

#[test]
fn search_through_a_template_url_encodes_the_terms() {
    let server = Server::start(vec![("/search", body(ACQUISITION))]);
    let feed = feed_with_search_link(&server.url("/opds/"), &server.url("/search?q={searchTerms}"));
    let result = Client::anonymous()
        .search(&feed, "Mark Twain & co")
        .expect("search");
    assert_eq!(result.title, "New Books");
    let request = &server.requests_to("/search")[0];
    assert_eq!(request.target, "/search?q=Mark%20Twain%20%26%20co");
}

#[test]
fn search_through_an_opensearch_description() {
    let osd = br#"<?xml version="1.0"?>
<OpenSearchDescription xmlns="http://a9.com/-/spec/opensearch/1.1/">
  <Url type="text/html" template="html/search?q={searchTerms}"/>
  <Url type="application/atom+xml" template="search?q={searchTerms}&amp;count={count?}"/>
</OpenSearchDescription>"#;
    let server = Server::start(vec![
        ("/opds/osd.xml", body(osd)),
        ("/opds/search", body(ACQUISITION)),
    ]);
    let feed = parse_feed(
        br#"<feed xmlns="http://www.w3.org/2005/Atom"><title>S</title><link rel="search" type="application/opensearchdescription+xml" href="osd.xml"/></feed>"#,
        &server.url("/opds/root.xml"),
    )
    .expect("feed parses");
    assert_eq!(feed.search_description, Some(server.url("/opds/osd.xml")));

    let result = Client::anonymous().search(&feed, "dumas").expect("search");
    assert_eq!(result.title, "New Books");
    assert_eq!(
        server.requests_to("/opds/search")[0].target,
        "/opds/search?q=dumas&count="
    );
}

#[test]
fn a_feed_without_search_says_so() {
    let feed = parse_feed(ACQUISITION, "https://example.org/books").expect("feed parses");
    assert_eq!(Client::anonymous().search(&feed, "x"), Err(Error::NoSearch));
}

#[test]
fn download_writes_the_suggested_name() {
    let payload: &'static [u8] = b"EPUB-BYTES";
    let server = Server::start(vec![(
        "/files/book.epub",
        Box::new(move |_: &support::Seen| {
            Reply::ok(payload).with_header("Content-Disposition", "attachment; filename=\"Server Name.epub\"")
        }) as Handler,
    )]);
    let acquisition = Acquisition {
        url: server.url("/files/book.epub"),
        mime: Some("application/epub+zip".to_owned()),
        title: None,
        size: None,
        indirect: false,
        book_name: "Jane Doe - Pride/Prejudice".to_owned(),
    };
    let scratch = Scratch::new("download");
    let cancel = AtomicBool::new(false);
    let mut calls = Vec::new();
    let path = Client::anonymous()
        .download(
            &acquisition,
            scratch.path(),
            |done, total| calls.push((done, total)),
            &cancel,
        )
        .expect("download");

    assert_eq!(path, scratch.path().join("Jane Doe - Pride_Prejudice.epub"));
    assert_eq!(fs::read(&path).expect("file"), payload);
    assert_eq!(scratch.file_names(), ["Jane Doe - Pride_Prejudice.epub"]);
    assert_eq!(
        calls.last(),
        Some(&(payload.len() as u64, Some(payload.len() as u64)))
    );
}

/// Serves `head`, then `body`, then closes the connection. A `Content-Length` in
/// `head` that is larger than `body` makes the body end early.
fn cut_off_server(head: &'static str, body: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let base = format!("http://{}", listener.local_addr().expect("local address"));
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        // Read the request head, so the client has finished sending before the reply.
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            if stream.read(&mut byte).unwrap_or(0) == 0 {
                return;
            }
            request.push(byte[0]);
        }
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(body);
    });
    base
}

fn epub_at(url: String) -> Acquisition {
    Acquisition {
        url,
        mime: Some("application/epub+zip".to_owned()),
        title: None,
        size: None,
        indirect: false,
        book_name: "Book".to_owned(),
    }
}

#[test]
fn a_failed_body_leaves_no_partial_file() {
    let scratch = Scratch::new("partial");
    let cancel = AtomicBool::new(false);
    let client = Client::anonymous().with_max_download(100);

    // The server promises 1000 bytes, sends 9 and closes. The download is an error.
    let short = cut_off_server(
        "HTTP/1.1 200 OK\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n",
        b"EPUB-PART",
    );
    let result = client.download_as(
        &epub_at(format!("{short}/short.epub")),
        scratch.path(),
        Some("short.epub"),
        |_, _| {},
        &cancel,
    );
    assert!(result.is_err(), "a short body is not saved as a book");
    assert!(
        scratch.file_names().is_empty(),
        "no partial file after a short body: {:?}",
        scratch.file_names()
    );

    // No length is given, so the 100 byte limit is passed in the middle of the body.
    let long = cut_off_server("HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n", &[7u8; 200]);
    assert_eq!(
        client.download_as(
            &epub_at(format!("{long}/long.epub")),
            scratch.path(),
            Some("long.epub"),
            |_, _| {},
            &cancel,
        ),
        Err(Error::TooLarge { limit: 100 })
    );
    assert!(
        scratch.file_names().is_empty(),
        "no partial file after the limit: {:?}",
        scratch.file_names()
    );
}

#[test]
fn download_as_takes_the_name_from_the_server_when_none_is_given() {
    let server = Server::start(vec![(
        "/files/book.epub",
        Box::new(|_: &support::Seen| {
            Reply::ok(b"x".to_vec())
                .with_header("Content-Disposition", "attachment; filename=\"Server Name.epub\"")
        }) as Handler,
    )]);
    let acquisition = Acquisition {
        url: server.url("/files/book.epub"),
        mime: Some("application/epub+zip".to_owned()),
        title: None,
        size: None,
        indirect: false,
        book_name: "Ignored".to_owned(),
    };
    let scratch = Scratch::new("server-name");
    let cancel = AtomicBool::new(false);
    let path = Client::anonymous()
        .download_as(&acquisition, scratch.path(), None, |_, _| {}, &cancel)
        .expect("download");
    assert_eq!(
        path.file_name().and_then(|n| n.to_str()),
        Some("Server Name.epub")
    );
}

#[test]
fn downloads_over_the_size_limit_are_refused() {
    let server = Server::start(vec![
        ("/files/big.bin", body(&[7u8; 1000])),
        ("/files/known.bin", body(&[7u8; 10])),
    ]);
    let client = Client::anonymous().with_max_download(100);
    let scratch = Scratch::new("limit");
    let cancel = AtomicBool::new(false);

    // The size is known before the request, so nothing is sent.
    let known_big = Acquisition {
        url: server.url("/files/known.bin"),
        mime: Some("application/octet-stream".to_owned()),
        title: None,
        size: Some(5000),
        indirect: false,
        book_name: "Big".to_owned(),
    };
    assert_eq!(
        client.download(&known_big, scratch.path(), |_, _| {}, &cancel),
        Err(Error::TooLarge { limit: 100 })
    );
    assert!(server.requests_to("/files/known.bin").is_empty());

    // The size is unknown until the response: the body is refused and removed.
    let unknown = Acquisition {
        url: server.url("/files/big.bin"),
        mime: Some("application/octet-stream".to_owned()),
        title: None,
        size: None,
        indirect: false,
        book_name: "Big".to_owned(),
    };
    assert_eq!(
        client.download_as(&unknown, scratch.path(), Some("big.bin"), |_, _| {}, &cancel),
        Err(Error::TooLarge { limit: 100 })
    );
    assert!(scratch.file_names().is_empty());
}

#[test]
fn a_cancelled_download_leaves_no_file_behind() {
    let server = Server::start(vec![("/files/huge.bin", body(&[1u8; 4 * 1024 * 1024]))]);
    let acquisition = Acquisition {
        url: server.url("/files/huge.bin"),
        mime: Some("application/octet-stream".to_owned()),
        title: None,
        size: None,
        indirect: false,
        book_name: "Huge".to_owned(),
    };
    let scratch = Scratch::new("cancel");
    let cancel = AtomicBool::new(false);
    let result = Client::anonymous().download_as(
        &acquisition,
        scratch.path(),
        Some("huge.bin"),
        |_, _| cancel.store(true, Ordering::Relaxed),
        &cancel,
    );
    assert_eq!(result, Err(Error::Cancelled));
    assert!(
        scratch.file_names().is_empty(),
        "no partial or final file is left"
    );
}

#[test]
fn indirect_acquisitions_resolve_to_their_files() {
    let server = Server::start(vec![("/books/1342/indirect", body(INDIRECT))]);
    let indirect = Acquisition {
        url: server.url("/books/1342/indirect"),
        mime: Some("application/epub+zip".to_owned()),
        title: None,
        size: None,
        indirect: true,
        book_name: "Jane Austen - Pride and Prejudice".to_owned(),
    };
    let client = Client::anonymous();
    let scratch = Scratch::new("indirect");
    let cancel = AtomicBool::new(false);
    assert_eq!(
        client.download(&indirect, scratch.path(), |_, _| {}, &cancel),
        Err(Error::Indirect)
    );

    let resolved = client.resolve_indirect(&indirect).expect("resolves");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].url, server.url("/files/1342.epub"));
    assert_eq!(
        resolved[0].file_name().as_deref(),
        Some("Jane Austen - Pride and Prejudice.epub")
    );
}

#[test]
fn pse_pages_are_fetched_from_the_template() {
    let server = Server::start(vec![(
        "/pse/7/0",
        Box::new(|request: &support::Seen| {
            if request.target == "/pse/7/0?maxWidth=800" {
                Reply::ok(b"\x89PNG page".to_vec())
            } else {
                Reply::status(404)
            }
        }) as Handler,
    )]);
    let feed = parse_feed(PSE, &server.url("/opds/")).expect("feed parses");
    let pse = feed.entries[0].pse.as_ref().expect("pse");
    let page = Client::anonymous()
        .get_bytes(&pse.page_url(0, 800))
        .expect("page image");
    assert_eq!(page, b"\x89PNG page");
}

#[test]
fn kavita_reports_the_last_page_read() {
    let server = Server::start(vec![
        (
            "/api/Plugin/authenticate",
            Box::new(|request: &support::Seen| {
                if request.header("authentication") == Some("KEY123") {
                    Reply::ok(br#"{"token":"tok-1","refreshToken":"r"}"#.to_vec())
                } else {
                    Reply::status(401)
                }
            }) as Handler,
        ),
        (
            "/api/Reader/get-progress",
            Box::new(|request: &support::Seen| {
                if request.authorization.as_deref() == Some("Bearer tok-1")
                    && request.target.contains("chapterId=42")
                {
                    Reply::ok(br#"{"pageNum":12,"seriesId":3}"#.to_vec())
                } else {
                    Reply::status(401)
                }
            }) as Handler,
        ),
    ]);
    let client = Client::anonymous();
    let page = server.url("/api/opds/KEY123/image?chapterId=42&pageNumber={pageNumber}");
    assert_eq!(client.kavita_last_page(&page), Some(12));

    let wrong_key = server.url("/api/opds/NOPE/image?chapterId=42&pageNumber={pageNumber}");
    assert_eq!(client.kavita_last_page(&wrong_key), None);
}

#[test]
fn missing_pages_are_reported() {
    let server = Server::start(vec![]);
    assert_eq!(
        Client::anonymous().get_bytes(&server.url("/nothing")),
        Err(Error::NotFound)
    );
}
