//! Compatibility with the glibc 2.15 sysroot of the Kobo toolchain.
//!
//! Rust std calls `getauxval`, which glibc added in 2.16. The Kobo build
//! links against glibc 2.15, so libkaia defines it here by reading
//! `/proc/self/auxv`. When the device's libc is newer and has its own
//! `getauxval`, the dynamic linker finds libc's copy first, because libc is
//! loaded before libkaia. Either answer is correct.

use libc::{c_ulong, c_void};

/// Returns the auxiliary vector entry `kind`, or 0 with errno set to ENOENT.
///
/// # Safety
/// Called by C code and by Rust std. It only uses open, read and close.
#[no_mangle]
pub unsafe extern "C" fn getauxval(kind: c_ulong) -> c_ulong {
    let fd = libc::open(c"/proc/self/auxv".as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC);
    if fd < 0 {
        *libc::__errno_location() = libc::ENOENT;
        return 0;
    }
    let mut pair: [c_ulong; 2] = [0; 2];
    let size = core::mem::size_of_val(&pair);
    let mut found = None;
    loop {
        let n = libc::read(fd, pair.as_mut_ptr().cast::<c_void>(), size);
        if n < 0 || n as usize != size || pair[0] == 0 {
            break;
        }
        if pair[0] == kind {
            found = Some(pair[1]);
            break;
        }
    }
    libc::close(fd);
    match found {
        Some(value) => value,
        None => {
            *libc::__errno_location() = libc::ENOENT;
            0
        }
    }
}
