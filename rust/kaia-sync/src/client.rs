//! HTTP client for KOReader's progress sync protocol.
//!
//! Requests and responses follow `plugins/kosync.koplugin/api.json` and
//! `KOSyncClient.lua`. Nothing is sent until a method on [`SyncClient`] is
//! called.

use std::fmt::Write;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use ureq::http::Request;
#[cfg(feature = "tls")]
use ureq::tls::TlsConfig;

use crate::doc_id::round_percent;
use crate::error::Error;
use crate::model::{Credentials, Metadata, Progress};

/// The public server, as in `api.json`.
pub const DEFAULT_SERVER: &str = "https://sync.koreader.rocks:443";

/// Connection choices. Kaia verifies the server's TLS certificate by default.
/// KOReader does not verify it, which lets anyone on the network read the sync
/// password digest and progress; Kaia keeps that only as an explicit opt-in
/// (`accept_invalid_certs`) for self-hosted servers with self-signed
/// certificates, which the Settings screen offers per server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SyncOptions {
    pub accept_invalid_certs: bool,
    /// KOReader retries HTTP 202. Set true to accept the status listed in api.json.
    pub accept_202: bool,
}

const ACCEPT: &str = "application/vnd.koreader.v1+json";
/// Largest response body the client reads. Sync replies are a few hundred bytes.
const MAX_BODY: u64 = 64 * 1024;
const UNKNOWN_ERROR: &str = "Unknown server error";

/// Time limits for one group of calls.
///
/// `block` bounds each socket phase: connect, send, and waiting for the
/// response. `total` bounds the whole call. The plugin sets these through
/// LuaSocket's `setTimeout(block, total)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout {
    pub block: Duration,
    pub total: Duration,
}

/// Limits for progress calls (push, pull) and for login calls (register,
/// authorize, health).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    pub progress: Timeout,
    pub auth: Timeout,
}

impl Default for Timeouts {
    /// The plugin's values: `PROGRESS_TIMEOUTS = { 2, 5 }` and
    /// `AUTH_TIMEOUTS = { 5, 10 }`.
    fn default() -> Self {
        Self {
            progress: Timeout {
                block: Duration::from_secs(2),
                total: Duration::from_secs(5),
            },
            auth: Timeout {
                block: Duration::from_secs(5),
                total: Duration::from_secs(10),
            },
        }
    }
}

/// A client bound to one server, one account and one device.
///
/// Calls block the thread. Run them off the UI thread.
pub struct SyncClient {
    server: String,
    credentials: Credentials,
    device: String,
    device_id: String,
    progress_agent: ureq::Agent,
    auth_agent: ureq::Agent,
    options: SyncOptions,
}

impl SyncClient {
    /// Creates a client for `server` (for example [`DEFAULT_SERVER`] or a
    /// self-hosted `http://` or `https://` address). A trailing slash is
    /// removed. Fails when the address is not an http or https URL with a
    /// host, or when it is https and the `tls` feature is off. No request is
    /// made.
    pub fn new(
        server: &str,
        credentials: Credentials,
        device_name: &str,
        device_id: &str,
        timeouts: Timeouts,
    ) -> Result<Self, Error> {
        Self::new_with_options(
            server,
            credentials,
            device_name,
            device_id,
            timeouts,
            SyncOptions::default(),
        )
    }

    pub fn new_with_options(
        server: &str,
        credentials: Credentials,
        device_name: &str,
        device_id: &str,
        timeouts: Timeouts,
        options: SyncOptions,
    ) -> Result<Self, Error> {
        Ok(Self {
            server: normalize_server(server)?,
            credentials,
            device: device_name.to_owned(),
            device_id: device_id.to_owned(),
            progress_agent: agent(timeouts.progress, options),
            auth_agent: agent(timeouts.auth, options),
            options,
        })
    }

    /// Builds a [`Progress`] for this device, ready for [`push`](Self::push).
    /// `timestamp` is left empty because the server sets it.
    pub fn local_progress(
        &self,
        document: &str,
        progress: &str,
        percentage: f64,
        metadata: Option<Metadata>,
    ) -> Progress {
        Progress {
            document: document.to_owned(),
            progress: progress.to_owned(),
            percentage,
            device: self.device.clone(),
            device_id: self.device_id.clone(),
            timestamp: None,
            metadata,
        }
    }

    /// Creates the account on the server (`POST /users/create`). The password
    /// goes as its MD5 digest, as in `KOSync:doRegister`. Succeeds on 201.
    /// An existing user gives [`Error::Server`] with status 402 and the
    /// server's message.
    pub fn register(&self) -> Result<(), Error> {
        let body = serde_json::json!({
            "username": self.credentials.username(),
            "password": self.credentials.userkey(),
        })
        .to_string();
        let reply = self.exchange(&self.auth_agent, "POST", "/users/create", false, Some(body))?;
        if reply.status == 201 {
            Ok(())
        } else {
            Err(status_error(&reply))
        }
    }

    /// Checks the login (`GET /users/auth`). Succeeds on 200. A wrong
    /// password gives [`Error::Unauthorized`].
    pub fn authorize(&self) -> Result<(), Error> {
        let reply = self.exchange(&self.auth_agent, "GET", "/users/auth", true, None)?;
        if reply.status == 200 {
            Ok(())
        } else {
            Err(status_error(&reply))
        }
    }

    /// Stores a reading position (`PUT /syncs/progress`). The percentage is
    /// rounded down to four decimals, as the plugin does. Succeeds on 200 or
    /// 202 only if `accept_202` is set. KOReader retries 202 by default.
    pub fn push(&self, progress: &Progress) -> Result<(), Error> {
        let body = serde_json::to_string(&PushBody {
            document: &progress.document,
            progress: &progress.progress,
            percentage: round_percent(progress.percentage),
            device: &progress.device,
            device_id: &progress.device_id,
            metadata: progress.metadata.as_ref(),
        })
        .map_err(|e| Error::Protocol(e.to_string()))?;
        let reply = self.exchange(&self.progress_agent, "PUT", "/syncs/progress", true, Some(body))?;
        match reply.status {
            200 => Ok(()),
            202 if self.options.accept_202 => Ok(()),
            _ => Err(status_error(&reply)),
        }
    }

    /// Reads the stored position for `document` (`GET /syncs/progress/:document`).
    ///
    /// Returns `Ok(None)` when the server has no position. The server
    /// answers 200 with no `percentage` then, and the plugin treats that as
    /// "No progress found". The percentage is rounded down to four decimals.
    pub fn pull(&self, document: &str) -> Result<Option<Progress>, Error> {
        let path = format!("/syncs/progress/{}", encode_segment(document));
        let reply = self.exchange(&self.progress_agent, "GET", &path, true, None)?;
        if reply.status != 200 {
            return Err(status_error(&reply));
        }
        let body: PullBody =
            serde_json::from_str(&reply.body).map_err(|e| Error::Protocol(format!("progress body: {e}")))?;
        let Some(percentage) = body.percentage else {
            return Ok(None);
        };
        Ok(Some(Progress {
            document: document.to_owned(),
            progress: progress_text(body.progress),
            percentage: round_percent(percentage),
            device: body.device.unwrap_or_default(),
            device_id: body.device_id.unwrap_or_default(),
            timestamp: body.timestamp,
            metadata: None,
        }))
    }

    /// Asks the server whether it is up (`GET /healthcheck`). Needs no login.
    pub fn health(&self) -> Result<(), Error> {
        let reply = self.exchange(&self.auth_agent, "GET", "/healthcheck", false, None)?;
        if reply.status == 200 {
            Ok(())
        } else {
            Err(status_error(&reply))
        }
    }

    /// Sends one request and reads the whole reply. Adds the Accept header
    /// and, when `auth` is set, the `x-auth-user` and `x-auth-key` headers.
    fn exchange(
        &self,
        agent: &ureq::Agent,
        method: &str,
        path: &str,
        auth: bool,
        json: Option<String>,
    ) -> Result<Reply, Error> {
        let mut builder = Request::builder()
            .method(method)
            .uri(format!("{}{}", self.server, path))
            .header("accept", ACCEPT);
        if auth {
            builder = builder
                .header("x-auth-user", self.credentials.username())
                .header("x-auth-key", self.credentials.userkey());
        }
        if json.is_some() {
            builder = builder.header("content-type", "application/json");
        }
        let request = builder
            .body(json.unwrap_or_default())
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let response = agent.run(request).map_err(transport_error)?;
        let status = response.status().as_u16();
        let mut body = response.into_body();
        let text = body
            .with_config()
            .limit(MAX_BODY)
            .read_to_string()
            .map_err(transport_error)?;
        Ok(Reply { status, body: text })
    }
}

/// A reply with its body read as text.
struct Reply {
    status: u16,
    body: String,
}

#[derive(Serialize)]
struct PushBody<'a> {
    document: &'a str,
    progress: &'a str,
    percentage: f64,
    device: &'a str,
    device_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<&'a Metadata>,
}

#[derive(Deserialize)]
struct PullBody {
    progress: Option<Value>,
    #[serde(default, deserialize_with = "optional_number")]
    percentage: Option<f64>,
    device: Option<String>,
    device_id: Option<String>,
    #[serde(default, deserialize_with = "optional_timestamp")]
    timestamp: Option<i64>,
}

fn optional_number<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }))
}

fn optional_timestamp<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<i64>, D::Error> {
    Ok(optional_number(deserializer)?.map(|number| number as i64))
}

#[derive(Deserialize)]
struct ErrorBody {
    code: Option<i64>,
    message: Option<String>,
}

/// The error for a status the call does not expect. 401 gives
/// [`Error::Unauthorized`]. Other statuses give [`Error::Server`].
fn status_error(reply: &Reply) -> Error {
    let parsed: Option<ErrorBody> = serde_json::from_str(&reply.body).ok();
    let (code, message) = match parsed {
        Some(body) => (body.code, body.message),
        None => (None, None),
    };
    if reply.status == 401 {
        Error::Unauthorized {
            code,
            message: message.unwrap_or_else(|| "Unauthorized".to_owned()),
        }
    } else {
        Error::Server {
            status: reply.status,
            code,
            message: message.unwrap_or_else(|| UNKNOWN_ERROR.to_owned()),
        }
    }
}

fn transport_error(err: ureq::Error) -> Error {
    match err {
        ureq::Error::Timeout(_) => Error::Timeout,
        ureq::Error::BodyExceedsLimit(limit) => Error::Protocol(format!("response body over {limit} bytes")),
        other => Error::Network(other.to_string()),
    }
}

/// The position comes back as a string. Numbers are accepted too, since
/// KOReader sends page numbers as text but other clients may not.
fn progress_text(value: Option<Value>) -> String {
    match value {
        Some(Value::String(text)) => text,
        Some(Value::Number(number)) => number.to_string(),
        _ => String::new(),
    }
}

/// Percent-encodes one path segment. Document ids are hex digests, so this
/// only matters for ids that come from other clients.
fn encode_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => out.push(char::from(byte)),
            _ => {
                // Writing to a String cannot fail.
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

fn normalize_server(server: &str) -> Result<String, Error> {
    let trimmed = server.trim().trim_end_matches('/');
    let invalid = || Error::InvalidServerUrl(server.to_owned());
    let uri: ureq::http::Uri = trimmed.parse().map_err(|_| invalid())?;
    match uri.scheme_str() {
        Some("http") => {}
        Some("https") => {
            if !cfg!(feature = "tls") {
                return Err(Error::TlsUnavailable);
            }
        }
        _ => return Err(invalid()),
    }
    if uri.authority().is_none() {
        return Err(invalid());
    }
    Ok(trimmed.to_owned())
}

/// One agent per timeout group. The plugin sends no redirects by default, so
/// redirects are off here too. Credentials must not follow a redirect to
/// another host, and a 3xx is reported as a server error.
fn agent(timeout: Timeout, options: SyncOptions) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .max_redirects_will_error(false)
        .timeout_global(Some(timeout.total))
        .timeout_connect(Some(timeout.block))
        .timeout_send_request(Some(timeout.block))
        .timeout_send_body(Some(timeout.block))
        .timeout_recv_response(Some(timeout.block))
        .timeout_recv_body(Some(timeout.block));
    #[cfg(feature = "tls")]
    let config = config.tls_config(
        TlsConfig::builder()
            .disable_verification(options.accept_invalid_certs)
            .build(),
    );
    #[cfg(not(feature = "tls"))]
    let _ = options;
    let config = config.build();
    ureq::Agent::new_with_config(config)
}

#[cfg(test)]
mod tests;
