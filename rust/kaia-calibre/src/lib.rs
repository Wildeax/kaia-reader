//! Calibre smart device client and KOReader format metadata cache.

use serde_json::{json, Map, Value};
use sha1::{Digest, Sha1};
use std::collections::VecDeque;
use std::fs::{self, File};
use std::io::{self, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const BROADCAST_PORTS: [u16; 5] = [54982, 48123, 39001, 44044, 59678];
const MAX_JSON: usize = 16 * 1024 * 1024;
const FIELDS: [&str; 10] = [
    "uuid",
    "lpath",
    "last_modified",
    "size",
    "title",
    "authors",
    "author_sort",
    "tags",
    "series",
    "series_index",
];

pub mod opcode {
    pub const OK: u64 = 0;
    pub const SET_CALIBRE_DEVICE_INFO: u64 = 1;
    pub const SET_CALIBRE_DEVICE_NAME: u64 = 2;
    pub const GET_DEVICE_INFORMATION: u64 = 3;
    pub const TOTAL_SPACE: u64 = 4;
    pub const FREE_SPACE: u64 = 5;
    pub const GET_BOOK_COUNT: u64 = 6;
    pub const SEND_BOOKLISTS: u64 = 7;
    pub const SEND_BOOK: u64 = 8;
    pub const GET_INITIALIZATION_INFO: u64 = 9;
    pub const BOOK_DONE: u64 = 11;
    pub const NOOP: u64 = 12;
    pub const DELETE_BOOK: u64 = 13;
    pub const GET_BOOK_FILE_SEGMENT: u64 = 14;
    pub const GET_BOOK_METADATA: u64 = 15;
    pub const SEND_BOOK_METADATA: u64 = 16;
    pub const DISPLAY_MESSAGE: u64 = 17;
    pub const CALIBRE_BUSY: u64 = 18;
    pub const SET_LIBRARY_INFO: u64 = 19;
    pub const ERROR: u64 = 20;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub address: SocketAddr,
    pub name: String,
    pub opds_port: u16,
}

/// Broadcast discovery is active only while this function runs.
pub fn discover(timeout: Duration) -> Vec<Server> {
    try_discover(timeout).unwrap_or_default()
}

/// Return discovery errors to callers that need to show them.
pub fn try_discover(timeout: Duration) -> io::Result<Vec<Server>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 8134))?;
    socket.set_broadcast(true)?;
    discover_with_socket(&socket, timeout, &BROADCAST_PORTS, Ipv4Addr::BROADCAST)
}

fn discover_with_socket(
    socket: &UdpSocket,
    timeout: Duration,
    ports: &[u16],
    target: Ipv4Addr,
) -> io::Result<Vec<Server>> {
    for port in ports {
        socket.send_to(b"hello", (target, *port))?;
    }
    let deadline = Instant::now() + timeout;
    let mut servers = Vec::new();
    let mut buffer = [0; 1024];
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        socket.set_read_timeout(Some(left))?;
        match socket.recv_from(&mut buffer) {
            Ok((len, sender)) => {
                if let Some((name, opds_port, port)) = parse_discovery(&buffer[..len]) {
                    let address = SocketAddr::new(sender.ip(), port);
                    if !servers.iter().any(|s: &Server| s.address == address) {
                        servers.push(Server {
                            address,
                            name,
                            opds_port,
                        });
                    }
                }
            }
            Err(err) if matches!(err.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => break,
            Err(err) => return Err(err),
        }
    }
    Ok(servers)
}

fn parse_discovery(bytes: &[u8]) -> Option<(String, u16, u16)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let rest = text.strip_prefix("calibre wireless device client (on ")?;
    let (name, ports) = rest.split_once(");")?;
    let (opds, socket) = ports.split_once(',')?;
    Some((name.to_owned(), opds.parse().ok()?, socket.parse().ok()?))
}

pub trait Storage {
    fn free_space(&self, root: &Path) -> io::Result<u64>;
    fn total_space(&self, root: &Path) -> io::Result<u64>;
    fn write_file_atomically(&self, path: &Path, source: &mut dyn Read, length: u64) -> io::Result<()>;
    fn delete(&self, path: &Path) -> io::Result<()>;
}

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub model: String,
    pub version: String,
    pub drive_uuid: String,
    pub accepted_extensions: Vec<String>,
}

impl Default for DeviceInfo {
    fn default() -> Self {
        let formats = [
            "epub", "fb2", "mobi", "azw", "xps", "doc", "docx", "djv", "djvu", "pdf", "cbz", "htm", "html",
            "xhtml", "pdb", "prc", "rtf", "txt", "md", "chm", "zip",
        ];
        Self {
            model: "Kobo".into(),
            version: "Kaia Reader".into(),
            drive_uuid: String::new(),
            accepted_extensions: formats.iter().map(|s| (*s).into()).collect(),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum Event {
    BookReceived { path: PathBuf, metadata: Value },
    BookDeleted { path: PathBuf },
    Message { text: String, kind: Option<u64> },
    Disconnected,
    None,
}

pub struct Connection<S: Storage> {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
    password: String,
    device: DeviceInfo,
    root: PathBuf,
    storage: S,
    books: Vec<Value>,
    drive: Value,
    cancelled: Arc<AtomicBool>,
    events: VecDeque<Event>,
}

impl<S: Storage> Connection<S> {
    pub fn connect(
        server: &Server,
        password: impl Into<String>,
        device_info: DeviceInfo,
        library_root: impl Into<PathBuf>,
        storage: S,
    ) -> io::Result<Self> {
        let stream = TcpStream::connect_timeout(&server.address, Duration::from_secs(5))?;
        stream.set_read_timeout(Some(Duration::from_millis(250)))?;
        let writer = stream.try_clone()?;
        let root = library_root.into();
        fs::create_dir_all(&root)?;
        let books = read_metadata_cache(&root)?;
        let drive = read_drive_info(&root)?;
        Ok(Self {
            reader: BufReader::new(stream),
            writer,
            password: password.into(),
            device: device_info,
            root,
            storage,
            books,
            drive,
            cancelled: Arc::new(AtomicBool::new(false)),
            events: VecDeque::new(),
        })
    }

    pub fn cancellation_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
    pub fn books(&self) -> &[Value] {
        &self.books
    }

    pub fn run_once(&mut self) -> io::Result<Event> {
        if let Some(event) = self.events.pop_front() {
            return Ok(event);
        }
        if self.cancelled.load(Ordering::SeqCst) {
            return Ok(Event::Disconnected);
        }
        let (op, arg) = match self.read_message() {
            Ok(Some(message)) => message,
            Ok(None) => return Ok(Event::Disconnected),
            Err(err) => {
                let _ = self.send(opcode::ERROR, json!({"message": err.to_string()}));
                return Err(err);
            }
        };
        self.handle(op, arg)
    }

    fn handle(&mut self, op: u64, arg: Value) -> io::Result<Event> {
        use opcode::*;
        match op {
            GET_INITIALIZATION_INFO => {
                let hash = match arg.get("passwordChallenge").and_then(Value::as_str) {
                    Some(challenge) if !self.password.is_empty() => format!("{:x}", Sha1::digest(format!("{}{}", self.password, challenge).as_bytes())),
                    _ => String::new(),
                };
                let lengths: Map<String, Value> = self.device.accepted_extensions.iter().map(|ext| (ext.clone(), json!(37))).collect();
                self.send(OK, json!({"appName":"KOReader", "acceptedExtensions":self.device.accepted_extensions, "cacheUsesLpaths":true, "canAcceptLibraryInfo":true, "canDeleteMultipleBooks":true, "canReceiveBookBinary":true, "canSendOkToSendbook":true, "canStreamBooks":true, "canStreamMetadata":true, "canUseCachedMetadata":true, "ccVersionNumber":self.device.version, "coverHeight":240, "deviceKind":self.device.model, "deviceName":format!("KOReader ({})",self.device.model), "extensionPathLengths":lengths, "passwordHash":hash, "maxBookContentPacketLen":4096, "useUuidFileNames":false, "versionOK":true}))?;
            }
            GET_DEVICE_INFORMATION => self.send(OK, json!({"device_info":{"device_store_uuid":self.device.drive_uuid,"device_name":format!("KOReader ({})",self.device.model)},"version":self.device.version,"device_version":self.device.version}))?,
            SET_CALIBRE_DEVICE_INFO => {
                let mut value = arg;
                if let (Some(old), Some(new)) = (self.drive.get("device_name"), value.as_object_mut()) { new.insert("device_name".into(), old.clone()); }
                self.drive = value;
                write_drive_info(&self.root, &self.drive)?;
                self.send(OK, json!({}))?;
            }
            SET_CALIBRE_DEVICE_NAME => {
                let name = arg.get("device_name").or_else(|| arg.get("name")).and_then(Value::as_str).or_else(|| arg.as_str()).ok_or_else(|| invalid("missing device name"))?;
                if let Some(info) = self.drive.as_object_mut() { info.insert("device_name".into(), json!(name)); }
                write_drive_info(&self.root, &self.drive)?;
                self.send(OK, json!({}))?;
            }
            GET_BOOK_METADATA => {
                let index = required_u64(&arg, "priKey")?;
                let book = self.books.get(index.saturating_sub(1) as usize).cloned().unwrap_or(Value::Null);
                self.send(OK, book)?;
            }
            TOTAL_SPACE => self.send(OK, json!({"total_space_on_device":self.storage.total_space(&self.root)?}))?,
            FREE_SPACE => self.send(OK, json!({"free_space_on_device":self.storage.free_space(&self.root)?}))?,
            SET_LIBRARY_INFO | SEND_BOOKLISTS | BOOK_DONE | CALIBRE_BUSY => self.send(OK, json!({}))?,
            GET_BOOK_COUNT => {
                self.send(OK, json!({"willStream":true,"willScan":true,"count":self.books.len()}))?;
                let ids: Vec<Value> = self.books.iter().enumerate().map(|(i, book)| json!({"priKey":i+1,"uuid":book["uuid"],"lpath":book["lpath"],"last_modified":book["last_modified"]})).collect();
                for id in ids { self.send(OK, id)?; }
            }
            NOOP => {
                if arg.get("count").is_some() { return Ok(Event::None); }
                if let Some(index) = arg.get("priKey").and_then(Value::as_u64) {
                    let book = self.books.get(index.saturating_sub(1) as usize).cloned().unwrap_or(Value::Null);
                    self.send(OK, book)?;
                } else {
                    self.send(OK, json!({}))?;
                }
                if arg.get("ejecting").and_then(Value::as_bool) == Some(true) { return Ok(Event::Disconnected); }
            }
            SEND_BOOK => return self.receive_book(&arg),
            SEND_BOOK_METADATA => {
                if let Some(data) = arg.get("data") { self.upsert_book(data); write_metadata_cache(&self.root, &self.books)?; }
            }
            DELETE_BOOK => return self.delete_books(&arg),
            GET_BOOK_FILE_SEGMENT => {
                let path = safe_path(&self.root, required_str(&arg, "lpath")?)?;
                match File::open(path) {
                    Ok(mut file) => {
                        let len = file.metadata()?.len();
                        self.send(OK, json!({"fileLength":len}))?;
                        io::copy(&mut file, &mut self.writer)?;
                    }
                    Err(_) => self.send(NOOP, json!({}))?,
                }
            }
            DISPLAY_MESSAGE => return Ok(Event::Message { text: arg.get("message").and_then(Value::as_str).unwrap_or("").into(), kind: arg.get("messageKind").and_then(Value::as_u64) }),
            ERROR => return Ok(Event::Message { text: arg.get("message").and_then(Value::as_str).unwrap_or("Calibre error").into(), kind: Some(ERROR) }),
            OK => {}
            _ => return Err(invalid("unsupported opcode")),
        }
        Ok(Event::None)
    }

    fn receive_book(&mut self, arg: &Value) -> io::Result<Event> {
        let length = required_u64(arg, "length")?;
        let lpath = required_str(arg, "lpath")?;
        let path = safe_path(&self.root, lpath)?;
        if self.storage.free_space(&self.root)? < length.saturating_add(128 * 1024) {
            self.send(opcode::ERROR, json!({"message":"No space left on device"}))?;
            return Ok(Event::None);
        }
        self.send(opcode::OK, json!({}))?;
        let mut source = CancelReader {
            reader: &mut self.reader,
            cancelled: &self.cancelled,
        };
        let mut limited = (&mut source).take(length);
        self.storage.write_file_atomically(&path, &mut limited, length)?;
        if limited.limit() != 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short book"));
        }
        let metadata = arg.get("metadata").cloned().unwrap_or(Value::Null);
        self.upsert_book(&metadata);
        write_metadata_cache(&self.root, &self.books)?;
        Ok(Event::BookReceived { path, metadata })
    }

    fn delete_books(&mut self, arg: &Value) -> io::Result<Event> {
        self.send(opcode::OK, json!({}))?;
        let paths = arg
            .get("lpaths")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("missing lpaths"))?;
        let mut deleted = Vec::new();
        for lpath in paths {
            let lpath = lpath.as_str().ok_or_else(|| invalid("invalid lpath"))?;
            let path = safe_path(&self.root, lpath)?;
            let uuid = self
                .books
                .iter()
                .find(|book| book["lpath"] == lpath)
                .and_then(|book| book["uuid"].as_str())
                .unwrap_or("none")
                .to_owned();
            if uuid != "none" {
                self.storage.delete(&path)?;
                self.books.retain(|book| book["lpath"] != lpath);
                deleted.push(path);
            }
            self.send(opcode::OK, json!({"uuid":uuid}))?;
        }
        write_metadata_cache(&self.root, &self.books)?;
        self.events
            .extend(deleted.into_iter().map(|path| Event::BookDeleted { path }));
        Ok(self.events.pop_front().unwrap_or(Event::None))
    }

    fn upsert_book(&mut self, book: &Value) {
        let slim = slim_book(book);
        if let Some(existing) = self.books.iter_mut().find(|old| old["lpath"] == slim["lpath"]) {
            *existing = slim;
        } else {
            self.books.push(slim);
        }
    }

    fn read_message(&mut self) -> io::Result<Option<(u64, Value)>> {
        let mut length = 0usize;
        loop {
            if self.cancelled.load(Ordering::SeqCst) {
                return Ok(None);
            }
            let byte = match self.read_byte()? {
                Some(byte) => byte,
                None => return Ok(None),
            };
            if byte == b'[' {
                if length == 0 || length > MAX_JSON {
                    return Err(invalid("invalid JSON length"));
                }
                let mut json_bytes = vec![0; length];
                json_bytes[0] = byte;
                self.read_exact_cancelled(&mut json_bytes[1..])?;
                let message: Value = serde_json::from_slice(&json_bytes)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                let array = message
                    .as_array()
                    .ok_or_else(|| invalid("message is not an array"))?;
                if array.len() != 2 {
                    return Err(invalid("message needs opcode and payload"));
                }
                return Ok(Some((
                    array[0].as_u64().ok_or_else(|| invalid("invalid opcode"))?,
                    array[1].clone(),
                )));
            }
            if !byte.is_ascii_digit() {
                return Err(invalid("invalid JSON length"));
            }
            length = length
                .checked_mul(10)
                .and_then(|n| n.checked_add((byte - b'0') as usize))
                .ok_or_else(|| invalid("JSON length overflow"))?;
            if length > MAX_JSON {
                return Err(invalid("JSON too large"));
            }
        }
    }

    fn read_byte(&mut self) -> io::Result<Option<u8>> {
        let mut byte = [0];
        loop {
            match self.reader.read(&mut byte) {
                Ok(0) => return Ok(None),
                Ok(_) => return Ok(Some(byte[0])),
                Err(err) if matches!(err.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {
                    if self.cancelled.load(Ordering::SeqCst) {
                        return Ok(None);
                    }
                }
                Err(err) => return Err(err),
            }
        }
    }

    fn read_exact_cancelled(&mut self, buffer: &mut [u8]) -> io::Result<()> {
        let mut done = 0;
        while done < buffer.len() {
            if self.cancelled.load(Ordering::SeqCst) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            match self.reader.read(&mut buffer[done..]) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short message")),
                Ok(n) => done += n,
                Err(err) if matches!(err.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {}
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    fn send(&mut self, opcode: u64, payload: Value) -> io::Result<()> {
        let body = serde_json::to_vec(&json!([opcode, payload])).map_err(io::Error::other)?;
        write!(self.writer, "{}", body.len())?;
        self.writer.write_all(&body)
    }
}

struct CancelReader<'a> {
    reader: &'a mut BufReader<TcpStream>,
    cancelled: &'a AtomicBool,
}

impl Read for CancelReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        loop {
            if self.cancelled.load(Ordering::SeqCst) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            match self.reader.read(buffer) {
                Err(err) if matches!(err.kind(), io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock) => {}
                result => return result,
            }
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn required_str<'a>(value: &'a Value, key: &str) -> io::Result<&'a str> {
    value.get(key).and_then(Value::as_str).ok_or_else(|| invalid(key))
}
fn required_u64(value: &Value, key: &str) -> io::Result<u64> {
    value.get(key).and_then(Value::as_u64).ok_or_else(|| invalid(key))
}
fn safe_path(root: &Path, lpath: &str) -> io::Result<PathBuf> {
    let relative = Path::new(lpath);
    if lpath.is_empty()
        || !relative
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err(invalid("unsafe book path"));
    }
    let mut path = root.to_path_buf();
    for part in relative.components() {
        path.push(part);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => return Err(invalid("symlink in book path")),
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(path)
}

fn slim_book(book: &Value) -> Value {
    let mut map = Map::new();
    for field in FIELDS {
        map.insert(
            field.into(),
            book.get(field).cloned().unwrap_or_else(|| {
                if field == "authors" || field == "tags" {
                    json!([])
                } else {
                    Value::Null
                }
            }),
        );
    }
    Value::Object(map)
}

pub fn read_metadata_cache(root: &Path) -> io::Result<Vec<Value>> {
    let path = existing_file(root, "metadata.calibre");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let value: Value = serde_json::from_reader(File::open(path)?).map_err(io::Error::other)?;
    let books = value
        .as_array()
        .ok_or_else(|| invalid("metadata.calibre must be an array"))?;
    Ok(books.iter().map(slim_book).collect())
}

pub fn write_metadata_cache(root: &Path, books: &[Value]) -> io::Result<()> {
    let path = existing_file(root, "metadata.calibre");
    write_json_atomically(&path, &Value::Array(books.iter().map(slim_book).collect()))
}

pub fn read_drive_info(root: &Path) -> io::Result<Value> {
    let path = existing_file(root, "driveinfo.calibre");
    if !path.exists() {
        return Ok(json!({}));
    }
    serde_json::from_reader(File::open(path)?).map_err(io::Error::other)
}

pub fn write_drive_info(root: &Path, info: &Value) -> io::Result<()> {
    write_json_atomically(&existing_file(root, "driveinfo.calibre"), info)
}

fn existing_file(root: &Path, name: &str) -> PathBuf {
    let visible = root.join(name);
    if visible.exists() {
        visible
    } else {
        let hidden = root.join(format!(".{name}"));
        if hidden.exists() {
            hidden
        } else {
            visible
        }
    }
}

fn write_json_atomically(path: &Path, value: &Value) -> io::Result<()> {
    let parent = path.parent().ok_or_else(|| invalid("missing parent"))?;
    fs::create_dir_all(parent)?;
    let temp = path.with_extension(format!("calibre.tmp.{}", std::process::id()));
    let result = (|| {
        let mut file = File::create(&temp)?;
        serde_json::to_writer_pretty(&mut file, value).map_err(io::Error::other)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{TcpListener, UdpSocket};
    use std::thread;

    struct TestStorage;
    impl Storage for TestStorage {
        fn free_space(&self, _: &Path) -> io::Result<u64> {
            Ok(1_000_000)
        }
        fn total_space(&self, _: &Path) -> io::Result<u64> {
            Ok(2_000_000)
        }
        fn write_file_atomically(&self, path: &Path, source: &mut dyn Read, length: u64) -> io::Result<()> {
            fs::create_dir_all(path.parent().unwrap())?;
            let tmp = path.with_extension("tmp");
            let mut file = File::create(&tmp)?;
            let copied = io::copy(&mut source.take(length), &mut file)?;
            if copied != length {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short book"));
            }
            fs::rename(tmp, path)
        }
        fn delete(&self, path: &Path) -> io::Result<()> {
            fs::remove_file(path)
        }
    }

    fn temp_root() -> PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "kaia-calibre-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn pair() -> (Connection<TestStorage>, TcpStream, PathBuf) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let root = temp_root();
        let server = Server {
            address: listener.local_addr().unwrap(),
            name: "test".into(),
            opds_port: 8080,
        };
        let client =
            Connection::connect(&server, "secret", DeviceInfo::default(), &root, TestStorage).unwrap();
        let (peer, _) = listener.accept().unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        (client, peer, root)
    }

    fn send(peer: &mut TcpStream, op: u64, arg: Value) {
        let body = serde_json::to_vec(&json!([op, arg])).unwrap();
        write!(peer, "{}", body.len()).unwrap();
        peer.write_all(&body).unwrap();
    }

    fn recv(peer: &mut TcpStream) -> (u64, Value) {
        let mut length = 0usize;
        let mut one = [0];
        loop {
            peer.read_exact(&mut one).unwrap();
            if one[0] == b'[' {
                break;
            }
            length = length * 10 + (one[0] - b'0') as usize;
        }
        let mut body = vec![0; length];
        body[0] = b'[';
        peer.read_exact(&mut body[1..]).unwrap();
        let array: Value = serde_json::from_slice(&body).unwrap();
        (array[0].as_u64().unwrap(), array[1].clone())
    }

    #[test]
    fn discovery_on_loopback() {
        let responder = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = responder.local_addr().unwrap().port();
        let worker = thread::spawn(move || {
            let mut buffer = [0; 64];
            let (len, source) = responder.recv_from(&mut buffer).unwrap();
            assert_eq!(&buffer[..len], b"hello");
            responder
                .send_to(b"calibre wireless device client (on laptop);8080,9090", source)
                .unwrap();
        });
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let servers =
            discover_with_socket(&socket, Duration::from_millis(100), &[port], Ipv4Addr::LOCALHOST).unwrap();
        worker.join().unwrap();
        assert_eq!(
            servers,
            vec![Server {
                address: SocketAddr::from((Ipv4Addr::LOCALHOST, 9090)),
                name: "laptop".into(),
                opds_port: 8080
            }]
        );
    }

    #[test]
    fn initialization_and_password() {
        let (mut client, mut peer, root) = pair();
        send(
            &mut peer,
            opcode::GET_INITIALIZATION_INFO,
            json!({"calibre_version":[7,0,0],"passwordChallenge":"salt"}),
        );
        assert_eq!(client.run_once().unwrap(), Event::None);
        let (op, info) = recv(&mut peer);
        assert_eq!(op, opcode::OK);
        assert_eq!(info["passwordHash"], format!("{:x}", Sha1::digest(b"secretsalt")));
        assert_eq!(info["extensionPathLengths"]["epub"], 37);
        assert_eq!(info["cacheUsesLpaths"], true);
        client.password.clear();
        send(&mut peer, opcode::GET_INITIALIZATION_INFO, json!({}));
        client.run_once().unwrap();
        assert_eq!(recv(&mut peer).1["passwordHash"], "");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn booklist_receive_delete_and_space() {
        let (mut client, mut peer, root) = pair();
        let book = json!({"uuid":"id-1","lpath":"Author/Book.epub","last_modified":"2026-10-13T00:00:00","title":"Book","authors":["A"],"size":9});
        send(
            &mut peer,
            opcode::SEND_BOOK,
            json!({"lpath":"Author/Book.epub","length":9,"metadata":book}),
        );
        peer.write_all(b"abc").unwrap();
        peer.write_all(b"defghi").unwrap();
        send(&mut peer, opcode::FREE_SPACE, json!({}));
        let event = client.run_once().unwrap();
        assert!(matches!(event, Event::BookReceived { .. }));
        assert_eq!(recv(&mut peer).0, opcode::OK);
        assert_eq!(fs::read(root.join("Author/Book.epub")).unwrap(), b"abcdefghi");
        client.run_once().unwrap();
        assert_eq!(recv(&mut peer).1["free_space_on_device"], 1_000_000);
        send(&mut peer, opcode::TOTAL_SPACE, json!({}));
        client.run_once().unwrap();
        assert_eq!(recv(&mut peer).1["total_space_on_device"], 2_000_000);
        send(&mut peer, opcode::GET_BOOK_COUNT, json!({}));
        client.run_once().unwrap();
        assert_eq!(recv(&mut peer).1["count"], 1);
        assert_eq!(recv(&mut peer).1["priKey"], 1);
        send(&mut peer, opcode::NOOP, json!({"count":1}));
        client.run_once().unwrap();
        send(&mut peer, opcode::NOOP, json!({"priKey":1}));
        client.run_once().unwrap();
        assert_eq!(recv(&mut peer).1["title"], "Book");
        send(
            &mut peer,
            opcode::DELETE_BOOK,
            json!({"lpaths":["Author/Book.epub"]}),
        );
        assert!(matches!(client.run_once().unwrap(), Event::BookDeleted { .. }));
        assert_eq!(recv(&mut peer).0, opcode::OK);
        assert_eq!(recv(&mut peer).1["uuid"], "id-1");
        assert!(!root.join("Author/Book.epub").exists());
        assert!(read_metadata_cache(&root).unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn malformed_message_and_disconnect() {
        let (mut client, mut peer, root) = pair();
        peer.write_all(b"5[bad]").unwrap();
        assert_eq!(client.run_once().unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(recv(&mut peer).0, opcode::ERROR);
        drop(peer);
        assert_eq!(client.run_once().unwrap(), Event::Disconnected);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn device_info_messages_and_file_segment() {
        let (mut client, mut peer, root) = pair();
        send(&mut peer, opcode::GET_DEVICE_INFORMATION, json!({}));
        client.run_once().unwrap();
        assert_eq!(recv(&mut peer).1["device_info"]["device_name"], "KOReader (Kobo)");
        send(
            &mut peer,
            opcode::SET_CALIBRE_DEVICE_INFO,
            json!({"device_store_uuid":"drive","device_name":"KOReader"}),
        );
        client.run_once().unwrap();
        assert_eq!(recv(&mut peer).0, opcode::OK);
        assert_eq!(read_drive_info(&root).unwrap()["device_store_uuid"], "drive");
        send(
            &mut peer,
            opcode::SET_CALIBRE_DEVICE_NAME,
            json!({"name":"Reader"}),
        );
        client.run_once().unwrap();
        recv(&mut peer);
        assert_eq!(read_drive_info(&root).unwrap()["device_name"], "Reader");
        fs::write(root.join("book.epub"), b"book").unwrap();
        send(
            &mut peer,
            opcode::GET_BOOK_FILE_SEGMENT,
            json!({"lpath":"book.epub"}),
        );
        client.run_once().unwrap();
        assert_eq!(recv(&mut peer).1["fileLength"], 4);
        let mut bytes = [0; 4];
        peer.read_exact(&mut bytes).unwrap();
        assert_eq!(&bytes, b"book");
        send(
            &mut peer,
            opcode::DISPLAY_MESSAGE,
            json!({"message":"Hello","messageKind":1}),
        );
        assert_eq!(
            client.run_once().unwrap(),
            Event::Message {
                text: "Hello".into(),
                kind: Some(1)
            }
        );
        send(&mut peer, opcode::NOOP, json!({"ejecting":true}));
        assert_eq!(client.run_once().unwrap(), Event::Disconnected);
        assert_eq!(recv(&mut peer).0, opcode::OK);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancellation_stops_waiting_for_server() {
        let (mut client, _peer, root) = pair();
        let flag = client.cancellation_handle();
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            flag.store(true, Ordering::SeqCst);
        });
        assert_eq!(client.run_once().unwrap(), Event::Disconnected);
        worker.join().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn metadata_cache_matches_koreader_fields() {
        let root = temp_root();
        let book = json!({"uuid":"u","lpath":"book.epub","title":"Book","authors":["A"],"extra":"drop"});
        write_metadata_cache(&root, &[book]).unwrap();
        let books = read_metadata_cache(&root).unwrap();
        let fields: Vec<&str> = books[0].as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            fields,
            [
                "author_sort",
                "authors",
                "last_modified",
                "lpath",
                "series",
                "series_index",
                "size",
                "tags",
                "title",
                "uuid"
            ]
        );
        assert_eq!(books[0]["tags"], json!([]));
        assert!(books[0]["series"].is_null());
        assert!(fs::read_to_string(root.join("metadata.calibre"))
            .unwrap()
            .contains("\"last_modified\": null"));
        fs::remove_dir_all(root).unwrap();
    }
}
