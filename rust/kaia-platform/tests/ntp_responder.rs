//! The SNTP client against a local UDP responder. No test contacts pool.ntp.org.

use std::net::UdpSocket;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use kaia_platform::ntp::{self, NtpError};

/// Seconds from 1900 to 1970, written here so the test does not reuse the crate's conversion.
const NTP_EPOCH_SECONDS: u64 = 2_208_988_800;

/// The 8-byte NTP form of `time`.
fn ntp_bytes(time: SystemTime) -> [u8; 8] {
    let since = time.duration_since(UNIX_EPOCH).unwrap();
    let seconds = since.as_secs() + NTP_EPOCH_SECONDS;
    let fraction = (u64::from(since.subsec_nanos()) << 32) / 1_000_000_000;
    ((seconds << 32) | fraction).to_be_bytes()
}

/// A good server reply to `request`. The server clock runs `skew_seconds` ahead of this machine, or behind when negative.
fn good_reply(request: &[u8], skew_seconds: i64) -> Vec<u8> {
    let now = SystemTime::now();
    let skew = Duration::from_secs(skew_seconds.unsigned_abs());
    let server_time = if skew_seconds >= 0 { now + skew } else { now - skew };
    let server_now = ntp_bytes(server_time);
    let mut packet = vec![0u8; 48];
    packet[0] = 0x24; // leap 0, version 4, mode 4 (server)
    packet[1] = 2; // stratum 2
    packet[24..32].copy_from_slice(&request[40..48]); // origin: echo the transmit time
    packet[32..40].copy_from_slice(&server_now); // receive
    packet[40..48].copy_from_slice(&server_now); // transmit
    packet
}

/// Starts a responder on a loopback port. It takes one request, replies with
/// `build` (or stays silent when `build` returns None), and returns the request it got.
fn serve_once<F>(build: F) -> (String, thread::JoinHandle<Option<Vec<u8>>>)
where
    F: FnOnce(&[u8]) -> Option<Vec<u8>> + Send + 'static,
{
    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let address = socket.local_addr().unwrap().to_string();
    let handle = thread::spawn(move || {
        let mut buffer = [0u8; 512];
        let (len, peer) = socket.recv_from(&mut buffer).ok()?;
        let request = buffer[..len].to_vec();
        if let Some(reply) = build(&request) {
            socket.send_to(&reply, peer).unwrap();
        }
        Some(request)
    });
    (address, handle)
}

fn take_request(handle: thread::JoinHandle<Option<Vec<u8>>>) -> Vec<u8> {
    let request = handle.join().unwrap().expect("the responder got a request");
    assert_eq!(request.len(), 48, "a request is one 48-byte packet");
    assert_eq!(request[0], 0x23, "leap 0, version 4, mode 3 (client)");
    request
}

#[test]
fn a_good_reply_gives_the_offset_of_the_server_clock() {
    let (address, handle) = serve_once(|request| Some(good_reply(request, 3)));
    let sample = ntp::query(&address, Duration::from_secs(5)).expect("a good reply");
    take_request(handle);
    // The server is 3 seconds ahead. Network delay on loopback is a few milliseconds.
    assert!(
        (sample.offset_seconds - 3.0).abs() < 0.05,
        "offset {}",
        sample.offset_seconds
    );
    assert!(
        sample.delay_seconds >= 0.0 && sample.delay_seconds < 0.5,
        "delay {}",
        sample.delay_seconds
    );
    assert_eq!(sample.stratum, 2);
}

#[test]
fn a_server_behind_the_local_clock_gives_a_negative_offset() {
    let (address, handle) = serve_once(|request| Some(good_reply(request, -2)));
    let sample = ntp::query(&address, Duration::from_secs(5)).expect("a good reply");
    take_request(handle);
    assert!(
        (sample.offset_seconds + 2.0).abs() < 0.05,
        "offset {}",
        sample.offset_seconds
    );
}

#[test]
fn a_client_mode_reply_is_refused() {
    let (address, handle) = serve_once(|request| {
        let mut reply = good_reply(request, 0);
        reply[0] = 0x23; // mode 3 is a client packet
        Some(reply)
    });
    let result = ntp::query(&address, Duration::from_secs(5));
    take_request(handle);
    assert!(matches!(result, Err(NtpError::BadMode(3))), "{result:?}");
}

#[test]
fn a_stratum_zero_reply_is_refused() {
    let (address, handle) = serve_once(|request| {
        let mut reply = good_reply(request, 0);
        reply[1] = 0; // kiss-of-death
        Some(reply)
    });
    let result = ntp::query(&address, Duration::from_secs(5));
    take_request(handle);
    assert!(matches!(result, Err(NtpError::KissOfDeath)), "{result:?}");
}

#[test]
fn a_zero_transmit_time_is_refused() {
    let (address, handle) = serve_once(|request| {
        let mut reply = good_reply(request, 0);
        reply[40..48].fill(0);
        Some(reply)
    });
    let result = ntp::query(&address, Duration::from_secs(5));
    take_request(handle);
    assert!(matches!(result, Err(NtpError::ZeroTransmit)), "{result:?}");
}

#[test]
fn a_reply_that_does_not_echo_the_request_is_refused() {
    let (address, handle) = serve_once(|request| {
        let mut reply = good_reply(request, 0);
        reply[24..32].fill(0);
        Some(reply)
    });
    let result = ntp::query(&address, Duration::from_secs(5));
    take_request(handle);
    assert!(matches!(result, Err(NtpError::OriginMismatch)), "{result:?}");
}

#[test]
fn a_short_reply_is_refused() {
    let (address, handle) = serve_once(|request| {
        let mut reply = good_reply(request, 0);
        reply.truncate(20);
        Some(reply)
    });
    let result = ntp::query(&address, Duration::from_secs(5));
    take_request(handle);
    assert!(matches!(result, Err(NtpError::ShortReply(20))), "{result:?}");
}

#[test]
fn silence_times_out_without_a_clock_value() {
    let (address, handle) = serve_once(|_| None);
    let started = std::time::Instant::now();
    let result = ntp::query(&address, Duration::from_millis(300));
    take_request(handle);
    assert!(matches!(result, Err(NtpError::Timeout)), "{result:?}");
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the wait is bounded by the timeout"
    );
}

#[test]
fn an_address_that_does_not_resolve_is_an_error() {
    let result = ntp::query("nonexistent.invalid:123", Duration::from_millis(300));
    assert!(result.is_err(), "{result:?}");
}
