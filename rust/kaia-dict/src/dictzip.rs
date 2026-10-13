//! Data files: plain `.dict` and dictzip `.dict.dz`.
//!
//! Dictzip is gzip with an `RA` extra field that lists independently compressed chunks of
//! fixed uncompressed size. A read decompresses only the chunks it touches.
//!
//! Each chunk is a raw deflate stream written with a full flush. It ends on a byte boundary
//! with an empty stored block (`00 00 ff ff`) and has no final block, so it must be inflated
//! as an unterminated stream. The gzip trailer follows the last chunk, after an empty final
//! block (`03 00`).

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use flate2::{Decompress, FlushDecompress, Status};

use crate::error::Error;

/// Random access to the uncompressed definition data.
#[derive(Debug)]
pub(crate) enum DataFile {
    Plain { file: Mutex<File>, len: u64 },
    DictZip(DictZip),
}

impl DataFile {
    pub(crate) fn open_plain(path: &Path) -> Result<Self, Error> {
        let file = File::open(path).map_err(|e| Error::from(e).at(path))?;
        let len = file.metadata().map_err(|e| Error::from(e).at(path))?.len();
        Ok(DataFile::Plain {
            file: Mutex::new(file),
            len,
        })
    }

    pub(crate) fn len(&self) -> u64 {
        match self {
            DataFile::Plain { len, .. } => *len,
            DataFile::DictZip(dz) => dz.total_len,
        }
    }

    /// Reads `size` bytes at uncompressed `offset`.
    pub(crate) fn read(&self, offset: u64, size: usize) -> Result<Vec<u8>, Error> {
        match self {
            DataFile::Plain { file, len } => {
                let end = offset.checked_add(size as u64).filter(|end| end <= len);
                if end.is_none() {
                    return Err(Error::format("definition lies past the end of the .dict file"));
                }
                let mut file = lock(file);
                file.seek(SeekFrom::Start(offset))?;
                let mut out = vec![0; size];
                file.read_exact(&mut out)?;
                Ok(out)
            }
            DataFile::DictZip(dz) => dz.read(offset, size),
        }
    }
}

/// An open dictzip file with its chunk table.
#[derive(Debug)]
pub(crate) struct DictZip {
    file: Mutex<File>,
    chunk_len: u64,
    /// Compressed file offset of each chunk, followed by the end of the last chunk.
    chunk_starts: Vec<u64>,
    total_len: u64,
    inflated: AtomicUsize,
    /// The last chunk inflated, with its number. Neighbouring records share a chunk, so
    /// this saves most of the work on a run of lookups or a scan.
    cache: Mutex<Option<(usize, Vec<u8>)>>,
}

impl DictZip {
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        let file = File::open(path).map_err(|e| Error::from(e).at(path))?;
        let file_len = file.metadata().map_err(|e| Error::from(e).at(path))?.len();
        let mut reader = BufReader::new(file);
        let (ra, data_start) = read_gzip_header(&mut reader).map_err(|e| e.at(path))?;
        let (chunk_len, sizes) = parse_ra(&ra).map_err(|e| e.at(path))?;

        let mut chunk_starts = Vec::with_capacity(sizes.len() + 1);
        let mut pos = data_start;
        for size in &sizes {
            chunk_starts.push(pos);
            pos += u64::from(*size);
        }
        chunk_starts.push(pos);
        if pos > file_len {
            return Err(Error::format("dictzip chunk table points past the end of the file").at(path));
        }

        let mut dz = DictZip {
            file: Mutex::new(reader.into_inner()),
            chunk_len,
            chunk_starts,
            total_len: 0,
            inflated: AtomicUsize::new(0),
            cache: Mutex::new(None),
        };
        dz.total_len = match sizes.len() {
            0 => 0,
            n => {
                let last = dz.inflate(n - 1, false).map_err(|e| e.at(path))?;
                (n as u64 - 1) * chunk_len + last.len() as u64
            }
        };
        dz.inflated.store(0, Ordering::Relaxed);
        Ok(dz)
    }

    /// Number of chunks decompressed since the file was opened.
    #[cfg(test)]
    pub(crate) fn chunks_inflated(&self) -> usize {
        self.inflated.load(Ordering::Relaxed)
    }

    pub(crate) fn read(&self, offset: u64, size: usize) -> Result<Vec<u8>, Error> {
        let end = offset
            .checked_add(size as u64)
            .filter(|end| *end <= self.total_len)
            .ok_or_else(|| Error::format("definition lies past the end of the dictzip data"))?;
        let mut out = Vec::with_capacity(size);
        let mut pos = offset;
        while pos < end {
            let chunk_no = (pos / self.chunk_len) as usize;
            let mut cache = lock(&self.cache);
            if !matches!(cache.as_ref(), Some((cached, _)) if *cached == chunk_no) {
                *cache = Some((chunk_no, self.inflate(chunk_no, true)?));
            }
            let chunk = &cache
                .as_ref()
                .ok_or_else(|| Error::format("chunk cache is empty"))?
                .1;
            let within = (pos - chunk_no as u64 * self.chunk_len) as usize;
            let take = (end - pos).min((chunk.len().saturating_sub(within)) as u64) as usize;
            if take == 0 {
                return Err(Error::format(
                    "dictzip chunk is shorter than its chunk table says",
                ));
            }
            out.extend_from_slice(&chunk[within..within + take]);
            pos += take as u64;
        }
        Ok(out)
    }

    /// Decompresses one chunk. Non-final chunks must inflate to exactly `chunk_len` bytes.
    fn inflate(&self, chunk_no: usize, count: bool) -> Result<Vec<u8>, Error> {
        let start = self.chunk_starts[chunk_no];
        let end = self.chunk_starts[chunk_no + 1];
        let mut compressed = vec![0; (end - start) as usize];
        {
            let mut file = lock(&self.file);
            file.seek(SeekFrom::Start(start))?;
            file.read_exact(&mut compressed)?;
        }
        let max_len = self.chunk_len as usize;
        let out = inflate_chunk(&compressed, max_len).ok_or_else(|| {
            Error::format(format!(
                "dictzip chunk {chunk_no} is not valid deflate data or inflates to too much data"
            ))
        })?;
        if count {
            self.inflated.fetch_add(1, Ordering::Relaxed);
        }
        let is_last = chunk_no + 2 == self.chunk_starts.len();
        if !is_last && out.len() != max_len {
            return Err(Error::format(format!(
                "dictzip chunk {chunk_no} has the wrong size"
            )));
        }
        Ok(out)
    }
}

/// Inflates one raw deflate chunk that may end without a final block.
///
/// Returns `None` when the data is not valid deflate, makes no progress, or inflates to more
/// than `max_len` bytes.
fn inflate_chunk(compressed: &[u8], max_len: usize) -> Option<Vec<u8>> {
    let mut decoder = Decompress::new(false);
    // One spare byte lets an oversized chunk show up as too long instead of being cut short.
    let mut out = Vec::with_capacity(max_len + 1);
    loop {
        let consumed = decoder.total_in() as usize;
        let before = (consumed, out.len());
        let status = decoder
            .decompress_vec(&compressed[consumed..], &mut out, FlushDecompress::Sync)
            .ok()?;
        if out.len() > max_len {
            return None;
        }
        if status == Status::StreamEnd || decoder.total_in() as usize == compressed.len() {
            return Some(out);
        }
        if (decoder.total_in() as usize, out.len()) == before {
            return None;
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A poisoned lock only means another reader panicked. The file position is reset
    // by every read, so the guard is safe to reuse.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Reads the gzip header. Returns the `RA` extra field and the offset where data starts.
fn read_gzip_header(reader: &mut BufReader<File>) -> Result<(Vec<u8>, u64), Error> {
    let mut fixed = [0u8; 10];
    reader
        .read_exact(&mut fixed)
        .map_err(|_| Error::format("file is too short for a gzip header"))?;
    if fixed[0] != 0x1f || fixed[1] != 0x8b || fixed[2] != 8 {
        return Err(Error::format("file is not gzip data"));
    }
    let flags = fixed[3];
    const FHCRC: u8 = 0x02;
    const FEXTRA: u8 = 0x04;
    const FNAME: u8 = 0x08;
    const FCOMMENT: u8 = 0x10;
    if flags & FEXTRA == 0 {
        return Err(Error::format(
            "gzip file has no RA extra field, so it is not dictzip",
        ));
    }
    let mut xlen = [0u8; 2];
    reader
        .read_exact(&mut xlen)
        .map_err(|_| Error::format("gzip header is truncated"))?;
    let mut extra = vec![0; u16::from_le_bytes(xlen) as usize];
    reader
        .read_exact(&mut extra)
        .map_err(|_| Error::format("gzip header is truncated"))?;
    if flags & FNAME != 0 {
        skip_zero_terminated(reader)?;
    }
    if flags & FCOMMENT != 0 {
        skip_zero_terminated(reader)?;
    }
    if flags & FHCRC != 0 {
        let mut crc = [0u8; 2];
        reader
            .read_exact(&mut crc)
            .map_err(|_| Error::format("gzip header is truncated"))?;
    }
    let data_start = reader.stream_position().map_err(Error::from)?;

    let mut pos = 0;
    while pos + 4 <= extra.len() {
        let sub_len = u16::from_le_bytes([extra[pos + 2], extra[pos + 3]]) as usize;
        let body_end = pos + 4 + sub_len;
        if body_end > extra.len() {
            return Err(Error::format("gzip extra field is truncated"));
        }
        if extra[pos] == b'R' && extra[pos + 1] == b'A' {
            return Ok((extra[pos + 4..body_end].to_vec(), data_start));
        }
        pos = body_end;
    }
    Err(Error::format(
        "gzip file has no RA extra field, so it is not dictzip",
    ))
}

fn skip_zero_terminated(reader: &mut BufReader<File>) -> Result<(), Error> {
    loop {
        let mut byte = [0u8; 1];
        reader
            .read_exact(&mut byte)
            .map_err(|_| Error::format("gzip header is truncated"))?;
        if byte[0] == 0 {
            return Ok(());
        }
    }
}

/// Parses the RA field: version 1, chunk length, chunk count, then one size per chunk.
fn parse_ra(ra: &[u8]) -> Result<(u64, Vec<u16>), Error> {
    let u16_at =
        |pos: usize| -> Option<u16> { ra.get(pos..pos + 2).map(|b| u16::from_le_bytes([b[0], b[1]])) };
    let version = u16_at(0).ok_or_else(|| Error::format("RA field is truncated"))?;
    if version != 1 {
        return Err(Error::format(format!("unsupported dictzip version {version}")));
    }
    let chunk_len = u16_at(2).ok_or_else(|| Error::format("RA field is truncated"))?;
    let count = u16_at(4).ok_or_else(|| Error::format("RA field is truncated"))? as usize;
    if chunk_len == 0 {
        return Err(Error::format("dictzip chunk length is zero"));
    }
    let sizes = (0..count)
        .map(|i| u16_at(6 + 2 * i).ok_or_else(|| Error::format("RA chunk table is truncated")))
        .collect::<Result<Vec<u16>, Error>>()?;
    Ok((u64::from(chunk_len), sizes))
}
