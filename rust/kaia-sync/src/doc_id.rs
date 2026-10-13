//! Document ids and the hash helpers that the protocol needs.

use std::fmt::Write;
use std::path::Path;

use md5::{Digest, Md5};

pub use kaia_id::partial_md5;

/// The file name digest the plugin uses with "Filename" matching: MD5 of the
/// file name without its directory (`KOSync:getFileNameDigest`). `None` when
/// the path has no file name.
pub fn filename_md5(path: &Path) -> Option<String> {
    let name = path.file_name()?;
    Some(md5_hex(name.as_encoded_bytes()))
}

/// MD5 of `data` as 32 lowercase hex characters.
pub fn md5_hex(data: &[u8]) -> String {
    let digest = Md5::digest(data);
    let mut out = String::with_capacity(32);
    for byte in digest.iter() {
        // Writing to a String cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Rounds a fraction down to four decimals, as `Math.roundPercent` in
/// `frontend/optmath.lua` does.
pub fn round_percent(percent: f64) -> f64 {
    (percent * 10_000.0).floor() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn md5_hex_matches_known_vectors() {
        assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn filename_md5_hashes_only_the_base_name() {
        let with_dir = filename_md5(Path::new("/books/sub/novel.epub")).unwrap();
        let bare = filename_md5(Path::new("novel.epub")).unwrap();
        assert_eq!(with_dir, bare);
        assert_eq!(bare, md5_hex(b"novel.epub"));
    }

    #[test]
    fn filename_md5_is_none_without_a_file_name() {
        assert_eq!(filename_md5(Path::new("/")), None);
    }

    #[test]
    fn round_percent_floors_to_four_decimals() {
        assert_eq!(round_percent(0.12349), 0.1234);
        assert_eq!(round_percent(0.5), 0.5);
        assert_eq!(round_percent(1.0), 1.0);
        assert_eq!(round_percent(0.0), 0.0);
    }

    #[test]
    fn partial_md5_is_the_reader_digest() {
        // Files shorter than one sample hash only the bytes they have, so the
        // result equals the MD5 of the whole content.
        let dir = std::env::temp_dir().join(format!("kaia-sync-md5-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("short.txt");
        fs::write(&file, b"abc").unwrap();
        assert_eq!(partial_md5(&file).unwrap(), md5_hex(b"abc"));
        fs::remove_dir_all(&dir).unwrap();
    }
}
