//! A tiny HTTP server on 127.0.0.1 for the client tests. Each test starts its
//! own server with a fixed set of routes. Routes match on the path, and the
//! handler sees the whole request, including the query and Authorization
//! header. Every request is recorded for the test to check. Nothing here
//! reaches another host.

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// A request as the server received it.
#[derive(Debug, Clone)]
pub struct Seen {
    pub method: String,
    /// Path and query, as sent.
    pub target: String,
    pub authorization: Option<String>,
    /// Header names are lowercase.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }
}

/// A response to send back.
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub fn redirect(status: u16, location: &str) -> Self {
        Self {
            status,
            headers: vec![("Location".to_owned(), location.to_owned())],
            body: Vec::new(),
        }
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_owned(), value.to_owned()));
        self
    }
}

pub type Handler = Box<dyn Fn(&Seen) -> Reply + Send + Sync>;

/// A running server. Its routes stay up until the test process exits.
pub struct Server {
    pub base: String,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    /// Starts a server with routes given as (path, handler) pairs. A request
    /// for a path without a route gets 404.
    pub fn start(routes: Vec<(&'static str, Handler)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let base = format!("http://{}", listener.local_addr().expect("local address"));
        let routes = Arc::new(routes);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let shared_seen = Arc::clone(&seen);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let routes = Arc::clone(&routes);
                let seen = Arc::clone(&shared_seen);
                thread::spawn(move || handle(stream, &routes, &seen));
            }
        });
        Self { base, seen }
    }

    /// The absolute URL for a path on this server.
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    /// Every request received so far, in order.
    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().map(|list| list.clone()).unwrap_or_default()
    }

    /// The requests for one path.
    pub fn requests_to(&self, path: &str) -> Vec<Seen> {
        self.requests().into_iter().filter(|r| r.path() == path).collect()
    }
}

/// A handler that always returns the same reply.
pub fn fixed(reply: impl Fn() -> Reply + Send + Sync + 'static) -> Handler {
    Box::new(move |_| reply())
}

/// A handler that returns a static body with status 200.
pub fn body(bytes: &'static [u8]) -> Handler {
    Box::new(move |_| Reply::ok(bytes))
}

fn handle(stream: TcpStream, routes: &[(&'static str, Handler)], seen: &Mutex<Vec<Seen>>) {
    let Ok(clone) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(clone);
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_owned();
    let target = parts.next().unwrap_or("/").to_owned();

    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).unwrap_or(0) == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let length: usize = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    let _ = reader.read_exact(&mut body);

    let authorization = headers
        .iter()
        .find(|(name, _)| name == "authorization")
        .map(|(_, value)| value.clone());
    let request = Seen {
        method,
        target,
        authorization,
        headers,
        body,
    };
    if let Ok(mut list) = seen.lock() {
        list.push(request.clone());
    }

    let reply = match routes.iter().find(|(path, _)| *path == request.path()) {
        Some((_, handler)) => handler(&request),
        None => Reply::status(404),
    };
    let mut out = stream;
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        reason(reply.status),
        reply.body.len()
    );
    for (name, value) in &reply.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    // The client may stop reading early (for example after a cancel), so write
    // errors are ignored.
    let _ = out.write_all(head.as_bytes());
    let _ = out.write_all(&reply.body);
    let _ = out.flush();
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        307 => "Temporary Redirect",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        406 => "Not Acceptable",
        _ => "Status",
    }
}
