//! KOReader's partial MD5, the same digest as `util.partialMD5` in
//! `frontend/util.lua` and as `kaia_library::partial_md5`. The digest is the
//! key of the Kaia state file and of KOReader's `partial_md5_checksum`.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use md5::{Digest, Md5};

/// Bytes read at each sample offset.
const SAMPLE_SIZE: usize = 1024;

/// Sample offsets. KOReader computes `bit.lshift(1024, 2*i)` for i from -1 to
/// 10. LuaJIT's bit operations take the shift count modulo 32, so i = -1 wraps
/// to offset 0. The first sample is therefore at offset 0, not 256.
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
/// 12 KiB. Samples past the end of the file are skipped.
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
