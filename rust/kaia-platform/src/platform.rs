//! The platform bundle and detection.

use std::path::{Path, PathBuf};

use crate::battery::{Battery, KoboBattery, NoBattery, MONZA_BATTERY_DIR};
use crate::device::{self, DeviceInfo, Model};
use crate::error::PlatformError;
use crate::frontlight::{self, Frontlight, FrontlightConfig, SysfsFrontlight};
use crate::network::{DesktopNetwork, KoboNetwork, Network};
use crate::paths::Paths;
use crate::plan::{execute, Outcome, Plan};
use crate::power::PowerManager;
use crate::sysfs::{Root, SystemIo};

/// A Kobo has this directory (`/mnt/onboard/.kobo`). Kaia uses it to detect the device.
pub const KOBO_MARKER: &str = "/mnt/onboard/.kobo";

/// Which platform is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A Kobo with a profile.
    Kobo(Model),
    /// A desktop stand-in.
    Desktop,
}

/// Environment values that detection reads. Tests pass their own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    /// `PRODUCT`, the codename (`koreader.sh:242-249`).
    pub product: Option<String>,
    /// `MODEL_NUMBER`, the product id (`device.lua:1221`).
    pub model_number: Option<String>,
    /// `INTERFACE`, the Wi-Fi interface. Default `eth0` (`koreader.sh:285-290`).
    pub interface: Option<String>,
    /// `WIFI_MODULE`, passed to the Wi-Fi scripts (`koreader.sh:191`).
    pub wifi_module: Option<String>,
    /// `PLATFORM`, passed to the Wi-Fi scripts (`koreader.sh:266-282`).
    pub platform: Option<String>,
    /// The saved warmth, 0 to 100, from the app's settings. A Kobo writes it to the
    /// mixer at startup. KOReader does the same and does not read the mixer back
    /// (`powerd.lua:186-191`). `None` leaves the mixer alone and reports warmth 0
    /// until the app calls `set_warmth`. Not read from the process environment.
    pub saved_warmth: Option<u8>,
    /// `HOME`, for desktop paths.
    pub home: Option<PathBuf>,
    /// `XDG_DATA_HOME`, for desktop paths.
    pub xdg_data_home: Option<PathBuf>,
}

impl Env {
    /// Reads the variables from the process environment. Empty values count as unset.
    pub fn from_process() -> Self {
        fn var(name: &str) -> Option<String> {
            std::env::var(name).ok().filter(|v| !v.is_empty())
        }
        Env {
            product: var("PRODUCT"),
            model_number: var("MODEL_NUMBER"),
            interface: var("INTERFACE"),
            wifi_module: var("WIFI_MODULE"),
            platform: var("PLATFORM"),
            saved_warmth: None,
            home: var("HOME").map(PathBuf::from),
            xdg_data_home: var("XDG_DATA_HOME").map(PathBuf::from),
        }
    }
}

/// The device services, bundled. Fields are public so the app can pick the parts it needs.
pub struct Platform {
    /// Kobo model or desktop.
    pub kind: Kind,
    /// Install, data and books locations.
    pub paths: Paths,
    /// Model, firmware and screen.
    pub device: DeviceInfo,
    /// Battery state.
    pub battery: Box<dyn Battery>,
    /// Frontlight and warmth.
    pub frontlight: Box<dyn Frontlight>,
    /// Wi-Fi.
    pub network: Box<dyn Network>,
    /// Suspend, resume, standby and wakeup alarms. Plans only; run them with [`Platform::run_plan`].
    pub power: PowerManager,
    io: SystemIo,
}

impl Platform {
    /// Detects the platform on this machine.
    pub fn detect() -> Result<Self, PlatformError> {
        Self::detect_in(Path::new("/"), &Env::from_process())
    }

    /// Detects the platform with the device tree at `root` and the given environment.
    /// A Kobo is recognised by [`KOBO_MARKER`] under `root`. Everything else is a desktop.
    pub fn detect_in(root: &Path, env: &Env) -> Result<Self, PlatformError> {
        let root = Root::new(root);
        if root.exists(KOBO_MARKER) {
            Self::kobo(root, env)
        } else {
            Self::desktop(env)
        }
    }

    fn kobo(root: Root, env: &Env) -> Result<Self, PlatformError> {
        let mut io = SystemIo::new(root.clone());
        let codename =
            device::resolve_codename(env.product.as_deref(), &mut io).ok_or(PlatformError::UnknownModel)?;
        let model = Model::from_codename(&codename)
            .ok_or_else(|| PlatformError::UnsupportedModel(codename.clone()))?;
        let version = device::read_version_line(&root);
        let product_id = env
            .model_number
            .clone()
            .unwrap_or_else(|| device::product_id_from_version(version.as_deref()));
        let firmware = version.as_deref().and_then(device::firmware_from_version);
        let device = DeviceInfo::libra_colour(codename, product_id, firmware);

        let paths = Paths::kobo(&root);
        let iface = env.interface.clone().unwrap_or_else(|| "eth0".to_string());
        let mut script_env = vec![("INTERFACE".to_string(), iface.clone())];
        if let Some(module) = &env.wifi_module {
            script_env.push(("WIFI_MODULE".to_string(), module.clone()));
        }
        if let Some(platform) = &env.platform {
            script_env.push(("PLATFORM".to_string(), platform.clone()));
        }

        let frontlight =
            SysfsFrontlight::open(FrontlightConfig::monza(), io.clone(), &root, env.saved_warmth);
        let power = PowerManager::kobo(&root, paths.install_dir.clone(), script_env.clone(), true);
        let network = KoboNetwork::new(
            root.clone(),
            io.clone(),
            paths.install_dir.clone(),
            &iface,
            script_env,
        );

        Ok(Platform {
            kind: Kind::Kobo(model),
            paths,
            device,
            battery: Box::new(KoboBattery::new(root, MONZA_BATTERY_DIR)),
            frontlight: Box::new(frontlight),
            network: Box::new(network),
            power,
            io,
        })
    }

    fn desktop(env: &Env) -> Result<Self, PlatformError> {
        let home = env.home.as_deref().ok_or(PlatformError::NoHome)?;
        let paths = Paths::desktop(home, env.xdg_data_home.as_deref(), cfg!(target_os = "macos"));
        Ok(Platform {
            kind: Kind::Desktop,
            paths,
            device: DeviceInfo::desktop(1264, 1680, 300),
            battery: Box::new(NoBattery),
            frontlight: Box::new(frontlight::desktop_frontlight()),
            network: Box::new(DesktopNetwork),
            power: PowerManager::none(),
            io: SystemIo::new(Root::new("/")),
        })
    }

    /// No verified MT8113T page-button wake node is known yet.
    pub fn supports_page_button_wake(&self) -> bool {
        false
    }

    /// Runs a plan on this device. Sleeps block the calling thread.
    pub fn run_plan(&mut self, plan: &Plan) -> Outcome {
        execute(plan, &mut self.io, &mut |d| std::thread::sleep(d))
    }

    /// A copy of the device I/O for a worker thread. It writes under the same root
    /// as this platform, so a worker on a fake device tree never touches the host.
    pub fn io_handle(&self) -> SystemIo {
        self.io.clone()
    }
}
