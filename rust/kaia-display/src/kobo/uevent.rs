//! Nonblocking kernel device events. The platform crate classifies USB events.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

pub struct Uevents(OwnedFd);

impl Uevents {
    pub fn open() -> io::Result<Self> {
        // SAFETY: socket has no pointer arguments and returns a new descriptor.
        let raw = unsafe {
            libc::socket(
                libc::AF_NETLINK,
                libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                libc::NETLINK_KOBJECT_UEVENT,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: raw is a new descriptor owned only by this value.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        // SAFETY: all-zero bytes are a valid initial sockaddr_nl, including its padding.
        let mut addr: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
        addr.nl_family = libc::AF_NETLINK as _;
        addr.nl_groups = 1;
        // SAFETY: addr points to a live sockaddr_nl of the supplied size.
        let rc = unsafe {
            libc::bind(
                fd.as_raw_fd(),
                (&addr as *const libc::sockaddr_nl).cast(),
                size_of::<libc::sockaddr_nl>() as _,
            )
        };
        if rc < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(fd))
    }

    pub fn drain(&self) -> io::Result<Vec<Vec<u8>>> {
        let mut events = Vec::new();
        loop {
            let mut buf = vec![0u8; 8192];
            // SAFETY: zero is a valid initial sockaddr_nl.
            let mut sender: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
            let mut len = size_of::<libc::sockaddr_nl>() as libc::socklen_t;
            // SAFETY: buf, sender and len are writable for their declared sizes. The fd is open.
            let n = unsafe {
                libc::recvfrom(
                    self.0.as_raw_fd(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                    0,
                    (&mut sender as *mut libc::sockaddr_nl).cast(),
                    &mut len,
                )
            };
            if n < 0 {
                let error = io::Error::last_os_error();
                match error.kind() {
                    io::ErrorKind::WouldBlock => return Ok(events),
                    io::ErrorKind::Interrupted => continue,
                    _ => return Err(error),
                }
            }
            if sender.nl_pid == 0 && n > 0 {
                buf.truncate(n as usize);
                events.push(buf);
            }
        }
    }
}
