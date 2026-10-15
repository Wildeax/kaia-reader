//! wpa_supplicant control interface client.
//!
//! KOReader talks to wpa_supplicant through `lj-wpaclient`, a LuaJIT module
//! fetched at build time (`base/thirdparty/lj-wpaclient/CMakeLists.txt`). Its
//! source is not in this repository. The commands below follow the
//! wpa_supplicant control interface and the calls that
//! `frontend/ui/network/wpa_supplicant.lua` makes: `SCAN`, `SCAN_RESULTS`,
//! `ADD_NETWORK`, `SET_NETWORK`, `ENABLE_NETWORK`, `REMOVE_NETWORK`,
//! `STATUS`, `ATTACH`, and event reads. Unverified against a real
//! wpa_supplicant.
//!
//! The socket is a Unix datagram socket at `/var/run/wpa_supplicant/<iface>`
//! (`frontend/device/kobo/device.lua:1038`). The client binds its own
//! datagram socket so that replies have an address.

use std::collections::VecDeque;
use std::io;
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::error::PlatformError;

/// One row of `SCAN_RESULTS`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanResult {
    /// Network name, decoded from wpa_supplicant's escaping.
    pub ssid: String,
    /// Access point address, `aa:bb:cc:dd:ee:ff`.
    pub bssid: String,
    /// Centre frequency in MHz.
    pub frequency_mhz: u32,
    /// Signal level in dBm.
    pub signal_dbm: i32,
    /// Security flags such as `[WPA2-PSK-CCMP][ESS]`.
    pub flags: String,
}

/// A saved supplicant entry. The id, rather than its name, identifies removals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedNetwork {
    pub id: u32,
    pub ssid: String,
}

/// The parts of `STATUS` that Kaia uses.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WpaStatus {
    /// `wpa_state`, for example `COMPLETED` or `SCANNING`.
    pub wpa_state: String,
    /// Network id of the current association.
    pub id: Option<u32>,
    /// Decoded SSID of the current association.
    pub ssid: Option<String>,
}

/// A connection to the wpa_supplicant control socket.
pub struct WpaCtrl {
    socket: UnixDatagram,
    local: PathBuf,
    timeout: Duration,
    events: VecDeque<String>,
}

static CLIENT_COUNTER: AtomicU64 = AtomicU64::new(0);

impl WpaCtrl {
    /// Binds `local` and connects to the server socket at `server`.
    pub fn connect(server: &Path, local: &Path, timeout: Duration) -> Result<Self, PlatformError> {
        let _ = std::fs::remove_file(local);
        let socket =
            UnixDatagram::bind(local).map_err(|e| PlatformError::io(local.display().to_string(), e))?;
        socket
            .connect(server)
            .map_err(|e| PlatformError::io(server.display().to_string(), e))?;
        socket
            .set_read_timeout(Some(timeout))
            .map_err(|e| PlatformError::io(local.display().to_string(), e))?;
        Ok(WpaCtrl {
            socket,
            local: local.to_path_buf(),
            timeout,
            events: VecDeque::new(),
        })
    }

    /// A unique local socket name in `dir`, for example `kaia_wpa_<pid>_<n>`.
    pub fn local_name(dir: &Path) -> PathBuf {
        let n = CLIENT_COUNTER.fetch_add(1, Ordering::Relaxed);
        dir.join(format!("kaia_wpa_{}_{n}", std::process::id()))
    }

    /// Sends one command and returns its reply. Event datagrams that arrive
    /// first are queued for [`WpaCtrl::next_event`]. A reply starting with `FAIL` is an error.
    pub fn request(&mut self, command: &str) -> Result<String, PlatformError> {
        self.socket
            .send(command.as_bytes())
            .map_err(|e| PlatformError::io(format!("wpa {command}"), e))?;
        loop {
            let reply = self.recv()?;
            if is_event(&reply) {
                self.events.push_back(reply);
                continue;
            }
            if reply.starts_with("FAIL") || reply.starts_with("UNKNOWN COMMAND") {
                return Err(PlatformError::Wpa(format!("{command}: {reply}")));
            }
            return Ok(reply);
        }
    }

    /// Next unsolicited event, from the queue or the socket. `None` on timeout.
    /// Needs [`WpaCtrl::attach`] first.
    pub fn next_event(&mut self, wait: Duration) -> Result<Option<String>, PlatformError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(Some(event));
        }
        self.socket
            .set_read_timeout(Some(wait))
            .map_err(|e| PlatformError::io("wpa socket", e))?;
        let result = match self.recv() {
            Ok(text) if is_event(&text) => Ok(Some(text)),
            Ok(_) => Ok(None),
            Err(PlatformError::Io { source, .. })
                if matches!(source.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        };
        self.socket
            .set_read_timeout(Some(self.timeout))
            .map_err(|e| PlatformError::io("wpa socket", e))?;
        result
    }

    fn recv(&self) -> Result<String, PlatformError> {
        let mut buf = vec![0u8; 65536];
        let n = self
            .socket
            .recv(&mut buf)
            .map_err(|e| PlatformError::io("wpa recv", e))?;
        Ok(String::from_utf8_lossy(&buf[..n]).trim_end().to_string())
    }

    pub fn list_networks(&mut self) -> Result<Vec<SavedNetwork>, PlatformError> {
        let reply = self.request("LIST_NETWORKS")?;
        Ok(reply
            .lines()
            .skip(1)
            .filter_map(|line| {
                let mut fields = line.split('\t');
                Some(SavedNetwork {
                    id: fields.next()?.parse().ok()?,
                    ssid: decode_ssid(fields.next()?),
                })
            })
            .collect())
    }

    pub fn save_config(&mut self) -> Result<(), PlatformError> {
        self.request("SAVE_CONFIG").map(|_| ())
    }

    /// `ATTACH`: receive unsolicited events on this socket.
    pub fn attach(&mut self) -> Result<(), PlatformError> {
        self.request("ATTACH").map(|_| ())
    }

    /// `DETACH`: stop receiving events. Errors are ignored by the caller.
    pub fn detach(&mut self) -> Result<(), PlatformError> {
        self.request("DETACH").map(|_| ())
    }

    /// `SCAN`: starts a scan. The reply must be `OK`.
    pub fn start_scan(&mut self) -> Result<(), PlatformError> {
        let reply = self.request("SCAN")?;
        if reply == "OK" {
            Ok(())
        } else {
            Err(PlatformError::Wpa(format!("SCAN: {reply}")))
        }
    }

    /// `SCAN_RESULTS`, parsed.
    pub fn scan_results(&mut self) -> Result<Vec<ScanResult>, PlatformError> {
        let text = self.request("SCAN_RESULTS")?;
        Ok(parse_scan_results(&text))
    }

    /// `STATUS`, parsed.
    pub fn status(&mut self) -> Result<WpaStatus, PlatformError> {
        let text = self.request("STATUS")?;
        Ok(parse_status(&text))
    }

    /// `ADD_NETWORK`. Returns the new network id.
    pub fn add_network(&mut self) -> Result<u32, PlatformError> {
        let reply = self.request("ADD_NETWORK")?;
        reply
            .trim()
            .parse()
            .map_err(|_| PlatformError::Wpa(format!("ADD_NETWORK: unexpected reply {reply:?}")))
    }

    /// `SET_NETWORK <id> <key> <value>`. The value is sent as given.
    pub fn set_network(&mut self, id: u32, key: &str, value: &str) -> Result<(), PlatformError> {
        let reply = self.request(&format!("SET_NETWORK {id} {key} {value}"))?;
        if reply == "OK" {
            Ok(())
        } else {
            Err(PlatformError::Wpa(format!("SET_NETWORK {key}: {reply}")))
        }
    }

    /// `ENABLE_NETWORK <id>`.
    pub fn enable_network(&mut self, id: u32) -> Result<(), PlatformError> {
        let reply = self.request(&format!("ENABLE_NETWORK {id}"))?;
        if reply == "OK" {
            Ok(())
        } else {
            Err(PlatformError::Wpa(format!("ENABLE_NETWORK {id}: {reply}")))
        }
    }

    /// `REMOVE_NETWORK <id>`.
    pub fn remove_network(&mut self, id: u32) -> Result<(), PlatformError> {
        self.request(&format!("REMOVE_NETWORK {id}")).map(|_| ())
    }
}

impl Drop for WpaCtrl {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.local);
    }
}

fn is_event(text: &str) -> bool {
    text.starts_with('<')
}

/// Encodes an SSID as hex, the form KOReader sends (`bin_to_hex(network.ssid)`).
pub fn encode_ssid(ssid: &str) -> String {
    ssid.bytes().map(|b| format!("{b:02x}")).collect()
}

/// Checks a passphrase and returns the value for `SET_NETWORK psk`.
///
/// KOReader computes the 64-digit PSK with PBKDF2 (`wpa_supplicant.lua:109`).
/// Kaia sends the passphrase in quotes and lets wpa_supplicant derive the
/// PSK. A 64-digit hex string is sent as the PSK itself.
pub fn psk_value(passphrase: &str) -> Result<String, PlatformError> {
    if passphrase.len() == 64 && passphrase.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(passphrase.to_string());
    }
    let len = passphrase.len();
    if !(8..=63).contains(&len) {
        return Err(PlatformError::InvalidInput(
            "passphrase must be 8 to 63 characters".to_string(),
        ));
    }
    if passphrase
        .bytes()
        .any(|b| !(0x20..=0x7e).contains(&b) || b == b'"')
    {
        return Err(PlatformError::InvalidInput(
            "passphrase must be printable ASCII without quotes".to_string(),
        ));
    }
    Ok(format!("\"{passphrase}\""))
}

/// Parses `SCAN_RESULTS`. The first line is a header. Fields are tab-separated.
pub fn parse_scan_results(text: &str) -> Vec<ScanResult> {
    text.lines()
        .filter(|line| !line.starts_with("bssid") && !line.trim().is_empty())
        .filter_map(|line| {
            let mut fields = line.splitn(5, '\t');
            let bssid = fields.next()?.to_string();
            let frequency_mhz = fields.next()?.parse().ok()?;
            let signal_dbm = fields.next()?.parse().ok()?;
            let flags = fields.next()?.to_string();
            let ssid = decode_ssid(fields.next().unwrap_or(""));
            Some(ScanResult {
                ssid,
                bssid,
                frequency_mhz,
                signal_dbm,
                flags,
            })
        })
        .collect()
}

/// Parses `STATUS`, which is `key=value` lines.
pub fn parse_status(text: &str) -> WpaStatus {
    let mut status = WpaStatus::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "wpa_state" => status.wpa_state = value.to_string(),
            "id" => status.id = value.parse().ok(),
            "ssid" => status.ssid = Some(decode_ssid(value)),
            _ => {}
        }
    }
    status
}

/// Undoes wpa_supplicant's escaping: `\xHH` bytes and `\\`. Mirrors
/// `decodeSSID` in `frontend/ui/network/wpa_supplicant.lua:20-35`.
pub fn decode_ssid(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'\\' {
                out.push(b'\\');
                i += 2;
                continue;
            }
            if bytes[i + 1] == b'x' && i + 4 <= bytes.len() {
                let parsed = std::str::from_utf8(&bytes[i + 2..i + 4])
                    .ok()
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok());
                if let Some(byte) = parsed {
                    out.push(byte);
                    i += 4;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssid_round_trip_through_hex_and_decode() {
        assert_eq!(encode_ssid("Home"), "486f6d65");
        assert_eq!(decode_ssid("caf\\xc3\\xa9"), "café");
        assert_eq!(decode_ssid("a\\\\b"), "a\\b");
        assert_eq!(decode_ssid("plain"), "plain");
    }

    #[test]
    fn psk_rules() {
        assert_eq!(psk_value("correct horse").unwrap(), "\"correct horse\"");
        assert!(psk_value("short").is_err());
        assert!(psk_value("has\"quote1").is_err());
        let hex = "a".repeat(64);
        assert_eq!(psk_value(&hex).unwrap(), hex);
    }

    #[test]
    fn scan_results_parse() {
        let text = "bssid / frequency / signal level / flags / ssid\n\
                    aa:bb:cc:dd:ee:01\t2437\t-52\t[WPA2-PSK-CCMP][ESS]\tHome\n\
                    aa:bb:cc:dd:ee:02\t5180\t-70\t[ESS]\tCaf\\xc3\\xa9 net\n";
        let rows = parse_scan_results(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].ssid, "Home");
        assert_eq!(rows[0].frequency_mhz, 2437);
        assert_eq!(rows[0].signal_dbm, -52);
        assert_eq!(rows[1].ssid, "Café net");
        assert_eq!(rows[1].flags, "[ESS]");
    }

    #[test]
    fn status_parse() {
        let status =
            parse_status("bssid=aa:bb:cc:dd:ee:01\nfreq=2437\nssid=Home\nid=3\nwpa_state=COMPLETED\n");
        assert_eq!(status.wpa_state, "COMPLETED");
        assert_eq!(status.id, Some(3));
        assert_eq!(status.ssid.as_deref(), Some("Home"));
    }
}
