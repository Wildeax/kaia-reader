//! Wi-Fi control for the Kobo.
//!
//! KOReader's Wi-Fi on Kobo is three parts:
//! - the chip and modules, by the shell scripts in `platform/kobo/`, run from
//!   the KOReader directory (`frontend/device/kobo/device.lua:1020-1023`,
//!   `koboEnableWifi` at `device.lua:24-31`);
//! - DHCP, by `obtain-ip.sh` and `release-ip.sh` (`device.lua:1040-1046`);
//! - association, by wpa_supplicant over its control socket
//!   (`device.lua:1038`, `frontend/ui/network/wpa_supplicant.lua`).
//!
//! Kaia runs the same scripts from its install directory. Those scripts
//! call `./luajit frontend/device/kobo/ntx_io.lua` on some boards
//! (`platform/kobo/enable-wifi.sh:85`), so the install directory must hold
//! a LuaJIT binary and that file for those boards to work. Unverified.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::error::PlatformError;
use crate::plan::{execute, OnError, Outcome, Plan, Step};
use crate::sysfs::{Root, Runner, SysfsWriter};
use crate::wpa::{encode_ssid, psk_value, ScanResult, WpaCtrl};

/// Wi-Fi operations. Methods that talk to the device return `Result`.
pub trait Network {
    /// Powers the Wi-Fi chip and loads its driver.
    fn enable_wifi(&mut self) -> Result<(), PlatformError>;
    /// Releases the DHCP lease and powers Wi-Fi down.
    fn disable_wifi(&mut self) -> Result<(), PlatformError>;
    /// Whether the Wi-Fi interface exists (the driver is loaded).
    fn is_wifi_on(&self) -> bool;
    /// Whether the interface has a default route.
    fn is_connected(&self) -> bool;
    /// Runs DHCP on the interface.
    fn obtain_ip(&mut self) -> Result<(), PlatformError>;
    /// Releases the DHCP lease.
    fn release_ip(&mut self) -> Result<(), PlatformError>;
    /// Scans and returns the visible networks.
    fn scan(&mut self) -> Result<Vec<ScanResult>, PlatformError>;
    /// Joins a network. `psk` of `None` or `Some("")` joins an open network.
    fn connect(&mut self, ssid: &str, psk: Option<&str>) -> Result<(), PlatformError>;
}

/// Steps that turn Wi-Fi off, as `NetworkMgr:turnOffWifi` does
/// (`frontend/device/kobo/device.lua:1020-1023`): release the lease, then run `disable-wifi.sh`.
pub fn disable_wifi_steps(install_dir: &Path, env: &[(String, String)]) -> Vec<Step> {
    vec![
        Step::run(
            &script(install_dir, "release-ip.sh"),
            &[],
            Some(install_dir),
            env,
            "frontend/device/kobo/device.lua:1020-1023 (turnOffWifi, releaseIP)",
        ),
        Step::run(
            &script(install_dir, "disable-wifi.sh"),
            &[],
            Some(install_dir),
            env,
            "frontend/device/kobo/device.lua:24-31 (koboEnableWifi false)",
        ),
    ]
}

fn script(install_dir: &Path, name: &str) -> String {
    install_dir.join(name).to_string_lossy().into_owned()
}

/// Kobo Wi-Fi: scripts and wpa_supplicant.
pub struct KoboNetwork<IO: SysfsWriter + Runner> {
    root: Root,
    io: IO,
    install_dir: PathBuf,
    iface: String,
    env: Vec<(String, String)>,
    /// wpa_supplicant control directory (`device.lua:1038`).
    pub wpa_dir: String,
    /// Directory for the client socket. The device has `/tmp`.
    pub tmp_dir: String,
    /// Wait for each socket reply.
    pub reply_timeout: Duration,
    /// Time allowed for a scan to finish.
    pub scan_wait: Duration,
    /// Seconds to wait for association, one per attempt.
    pub connect_attempts: u32,
}

impl<IO: SysfsWriter + Runner> KoboNetwork<IO> {
    /// `install_dir` holds the scripts. `iface` is `INTERFACE`, `eth0` by default
    /// (`platform/kobo/koreader.sh:285-290`). `env` is passed to the scripts.
    pub fn new(root: Root, io: IO, install_dir: PathBuf, iface: &str, env: Vec<(String, String)>) -> Self {
        KoboNetwork {
            root,
            io,
            install_dir,
            iface: iface.to_string(),
            env,
            wpa_dir: "/var/run/wpa_supplicant".to_string(),
            tmp_dir: "/tmp".to_string(),
            reply_timeout: Duration::from_secs(5),
            scan_wait: Duration::from_secs(15),
            connect_attempts: 30,
        }
    }

    /// Plan for `enable-wifi.sh`. A failed script is an error here.
    pub fn enable_plan(&self) -> Plan {
        Plan::new(vec![Step::run(
            &script(&self.install_dir, "enable-wifi.sh"),
            &[],
            Some(self.install_dir.as_path()),
            &self.env,
            "frontend/device/kobo/device.lua:24-31 (koboEnableWifi true)",
        )
        .on_error(OnError::Abort)])
    }

    /// Plan for turning Wi-Fi off.
    pub fn disable_plan(&self) -> Plan {
        Plan::new(disable_wifi_steps(&self.install_dir, &self.env))
    }

    fn run_plan(&mut self, plan: &Plan) -> Result<(), PlatformError> {
        match execute(plan, &mut self.io, &mut |d| std::thread::sleep(d)) {
            Outcome::Completed => Ok(()),
            Outcome::Skipped(reason) => Err(PlatformError::NotSupported(reason)),
            Outcome::Failed { step, .. } => {
                Err(PlatformError::Command(format!("step {step} of a Wi-Fi plan")))
            }
        }
    }

    fn socket_path(&self) -> PathBuf {
        self.root.path(&format!("{}/{}", self.wpa_dir, self.iface))
    }

    fn open_wpa(&self) -> Result<WpaCtrl, PlatformError> {
        let local = WpaCtrl::local_name(&self.root.path(&self.tmp_dir));
        WpaCtrl::connect(&self.socket_path(), &local, self.reply_timeout)
    }

    fn configure_and_wait(
        &self,
        wpa: &mut WpaCtrl,
        id: u32,
        ssid: &str,
        psk: Option<&str>,
    ) -> Result<(), PlatformError> {
        wpa.set_network(id, "ssid", &encode_ssid(ssid))?;
        match psk.filter(|p| !p.is_empty()) {
            None => wpa.set_network(id, "key_mgmt", "NONE")?,
            Some(passphrase) => wpa.set_network(id, "psk", &psk_value(passphrase)?)?,
        }
        wpa.enable_network(id)?;

        let mut auth_failures = 0;
        for _ in 0..self.connect_attempts {
            let status = wpa.status()?;
            if status.wpa_state == "COMPLETED" && status.id == Some(id) {
                return Ok(());
            }
            if let Some(event) = wpa.next_event(Duration::from_secs(1))? {
                if event.contains("CTRL-EVENT-AUTH-REJECT") || event.contains("WRONG_KEY") {
                    auth_failures += 1;
                    if auth_failures > 3 {
                        return Err(PlatformError::AuthFailed);
                    }
                }
            }
        }
        Err(PlatformError::ConnectTimeout)
    }
}

/// Reads `/proc/net/route` for a default route on `iface`. The first line is a header.
fn has_default_route(root: &Root, iface: &str) -> bool {
    root.read_text("/proc/net/route")
        .map(|text| {
            text.lines().skip(1).any(|line| {
                let mut fields = line.split_whitespace();
                fields.next() == Some(iface) && fields.next() == Some("00000000")
            })
        })
        .unwrap_or(false)
}

impl<IO: SysfsWriter + Runner> Network for KoboNetwork<IO> {
    fn enable_wifi(&mut self) -> Result<(), PlatformError> {
        let plan = self.enable_plan();
        self.run_plan(&plan)
    }

    fn disable_wifi(&mut self) -> Result<(), PlatformError> {
        let plan = self.disable_plan();
        self.run_plan(&plan)
    }

    /// `NetworkMgr:sysfsWifiOn` (`frontend/ui/network/manager.lua:206-209`).
    fn is_wifi_on(&self) -> bool {
        self.root.exists(&format!("/sys/class/net/{}", self.iface))
    }

    /// KOReader uses `getifaddrs` for an assigned address (`manager.lua:249`).
    /// Kaia checks for a default route in `/proc/net/route`. Unverified.
    fn is_connected(&self) -> bool {
        has_default_route(&self.root, &self.iface)
    }

    fn obtain_ip(&mut self) -> Result<(), PlatformError> {
        let plan = Plan::new(vec![Step::run(
            &script(&self.install_dir, "obtain-ip.sh"),
            &[],
            Some(self.install_dir.as_path()),
            &self.env,
            "frontend/device/kobo/device.lua:1040-1042 (obtainIP)",
        )
        .on_error(OnError::Abort)]);
        self.run_plan(&plan)
    }

    fn release_ip(&mut self) -> Result<(), PlatformError> {
        let plan = Plan::new(vec![Step::run(
            &script(&self.install_dir, "release-ip.sh"),
            &[],
            Some(self.install_dir.as_path()),
            &self.env,
            "frontend/device/kobo/device.lua:1044-1046 (releaseIP)",
        )
        .on_error(OnError::Abort)]);
        self.run_plan(&plan)
    }

    /// `SCAN`, wait for `CTRL-EVENT-SCAN-RESULTS`, then `SCAN_RESULTS`.
    /// KOReader does the same in `WpaClient:scanThenGetResults`.
    fn scan(&mut self) -> Result<Vec<ScanResult>, PlatformError> {
        let mut wpa = self.open_wpa()?;
        wpa.attach()?;
        wpa.start_scan()?;
        let deadline = Instant::now() + self.scan_wait;
        loop {
            if Instant::now() >= deadline {
                return Err(PlatformError::Wpa("scan timed out".to_string()));
            }
            match wpa.next_event(Duration::from_secs(1))? {
                Some(event) if event.contains("CTRL-EVENT-SCAN-RESULTS") => break,
                Some(event) if event.contains("CTRL-EVENT-SCAN-FAILED") => {
                    return Err(PlatformError::Wpa(format!("scan failed: {event}")));
                }
                _ => {}
            }
        }
        let rows = wpa.scan_results()?;
        let _ = wpa.detach();
        Ok(rows)
    }

    /// Adds a network, sets SSID and key, enables it and waits for `COMPLETED`.
    /// Mirrors `WpaSupplicant:authenticateNetwork` (`wpa_supplicant.lua:80-185`).
    /// After three authentication rejects, or after `connect_attempts` seconds, the network is removed.
    fn connect(&mut self, ssid: &str, psk: Option<&str>) -> Result<(), PlatformError> {
        let mut wpa = self.open_wpa()?;
        wpa.attach()?;
        let id = wpa.add_network()?;
        match self.configure_and_wait(&mut wpa, id, ssid, psk) {
            Ok(()) => {
                let _ = wpa.detach();
                Ok(())
            }
            Err(e) => {
                let _ = wpa.remove_network(id);
                Err(e)
            }
        }
    }
}

/// Wi-Fi on a desktop. There is no radio. Enable and disable do nothing,
/// and scanning and connecting are not supported.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DesktopNetwork;

impl Network for DesktopNetwork {
    fn enable_wifi(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn disable_wifi(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn is_wifi_on(&self) -> bool {
        false
    }

    fn is_connected(&self) -> bool {
        false
    }

    fn obtain_ip(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn release_ip(&mut self) -> Result<(), PlatformError> {
        Ok(())
    }

    fn scan(&mut self) -> Result<Vec<ScanResult>, PlatformError> {
        Err(PlatformError::NotSupported("Wi-Fi scanning"))
    }

    fn connect(&mut self, _ssid: &str, _psk: Option<&str>) -> Result<(), PlatformError> {
        Err(PlatformError::NotSupported("Wi-Fi connection"))
    }
}
