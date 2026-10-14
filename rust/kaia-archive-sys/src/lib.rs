//! A read-only RAR/RAR5 reader. It never extracts paths to disk.
//! All entries, including skipped files, pass the size and path checks.

use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{self, Error, ErrorKind};
use std::path::Path;
use std::ptr::NonNull;

pub const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 10_000;
pub const MAX_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const MAX_PATH_BYTES: usize = 4096;

// Signatures match KOReader's staged archive.h and archive_entry.h. Its patch
// makes archive_entry_filetype int on both macOS and Kobo.
unsafe extern "C" {
    fn archive_read_new() -> *mut c_void;
    fn archive_read_support_format_rar(a: *mut c_void) -> c_int;
    fn archive_read_support_format_rar5(a: *mut c_void) -> c_int;
    fn archive_read_open_filename(a: *mut c_void, path: *const c_char, block: usize) -> c_int;
    fn archive_read_next_header(a: *mut c_void, entry: *mut *mut c_void) -> c_int;
    fn archive_read_data(a: *mut c_void, buffer: *mut c_void, size: usize) -> isize;
    fn archive_read_free(a: *mut c_void) -> c_int;
    fn archive_error_string(a: *mut c_void) -> *const c_char;
    fn archive_entry_pathname_utf8(entry: *mut c_void) -> *const c_char;
    fn archive_entry_size(entry: *mut c_void) -> i64;
    fn archive_entry_size_is_set(entry: *mut c_void) -> c_int;
    fn archive_entry_filetype(entry: *mut c_void) -> c_int;
    fn archive_entry_is_encrypted(entry: *mut c_void) -> c_int;
    fn archive_entry_hardlink(entry: *mut c_void) -> *const c_char;
    fn archive_entry_symlink(entry: *mut c_void) -> *const c_char;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub is_file: bool,
}

/// Owns one libarchive reader. The handle is never shared with another thread.
pub struct Reader {
    raw: NonNull<c_void>,
    remaining: Option<u64>,
    count: usize,
    total: u64,
    failed: bool,
}

fn invalid(message: &str) -> Error {
    Error::new(ErrorKind::InvalidData, message)
}

impl Reader {
    pub fn open(path: &Path) -> io::Result<Self> {
        let path = CString::new(path.as_os_str().as_encoded_bytes())
            .map_err(|_| invalid("archive filename contains NUL"))?;
        // SAFETY: archive_read_new takes no arguments and returns an owned handle.
        let raw = NonNull::new(unsafe { archive_read_new() })
            .ok_or_else(|| Error::other("cannot allocate libarchive reader"))?;
        let reader = Self {
            raw,
            remaining: None,
            count: 0,
            total: 0,
            failed: false,
        };
        // SAFETY: raw is a live, uniquely owned reader; path is NUL-terminated and
        // stays alive until open_filename returns. Libarchive opens the file now.
        unsafe {
            reader.check(archive_read_support_format_rar(raw.as_ptr()))?;
            reader.check(archive_read_support_format_rar5(raw.as_ptr()))?;
            reader.check(archive_read_open_filename(raw.as_ptr(), path.as_ptr(), 64 * 1024))?;
        }
        Ok(reader)
    }

    fn error(&self) -> Error {
        // SAFETY: the reader is live; the error string belongs to it and is copied
        // before another libarchive call can invalidate it. NULL means no message.
        unsafe {
            let message = archive_error_string(self.raw.as_ptr());
            if message.is_null() {
                invalid("libarchive rejected the archive")
            } else {
                Error::new(
                    ErrorKind::InvalidData,
                    CStr::from_ptr(message).to_string_lossy().into_owned(),
                )
            }
        }
    }

    fn check(&self, status: c_int) -> io::Result<()> {
        if status == 0 {
            Ok(())
        } else {
            Err(self.error())
        }
    }

    /// Drains the previous entry before advancing, so skipped data also has a
    /// bounded expansion and any data errors reach the caller. Warnings are errors.
    pub fn next_entry(&mut self) -> io::Result<Option<Entry>> {
        self.ensure_usable()?;
        let result = self.next_inner();
        self.failed = result.is_err();
        result
    }

    fn next_inner(&mut self) -> io::Result<Option<Entry>> {
        let mut scratch = [0; 64 * 1024];
        while self.read_chunk(&mut scratch)? != 0 {}
        let mut entry = std::ptr::null_mut();
        // SAFETY: raw is live and entry points to writable pointer storage. The
        // returned entry remains owned by raw until the next header or free call.
        let status = unsafe { archive_read_next_header(self.raw.as_ptr(), &mut entry) };
        if status == 1 {
            return Ok(None);
        }
        self.check(status)?;
        if entry.is_null() {
            return Err(invalid("missing archive entry"));
        }
        self.count += 1;
        if self.count > MAX_ENTRIES {
            return Err(invalid("archive entry count exceeds 10000"));
        }
        // SAFETY: entry is non-null and belongs to the live reader. All borrowed
        // strings are inspected/copied here before advancing or freeing the reader.
        let info = unsafe {
            let name = archive_entry_pathname_utf8(entry);
            if name.is_null() {
                return Err(invalid("archive path is not UTF-8"));
            }
            let name = CStr::from_ptr(name)
                .to_str()
                .map_err(|_| invalid("archive path is not UTF-8"))?;
            validate_path(name)?;
            if archive_entry_is_encrypted(entry) != 0
                || !archive_entry_hardlink(entry).is_null()
                || !archive_entry_symlink(entry).is_null()
            {
                return Err(invalid("encrypted entries and links are not supported"));
            }
            let kind = archive_entry_filetype(entry);
            if kind != 0o100000 && kind != 0o040000 {
                return Err(invalid("archive entry is not a regular file or directory"));
            }
            let size = archive_entry_size(entry);
            if archive_entry_size_is_set(entry) == 0 || size < 0 || size as u64 > MAX_ENTRY_BYTES {
                return Err(invalid("archive entry exceeds 64 MiB or has an unknown size"));
            }
            Entry {
                name: name.to_owned(),
                size: size as u64,
                is_file: kind == 0o100000,
            }
        };
        self.total += info.size;
        if self.total > MAX_TOTAL_BYTES {
            return Err(invalid("archive expansion exceeds 4 GiB"));
        }
        self.remaining = Some(info.size);
        Ok(Some(info))
    }

    fn ensure_usable(&self) -> io::Result<()> {
        if self.failed {
            Err(invalid("archive reader failed"))
        } else {
            Ok(())
        }
    }

    fn read_chunk(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let Some(remaining) = self.remaining else {
            return Ok(0);
        };
        // SAFETY: raw is live with a current entry. buffer is writable for its
        // full length and cannot alias libarchive's internal data.
        let count = unsafe { archive_read_data(self.raw.as_ptr(), buffer.as_mut_ptr().cast(), buffer.len()) };
        if count < 0 {
            return Err(self.error());
        }
        let count = count as usize;
        if count > buffer.len() || count as u64 > remaining {
            return Err(invalid("archive data exceeds the declared entry size"));
        }
        if count == 0 {
            if remaining != 0 {
                return Err(invalid("truncated archive entry"));
            }
            self.remaining = None;
        } else {
            self.remaining = Some(remaining - count as u64);
        }
        Ok(count)
    }

    /// Reads the current entry to its end and propagates libarchive data errors.
    /// Libarchive 3.8.8 does not check stored RAR4 payload CRCs at EOF.
    pub fn read_entry(&mut self) -> io::Result<Vec<u8>> {
        self.ensure_usable()?;
        let result = (|| {
            let mut data = Vec::new();
            let mut scratch = [0; 64 * 1024];
            loop {
                let count = self.read_chunk(&mut scratch)?;
                if count == 0 {
                    return Ok(data);
                }
                data.try_reserve(count)
                    .map_err(|_| Error::other("cannot allocate archive entry"))?;
                data.extend_from_slice(&scratch[..count]);
            }
        })();
        self.failed = result.is_err();
        result
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // SAFETY: raw is owned solely by this Reader and is freed exactly once.
        unsafe {
            archive_read_free(self.raw.as_ptr());
        }
    }
}

fn validate_path(name: &str) -> io::Result<()> {
    if name.is_empty()
        || name.len() > MAX_PATH_BYTES
        || name.starts_with(['/', '\\'])
        || name.contains(':')
        || name.split(['/', '\\']).any(|part| part == "..")
    {
        return Err(invalid("unsafe archive path"));
    }
    Ok(())
}
