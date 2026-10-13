//! HTTP client for OPDS catalogs: feeds, search, downloads and page images.
//!
//! Requests follow KOReader's `opdsbrowser.lua` and `opdspse.lua`: the same
//! Accept headers, identity encoding, and redirect rules. Redirects are
//! followed up to five times. A redirect from https to http is refused. The
//! catalog's credentials are sent only to the catalog's origin. Calls block
//! the calling thread, so run them off the UI thread.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use base64::Engine as _;
use serde_json::Value;
use ureq::http::{HeaderMap, Request};
use url::Url;

use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::files;
use crate::model::{Acquisition, Feed};
use crate::parse::{parse_feed, parse_indirect, parse_search_description};
use crate::resolve::{parse_url, Origin};
use crate::search::expand;

/// Largest file a download accepts by default: 512 MiB.
pub const DEFAULT_MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;

/// Largest feed body the client reads.
const MAX_FEED: u64 = 16 * 1024 * 1024;
/// Largest cover or page image the client reads.
const MAX_IMAGE: u64 = 32 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const CHUNK: usize = 64 * 1024;
const FEED_ACCEPT: &str = "application/opds+json, application/atom+xml;profile=opds-catalog, */*";
const ANY: &str = "*/*";
const REDIRECT_CODES: [u16; 5] = [301, 302, 303, 307, 308];

/// Timeouts from `socketutil.lua`. Feeds get a total limit. A download has no
/// total limit, because a large book on a slow link can take a long time. Each
/// socket phase is limited, and the body gets a long upper bound so a dead
/// connection cannot block forever.
const BLOCK_TIMEOUT: Duration = Duration::from_secs(10);
const FEED_TOTAL_TIMEOUT: Duration = Duration::from_secs(30);
const FILE_BLOCK_TIMEOUT: Duration = Duration::from_secs(15);
const FILE_BODY_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// A client for one catalog, or an anonymous one for any address.
pub struct Client {
    feed_agent: ureq::Agent,
    file_agent: ureq::Agent,
    auth: Option<Auth>,
    max_download: u64,
}

struct Auth {
    origin: Origin,
    header: String,
}

/// A response whose status is known and whose body is not read yet.
struct Exchange {
    url: Url,
    status: u16,
    headers: HeaderMap,
    body: ureq::Body,
}

impl Default for Client {
    fn default() -> Self {
        Self::anonymous()
    }
}

impl Client {
    /// A client with no credentials. It can read any http or https address.
    pub fn anonymous() -> Self {
        Self::build(None)
    }

    /// A client for one catalog. Its username and password go only to the
    /// catalog's scheme, host and port.
    pub fn for_catalog(catalog: &Catalog) -> Result<Self> {
        let auth = match &catalog.username {
            Some(username) => {
                let url = parse_url(&catalog.url)?;
                let origin = Origin::of(&url).ok_or_else(|| Error::InvalidUrl(catalog.url.clone()))?;
                let secret = format!("{username}:{}", catalog.password.as_deref().unwrap_or(""));
                let encoded = base64::engine::general_purpose::STANDARD.encode(secret);
                Some(Auth {
                    origin,
                    header: format!("Basic {encoded}"),
                })
            }
            None => None,
        };
        Ok(Self::build(auth))
    }

    /// Sets the largest file a download may write. The default is
    /// [`DEFAULT_MAX_DOWNLOAD`].
    pub fn with_max_download(mut self, bytes: u64) -> Self {
        self.max_download = bytes;
        self
    }

    fn build(auth: Option<Auth>) -> Self {
        Self {
            feed_agent: agent(Some(FEED_TOTAL_TIMEOUT), BLOCK_TIMEOUT, None),
            file_agent: agent(None, FILE_BLOCK_TIMEOUT, Some(FILE_BODY_TIMEOUT)),
            auth,
            max_download: DEFAULT_MAX_DOWNLOAD,
        }
    }

    /// Reads the feed at `url`. Relative links in the result are absolute.
    pub fn fetch(&self, url: &str) -> Result<Feed> {
        let (final_url, body) = self.get_at(&parse_url(url)?, FEED_ACCEPT, MAX_FEED)?;
        parse_feed(&body, final_url.as_str())
    }

    /// Runs a search on a feed. When the feed links an OpenSearch description,
    /// that document is fetched first to find the template. Fails with
    /// [`Error::NoSearch`] when the feed has no search.
    pub fn search(&self, feed: &Feed, terms: &str) -> Result<Feed> {
        let template = match (&feed.search_description, &feed.search_template) {
            (Some(description), _) => {
                let (final_url, body) = self.get_at(&parse_url(description)?, ANY, MAX_FEED)?;
                parse_search_description(&body, final_url.as_str())?.ok_or(Error::NoSearch)?
            }
            (None, Some(template)) => template.clone(),
            (None, None) => return Err(Error::NoSearch),
        };
        self.fetch(&expand(&template, terms))
    }

    /// Reads the body at `url`, up to 32 MiB. Used for cover images and PSE
    /// pages. Build the page address with [`Pse::page_url`](crate::Pse::page_url).
    pub fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let (_, body) = self.get_at(&parse_url(url)?, ANY, MAX_IMAGE)?;
        Ok(body)
    }

    /// Downloads an acquisition into `dest_dir`, under the name it suggests
    /// (see [`Acquisition::file_name`]). Falls back to the server's name when
    /// the file type is not supported. See [`Client::download_as`].
    pub fn download(
        &self,
        acquisition: &Acquisition,
        dest_dir: &Path,
        progress: impl FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> Result<PathBuf> {
        let name = acquisition.file_name();
        self.download_as(acquisition, dest_dir, name.as_deref(), progress, cancel)
    }

    /// Downloads an acquisition into `dest_dir` as `name`. `None` takes the
    /// name from the server, from `Content-Disposition` or the last path
    /// segment of the final URL, and adds the file type when it has no
    /// extension.
    ///
    /// The bytes go to a hidden `.<name>.part` file first. The file is renamed
    /// to its final name when the download completes. On an error or a
    /// cancel, the partial file is removed.
    ///
    /// `progress` gets the bytes written so far and the total size when the
    /// server sent one. `cancel` is checked before each chunk.
    pub fn download_as(
        &self,
        acquisition: &Acquisition,
        dest_dir: &Path,
        name: Option<&str>,
        mut progress: impl FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> Result<PathBuf> {
        if acquisition.indirect {
            return Err(Error::Indirect);
        }
        if acquisition.size.is_some_and(|size| size > self.max_download) {
            return Err(Error::TooLarge {
                limit: self.max_download,
            });
        }
        let url = parse_url(&acquisition.url)?;
        let exchange = self.send(&self.file_agent, "GET", url, ANY, &[], String::new())?;
        check_status(exchange.status)?;

        let file_name = match name {
            Some(name) => files::safe_file_name(name),
            None => {
                let disposition = exchange
                    .headers
                    .get("content-disposition")
                    .and_then(|value| value.to_str().ok());
                files::server_file_name(disposition, &exchange.url, acquisition.file_type())
            }
        };
        let total = exchange
            .headers
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok());
        if total.is_some_and(|total| total > self.max_download) {
            return Err(Error::TooLarge {
                limit: self.max_download,
            });
        }

        fs::create_dir_all(dest_dir).map_err(io_error)?;
        let destination = dest_dir.join(&file_name);
        let partial = dest_dir.join(format!(".{file_name}.part"));
        let written = self.write_body(exchange.body, &partial, total, &mut progress, cancel);
        match written.and_then(|()| fs::rename(&partial, &destination).map_err(io_error)) {
            Ok(()) => Ok(destination),
            Err(err) => {
                let _ = fs::remove_file(&partial);
                Err(err)
            }
        }
    }

    /// Reads the acquisitions an OPDS 2.0 indirect acquisition lists. Each
    /// result has the same book name as `acquisition`.
    pub fn resolve_indirect(&self, acquisition: &Acquisition) -> Result<Vec<Acquisition>> {
        let (final_url, body) = self.get_at(&parse_url(&acquisition.url)?, FEED_ACCEPT, MAX_FEED)?;
        parse_indirect(&body, &final_url, &acquisition.book_name)
    }

    /// The last page a Kavita server recorded for a chapter, 0-based as Kavita
    /// counts. `page_url` is a PSE template from a Kavita catalog, in the form
    /// `<server>/api/opds/<key>/image?chapterId=<id>&...`. Returns `None` when
    /// the URL is not a Kavita page or the server does not answer.
    pub fn kavita_last_page(&self, page_url: &str) -> Option<u32> {
        let api_key = kavita_api_key(page_url)?;
        let chapter = kavita_chapter(page_url)?;
        let base = &page_url[..page_url.rfind("/api")?];

        let auth_url = format!("{base}/api/Plugin/authenticate?apiKey={api_key}&pluginName=KOReader-OPDS");
        let auth = self.kavita_json("POST", &auth_url, &[("Authentication", api_key)])?;
        let token = auth.get("token")?.as_str()?;

        let progress_url = format!("{base}/api/Reader/get-progress?chapterId={chapter}");
        let bearer = format!("Bearer {token}");
        let progress = self.kavita_json("GET", &progress_url, &[("Authorization", bearer.as_str())])?;
        u32::try_from(progress.get("pageNum")?.as_u64()?).ok()
    }

    fn kavita_json(&self, method: &str, url: &str, headers: &[(&str, &str)]) -> Option<Value> {
        let exchange = self
            .send(
                &self.feed_agent,
                method,
                parse_url(url).ok()?,
                "application/json",
                headers,
                String::new(),
            )
            .ok()?;
        if exchange.status != 200 {
            return None;
        }
        let body = read_limited(exchange.body, MAX_FEED).ok()?;
        serde_json::from_slice(&body).ok()
    }

    /// GET with a status check and a size limit. Returns the final URL.
    fn get_at(&self, url: &Url, accept: &str, limit: u64) -> Result<(Url, Vec<u8>)> {
        let exchange = self.send(&self.feed_agent, "GET", url.clone(), accept, &[], String::new())?;
        check_status(exchange.status)?;
        let body = read_limited(exchange.body, limit)?;
        Ok((exchange.url, body))
    }

    /// Sends one request and follows redirects. Credentials are added only
    /// when the URL has the catalog's origin. `extra` headers replace the
    /// automatic Authorization header when they name one.
    fn send(
        &self,
        agent: &ureq::Agent,
        method: &str,
        start: Url,
        accept: &str,
        extra: &[(&str, &str)],
        payload: String,
    ) -> Result<Exchange> {
        let mut url = start;
        for _ in 0..=MAX_REDIRECTS {
            if url.scheme() == "https" && !cfg!(feature = "tls") {
                return Err(Error::TlsUnavailable);
            }
            let mut builder = Request::builder()
                .method(method)
                .uri(url.as_str())
                .header("accept", accept)
                .header("accept-encoding", "identity");
            for (name, value) in extra {
                builder = builder.header(*name, *value);
            }
            let has_explicit_auth = extra
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case("authorization"));
            if !has_explicit_auth {
                if let Some(value) = self.authorization_for(&url) {
                    builder = builder.header("authorization", value);
                }
            }
            let request = builder
                .body(payload.clone())
                .map_err(|e| Error::Network(e.to_string()))?;
            let response = agent.run(request).map_err(transport_error)?;
            let status = response.status().as_u16();
            if REDIRECT_CODES.contains(&status) {
                if let Some(location) = response.headers().get("location").and_then(|v| v.to_str().ok()) {
                    url = redirect_target(&url, location)?;
                    continue;
                }
            }
            return Ok(Exchange {
                url,
                status,
                headers: response.headers().clone(),
                body: response.into_body(),
            });
        }
        Err(Error::TooManyRedirects)
    }

    fn authorization_for(&self, url: &Url) -> Option<String> {
        let auth = self.auth.as_ref()?;
        (Origin::of(url).as_ref() == Some(&auth.origin)).then(|| auth.header.clone())
    }

    /// Streams the body to `partial`, checking the size limit, cancel flag and
    /// progress on each chunk.
    fn write_body(
        &self,
        body: ureq::Body,
        partial: &Path,
        total: Option<u64>,
        progress: &mut impl FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> Result<()> {
        let mut reader = body.into_with_config().reader();
        let mut file = File::create(partial).map_err(io_error)?;
        let mut buffer = vec![0u8; CHUNK];
        let mut written: u64 = 0;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let count = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) => return Err(body_error(&err)),
            };
            written += count as u64;
            if written > self.max_download {
                return Err(Error::TooLarge {
                    limit: self.max_download,
                });
            }
            file.write_all(&buffer[..count]).map_err(io_error)?;
            progress(written, total);
        }
        file.sync_all().map_err(io_error)
    }
}

fn agent(total: Option<Duration>, block: Duration, body: Option<Duration>) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        // Redirects are handled in `send`, so a 3xx comes back as a response.
        .max_redirects(0)
        .max_redirects_will_error(false)
        .timeout_global(total)
        .timeout_connect(Some(block))
        .timeout_send_request(Some(block))
        .timeout_send_body(Some(block))
        .timeout_recv_response(Some(block))
        .timeout_recv_body(body)
        .build();
    ureq::Agent::new_with_config(config)
}

fn check_status(status: u16) -> Result<()> {
    match status {
        200..=299 => Ok(()),
        401 => Err(Error::Unauthorized),
        403 => Err(Error::Forbidden),
        404 => Err(Error::NotFound),
        406 => Err(Error::NotAcceptable),
        other => Err(Error::Status(other)),
    }
}

/// The target of a redirect. A redirect from https to http is refused.
fn redirect_target(current: &Url, location: &str) -> Result<Url> {
    let next = current
        .join(location.trim())
        .map_err(|_| Error::InvalidUrl(location.to_owned()))?;
    if current.scheme() == "https" && next.scheme() == "http" {
        return Err(Error::Downgrade {
            from: current.to_string(),
            to: next.to_string(),
        });
    }
    match next.scheme() {
        "http" | "https" => Ok(next),
        other => Err(Error::UnsupportedScheme(other.to_owned())),
    }
}

fn read_limited(body: ureq::Body, limit: u64) -> Result<Vec<u8>> {
    body.into_with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|err| match err {
            ureq::Error::BodyExceedsLimit(_) => Error::TooLarge { limit },
            other => transport_error(other),
        })
}

fn transport_error(err: ureq::Error) -> Error {
    match err {
        ureq::Error::Timeout(_) => Error::Timeout,
        other => Error::Network(other.to_string()),
    }
}

fn body_error(err: &io::Error) -> Error {
    if err.kind() == io::ErrorKind::TimedOut {
        Error::Timeout
    } else {
        Error::Network(err.to_string())
    }
}

fn io_error(err: io::Error) -> Error {
    Error::Io(err.to_string())
}

/// The Kavita API key: the text between `opds/` and the last `/image`.
fn kavita_api_key(page_url: &str) -> Option<&str> {
    let start = page_url.find("opds/")? + "opds/".len();
    let end = page_url.rfind("/image")?;
    (end > start).then(|| &page_url[start..end])
}

/// The chapter id after `chapterId=`, as far as it has letters or digits.
fn kavita_chapter(page_url: &str) -> Option<&str> {
    let start = page_url.find("chapterId=")? + "chapterId=".len();
    let rest = &page_url[start..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric())
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kavita_fields_come_from_the_page_url() {
        let url = "https://k.example/api/opds/abc-123/image?chapterId=42&pageNumber={pageNumber}";
        assert_eq!(kavita_api_key(url), Some("abc-123"));
        assert_eq!(kavita_chapter(url), Some("42"));
        assert_eq!(kavita_api_key("https://x.org/page"), None);
        assert_eq!(kavita_chapter("https://x.org/page"), None);
    }

    #[test]
    fn redirects_refuse_a_downgrade() {
        let secure = Url::parse("https://x.org/a").expect("url");
        assert!(matches!(
            redirect_target(&secure, "http://x.org/b"),
            Err(Error::Downgrade { .. })
        ));
        assert_eq!(
            redirect_target(&secure, "/b").expect("relative").as_str(),
            "https://x.org/b"
        );
        let plain = Url::parse("http://x.org/a").expect("url");
        assert_eq!(
            redirect_target(&plain, "https://cdn.example/f.epub")
                .expect("upgrade")
                .as_str(),
            "https://cdn.example/f.epub"
        );
        assert!(matches!(
            redirect_target(&plain, "ftp://x.org/f"),
            Err(Error::UnsupportedScheme(_))
        ));
    }

    #[test]
    fn authorization_only_for_the_catalog_origin() {
        let catalog = Catalog {
            title: "Calibre".to_owned(),
            url: "http://192.168.1.2:8080/opds".to_owned(),
            username: Some("reader".to_owned()),
            password: Some("secret".to_owned()),
        };
        let client = Client::for_catalog(&catalog).expect("client");
        let same = Url::parse("http://192.168.1.2:8080/opds/new").expect("url");
        let other_port = Url::parse("http://192.168.1.2:9090/opds").expect("url");
        let other_host = Url::parse("http://elsewhere.example/opds").expect("url");
        assert_eq!(
            client.authorization_for(&same),
            Some("Basic cmVhZGVyOnNlY3JldA==".to_owned())
        );
        assert_eq!(client.authorization_for(&other_port), None);
        assert_eq!(client.authorization_for(&other_host), None);
    }
}
