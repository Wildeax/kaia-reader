//! A tiny sync server on 127.0.0.1 for the client tests. It keeps users and
//! positions in memory and returns the error codes from the brief: 2001
//! unauthorized, 2002 user exists, 2003 invalid fields. No test reaches a
//! host other than the loopback address.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub struct Stored {
    pub progress: String,
    pub percentage: f64,
    pub device: String,
    pub device_id: String,
    pub timestamp: i64,
}

#[derive(Default)]
pub struct State {
    /// Username to the password digest the server received at registration.
    pub users: HashMap<String, String>,
    /// (username, document) to the stored position.
    pub progress: HashMap<(String, String), Stored>,
    /// Status and body for the next request only, to simulate server faults.
    pub next_reply: Option<(u16, String)>,
    /// The raw body of the last PUT, for checking what the client sent.
    pub last_put_body: Option<String>,
}

pub struct TestServer {
    pub url: String,
    pub state: Arc<Mutex<State>>,
}

impl TestServer {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let url = format!("http://{}", listener.local_addr().expect("local addr"));
        let state = Arc::new(Mutex::new(State::default()));
        let shared = Arc::clone(&state);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = Arc::clone(&shared);
                thread::spawn(move || handle(stream, &shared));
            }
        });
        Self { url, state }
    }
}

/// A server that accepts connections and never answers.
pub fn silent_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let url = format!("http://{}", listener.local_addr().expect("local addr"));
    thread::spawn(move || {
        // Keep the sockets open so the client waits for a reply that never comes.
        let mut held = Vec::new();
        for stream in listener.incoming().flatten() {
            held.push(stream);
        }
    });
    url
}

/// An address with nothing listening. The port was free when the probe
/// listener closed.
pub fn closed_address() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let addr = listener.local_addr().expect("local addr");
    drop(listener);
    format!("http://{addr}")
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: String,
}

fn handle(mut stream: TcpStream, state: &Mutex<State>) {
    let Some(request) = read_request(&stream) else {
        return;
    };
    let (status, body) = route(&request, state);
    let reply = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {len}\r\nConnection: close\r\n\r\n{body}",
        reason = reason(status),
        len = body.len(),
    );
    let _ = stream.write_all(reply.as_bytes());
    let _ = stream.flush();
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?.to_owned();
    let path = parts.next()?.to_owned();

    let mut headers = HashMap::new();
    loop {
        line.clear();
        reader.read_line(&mut line).ok()?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let length: usize = headers
        .get("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn route(req: &Request, state: &Mutex<State>) -> (u16, String) {
    let mut st = state.lock().expect("state lock");
    if let Some(reply) = st.next_reply.take() {
        return reply;
    }
    let authed = |st: &State| match (req.headers.get("x-auth-user"), req.headers.get("x-auth-key")) {
        (Some(user), Some(key)) => st.users.get(user) == Some(key),
        _ => false,
    };
    let unauthorized = || (401, error_body(2001, "Unauthorized"));

    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/healthcheck") => (200, json!({"state": "OK"}).to_string()),
        ("POST", "/users/create") => {
            let Ok(body) = serde_json::from_str::<Value>(&req.body) else {
                return (400, error_body(2003, "Invalid request"));
            };
            let (Some(user), Some(password)) = (
                body.get("username").and_then(Value::as_str),
                body.get("password").and_then(Value::as_str),
            ) else {
                return (400, error_body(2003, "Invalid request"));
            };
            if st.users.contains_key(user) {
                return (402, error_body(2002, "Username is already registered."));
            }
            st.users.insert(user.to_owned(), password.to_owned());
            (201, json!({"username": user}).to_string())
        }
        ("GET", "/users/auth") => {
            if authed(&st) {
                (200, json!({"authorized": "OK"}).to_string())
            } else {
                unauthorized()
            }
        }
        ("PUT", "/syncs/progress") => {
            if !authed(&st) {
                return unauthorized();
            }
            st.last_put_body = Some(req.body.clone());
            let Ok(body) = serde_json::from_str::<Value>(&req.body) else {
                return (400, error_body(2003, "Invalid request"));
            };
            let user = req.headers["x-auth-user"].clone();
            let fields = (
                body.get("document").and_then(Value::as_str),
                body.get("progress").and_then(Value::as_str),
                body.get("percentage").and_then(Value::as_f64),
                body.get("device").and_then(Value::as_str),
                body.get("device_id").and_then(Value::as_str),
            );
            let (Some(document), Some(progress), Some(percentage), Some(device), Some(device_id)) = fields
            else {
                return (400, error_body(2003, "Invalid fields"));
            };
            let timestamp = now_seconds();
            st.progress.insert(
                (user, document.to_owned()),
                Stored {
                    progress: progress.to_owned(),
                    percentage,
                    device: device.to_owned(),
                    device_id: device_id.to_owned(),
                    timestamp,
                },
            );
            (200, json!({"timestamp": timestamp}).to_string())
        }
        ("GET", path) if path.starts_with("/syncs/progress/") => {
            if !authed(&st) {
                return unauthorized();
            }
            let document = &path["/syncs/progress/".len()..];
            let user = req.headers["x-auth-user"].clone();
            match st.progress.get(&(user, document.to_owned())) {
                Some(stored) => (
                    200,
                    json!({
                        "document": document,
                        "progress": stored.progress,
                        "percentage": stored.percentage,
                        "device": stored.device,
                        "device_id": stored.device_id,
                        "timestamp": stored.timestamp,
                    })
                    .to_string(),
                ),
                // No position stored: the official server answers 200 with no percentage.
                None => (200, "{}".to_owned()),
            }
        }
        _ => (404, error_body(0, "Not found")),
    }
}

fn error_body(code: i64, message: &str) -> String {
    json!({"code": code, "message": message}).to_string()
}

fn now_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}
