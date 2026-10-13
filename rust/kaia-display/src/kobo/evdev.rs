//! Reads the Kobo touch and key event nodes and hands the raw events to
//! [`EvdevDecoder`]. Linux only.
//!
//! The device nodes are the defaults from KOReader's `frontend/device/kobo/device.lua`:
//! `/dev/input/event0` for keys and switches, and `/dev/input/event1` for the touch
//! panel. KOReader finds them with FBInk's input scan, which this backend does not
//! use. Nothing here has run on a device. See the list in [`crate::kobo`].

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use crate::input::{InputEvent, InputSource};
use crate::kobo::decode::{code, Calibration, DecodedEvent, EvdevDecoder, RawEvent};
use crate::rotation::Rotation;

/// Keys and switches on the Kobo (KOReader's `ntx_dev`).
pub const KEY_NODE: &str = "/dev/input/event0";
/// The touch panel (KOReader's `touch_dev`).
pub const TOUCH_NODE: &str = "/dev/input/event1";

/// The Libra Colour ranges, used when `EVIOCGABS` gives no usable range. They are
/// assumptions, not measured values.
const FALLBACK_X_RANGE: (i32, i32) = (0, 1679);
const FALLBACK_Y_RANGE: (i32, i32) = (0, 1263);

/// `struct input_absinfo` has six `__s32` fields: value, minimum, maximum, fuzz, flat
/// and resolution.
const ABSINFO_BYTES: u32 = 24;
const IOC_READ: u32 = 2;

/// `EVIOCGABS(axis)`, which is `_IOR('E', 0x40 + axis, struct input_absinfo)`.
const fn eviocgabs(axis: u16) -> u32 {
    (IOC_READ << 30) | (ABSINFO_BYTES << 16) | ((b'E' as u32) << 8) | (0x40 + axis as u32)
}

/// Bytes in one `struct input_event` on this target. The time is a `timeval`, whose
/// fields have the width of `time_t`. Then come the `u16` type, the `u16` code and the
/// `i32` value. The kernel uses the same layout as the C library on each target.
const EVENT_SIZE: usize = 2 * size_of::<libc::time_t>() + 8;

const _: () = assert!(size_of::<libc::timeval>() == 2 * size_of::<libc::time_t>());
#[cfg(target_pointer_width = "64")]
const _: () = assert!(EVENT_SIZE == 24);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(EVENT_SIZE == 16);

/// Touch and key input from the Kobo's evdev nodes.
///
/// Raw events from both nodes go through one [`EvdevDecoder`], in time order. The
/// touch node sends the multitouch frames and the key node sends the buttons.
pub struct EvdevInput {
    touch: Option<Node>,
    keys: Option<Node>,
    decoder: EvdevDecoder,
}

struct Node {
    file: File,
    /// Bytes read but not yet forming a whole event.
    rest: Vec<u8>,
}

impl EvdevInput {
    /// Opens the touch node, and the key node when `keys` is given. The touch ranges
    /// are read with `EVIOCGABS`. Touch positions come out at `rotation`.
    pub fn open(touch: &Path, keys: Option<&Path>, rotation: Rotation) -> io::Result<Self> {
        let touch_file = open_node(touch)?;
        let x_range =
            axis_range(&touch_file, code::ABS_MT_POSITION_X, code::ABS_X).unwrap_or(FALLBACK_X_RANGE);
        let y_range =
            axis_range(&touch_file, code::ABS_MT_POSITION_Y, code::ABS_Y).unwrap_or(FALLBACK_Y_RANGE);
        let calibration = Calibration::libra_colour(x_range, y_range, rotation);
        let keys = keys.map(open_node).transpose()?;
        Ok(Self {
            touch: Some(Node {
                file: touch_file,
                rest: Vec::new(),
            }),
            keys: keys.map(|file| Node {
                file,
                rest: Vec::new(),
            }),
            decoder: EvdevDecoder::new(calibration),
        })
    }

    /// Opens the Libra Colour's default nodes ([`TOUCH_NODE`] and [`KEY_NODE`]).
    pub fn libra_colour(rotation: Rotation) -> io::Result<Self> {
        Self::open(Path::new(TOUCH_NODE), Some(Path::new(KEY_NODE)), rotation)
    }

    /// Changes the rotation that later touch positions use. Call it together with
    /// `Display::set_rotation`.
    pub fn set_rotation(&mut self, rotation: Rotation) {
        self.decoder.set_rotation(rotation);
    }

    fn is_open(&self) -> bool {
        self.touch.is_some() || self.keys.is_some()
    }

    /// Waits up to `timeout_ms` (-1 for no limit) for readable nodes, reads them, and
    /// returns the decoded raw events.
    fn read_ready(&mut self, timeout_ms: i32) -> Vec<RawEvent> {
        let mut fds: Vec<libc::pollfd> = Vec::with_capacity(2);
        let mut which: Vec<bool> = Vec::with_capacity(2); // true for the touch node
        for (is_touch, node) in [(true, &self.touch), (false, &self.keys)] {
            if let Some(node) = node {
                fds.push(libc::pollfd {
                    fd: node.file.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                });
                which.push(is_touch);
            }
        }
        if fds.is_empty() {
            return Vec::new();
        }

        // SAFETY: `fds` is a valid array of `fds.len()` pollfd structs for the duration
        // of the call. Each fd belongs to a `File` in `self`, which stays open here.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout_ms) };
        if ready <= 0 {
            // Timeout, or EINTR. The caller loops.
            return Vec::new();
        }

        let mut raw = Vec::new();
        for (fd, is_touch) in fds.iter().zip(which) {
            let slot = if is_touch { &mut self.touch } else { &mut self.keys };
            let Some(node) = slot.as_mut() else {
                continue;
            };
            let hung_up = fd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0;
            let alive = drain(node, &mut raw);
            if !alive || (hung_up && fd.revents & libc::POLLIN == 0) {
                // The node is gone. Stop polling it, so that the loop does not spin.
                *slot = None;
            }
        }
        // Keep the two nodes in time order. Each node is already in order.
        raw.sort_by_key(|event| event.time_us);
        raw
    }
}

impl InputSource for EvdevInput {
    /// Returns the input events that arrived. With `timeout` None, waits until an
    /// event arrives. When both nodes are gone, returns no events at once.
    fn poll(&mut self, timeout: Option<Duration>) -> Vec<InputEvent> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            if !self.is_open() {
                if let Some(deadline) = deadline {
                    thread::sleep(deadline.saturating_duration_since(Instant::now()));
                }
                return Vec::new();
            }
            let timeout_ms = match deadline {
                None => -1,
                Some(deadline) => deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(i32::MAX as u128) as i32,
            };
            let raw = self.read_ready(timeout_ms);
            // One frame per SYN_REPORT, so two fingers that move together reach
            // the gesture detector together, as in KOReader.
            let events: Vec<InputEvent> = self
                .decoder
                .feed_frames(&raw)
                .into_iter()
                .map(|decoded| match decoded {
                    DecodedEvent::Key(k) => InputEvent::Key(k),
                    DecodedEvent::TouchFrame(frame) => InputEvent::TouchFrame(frame),
                })
                .collect();
            if !events.is_empty() {
                return events;
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Vec::new();
            }
        }
    }
}

fn open_node(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

/// The `[minimum, maximum]` range of an axis, from the first axis that has one.
fn axis_range(file: &File, primary: u16, fallback: u16) -> Option<(i32, i32)> {
    [primary, fallback].into_iter().find_map(|axis| {
        let (min, max) = abs_info(file, axis)?;
        (max > min).then_some((min, max))
    })
}

/// Reads `minimum` and `maximum` for one axis with `EVIOCGABS`.
fn abs_info(file: &File, axis: u16) -> Option<(i32, i32)> {
    // Layout of `struct input_absinfo`: value, minimum, maximum, fuzz, flat, resolution.
    let mut info = [0i32; 6];
    // SAFETY: `file` is open for the duration of the call. `info` is 24 bytes, the size
    // of `struct input_absinfo`, and EVIOCGABS writes at most that many bytes.
    let rc = unsafe {
        libc::ioctl(
            file.as_raw_fd(),
            eviocgabs(axis) as _,
            info.as_mut_ptr().cast::<libc::c_void>(),
        )
    };
    if rc < 0 {
        return None;
    }
    Some((info[1], info[2]))
}

/// Reads every whole event that the node has ready. Returns false when the node
/// reached its end or failed, so the caller drops it.
fn drain(node: &mut Node, out: &mut Vec<RawEvent>) -> bool {
    let mut buf = [0u8; 64 * EVENT_SIZE];
    loop {
        match node.file.read(&mut buf) {
            Ok(0) => return false,
            Ok(n) => node.rest.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return false,
        }
    }
    let whole = node.rest.len() / EVENT_SIZE * EVENT_SIZE;
    let (events, _) = node.rest[..whole].as_chunks::<EVENT_SIZE>();
    out.extend(events.iter().map(|event| parse(event)));
    node.rest.drain(..whole);
    true
}

/// Decodes one `input_event` from exactly [`EVENT_SIZE`] bytes in native byte order.
fn parse(chunk: &[u8]) -> RawEvent {
    let t = size_of::<libc::time_t>();
    let sec = read_time(&chunk[..t]);
    let usec = read_time(&chunk[t..2 * t]);
    let at = 2 * t;
    RawEvent {
        time_us: (sec.max(0) as u64) * 1_000_000 + usec.max(0) as u64,
        kind: u16::from_ne_bytes([chunk[at], chunk[at + 1]]),
        code: u16::from_ne_bytes([chunk[at + 2], chunk[at + 3]]),
        value: i32::from_ne_bytes([chunk[at + 4], chunk[at + 5], chunk[at + 6], chunk[at + 7]]),
    }
}

/// Reads a `time_t`-sized field. The width is the target's `time_t`.
fn read_time(bytes: &[u8]) -> i64 {
    if bytes.len() == 8 {
        i64::from_ne_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ])
    } else {
        i32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eviocgabs_matches_the_kernel_request_numbers() {
        // `_IOR('E', 0x40 + 0x35, struct input_absinfo)` on Linux, where the size is 24.
        assert_eq!(eviocgabs(0x35), 0x8018_4575);
        assert_eq!(eviocgabs(0x36), 0x8018_4576);
        assert_eq!(eviocgabs(0), 0x8018_4540);
    }

    #[test]
    fn parses_one_event_in_native_layout() {
        let t = size_of::<libc::time_t>();
        let mut bytes = vec![0u8; EVENT_SIZE];
        let time_bytes = |v: i64| -> Vec<u8> {
            if t == 8 {
                v.to_ne_bytes().to_vec()
            } else {
                (v as i32).to_ne_bytes().to_vec()
            }
        };
        bytes[..t].copy_from_slice(&time_bytes(2));
        bytes[t..2 * t].copy_from_slice(&time_bytes(500_000));
        let at = 2 * t;
        bytes[at..at + 2].copy_from_slice(&code::EV_ABS.to_ne_bytes());
        bytes[at + 2..at + 4].copy_from_slice(&code::ABS_MT_POSITION_X.to_ne_bytes());
        bytes[at + 4..at + 8].copy_from_slice(&(-7i32).to_ne_bytes());

        assert_eq!(
            parse(&bytes),
            RawEvent {
                time_us: 2_500_000,
                kind: code::EV_ABS,
                code: code::ABS_MT_POSITION_X,
                value: -7,
            }
        );
    }
}
