//! Errors from the OPDS client and parsers.

use thiserror::Error;

/// Result type used across the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Errors from parsing and network calls. The messages are English. The app
/// shows its own translated text and picks the message from the variant.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Error {
    /// The text is not an absolute http or https URL.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    /// The URL uses a scheme other than http or https.
    #[error("unsupported address scheme: {0}")]
    UnsupportedScheme(String),
    /// An https address was used in a build without the `tls` feature.
    #[error("https needs the `tls` feature")]
    TlsUnavailable,
    /// The catalog needs a username and password (HTTP 401).
    #[error("authentication required")]
    Unauthorized,
    /// The server refused the credentials (HTTP 403).
    #[error("access denied")]
    Forbidden,
    /// The resource does not exist (HTTP 404).
    #[error("not found")]
    NotFound,
    /// The server refuses uncompressed content (HTTP 406).
    #[error("server refuses uncompressed content")]
    NotAcceptable,
    /// Any other status code that is not a success.
    #[error("server returned status {0}")]
    Status(u16),
    /// A redirect would move an https address to http.
    #[error("redirect from https to http refused: {from} to {to}")]
    Downgrade { from: String, to: String },
    /// The redirect chain is longer than the client allows.
    #[error("too many redirects")]
    TooManyRedirects,
    /// The server did not answer in time.
    #[error("request timed out")]
    Timeout,
    /// The connection failed: refused, unreachable host, TLS failure.
    #[error("network error: {0}")]
    Network(String),
    /// The body is not an OPDS feed or does not match the format.
    #[error("invalid feed: {0}")]
    Parse(String),
    /// The body is larger than the limit for this call.
    #[error("larger than the limit of {limit} bytes")]
    TooLarge { limit: u64 },
    /// The user cancelled the download. The partial file is removed.
    #[error("download cancelled")]
    Cancelled,
    /// The feed has no search link, or the search description has no template.
    #[error("the catalog has no search")]
    NoSearch,
    /// An indirect acquisition must be resolved with `Client::resolve_indirect`
    /// before it can be downloaded.
    #[error("indirect acquisition needs resolving first")]
    Indirect,
    /// Reading or writing the local file failed.
    #[error("file error: {0}")]
    Io(String),
}
