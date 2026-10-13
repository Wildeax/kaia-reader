//! KOReader's partial MD5, the same digest as `util.partialMD5` in
//! `frontend/util.lua`. The reader stores it in the sidecar as
//! `partial_md5_checksum`, so the library and the reader agree on identity.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use md5::{Digest, Md5};

/// Bytes read at each sample offset.
const SAMPLE_SIZE: usize = 1024;

/// Sample offsets. KOReader computes `bit.lshift(1024, 2*i)` for i from -1 to
/// 10. LuaJIT's bit operations work on 32-bit values and take the shift count
/// modulo 32, so i = -1 shifts by 30 and wraps to 0. The first sample is
/// therefore at offset 0, not 256. `luajit` gives the same list.
const SAMPLE_OFFSETS: [u64; 12] = [
    0,
    1 << 10,
    1 << 12,
    1 << 14,
    1 << 16,
    1 << 18,
    1 << 20,
    1 << 22,
    1 << 24,
    1 << 26,
    1 << 28,
    1 << 30,
];

/// The partial MD5 of a file as 32 lowercase hex characters. Reads at most
/// 12 KiB. A file shorter than a sample contributes the bytes it has. Samples
/// past the end are skipped, as in KOReader.
pub fn partial_md5(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Md5::new();
    let mut buf = [0u8; SAMPLE_SIZE];
    for &offset in &SAMPLE_OFFSETS {
        file.seek(SeekFrom::Start(offset))?;
        let n = read_up_to(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

/// Fills `buf` unless the file ends first, like Lua's `file:read(n)`.
fn read_up_to(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        let r = file.read(&mut buf[n..])?;
        if r == 0 {
            break;
        }
        n += r;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn empty_file_hashes_to_md5_of_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty");
        File::create(&path).unwrap();
        assert_eq!(partial_md5(&path).unwrap(), "d41d8cd98f00b204e9800998ecf8427e");
    }

    #[test]
    fn small_file_hashes_its_whole_content() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small");
        File::create(&path).unwrap().write_all(b"abc").unwrap();
        // Only the sample at offset 0 is read, and it holds "abc".
        assert_eq!(partial_md5(&path).unwrap(), "900150983cd24fb0d6963f7d28e17f72");
    }

    #[test]
    fn samples_follow_koreader_offsets() {
        // Build a file whose bytes equal their own index mod 251. Compare against
        // a hash of the sample bytes read at the offsets KOReader uses.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big");
        let data: Vec<u8> = (0..(2 << 20)).map(|i: usize| (i % 251) as u8).collect();
        File::create(&path).unwrap().write_all(&data).unwrap();

        let mut expected = Md5::new();
        for &off in &SAMPLE_OFFSETS {
            let off = off as usize;
            if off < data.len() {
                let end = (off + SAMPLE_SIZE).min(data.len());
                expected.update(&data[off..end]);
            }
        }
        let want: String = expected.finalize().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(partial_md5(&path).unwrap(), want);
    }

    #[test]
    fn missing_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(partial_md5(&dir.path().join("nope")).is_err());
    }
}
