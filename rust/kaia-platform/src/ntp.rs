//! Network time: an SNTP client and the device steps that set the Kobo clock.
//!
//! KOReader sets the clock on a Kobo with BusyBox `ntpd` and copies it to the
//! RTC. `plugins/timesync.koplugin/main.lua:21` runs `ntpd -q -n -p
//! pool.ntp.org`, which sets the clock and quits. `main.lua:65` then runs
//! `hwclock -u -w`. [`device_steps`] runs the same two commands as a [`Plan`],
//! so they run on the script worker in order with the Wi-Fi scripts. Kaia does
//! not write the clock itself.
//!
//! The desktop never changes its clock. [`query`] asks a server for the time,
//! and [`Sample::offset_seconds`] says how far the local clock is from it. The
//! client follows RFC 4330 (SNTP version 4): one 48-byte request and one
//! 48-byte reply over UDP.

use std::fmt;
use std::io;
use std::net::ToSocketAddrs;
use std::net::UdpSocket;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::plan::{OnError, Plan, Step};

/// The pool that KOReader and Kaia use.
pub const POOL_HOST: &str = "pool.ntp.org";
/// The pool with its NTP port, as [`query`] takes it.
pub const POOL_ADDRESS: &str = "pool.ntp.org:123";
/// How long one query waits for a reply.
pub const TIMEOUT: Duration = Duration::from_secs(5);
/// The longest time between two sync attempts while Wi-Fi is connected.
pub const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Length of an SNTP packet.
const PACKET_LEN: usize = 48;
/// Seconds from 1900-01-01 (the NTP epoch) to 1970-01-01 (the Unix epoch).
const UNIX_TO_NTP_SECONDS: u64 = 2_208_988_800;
/// Scale of the 32-bit fraction of an NTP timestamp.
const FRACTION_SCALE: f64 = 4_294_967_296.0;

/// A 64-bit NTP timestamp: seconds since 1900 in the high 32 bits, a binary fraction in the low 32.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp(u64);

impl Timestamp {
    /// The NTP time of `now`. None for a time before 1970.
    pub fn from_system_time(now: SystemTime) -> Option<Timestamp> {
        let since = now.duration_since(UNIX_EPOCH).ok()?;
        Some(Self::from_unix(since))
    }

    fn from_unix(since: Duration) -> Timestamp {
        let seconds = since.as_secs() + UNIX_TO_NTP_SECONDS;
        let fraction = (u64::from(since.subsec_nanos()) << 32) / 1_000_000_000;
        Timestamp((seconds << 32) | fraction)
    }
}

/// The result of one good query.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sample {
    /// Server time minus local time, in seconds. Positive means the local clock is behind.
    pub offset_seconds: f64,
    /// Round trip time minus the server's processing time, in seconds.
    pub delay_seconds: f64,
    /// Stratum of the server. One is a primary reference.
    pub stratum: u8,
}

/// Why a query gave no usable time.
#[derive(Debug)]
pub enum NtpError {
    /// The server name did not resolve.
    Resolve(io::Error),
    /// The name resolved to no address.
    NoAddress,
    /// The socket failed.
    Io(io::Error),
    /// No reply came before the timeout.
    Timeout,
    /// The local clock is before 1970, so it has no NTP time to send.
    BadLocalClock,
    /// The reply is shorter than an SNTP packet.
    ShortReply(usize),
    /// The reply mode is not server (4). A client or broadcast packet is refused.
    BadMode(u8),
    /// Stratum 0 is a kiss-of-death packet, or no reference at all.
    KissOfDeath,
    /// The server says its clock is not synchronized: leap indicator 3 or stratum 16 and above.
    Unsynchronized,
    /// The transmit time is zero, so the reply carries no time.
    ZeroTransmit,
    /// The reply does not echo the transmit time of the request. It may be a stale or forged reply.
    OriginMismatch,
}

impl fmt::Display for NtpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NtpError::Resolve(e) => write!(f, "cannot resolve the server: {e}"),
            NtpError::NoAddress => write!(f, "the server name has no address"),
            NtpError::Io(e) => write!(f, "socket error: {e}"),
            NtpError::Timeout => write!(f, "no reply in time"),
            NtpError::BadLocalClock => write!(f, "the local clock is before 1970"),
            NtpError::ShortReply(n) => write!(f, "reply of {n} bytes is shorter than 48"),
            NtpError::BadMode(m) => write!(f, "reply mode {m} is not server mode (4)"),
            NtpError::KissOfDeath => write!(f, "reply stratum is 0"),
            NtpError::Unsynchronized => write!(f, "server clock is not synchronized"),
            NtpError::ZeroTransmit => write!(f, "reply transmit time is zero"),
            NtpError::OriginMismatch => write!(f, "reply does not echo the request"),
        }
    }
}

impl std::error::Error for NtpError {}

/// The 48-byte client request. The transmit time is the send time. The server echoes it.
pub fn request(transmit: Timestamp) -> [u8; PACKET_LEN] {
    let mut packet = [0u8; PACKET_LEN];
    // Leap indicator 0, version 4, mode 3 (client).
    packet[0] = 0x23;
    packet[40..48].copy_from_slice(&transmit.0.to_be_bytes());
    packet
}

fn stamp(packet: &[u8], at: usize) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&packet[at..at + 8]);
    u64::from_be_bytes(bytes)
}

/// Checks a reply to the request sent at `sent` and computes the offset.
///
/// `received` is the local time when the reply arrived. The offset is
/// `((t2 - t1) + (t3 - t4)) / 2`, where t1 is `sent`, t2 the server's receive
/// time, t3 its transmit time and t4 `received`. Rejects a reply that is short,
/// has a mode other than server, has stratum 0, is unsynchronized, has a zero
/// transmit time or does not echo `sent`.
pub fn parse_reply(packet: &[u8], sent: Timestamp, received: Timestamp) -> Result<Sample, NtpError> {
    if packet.len() < PACKET_LEN {
        return Err(NtpError::ShortReply(packet.len()));
    }
    let leap = packet[0] >> 6;
    let mode = packet[0] & 0x07;
    if mode != 4 {
        return Err(NtpError::BadMode(mode));
    }
    let stratum = packet[1];
    if stratum == 0 {
        return Err(NtpError::KissOfDeath);
    }
    if leap == 3 || stratum >= 16 {
        return Err(NtpError::Unsynchronized);
    }
    let origin = stamp(packet, 24);
    let receive = stamp(packet, 32);
    let transmit = stamp(packet, 40);
    if transmit == 0 {
        return Err(NtpError::ZeroTransmit);
    }
    if origin != sent.0 {
        return Err(NtpError::OriginMismatch);
    }
    let t1 = i128::from(sent.0);
    let t2 = i128::from(receive);
    let t3 = i128::from(transmit);
    let t4 = i128::from(received.0);
    let offset = ((t2 - t1) + (t3 - t4)) / 2;
    let delay = (t4 - t1) - (t3 - t2);
    Ok(Sample {
        offset_seconds: to_seconds(offset),
        delay_seconds: to_seconds(delay),
        stratum,
    })
}

/// Converts a difference of NTP timestamps, in 2^-32 second units, to seconds.
fn to_seconds(units: i128) -> f64 {
    units as f64 / FRACTION_SCALE
}

/// Asks the server at `address` (a `host:port` pair) for the time.
///
/// The call waits at most `timeout` for the reply. It sends nothing else and
/// sets nothing. The caller decides what to do with the offset.
pub fn query(address: &str, timeout: Duration) -> Result<Sample, NtpError> {
    let server = address
        .to_socket_addrs()
        .map_err(NtpError::Resolve)?
        .next()
        .ok_or(NtpError::NoAddress)?;
    let bind = if server.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
    let socket = UdpSocket::bind(bind).map_err(NtpError::Io)?;
    socket.set_read_timeout(Some(timeout)).map_err(NtpError::Io)?;
    socket.set_write_timeout(Some(timeout)).map_err(NtpError::Io)?;
    socket.connect(server).map_err(NtpError::Io)?;

    let sent = Timestamp::from_system_time(SystemTime::now()).ok_or(NtpError::BadLocalClock)?;
    socket.send(&request(sent)).map_err(NtpError::Io)?;

    let mut buffer = [0u8; 512];
    let len = match socket.recv(&mut buffer) {
        Ok(len) => len,
        Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
            return Err(NtpError::Timeout)
        }
        Err(e) => return Err(NtpError::Io(e)),
    };
    let received = Timestamp::from_system_time(SystemTime::now()).ok_or(NtpError::BadLocalClock)?;
    parse_reply(&buffer[..len], sent, received)
}

/// Whether a sync is due. It is due when Wi-Fi is connected and no sync has
/// started in the last [`INTERVAL`]. A clock that moved back counts as due.
pub fn due(connected: bool, last_attempt: Option<SystemTime>, now: SystemTime) -> bool {
    if !connected {
        return false;
    }
    match last_attempt {
        None => true,
        Some(last) => match now.duration_since(last) {
            Ok(elapsed) => elapsed >= INTERVAL,
            Err(_) => true,
        },
    }
}

/// The steps that set the Kobo clock, as KOReader's timesync does. `ntpd -q`
/// sets the clock and exits. The clock copies to the RTC only when `ntpd`
/// succeeded, so a failed query leaves both clocks alone.
pub fn device_steps() -> Plan {
    Plan::new(vec![
        Step::run(
            "ntpd",
            &["-q", "-n", "-p", POOL_HOST],
            None,
            &[],
            "plugins/timesync.koplugin/main.lua:21 (ntpd -q -n -p pool.ntp.org)",
        )
        .on_error(OnError::Abort),
        Step::run(
            "hwclock",
            &["-u", "-w"],
            None,
            &[],
            "plugins/timesync.koplugin/main.lua:65 (hwclock -u -w)",
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Action;

    /// A timestamp `secs` seconds and `nanos` nanoseconds after 1970.
    fn at(secs: u64, nanos: u32) -> Timestamp {
        Timestamp::from_unix(Duration::new(secs, nanos))
    }

    /// A reply with the given first byte, stratum and origin, and receive and transmit times.
    fn reply(first: u8, stratum: u8, origin: Timestamp, receive: Timestamp, transmit: Timestamp) -> Vec<u8> {
        let mut packet = vec![0u8; PACKET_LEN];
        packet[0] = first;
        packet[1] = stratum;
        packet[24..32].copy_from_slice(&origin.0.to_be_bytes());
        packet[32..40].copy_from_slice(&receive.0.to_be_bytes());
        packet[40..48].copy_from_slice(&transmit.0.to_be_bytes());
        packet
    }

    #[test]
    fn request_is_client_mode_four_v4_with_transmit_time() {
        let sent = at(1_800_000_000, 250_000_000);
        let packet = request(sent);
        assert_eq!(packet.len(), 48);
        assert_eq!(packet[0], 0x23, "leap 0, version 4, mode 3");
        assert_eq!(&packet[40..48], &sent.0.to_be_bytes());
    }

    #[test]
    fn unix_epoch_maps_to_the_ntp_epoch_offset() {
        assert_eq!(at(0, 0).0, UNIX_TO_NTP_SECONDS << 32);
        assert_eq!(at(0, 500_000_000).0, (UNIX_TO_NTP_SECONDS << 32) | (1 << 31));
    }

    #[test]
    fn offset_and_delay_follow_the_rfc_formula() {
        // The client sends at t1 = 100.000. The server, two seconds ahead, receives at
        // 102.040 and transmits at 102.041. The reply arrives at t4 = 100.100.
        let t1 = at(100, 0);
        let good = reply(0x24, 2, t1, at(102, 40_000_000), at(102, 41_000_000));
        let sample = parse_reply(&good, t1, at(100, 100_000_000)).unwrap();
        // ((2.040) + (2.041 - 0.100)) / 2 = 1.9905
        assert!(
            (sample.offset_seconds - 1.9905).abs() < 1e-6,
            "{}",
            sample.offset_seconds
        );
        // (0.100) - (0.001) = 0.099
        assert!(
            (sample.delay_seconds - 0.099).abs() < 1e-6,
            "{}",
            sample.delay_seconds
        );
        assert_eq!(sample.stratum, 2);
    }

    #[test]
    fn a_server_behind_gives_a_negative_offset() {
        let t1 = at(500, 0);
        let good = reply(0x24, 1, t1, at(499, 0), at(499, 0));
        let sample = parse_reply(&good, t1, at(500, 0)).unwrap();
        assert!(
            (sample.offset_seconds + 1.0).abs() < 1e-6,
            "{}",
            sample.offset_seconds
        );
    }

    #[test]
    fn a_short_reply_is_refused() {
        let t1 = at(1, 0);
        let packet = reply(0x24, 2, t1, t1, t1);
        assert!(matches!(
            parse_reply(&packet[..47], t1, t1),
            Err(NtpError::ShortReply(47))
        ));
    }

    #[test]
    fn a_reply_that_is_not_server_mode_is_refused() {
        let t1 = at(1, 0);
        for mode in [0u8, 1, 2, 3, 5, 6, 7] {
            let packet = reply(0x20 | mode, 2, t1, t1, t1);
            match parse_reply(&packet, t1, t1) {
                Err(NtpError::BadMode(m)) => assert_eq!(m, mode),
                other => panic!("mode {mode}: {other:?}"),
            }
        }
    }

    #[test]
    fn stratum_zero_is_refused() {
        let t1 = at(1, 0);
        let packet = reply(0x24, 0, t1, t1, t1);
        assert!(matches!(parse_reply(&packet, t1, t1), Err(NtpError::KissOfDeath)));
    }

    #[test]
    fn an_unsynchronized_server_is_refused() {
        let t1 = at(1, 0);
        let alarm = reply(0x24 | 0xC0, 2, t1, t1, t1);
        assert!(matches!(
            parse_reply(&alarm, t1, t1),
            Err(NtpError::Unsynchronized)
        ));
        let stratum_16 = reply(0x24, 16, t1, t1, t1);
        assert!(matches!(
            parse_reply(&stratum_16, t1, t1),
            Err(NtpError::Unsynchronized)
        ));
    }

    #[test]
    fn a_zero_transmit_time_is_refused() {
        let t1 = at(1, 0);
        let packet = reply(0x24, 2, t1, t1, Timestamp(0));
        assert!(matches!(
            parse_reply(&packet, t1, t1),
            Err(NtpError::ZeroTransmit)
        ));
    }

    #[test]
    fn a_reply_that_does_not_echo_the_request_is_refused() {
        let t1 = at(1, 0);
        let stale = reply(0x24, 2, at(0, 0), t1, t1);
        assert!(matches!(
            parse_reply(&stale, t1, t1),
            Err(NtpError::OriginMismatch)
        ));
    }

    #[test]
    fn a_sync_is_due_only_when_connected_and_six_hours_have_passed() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);
        assert!(!due(false, None, now), "no Wi-Fi, no sync");
        assert!(due(true, None, now), "never synced");
        let hour = Duration::from_secs(3600);
        assert!(!due(true, Some(now - 5 * hour), now), "five hours is too soon");
        assert!(due(true, Some(now - 6 * hour), now), "six hours is due");
        assert!(
            due(true, Some(now + hour), now),
            "a clock moved back counts as due"
        );
    }

    #[test]
    fn device_steps_run_busybox_ntpd_then_hwclock() {
        let plan = device_steps();
        assert!(plan.skipped.is_none());
        assert_eq!(plan.steps.len(), 2);
        match &plan.steps[0].action {
            Action::Run { program, args, .. } => {
                assert_eq!(program, "ntpd");
                assert_eq!(args, &["-q", "-n", "-p", "pool.ntp.org"]);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            plan.steps[0].on_error,
            OnError::Abort,
            "a failed query skips hwclock"
        );
        match &plan.steps[1].action {
            Action::Run { program, args, .. } => {
                assert_eq!(program, "hwclock");
                assert_eq!(args, &["-u", "-w"]);
            }
            other => panic!("{other:?}"),
        }
    }
}
