//! Errors from the sync client.

use std::fmt;

use thiserror::Error;

/// The blank field or fields that failed the login check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidField {
    Username,
    Password,
    Both,
}

impl fmt::Display for InvalidField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InvalidField::Username => "invalid username",
            InvalidField::Password => "invalid password",
            InvalidField::Both => "invalid username and password",
        })
    }
}

/// Errors from [`SyncClient`](crate::SyncClient) and [`Credentials`](crate::Credentials).
///
/// The messages are English. The app shows its own translated text and uses
/// the variants to pick it.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum Error {
    /// A blank username or password. Checked before any request.
    #[error("{0}")]
    InvalidCredentials(InvalidField),
    /// The server address is not an http or https URL with a host.
    #[error("invalid server address: {0}")]
    InvalidServerUrl(String),
    /// An https address was given to a build without the `tls` feature.
    #[error("https needs the `tls` feature")]
    TlsUnavailable,
    /// The server rejected the credentials (HTTP 401). `code` is the
    /// server's error code when the body has one, for example 2001.
    #[error("{message}")]
    Unauthorized { code: Option<i64>, message: String },
    /// Any other status the protocol does not expect. Registration returns
    /// 402 with a message when the user already exists.
    #[error("server returned {status}: {message}")]
    Server {
        status: u16,
        code: Option<i64>,
        message: String,
    },
    /// The server did not answer within the configured timeout.
    #[error("request timed out")]
    Timeout,
    /// The connection failed: refused, unreachable host, TLS failure.
    #[error("network error: {0}")]
    Network(String),
    /// The server answered, but the body is not what the protocol defines.
    #[error("unexpected response: {0}")]
    Protocol(String),
}
