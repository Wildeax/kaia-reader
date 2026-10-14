//! Opt-in dictionary installation. No network activity occurs until `install`.
//! Archives stay in `.partial` until their pinned size and SHA-256 match.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::Library;

#[derive(Clone, Debug, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub lang_in: String,
    pub lang_out: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
    pub license: String,
}

#[derive(Deserialize)]
struct Catalog {
    entries: Vec<CatalogEntry>,
}

/// The pinned catalog is compiled into the app; it is never fetched at runtime.
pub fn catalog() -> &'static [CatalogEntry] {
    static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../../../resources/kaia-dict/catalog.json"))
            .expect("the checked-in dictionary catalog must parse")
    });
    &CATALOG.entries
}

/// Permission is checked before creating files or starting a request. Cancellation
/// also applies during extraction and immediately before the final rename.
pub struct Permission<'a> {
    pub allowed: bool,
    pub airplane_mode: bool,
    pub cancel: &'a AtomicBool,
}

/// Downloads and installs one pinned archive. Call on a worker thread.
/// Existing installations are left intact. Failure removes this attempt's files.
pub fn install(entry: &CatalogEntry, root: &Path, permission: Permission<'_>) -> io::Result<PathBuf> {
    if !permission.allowed {
        return Err(invalid("Dictionary downloads are off"));
    }
    if permission.airplane_mode {
        return Err(invalid("Airplane mode is on. Turn it off to use Wi-Fi."));
    }
    check_cancel(permission.cancel)?;
    if entry.id.is_empty() || !entry.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(invalid("Invalid dictionary install name"));
    }
    if entry.size == 0 || entry.sha256.len() != 64 || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(invalid("Invalid dictionary checksum or size"));
    }
    // All pinned URLs use HTTPS. HTTP is allowed for loopback test fixtures only.
    if !entry.url.starts_with("https://") && !entry.url.starts_with("http://127.0.0.1:") {
        return Err(invalid("Dictionary download requires HTTPS"));
    }
    directory(root)?;
    let partial = root.join(".partial");
    directory(&partial)?;
    let target = root.join(&entry.id);
    if target.symlink_metadata().is_ok() {
        return Err(invalid("Dictionary is already installed"));
    }
    // Serializes installers for this dictionary across processes. A crash leaves
    // the lock in .partial for manual cleanup, never a half-installed dictionary.
    let lock_path = partial.join(format!("{}.lock", entry.id));
    let lock = OpenOptions::new().write(true).create_new(true).open(&lock_path)?;
    let _lock = InstallLock {
        path: lock_path,
        _file: lock,
    };
    let work = tempfile::Builder::new()
        .prefix("dictionary-")
        .tempdir_in(&partial)?;
    let archive = work.path().join("download");
    let unpacked = work.path().join("unpacked");
    fs::create_dir(&unpacked)?;
    let mut output = File::create(&archive)?;
    let mut config = ureq::Agent::config_builder()
        .https_only(entry.url.starts_with("https://"))
        .max_redirects(5)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(30 * 60)));
    if entry.url.starts_with("http://127.0.0.1:") {
        config = config.proxy(None);
    }
    let agent = ureq::Agent::new_with_config(config.build());
    let url = url::Url::parse(&entry.url).map_err(io::Error::other)?;
    let mut response = agent
        .get(url.as_str())
        .header("Accept-Encoding", "identity")
        .call()
        .map_err(io::Error::other)?;
    let mut reader = response.body_mut().as_reader();
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buffer = [0; 64 * 1024];
    loop {
        check_cancel(permission.cancel)?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > entry.size {
            return Err(invalid("Dictionary size mismatch"));
        }
        hash.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
    }
    if size != entry.size {
        return Err(invalid("Dictionary size mismatch"));
    }
    if hex(&hash.finalize()) != entry.sha256.to_ascii_lowercase() {
        return Err(invalid("Dictionary SHA-256 mismatch"));
    }
    output.sync_all()?;
    drop(output);
    unpack(&archive, &unpacked, permission.cancel)?;
    let library = Library::open_dir(&unpacked).map_err(io::Error::other)?;
    if library.dictionaries().is_empty() || !library.errors().is_empty() {
        return Err(invalid("Archive does not contain valid StarDict dictionaries"));
    }
    drop(library);
    // Retain the source and license beside the dictionary, including when its
    // archive contains no license file. Preserve all archive license files too.
    let mut provenance = File::create(unpacked.join("kaia-source.txt"))?;
    writeln!(
        provenance,
        "{}\n{}\n{}\nSHA-256: {}",
        entry.name, entry.url, entry.license, entry.sha256
    )?;
    provenance.sync_all()?;
    check_cancel(permission.cancel)?;
    if target.symlink_metadata().is_ok() {
        return Err(invalid("Dictionary is already installed"));
    }
    fs::rename(&unpacked, &target)?;
    Ok(target)
}

struct InstallLock {
    path: PathBuf,
    _file: File,
}
impl Drop for InstallLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn directory(path: &Path) -> io::Result<()> {
    match path.symlink_metadata() {
        Ok(meta) if meta.is_symlink() || !meta.is_dir() => {
            Err(invalid("Dictionary folder must be a real directory"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir_all(path),
        Err(error) => Err(error),
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn check_cancel(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Dictionary download canceled",
        ))
    } else {
        Ok(())
    }
}

fn safe_path(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(invalid("Unsafe dictionary archive path"));
    }
    let normalized: PathBuf = path.components().filter(|c| *c != Component::CurDir).collect();
    if normalized.as_os_str().is_empty() {
        return Err(invalid("Empty dictionary archive path"));
    }
    Ok(normalized)
}

const MAX_UNPACKED: u64 = 4 * 1024 * 1024 * 1024;
const MAX_FILES: usize = 100_000;

fn unpack(archive: &Path, dest: &Path, cancel: &AtomicBool) -> io::Result<()> {
    let mut file = File::open(archive)?;
    let mut magic = [0; 4];
    file.read_exact(&mut magic)?;
    let file = File::open(archive)?;
    if magic[..2] == *b"PK" {
        let mut zip = zip::ZipArchive::new(file).map_err(io::Error::other)?;
        if zip.len() > MAX_FILES {
            return Err(invalid("Too many archive entries"));
        }
        let mut total = 0;
        for i in 0..zip.len() {
            check_cancel(cancel)?;
            let mut member = zip.by_index(i).map_err(io::Error::other)?;
            let path = safe_path(Path::new(member.name().map_err(io::Error::other)?.as_ref()))?;
            if member.is_symlink()
                || member
                    .unix_mode()
                    .is_some_and(|m| !matches!(m & 0o170000, 0 | 0o100000 | 0o040000))
            {
                return Err(invalid("Archive links and special files are not allowed"));
            }
            let is_dir = member.is_dir();
            extract(&mut member, &dest.join(path), is_dir, &mut total, cancel)?;
        }
        return Ok(());
    }
    let reader: Box<dyn Read> = if magic[..2] == [0x1f, 0x8b] {
        Box::new(flate2::read::MultiGzDecoder::new(file))
    } else if magic == [0x28, 0xb5, 0x2f, 0xfd] {
        Box::new(zstd::stream::read::Decoder::new(file)?)
    } else {
        return Err(invalid("Unsupported dictionary archive"));
    };
    let mut tar = tar::Archive::new(reader);
    let mut total = 0;
    for (i, member) in tar.entries()?.enumerate() {
        if i >= MAX_FILES {
            return Err(invalid("Too many archive entries"));
        }
        check_cancel(cancel)?;
        let mut member = member?;
        let path = member.path()?.into_owned();
        let kind = member.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err(invalid("Archive links and special files are not allowed"));
        }
        // Archives commonly include a root directory entry named '.'.
        if kind.is_dir() && path == Path::new(".") {
            continue;
        }
        let path = safe_path(&path)?;
        extract(&mut member, &dest.join(path), kind.is_dir(), &mut total, cancel)?;
    }
    Ok(())
}

fn extract(
    reader: &mut dyn Read,
    path: &Path,
    is_dir: bool,
    total: &mut u64,
    cancel: &AtomicBool,
) -> io::Result<()> {
    if is_dir {
        return fs::create_dir_all(path);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut output = OpenOptions::new().create_new(true).write(true).open(path)?;
    let mut buffer = [0; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        *total += count as u64;
        if *total > MAX_UNPACKED {
            return Err(invalid("Dictionary archive expands beyond the size limit"));
        }
        output.write_all(&buffer[..count])?;
    }
    output.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn files() -> Vec<(&'static str, Vec<u8>)> {
        let mut index = b"test\0".to_vec();
        index.extend(0u32.to_be_bytes());
        index.extend(10u32.to_be_bytes());
        vec![
            ("fixture/test.ifo", b"StarDict's dict ifo file\nversion=2.4.2\nbookname=Fixture\nwordcount=1\nidxfilesize=13\nsametypesequence=m\n".to_vec()),
            ("fixture/test.idx", index),
            ("fixture/test.dict", b"Definition".to_vec()),
        ]
    }

    fn archive(kind: &str) -> Vec<u8> {
        if kind == "zip" {
            let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
            for (name, bytes) in files() {
                zip.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(&bytes).unwrap();
            }
            return zip.finish().unwrap().into_inner();
        }
        let mut tar = tar::Builder::new(Vec::new());
        for (name, bytes) in files() {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, name, bytes.as_slice()).unwrap();
        }
        let bytes = tar.into_inner().unwrap();
        if kind == "zstd" {
            return zstd::encode_all(bytes.as_slice(), 1).unwrap();
        }
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gzip.write_all(&bytes).unwrap();
        gzip.finish().unwrap()
    }

    fn entry(bytes: &[u8]) -> CatalogEntry {
        CatalogEntry {
            id: "fixture".into(),
            name: "Fixture".into(),
            lang_in: "eng".into(),
            lang_out: "eng".into(),
            url: "http://127.0.0.1:1/never".into(),
            size: bytes.len() as u64,
            sha256: hex(&Sha256::digest(bytes)),
            license: "CC0-1.0".into(),
        }
    }

    fn read_request(stream: &mut std::net::TcpStream) {
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            assert!(request.len() < 4096);
            let mut byte = [0];
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
    }

    fn serve(bytes: Vec<u8>, advertised_size: usize) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/fixture", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {advertised_size}\r\nConnection: close\r\n\r\n"
            )
            .unwrap();
            stream.write_all(&bytes).unwrap();
        });
        (url, handle)
    }

    fn permit(cancel: &AtomicBool) -> Permission<'_> {
        Permission {
            allowed: true,
            airplane_mode: false,
            cancel,
        }
    }

    #[test]
    fn local_http_install_verifies_and_opens_gzip_zip_and_zstd() {
        for kind in ["gzip", "zip", "zstd"] {
            let bytes = archive(kind);
            let mut entry = entry(&bytes);
            let (url, server) = serve(bytes.clone(), bytes.len());
            entry.url = url;
            let dir = tempfile::tempdir().unwrap();
            let path = install(&entry, dir.path(), permit(&AtomicBool::new(false))).unwrap();
            server.join().unwrap();
            assert_eq!(
                Library::open_dir(path).unwrap().lookup("test")[0]
                    .definition
                    .plain_text(),
                "Definition"
            );
            assert_eq!(fs::read_dir(dir.path().join(".partial")).unwrap().count(), 0);
            assert!(install(&entry, dir.path(), permit(&AtomicBool::new(false)))
                .unwrap_err()
                .to_string()
                .contains("already installed"));
        }
    }

    #[test]
    fn local_http_hash_size_and_truncation_fail_without_installing() {
        for failure in ["hash", "size", "truncated"] {
            let bytes = archive("gzip");
            let mut entry = entry(&bytes);
            let advertised = bytes.len() + usize::from(failure == "truncated");
            let (url, server) = serve(bytes, advertised);
            entry.url = url;
            if failure == "hash" {
                entry.sha256 = "0".repeat(64);
            }
            if failure == "size" {
                entry.size += 1;
            }
            let dir = tempfile::tempdir().unwrap();
            let error = install(&entry, dir.path(), permit(&AtomicBool::new(false))).unwrap_err();
            if failure != "truncated" {
                assert!(error.to_string().contains("mismatch"));
            }
            server.join().unwrap();
            assert!(!dir.path().join("fixture").exists());
            assert_eq!(fs::read_dir(dir.path().join(".partial")).unwrap().count(), 0);
        }
    }

    #[test]
    fn disabled_airplane_and_canceled_do_not_touch_disk_or_connect() {
        for (allowed, airplane_mode, canceled) in
            [(false, false, false), (true, true, false), (true, false, true)]
        {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("dict");
            let cancel = AtomicBool::new(canceled);
            assert!(install(
                &entry(&[]),
                &root,
                Permission {
                    allowed,
                    airplane_mode,
                    cancel: &cancel
                }
            )
            .is_err());
            assert!(!root.exists());
        }
    }

    #[test]
    fn unsafe_zip_paths_and_tar_links_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        zip.start_file("../escape", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"bad").unwrap();
        let zip = zip.finish().unwrap().into_inner();
        let mut tar = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Symlink);
        header.set_size(0);
        header.set_mode(0o777);
        tar.append_link(&mut header, "link", "../escape").unwrap();
        let bytes = tar.into_inner().unwrap();
        let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        gzip.write_all(&bytes).unwrap();
        for bytes in [zip, gzip.finish().unwrap()] {
            let mut entry = entry(&bytes);
            let (url, server) = serve(bytes.clone(), bytes.len());
            entry.url = url;
            assert!(install(&entry, dir.path(), permit(&AtomicBool::new(false))).is_err());
            server.join().unwrap();
            assert!(!dir.path().join("fixture").exists());
            assert_eq!(fs::read_dir(dir.path().join(".partial")).unwrap().count(), 0);
        }
        assert!(!dir.path().parent().unwrap().join("escape").exists());
    }

    #[test]
    fn staged_dictionaries_are_never_discovered() {
        let dir = tempfile::tempdir().unwrap();
        for (name, bytes) in files() {
            let path = dir.path().join(".partial/unfinished").join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, bytes).unwrap();
        }
        assert!(Library::open_dir(dir.path()).unwrap().names().is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn symlinked_staging_directory_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), dir.path().join(".partial")).unwrap();
        let error = install(
            &entry(&archive("gzip")),
            dir.path(),
            permit(&AtomicBool::new(false)),
        )
        .unwrap_err();
        assert!(error.to_string().contains("real directory"));
        assert_eq!(fs::read_dir(elsewhere.path()).unwrap().count(), 0);
    }

    #[test]
    fn cancellation_during_response_cleans_staging() {
        use std::sync::{mpsc, Arc};
        let bytes = archive("gzip");
        let mut entry = entry(&bytes);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        entry.url = format!("http://{}/fixture", listener.local_addr().unwrap());
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (ready, receive) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .unwrap();
            ready.send(()).unwrap();
            wait.recv_timeout(Duration::from_secs(10)).unwrap();
            let _ = stream.write_all(&bytes);
        });
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_owned();
        let worker = std::thread::spawn(move || install(&entry, &root, permit(&worker_cancel)));
        receive.recv_timeout(Duration::from_secs(10)).unwrap();
        cancel.store(true, Ordering::Relaxed);
        release.send(()).unwrap();
        assert_eq!(
            worker.join().unwrap().unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
        server.join().unwrap();
        assert!(!dir.path().join("fixture").exists());
        assert_eq!(fs::read_dir(dir.path().join(".partial")).unwrap().count(), 0);
    }

    #[test]
    fn pinned_catalog_has_english_spanish_unique_ids_and_hashes() {
        let mut ids = std::collections::HashSet::new();
        for entry in catalog() {
            assert!(ids.insert(&entry.id));
            assert_eq!(entry.sha256.len(), 64);
            assert!(entry.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(entry.size > 0);
            assert!(!entry.license.is_empty());
            assert!(entry.url.starts_with("https://"));
        }
        for lang in ["eng", "spa"] {
            assert!(catalog()
                .iter()
                .any(|entry| entry.lang_in == lang && entry.lang_out == lang));
        }
    }
}
