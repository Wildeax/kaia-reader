//! Battery state.
//!
//! The Kobo reads `capacity` and `status` in the battery's sysfs directory.
//! KOReader's paths are in `frontend/device/kobo/powerd.lua:95-96` (the
//! `batt_capacity_file` and `is_charging_file` settings) and the directory
//! itself is `frontend/device/kobo/device.lua:553` (`battery_sysfs` for KoboMonza).

use crate::sysfs::Root;

/// Battery queries. `None` means the value could not be read.
pub trait Battery {
    /// Charge in percent, 0 to 100.
    fn capacity(&self) -> Option<u8>;
    /// Whether the charger is connected. The status file reads `Discharging` when it is not.
    /// `None` only on a platform without a battery.
    fn is_charging(&self) -> Option<bool>;
    /// Whether the battery is full on a charger.
    fn is_charged(&self) -> Option<bool>;
}

/// The battery directory of the Libra Colour: `frontend/device/kobo/device.lua:553`.
pub const MONZA_BATTERY_DIR: &str = "/sys/class/power_supply/bd71827_bat";

/// Battery state from the Kobo sysfs tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KoboBattery {
    root: Root,
    capacity_path: String,
    status_path: String,
}

impl KoboBattery {
    /// Reads `capacity` and `status` in `dir` under `root`.
    pub fn new(root: Root, dir: &str) -> Self {
        KoboBattery {
            root,
            capacity_path: format!("{dir}/capacity"),
            status_path: format!("{dir}/status"),
        }
    }

    fn status(&self) -> Option<String> {
        self.root.read_line(&self.status_path)
    }
}

impl Battery for KoboBattery {
    /// Reads `capacity`. Values outside 0 to 100 count as unreadable.
    fn capacity(&self) -> Option<u8> {
        let value = self.root.read_int(&self.capacity_path)?;
        u8::try_from(value).ok().filter(|c| *c <= 100)
    }

    /// `status` other than `Discharging` means the charger is connected. A
    /// status file that cannot be read counts as charging too, as
    /// `KoboPowerD:isChargingHW` does (`powerd.lua:299-301`, where the nil
    /// from `read_str_file` is not equal to `Discharging`). That is the safe
    /// side on MTK, where suspending while plugged in hangs the kernel.
    fn is_charging(&self) -> Option<bool> {
        Some(self.status().as_deref() != Some("Discharging"))
    }

    /// `Full` means charged. `Not charging` at 100 percent also means charged
    /// (`powerd.lua:303-315`).
    fn is_charged(&self) -> Option<bool> {
        let status = self.status()?;
        Some(match status.as_str() {
            "Full" => true,
            "Not charging" => self.capacity() == Some(100),
            _ => false,
        })
    }
}

/// A battery that reports nothing. Used on desktop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoBattery;

impl Battery for NoBattery {
    fn capacity(&self) -> Option<u8> {
        None
    }

    fn is_charging(&self) -> Option<bool> {
        None
    }

    fn is_charged(&self) -> Option<bool> {
        None
    }
}
