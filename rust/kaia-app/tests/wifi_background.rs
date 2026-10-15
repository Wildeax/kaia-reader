//! The status Wi-Fi panel keeps dispatching input while the radio is busy.
mod common;
use common::*;
use kaia_app::Route;
use kaia_input::Key;
use kaia_platform::{network::Network, wpa::ScanResult, PlatformError};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

struct SlowRadio {
    entered: Sender<&'static str>,
    release: Receiver<()>,
    connected: bool,
}
impl SlowRadio {
    fn pause(&self, operation: &'static str) {
        self.entered.send(operation).unwrap();
        self.release.recv_timeout(Duration::from_secs(10)).unwrap();
    }
}
impl Network for SlowRadio {
    fn is_wifi_on(&self) -> bool {
        true
    }
    fn is_connected(&self) -> bool {
        self.connected
    }
    fn enable_wifi(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn disable_wifi(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn release_ip(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn obtain_ip(&mut self) -> Result<(), PlatformError> {
        self.pause("dhcp");
        self.connected = true;
        Ok(())
    }
    fn scan(&mut self) -> Result<Vec<ScanResult>, PlatformError> {
        self.pause("scan");
        Ok(vec![ScanResult {
            ssid: "Test".into(),
            bssid: String::new(),
            frequency_mhz: 2412,
            signal_dbm: -45,
            flags: String::new(),
        }])
    }
    fn connect(&mut self, _: &str, _: Option<&str>) -> Result<(), PlatformError> {
        self.pause("join");
        Ok(())
    }
}

#[test]
fn scan_join_and_dhcp_leave_the_ui_responsive_and_restore_the_radio() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut app = make_app(&scratch("wifi-background"), vec![]);
    let (entered, operations) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    app.services_mut().platform.network = Box::new(SlowRadio {
        entered,
        release: gate,
        connected: false,
    });
    app.start().unwrap();
    app.command("status:wifi", &[]);
    app.process(vec![]).unwrap();
    for operation in ["scan", "join", "dhcp"] {
        if operation == "join" {
            app.command("wifi:network:0", &[]);
            app.process(vec![]).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Ok(received) = operations.try_recv() {
                assert_eq!(received, operation);
                break;
            }
            assert!(Instant::now() < deadline, "{operation} worker did not start");
            app.iterate(Some(Duration::from_millis(10))).unwrap();
        }
        let start = Instant::now();
        app.key(Key::Back).unwrap();
        app.process(vec![]).unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "UI waited for {operation}"
        );
        assert_eq!(app.top_route(), Some(&Route::Wifi));
        assert!(app.reader_busy());
        release.send(()).unwrap();
        if operation == "scan" {
            wait_for(&mut app, "scan result", |a| {
                a.find_rect("wifi-network-0").is_some()
            });
        }
    }
    wait_for(&mut app, "join result", |a| !a.reader_busy());
    assert!(app.services().platform.network.is_connected());
    app.key(Key::Back).unwrap();
    assert_eq!(app.top_route(), Some(&Route::Home));
    // The real backend remains usable after the panel closes.
    assert!(app.services_mut().platform.network.release_ip().is_ok());
}
