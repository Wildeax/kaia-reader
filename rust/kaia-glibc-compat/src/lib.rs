//! Fallbacks for the glibc 2.15 sysroot of the Kobo firmware 4.x toolchain.
//!
//! Rust std calls a few glibc functions that 2.15 lacks. Rust binaries that
//! link against that sysroot fail at link time without them. This crate
//! defines them. It is compiled only for armv7 `gnu` with the `glibc215`
//! feature, so every other target gets an empty library.
//!
//! The build script `tools/kaia/build-rust-kobo.sh` builds this crate as a
//! staticlib and passes it to the link step of the `kobo` target. The linker
//! then uses a fallback only when nothing else defines the symbol. The
//! same fallbacks exist in `kaia-ffi` (`src/glibc215.rs`), which is a cdylib.
//!
//! Symbols provided:
//! - `getauxval` (glibc 2.16): reads `/proc/self/auxv`.

#![no_std]

#[cfg(all(
    feature = "glibc215",
    target_arch = "arm",
    target_os = "linux",
    target_env = "gnu"
))]
mod glibc215 {
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
}
