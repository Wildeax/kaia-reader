//! Network requests and file indexing run on a worker after a user action.
use std::fs;
use std::io::{self, Read, Write};
use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc,
};
use std::time::{Duration, Instant};

use kaia_calibre::{check_target, Connection, DeviceInfo, Server, Storage};
use kaia_gfx::{
    image::{decode_rgb, DecodeLimits},
    Pixmap,
};
use kaia_i18n::{format as tr_format, tr};
use kaia_library::Library;
use kaia_opds::{Acquisition, Catalog, Client, Feed};

pub enum Request {
    Feed(Catalog, String),
    Search(Catalog, Feed, String),
    Cover(Catalog, String),
    Download(Catalog, Acquisition),
    Discover,
    Connect { address: String, password: String },
}

/// What the worker sends back. The worker thread has no translation catalog, so its
/// text is English. The UI thread turns it into the language in use, see [`worker_text`].
pub enum Reply {
    Progress(Progress),
    Feed(Feed),
    Cover(Pixmap),
    Servers(Vec<Server>),
    Book(i64),
    Finished(Result<(), String>),
}

/// A line of status text. A line with numbers keeps the numbers, so the UI thread can
/// write it in the language in use.
pub enum Progress {
    /// English text with no numbers. See [`worker_text`].
    Text(String),
    /// Kilobytes received, and the size of the file when the server sent one.
    Downloading { done_kb: u64, total_kb: Option<u64> },
    /// Kilobytes received, and the size of the file.
    Receiving { done_kb: u64, total_kb: u64 },
}

impl Progress {
    /// The line to show. Call this on the UI thread.
    pub fn text(&self) -> String {
        match self {
            Self::Text(text) => worker_text(text),
            Self::Downloading {
                done_kb,
                total_kb: Some(total_kb),
            } => tr_format(
                &tr("Downloading %1 of %2 KB"),
                &[&done_kb.to_string(), &total_kb.to_string()],
            ),
            Self::Downloading {
                done_kb,
                total_kb: None,
            } => tr_format(&tr("Downloading %1 KB"), &[&done_kb.to_string()]),
            Self::Receiving { done_kb, total_kb } => tr_format(
                &tr("Receiving %1 of %2 KB"),
                &[&done_kb.to_string(), &total_kb.to_string()],
            ),
        }
    }
}

pub struct Worker {
    rx: Receiver<Reply>,
    cancel: Arc<AtomicBool>,
}

impl Worker {
    pub fn start(request: Request, root: PathBuf, database: PathBuf, enable_wifi: bool) -> Self {
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let failure = tx.clone();
        if std::thread::Builder::new()
            .name("kaia-catalogs".into())
            .spawn(move || {
                let result = run(request, &root, &database, enable_wifi, &tx, &stop);
                let result = if stop.load(Ordering::Relaxed) {
                    Err("Cancelled.".into())
                } else {
                    result
                };
                let _ = tx.send(Reply::Finished(result));
            })
            .is_err()
        {
            let _ = failure.send(Reply::Finished(Err("Cannot start the network worker.".into())));
        }
        Self { rx, cancel }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn next(&self) -> Option<Reply> {
        match self.rx.try_recv() {
            Ok(reply) => Some(reply),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Reply::Finished(Err("Network worker stopped.".into())))
            }
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn client(catalog: &Catalog, stop: &Arc<AtomicBool>) -> Result<Client, String> {
    Client::for_catalog(catalog)
        .map(|c| c.with_cancellation(stop.clone()))
        .map_err(opds_error)
}

fn run(
    request: Request,
    root: &Path,
    database: &Path,
    enable_wifi: bool,
    tx: &Sender<Reply>,
    stop: &Arc<AtomicBool>,
) -> Result<(), String> {
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    if enable_wifi {
        progress(tx, "Turning on Wi-Fi...");
        // Platform owns the scripts and device rules. Construct its service on this thread.
        let mut platform = kaia_platform::Platform::detect().map_err(|_| "Cannot start Wi-Fi.")?;
        platform
            .network
            .enable_wifi()
            .map_err(|_| "Cannot turn on Wi-Fi.")?;
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        platform
            .network
            .obtain_ip()
            .map_err(|_| "Cannot join the saved Wi-Fi network.")?;
    }
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    match request {
        Request::Feed(c, url) => {
            progress(tx, "Loading catalog...");
            let feed = client(&c, stop)?.fetch(&url).map_err(opds_error)?;
            let _ = tx.send(Reply::Feed(feed));
        }
        Request::Search(c, feed, terms) => {
            progress(tx, "Searching catalog...");
            let feed = client(&c, stop)?.search(&feed, &terms).map_err(opds_error)?;
            let _ = tx.send(Reply::Feed(feed));
        }
        Request::Cover(c, url) => {
            progress(tx, "Loading cover...");
            let bytes = client(&c, stop)?.get_bytes(&url).map_err(opds_error)?;
            let cover = decode_rgb(&bytes, DecodeLimits::default()).map_err(|_| "Cannot read this cover.")?;
            let _ = tx.send(Reply::Cover(cover));
        }
        Request::Download(c, acquisition) => {
            progress(tx, "Downloading book...");
            let client = client(&c, stop)?;
            let acquisition = if acquisition.indirect {
                progress(tx, "Finding the download format...");
                let format = acquisition.file_type();
                client
                    .resolve_indirect(&acquisition)
                    .map_err(opds_error)?
                    .into_iter()
                    .find(|a| {
                        !a.indirect
                            && a.file_type().is_some()
                            && (format.is_none() || a.file_type() == format)
                    })
                    .ok_or("The catalog did not offer this download format.")?
            } else {
                acquisition
            };
            let mut last = Instant::now();
            fs::create_dir_all(root).map_err(|_| "Cannot create the downloads folder.")?;
            let staging = tempfile::tempdir_in(root).map_err(|_| "Cannot prepare the download.")?;
            let path = client
                .download(
                    &acquisition,
                    staging.path(),
                    |bytes, total| {
                        if last.elapsed() >= Duration::from_millis(500) {
                            let _ = tx.send(Reply::Progress(Progress::Downloading {
                                done_kb: bytes / 1024,
                                total_kb: total.map(|n| n / 1024),
                            }));
                            last = Instant::now();
                        }
                    },
                    stop,
                )
                .map_err(opds_error)?;
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let path = publish_download(&path, root)?;
            progress(tx, "Adding book to the library...");
            // Once a file is published, finish its index even if the user leaves.
            let id = index_file(database, &path)?;
            let _ = tx.send(Reply::Book(id));
        }
        Request::Discover => {
            progress(tx, "Looking for Calibre...");
            let servers = kaia_calibre::try_discover_cancelled(Duration::from_secs(3), stop)
                .map_err(|_| "Cannot discover Calibre. Try a manual address.")?;
            let _ = tx.send(Reply::Servers(servers));
        }
        Request::Connect { address, password } => {
            progress(tx, "Connecting to Calibre...");
            let address = address
                .to_socket_addrs()
                .map_err(|_| "Use a server address and port.")?
                .next()
                .ok_or("Cannot find this server.")?;
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let server = Server {
                address,
                name: "Calibre".into(),
                opds_port: 0,
            };
            let storage = BookStorage {
                tx: tx.clone(),
                stop: stop.clone(),
                root: root.to_path_buf(),
                database: database.to_path_buf(),
            };
            let mut connection = Connection::connect(&server, password, DeviceInfo::default(), root, storage)
                .map_err(|_| "Cannot connect to Calibre.")?
                .with_cancellation(stop.clone());
            progress(tx, "Connected. Send books from Calibre.");
            loop {
                match connection.run_once().map_err(calibre_error)? {
                    kaia_calibre::Event::BookReceived { path, .. } => {
                        progress(tx, "Adding book to the library...");
                        let id = index_file(database, &path)?;
                        let _ = tx.send(Reply::Book(id));
                        progress(tx, "Book received. Connected to Calibre.");
                    }
                    kaia_calibre::Event::BookDeleted { path } => {
                        let mut library = Library::open(database).map_err(|_| "Cannot open the library.")?;
                        if let Some(book) =
                            library.book_by_path(&path).map_err(|_| "Cannot find the book.")?
                        {
                            library
                                .remove_book(book.id)
                                .map_err(|_| "Cannot update the library.")?;
                        }
                    }
                    kaia_calibre::Event::Message { .. } => {
                        progress(tx, "Calibre sent a message. Check the server.")
                    }
                    kaia_calibre::Event::Disconnected => break,
                    kaia_calibre::Event::None => {}
                }
            }
        }
    }
    Ok(())
}

fn progress(tx: &Sender<Reply>, text: &str) {
    let _ = tx.send(Reply::Progress(Progress::Text(text.into())));
}

pub fn opds_error(error: kaia_opds::Error) -> String {
    use kaia_opds::Error;
    match error {
        Error::Unauthorized | Error::Forbidden => "Authentication failed. Check the username and password.",
        Error::Parse(_) => "This address did not return an OPDS feed.",
        Error::Cancelled => "Cancelled.",
        Error::Timeout => "The server took too long to reply.",
        Error::Io(_) => "Cannot save the book. Check the downloads folder.",
        Error::NotFound => "The server could not find this item.",
        Error::Indirect => "This format needs an external download service.",
        _ => "Cannot load this item. Check the address and connection.",
    }
    .into()
}

/// Scan one file through the library's directory scanner. It resolves this link
/// to the final file, so the database keeps the books folder path.
fn index_file(database: &Path, path: &Path) -> Result<i64, String> {
    let dir = tempfile::tempdir().map_err(|_| "Cannot prepare the library scan.")?;
    let path = fs::canonicalize(path).map_err(|_| "Cannot find the downloaded book.")?;
    let name = path.file_name().ok_or("The book has no filename.")?;
    std::os::unix::fs::symlink(&path, dir.path().join(name))
        .map_err(|_| "Cannot prepare the library scan.")?;
    let mut library = Library::open(database).map_err(|_| "Cannot open the library.")?;
    library
        .scan(&[dir.path().into()], |_| {}, &AtomicBool::new(false))
        .map_err(|_| "The book was saved but could not be indexed.")?;
    library
        .book_by_path(&path)
        .map_err(|_| "Cannot read the library.")?
        .map(|b| b.id)
        .ok_or_else(|| "The book format is not supported.".into())
}

struct BookStorage {
    tx: Sender<Reply>,
    stop: Arc<AtomicBool>,
    /// The downloads root. Every target must lie under it and pass the name rules.
    root: PathBuf,
    /// The library database, for the files it indexes.
    database: PathBuf,
}
impl Storage for BookStorage {
    fn free_space(&self, root: &Path) -> io::Result<u64> {
        fs2::available_space(root)
    }
    fn total_space(&self, root: &Path) -> io::Result<u64> {
        fs2::total_space(root)
    }
    fn write_file_atomically(&self, path: &Path, source: &mut dyn Read, length: u64) -> io::Result<()> {
        check_target(&self.root, path)?;
        let parent = path.parent().ok_or_else(|| io::Error::other("No books folder"))?;
        fs::create_dir_all(parent)?;
        let mut part = tempfile::NamedTempFile::new_in(parent)?;
        let mut written = 0;
        let mut buffer = [0; 64 * 1024];
        let mut last = Instant::now();
        while written < length {
            if self.stop.load(Ordering::Relaxed) {
                return Err(io::Error::other("Cancelled"));
            }
            let count = source.read(&mut buffer)?;
            if count == 0 {
                return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Short book"));
            }
            part.write_all(&buffer[..count])?;
            written += count as u64;
            if last.elapsed() >= Duration::from_millis(500) {
                let _ = self.tx.send(Reply::Progress(Progress::Receiving {
                    done_kb: written / 1024,
                    total_kb: length / 1024,
                }));
                last = Instant::now();
            }
        }
        if self.stop.load(Ordering::Relaxed) {
            return Err(io::Error::other("Cancelled"));
        }
        part.as_file().sync_all()?;
        part.persist(path).map_err(|e| e.error)?;
        Ok(())
    }
    fn delete(&self, path: &Path) -> io::Result<()> {
        check_target(&self.root, path)?;
        fs::remove_file(path)
    }
    fn is_indexed(&self, path: &Path) -> bool {
        let Ok(path) = fs::canonicalize(path) else {
            return false;
        };
        Library::open(&self.database)
            .ok()
            .and_then(|library| library.book_by_path(&path).ok().flatten())
            .is_some()
    }
}

/// The text for a connection error. A refused file has its own text, and every
/// other error means that the connection ended.
fn calibre_error(error: io::Error) -> String {
    if error.kind() == io::ErrorKind::PermissionDenied {
        "Calibre sent a file this app does not accept.".into()
    } else {
        "Calibre connection ended. Check the server and password.".into()
    }
}

fn publish_download(source: &Path, root: &Path) -> Result<PathBuf, String> {
    let name = source
        .file_name()
        .ok_or("The book has no filename.")?
        .to_string_lossy();
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    let extension = source.extension().unwrap_or_default().to_string_lossy();
    for number in 0..10_000 {
        let name = if number == 0 {
            name.to_string()
        } else {
            format!("{stem} ({number}).{extension}")
        };
        let target = root.join(name);
        match fs::OpenOptions::new().write(true).create_new(true).open(&target) {
            Ok(file) => {
                drop(file);
                if fs::rename(source, &target).is_err() {
                    let _ = fs::remove_file(&target);
                    return Err("Cannot save the downloaded book.".into());
                }
                return Ok(target);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("Cannot save the downloaded book.".into()),
        }
    }
    Err("Too many books share this filename.".into())
}

/// The words for English text that the worker sent. The catalog belongs to the UI
/// thread, so call this on the UI thread. Text that is not listed comes back as it is.
pub fn worker_text(english: &str) -> String {
    match english {
        "Turning on Wi-Fi..." => tr("Turning on Wi-Fi..."),
        "Loading catalog..." => tr("Loading catalog..."),
        "Searching catalog..." => tr("Searching catalog..."),
        "Loading cover..." => tr("Loading cover..."),
        "Downloading book..." => tr("Downloading book..."),
        "Finding the download format..." => tr("Finding the download format..."),
        "Adding book to the library..." => tr("Adding book to the library..."),
        "Looking for Calibre..." => tr("Looking for Calibre..."),
        "Connecting to Calibre..." => tr("Connecting to Calibre..."),
        "Connected. Send books from Calibre." => tr("Connected. Send books from Calibre."),
        "Book received. Connected to Calibre." => tr("Book received. Connected to Calibre."),
        "Calibre sent a message. Check the server." => tr("Calibre sent a message. Check the server."),
        "Cancelled." => tr("Cancelled."),
        "Cannot start the network worker." => tr("Cannot start the network worker."),
        "Network worker stopped." => tr("Network worker stopped."),
        "Cannot start Wi-Fi." => tr("Cannot start Wi-Fi."),
        "Cannot turn on Wi-Fi." => tr("Cannot turn on Wi-Fi."),
        "Cannot join the saved Wi-Fi network." => tr("Cannot join the saved Wi-Fi network."),
        "Cannot read this cover." => tr("Cannot read this cover."),
        "The catalog did not offer this download format." => {
            tr("The catalog did not offer this download format.")
        }
        "Cannot create the downloads folder." => tr("Cannot create the downloads folder."),
        "Cannot prepare the download." => tr("Cannot prepare the download."),
        "Cannot discover Calibre. Try a manual address." => {
            tr("Cannot discover Calibre. Try a manual address.")
        }
        "Use a server address and port." => tr("Use a server address and port."),
        "Cannot find this server." => tr("Cannot find this server."),
        "Cannot connect to Calibre." => tr("Cannot connect to Calibre."),
        "Calibre connection ended. Check the server and password." => {
            tr("Calibre connection ended. Check the server and password.")
        }
        "Calibre sent a file this app does not accept." => {
            tr("Calibre sent a file this app does not accept.")
        }
        "Cannot open the library." => tr("Cannot open the library."),
        "Cannot find the book." => tr("Cannot find the book."),
        "Cannot update the library." => tr("Cannot update the library."),
        "Cannot prepare the library scan." => tr("Cannot prepare the library scan."),
        "Cannot find the downloaded book." => tr("Cannot find the downloaded book."),
        "The book has no filename." => tr("The book has no filename."),
        "The book was saved but could not be indexed." => tr("The book was saved but could not be indexed."),
        "Cannot read the library." => tr("Cannot read the library."),
        "The book format is not supported." => tr("The book format is not supported."),
        "Cannot save the downloaded book." => tr("Cannot save the downloaded book."),
        "Too many books share this filename." => tr("Too many books share this filename."),
        "Authentication failed. Check the username and password." => {
            tr("Authentication failed. Check the username and password.")
        }
        "This address did not return an OPDS feed." => tr("This address did not return an OPDS feed."),
        "The server took too long to reply." => tr("The server took too long to reply."),
        "Cannot save the book. Check the downloads folder." => {
            tr("Cannot save the book. Check the downloads folder.")
        }
        "The server could not find this item." => tr("The server could not find this item."),
        "This format needs an external download service." => {
            tr("This format needs an external download service.")
        }
        "Cannot load this item. Check the address and connection." => {
            tr("Cannot load this item. Check the address and connection.")
        }
        other => other.to_string(),
    }
}
