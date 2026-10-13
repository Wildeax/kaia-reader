//! wpa_supplicant control exchanges against a fake Unix datagram socket.

mod common;

use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use common::FakeIo;
use kaia_platform::error::PlatformError;
use kaia_platform::network::{KoboNetwork, Network};
use kaia_platform::sysfs::Root;

/// Each entry: the command the client must send (prefix), and the datagrams to send back.
type Script = Vec<(&'static str, Vec<&'static str>)>;

/// Binds the fake wpa_supplicant socket at `path` and serves `script` in a thread.
/// Returns the commands it received.
fn serve(path: PathBuf, script: Script) -> thread::JoinHandle<Vec<String>> {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let sock = UnixDatagram::bind(&path).unwrap();
    sock.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    thread::spawn(move || {
        let mut seen = Vec::new();
        let mut buf = vec![0u8; 65536];
        for (expected, replies) in script {
            let Ok((n, from)) = sock.recv_from(&mut buf) else {
                break;
            };
            let cmd = String::from_utf8_lossy(&buf[..n]).to_string();
            seen.push(cmd.clone());
            if !cmd.starts_with(expected) {
                break;
            }
            let client = from
                .as_pathname()
                .expect("client socket has a path")
                .to_path_buf();
            for reply in replies {
                sock.send_to(reply.as_bytes(), &client).unwrap();
            }
        }
        seen
    })
}

fn network(root: &Path) -> KoboNetwork<FakeIo> {
    std::fs::create_dir_all(root.join("tmp")).unwrap();
    let mut net = KoboNetwork::new(
        Root::new(root),
        FakeIo::new(root),
        PathBuf::from("/mnt/onboard/.adds/kaia"),
        "eth0",
        vec![],
    );
    net.reply_timeout = Duration::from_secs(2);
    net
}

fn socket_path(root: &Path) -> PathBuf {
    root.join("var/run/wpa_supplicant/eth0")
}

#[test]
fn scan_attaches_waits_for_results_and_parses_rows() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(
        socket_path(dir.path()),
        vec![
            ("ATTACH", vec!["OK"]),
            ("SCAN", vec!["OK", "<3>CTRL-EVENT-SCAN-RESULTS "]),
            (
                "SCAN_RESULTS",
                vec!["bssid / frequency / signal level / flags / ssid\naa:bb:cc:dd:ee:01\t2437\t-52\t[WPA2-PSK-CCMP][ESS]\tHome\n"],
            ),
            ("DETACH", vec!["OK"]),
        ],
    );
    let mut net = network(dir.path());
    let rows = net.scan().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ssid, "Home");
    assert_eq!(rows[0].signal_dbm, -52);
    assert_eq!(
        server.join().unwrap(),
        vec!["ATTACH", "SCAN", "SCAN_RESULTS", "DETACH"]
    );
}

#[test]
fn connect_sets_ssid_psk_enables_and_waits_for_completed() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(
        socket_path(dir.path()),
        vec![
            ("ATTACH", vec!["OK"]),
            ("ADD_NETWORK", vec!["0"]),
            ("SET_NETWORK 0 ssid 486f6d65", vec!["OK"]),
            ("SET_NETWORK 0 psk \"password1\"", vec!["OK"]),
            ("ENABLE_NETWORK 0", vec!["OK"]),
            ("STATUS", vec!["wpa_state=ASSOCIATING\nid=0\n"]),
            ("STATUS", vec!["wpa_state=COMPLETED\nid=0\nssid=Home\n"]),
            ("DETACH", vec!["OK"]),
        ],
    );
    let mut net = network(dir.path());
    net.connect("Home", Some("password1")).unwrap();
    let seen = server.join().unwrap();
    assert_eq!(seen.len(), 8);
    assert_eq!(seen.last().map(String::as_str), Some("DETACH"));
}

#[test]
fn open_network_sets_key_mgmt_none() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(
        socket_path(dir.path()),
        vec![
            ("ATTACH", vec!["OK"]),
            ("ADD_NETWORK", vec!["4"]),
            ("SET_NETWORK 4 ssid", vec!["OK"]),
            ("SET_NETWORK 4 key_mgmt NONE", vec!["OK"]),
            ("ENABLE_NETWORK 4", vec!["OK"]),
            ("STATUS", vec!["wpa_state=COMPLETED\nid=4\n"]),
            ("DETACH", vec!["OK"]),
        ],
    );
    let mut net = network(dir.path());
    net.connect("Cafe", None).unwrap();
    assert_eq!(server.join().unwrap().len(), 7);
}

#[test]
fn repeated_auth_rejects_remove_the_network() {
    let dir = tempfile::tempdir().unwrap();
    let reject = "<3>CTRL-EVENT-AUTH-REJECT id=1";
    let status = "wpa_state=ASSOCIATING\nid=1\n";
    let server = serve(
        socket_path(dir.path()),
        vec![
            ("ATTACH", vec!["OK"]),
            ("ADD_NETWORK", vec!["1"]),
            ("SET_NETWORK 1 ssid", vec!["OK"]),
            ("SET_NETWORK 1 psk", vec!["OK"]),
            ("ENABLE_NETWORK 1", vec!["OK"]),
            ("STATUS", vec![reject, status]),
            ("STATUS", vec![reject, status]),
            ("STATUS", vec![reject, status]),
            ("STATUS", vec![reject, status]),
            ("REMOVE_NETWORK 1", vec!["OK"]),
        ],
    );
    let mut net = network(dir.path());
    let err = net.connect("Home", Some("wrongpass")).unwrap_err();
    assert!(matches!(err, PlatformError::AuthFailed), "{err}");
    let seen = server.join().unwrap();
    assert_eq!(seen.last().map(String::as_str), Some("REMOVE_NETWORK 1"));
}

#[test]
fn invalid_passphrase_is_rejected_before_enable() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(
        socket_path(dir.path()),
        vec![
            ("ATTACH", vec!["OK"]),
            ("ADD_NETWORK", vec!["2"]),
            ("SET_NETWORK 2 ssid", vec!["OK"]),
            ("REMOVE_NETWORK 2", vec!["OK"]),
        ],
    );
    let mut net = network(dir.path());
    let err = net.connect("Home", Some("short")).unwrap_err();
    assert!(matches!(err, PlatformError::InvalidInput(_)), "{err}");
    assert_eq!(
        server.join().unwrap().last().map(String::as_str),
        Some("REMOVE_NETWORK 2")
    );
}

#[test]
fn fail_reply_is_an_error_and_the_network_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let server = serve(
        socket_path(dir.path()),
        vec![
            ("ATTACH", vec!["OK"]),
            ("ADD_NETWORK", vec!["0"]),
            ("SET_NETWORK 0 ssid", vec!["FAIL"]),
            ("REMOVE_NETWORK 0", vec!["OK"]),
        ],
    );
    let mut net = network(dir.path());
    let err = net.connect("Home", None).unwrap_err();
    assert!(matches!(err, PlatformError::Wpa(_)), "{err}");
    server.join().unwrap();
}

#[test]
fn missing_control_socket_is_an_io_error() {
    let dir = tempfile::tempdir().unwrap();
    let mut net = network(dir.path());
    let err = net.scan().unwrap_err();
    assert!(matches!(err, PlatformError::Io { .. }), "{err}");
}
