//! Client tests against the in-memory server on 127.0.0.1. The only
//! unreachable addresses used are closed loopback ports.

use std::time::{Duration, Instant};

use super::*;
use crate::test_server::{closed_address, silent_server, TestServer};
use crate::InvalidField;

const DEVICE: &str = "Kobo Libra Colour";
const DEVICE_ID: &str = "device-kobo-1";

fn creds(user: &str, password: &str) -> Credentials {
    Credentials::new(user, password).expect("valid credentials")
}

fn client(server: &str, user: &str, password: &str) -> SyncClient {
    SyncClient::new(
        server,
        creds(user, password),
        DEVICE,
        DEVICE_ID,
        Timeouts::default(),
    )
    .expect("client")
}

fn sample(client: &SyncClient, document: &str, progress: &str, percentage: f64) -> Progress {
    client.local_progress(document, progress, percentage, None)
}

#[test]
fn credentials_trim_username_and_hash_password() {
    let c = creds("  reader  ", "secret");
    assert_eq!(c.username(), "reader");
    // MD5 of "secret".
    assert_eq!(c.userkey(), "5ebe2294ecd0e0f08eab7690d2a6ee69");
}

// main.lua:162-178, validateUser.
#[test]
fn credentials_reject_blank_fields() {
    assert_eq!(
        Credentials::new("   ", "pw"),
        Err(Error::InvalidCredentials(InvalidField::Username))
    );
    assert_eq!(
        Credentials::new("user", "  \t "),
        Err(Error::InvalidCredentials(InvalidField::Password))
    );
    assert_eq!(
        Credentials::new("", ""),
        Err(Error::InvalidCredentials(InvalidField::Both))
    );
}

#[test]
fn credentials_debug_hides_the_digest() {
    let c = creds("reader", "secret");
    let shown = format!("{c:?}");
    assert!(shown.contains("reader"));
    assert!(!shown.contains(c.userkey()));
}

#[test]
fn stored_login_restores_without_password() {
    let original = creds("reader", "secret");
    let restored = Credentials::from_stored("reader", original.userkey()).unwrap();
    assert_eq!(restored, original);
    assert!(Credentials::from_stored("", "abc").is_err());
}

#[test]
fn default_timeouts_match_the_plugin() {
    let t = Timeouts::default();
    assert_eq!(t.progress.block, Duration::from_secs(2));
    assert_eq!(t.progress.total, Duration::from_secs(5));
    assert_eq!(t.auth.block, Duration::from_secs(5));
    assert_eq!(t.auth.total, Duration::from_secs(10));
}

#[test]
fn options_verify_certificates_and_keep_koreader_status_handling() {
    let options = SyncOptions::default();
    assert!(!options.accept_invalid_certs);
    assert!(!options.accept_202);
}

#[test]
fn status_202_is_retryable_unless_enabled() {
    let server = TestServer::start();
    let client = client(&server.url, "reader", "secret");
    server.state.lock().unwrap().next_reply = Some((202, "{}".into()));
    assert!(matches!(
        client.push(&sample(&client, "book", "1", 0.1)),
        Err(Error::Server { status: 202, .. })
    ));
    let compatible = SyncClient::new_with_options(
        &server.url,
        creds("reader", "secret"),
        DEVICE,
        DEVICE_ID,
        Timeouts::default(),
        SyncOptions {
            accept_202: true,
            ..SyncOptions::default()
        },
    )
    .unwrap();
    server.state.lock().unwrap().next_reply = Some((202, "{}".into()));
    assert!(compatible.push(&sample(&compatible, "book", "1", 0.1)).is_ok());
}

#[test]
fn pull_accepts_string_percentage_and_float_timestamp() {
    let server = TestServer::start();
    let client = client(&server.url, "reader", "secret");
    server.state.lock().unwrap().next_reply = Some((
        200,
        r#"{"progress":"12","percentage":"0.75","timestamp":1234.5,"device":"other"}"#.into(),
    ));
    let pulled = client.pull("book").unwrap().unwrap();
    assert_eq!(pulled.percentage, 0.75);
    assert_eq!(pulled.timestamp, Some(1234));
}

#[test]
fn new_rejects_bad_addresses_without_connecting() {
    let make =
        |server: &str| SyncClient::new(server, creds("u", "p"), DEVICE, DEVICE_ID, Timeouts::default()).err();
    assert!(matches!(
        make("sync.example.org"),
        Some(Error::InvalidServerUrl(_))
    ));
    assert!(matches!(
        make("ftp://example.org"),
        Some(Error::InvalidServerUrl(_))
    ));
    assert!(matches!(make("http://"), Some(Error::InvalidServerUrl(_))));
    assert!(matches!(make(""), Some(Error::InvalidServerUrl(_))));
}

#[cfg(not(feature = "tls"))]
#[test]
fn https_is_refused_without_tls_feature() {
    let err = SyncClient::new(
        DEFAULT_SERVER,
        creds("u", "p"),
        DEVICE,
        DEVICE_ID,
        Timeouts::default(),
    )
    .err();
    assert_eq!(err, Some(Error::TlsUnavailable));
}

#[test]
fn trailing_slash_is_removed_from_server_address() {
    let server = TestServer::start();
    let c = client(&format!("{}/", server.url), "reader", "secret");
    c.register().unwrap();
    assert!(server.state.lock().unwrap().users.contains_key("reader"));
}

// api.json: register, POST /users/create, expected 201 on success.
#[test]
fn register_creates_user_and_rejects_duplicates() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().expect("first registration");
    assert_eq!(
        server
            .state
            .lock()
            .unwrap()
            .users
            .get("reader")
            .map(String::as_str),
        Some(creds("reader", "secret").userkey())
    );

    let err = c.register().unwrap_err();
    assert_eq!(
        err,
        Error::Server {
            status: 402,
            code: Some(2002),
            message: "Username is already registered.".to_owned(),
        }
    );
}

// api.json: authorize, GET /users/auth, expected 200 or 401.
#[test]
fn authorize_accepts_the_right_password_only() {
    let server = TestServer::start();
    client(&server.url, "reader", "secret").register().unwrap();

    client(&server.url, "reader", "secret")
        .authorize()
        .expect("right password");

    let err = client(&server.url, "reader", "wrong").authorize().unwrap_err();
    assert_eq!(
        err,
        Error::Unauthorized {
            code: Some(2001),
            message: "Unauthorized".to_owned(),
        }
    );
}

#[test]
fn push_then_pull_round_trips_the_position() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();

    c.push(&sample(&c, "doc-1", "/body/p[12]", 0.25)).expect("push");
    let pulled = c.pull("doc-1").expect("pull").expect("progress exists");

    assert_eq!(pulled.document, "doc-1");
    assert_eq!(pulled.progress, "/body/p[12]");
    assert_eq!(pulled.percentage, 0.25);
    assert_eq!(pulled.device, DEVICE);
    assert_eq!(pulled.device_id, DEVICE_ID);
    assert!(pulled.timestamp.is_some_and(|t| t > 0));
    assert_eq!(pulled.metadata, None);
}

// The plugin sends `percentage` rounded with Math.roundPercent. The floor of
// a double can land one step low: 0.3712 * 10000 is 3711.999..., so it
// becomes 0.3711. LuaJIT doubles behave the same way.
#[test]
fn round_percent_keeps_the_plugin_float_behaviour() {
    assert_eq!(crate::round_percent(0.3712), 0.3711);
}

// The plugin sends `percentage` rounded with Math.roundPercent.
#[test]
fn push_rounds_percentage_down_to_four_decimals() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();

    c.push(&sample(&c, "doc-1", "7", 0.123_49)).unwrap();
    let stored = server.state.lock().unwrap().progress[&("reader".to_owned(), "doc-1".to_owned())].percentage;
    assert_eq!(stored, 0.1234);
}

#[test]
fn push_sends_only_the_plugin_fields_by_default() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();

    c.push(&sample(&c, "doc-1", "7", 0.5)).unwrap();
    let body: Value =
        serde_json::from_str(server.state.lock().unwrap().last_put_body.as_deref().unwrap()).unwrap();
    let mut keys: Vec<&str> = body.as_object().unwrap().keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["device", "device_id", "document", "percentage", "progress"]
    );
    assert_eq!(body["progress"], "7");
}

// main.lua:1075 metadata: filename, title, authors are sent when enabled.
#[test]
fn push_sends_metadata_when_given() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();

    let meta = Metadata {
        filename: Some("novel.epub".to_owned()),
        title: Some("A Novel".to_owned()),
        authors: None,
    };
    let progress = c.local_progress("doc-1", "7", 0.5, Some(meta));
    c.push(&progress).unwrap();

    let body: Value =
        serde_json::from_str(server.state.lock().unwrap().last_put_body.as_deref().unwrap()).unwrap();
    assert_eq!(body["metadata"]["filename"], "novel.epub");
    assert_eq!(body["metadata"]["title"], "A Novel");
    assert!(body["metadata"].get("authors").is_none());
}

#[test]
fn push_with_wrong_password_is_unauthorized() {
    let server = TestServer::start();
    client(&server.url, "reader", "secret").register().unwrap();

    let bad = client(&server.url, "reader", "wrong");
    let err = bad.push(&sample(&bad, "doc-1", "7", 0.5)).unwrap_err();
    assert!(
        matches!(err, Error::Unauthorized { code: Some(2001), .. }),
        "{err:?}"
    );
    assert!(server.state.lock().unwrap().progress.is_empty());
}

#[test]
fn pull_without_stored_position_returns_none() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();
    assert_eq!(c.pull("never-pushed").unwrap(), None);
}

// Positions belong to the user: another account sees nothing.
#[test]
fn positions_are_per_user() {
    let server = TestServer::start();
    let alice = client(&server.url, "alice", "pw");
    alice.register().unwrap();
    alice.push(&sample(&alice, "doc-1", "7", 0.5)).unwrap();

    let bob = client(&server.url, "bob", "pw");
    bob.register().unwrap();
    assert_eq!(bob.pull("doc-1").unwrap(), None);
}

// An older server sends no timestamp. The client reports None for it, and
// the percentage decides the direction (main.lua:901-902).
#[test]
fn pull_accepts_a_reply_without_timestamp() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();
    server.state.lock().unwrap().next_reply = Some((
        200,
        r#"{"progress":"12","percentage":0.4,"device":"Old","device_id":"x"}"#.to_owned(),
    ));

    let pulled = c.pull("doc-old").unwrap().expect("progress");
    assert_eq!(pulled.timestamp, None);
    assert_eq!(pulled.progress, "12");
    assert_eq!(pulled.percentage, 0.4);
}

// KOReader sends page numbers as text, but a server may return them as JSON numbers.
#[test]
fn pull_accepts_numeric_progress() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();
    server.state.lock().unwrap().next_reply = Some((200, r#"{"progress":12,"percentage":0.4}"#.to_owned()));

    let pulled = c.pull("doc-num").unwrap().expect("progress");
    assert_eq!(pulled.progress, "12");
}

// A 200 with no percentage is "No progress found" (main.lua:851-859).
#[test]
fn pull_reply_without_percentage_is_none() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();
    server.state.lock().unwrap().next_reply = Some((200, "{}".to_owned()));
    assert_eq!(c.pull("doc-x").unwrap(), None);
}

#[test]
fn health_reports_ok_server() {
    let server = TestServer::start();
    client(&server.url, "reader", "secret").health().expect("health");
}

#[test]
fn server_error_with_json_message_is_reported() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();

    server.state.lock().unwrap().next_reply =
        Some((503, r#"{"code":2005,"message":"Maintenance"}"#.to_owned()));
    let err = c.authorize().unwrap_err();
    assert_eq!(
        err,
        Error::Server {
            status: 503,
            code: Some(2005),
            message: "Maintenance".to_owned(),
        }
    );
}

#[test]
fn server_error_with_plain_body_uses_default_message() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();

    server.state.lock().unwrap().next_reply = Some((500, "boom".to_owned()));
    let err = c.push(&sample(&c, "doc-1", "7", 0.5)).unwrap_err();
    assert_eq!(
        err,
        Error::Server {
            status: 500,
            code: None,
            message: "Unknown server error".to_owned(),
        }
    );
}

#[test]
fn pull_with_malformed_body_is_a_protocol_error() {
    let server = TestServer::start();
    let c = client(&server.url, "reader", "secret");
    c.register().unwrap();

    server.state.lock().unwrap().next_reply = Some((200, "not json".to_owned()));
    assert!(matches!(c.pull("doc-1"), Err(Error::Protocol(_))));
}

#[test]
fn unreachable_server_is_a_network_error() {
    let c = client(&closed_address(), "reader", "secret");
    let err = c.health().unwrap_err();
    assert!(matches!(err, Error::Network(_)), "{err:?}");
}

// A server that never answers must not block the caller past the timeout.
#[test]
fn silent_server_times_out() {
    let url = silent_server();
    let timeouts = Timeouts {
        progress: Timeout {
            block: Duration::from_millis(200),
            total: Duration::from_millis(400),
        },
        auth: Timeout {
            block: Duration::from_millis(200),
            total: Duration::from_millis(400),
        },
    };
    let c = SyncClient::new(&url, creds("reader", "secret"), DEVICE, DEVICE_ID, timeouts).unwrap();

    let started = Instant::now();
    let err = c.pull("doc-1").unwrap_err();
    let elapsed = started.elapsed();

    assert_eq!(err, Error::Timeout);
    assert!(elapsed < Duration::from_secs(5), "took {elapsed:?}");
}
