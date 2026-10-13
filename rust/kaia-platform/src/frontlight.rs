//! Frontlight intensity and warmth for the Libra Colour.
//!
//! The Libra Colour (codename `monza`) has a white LED on `mxc_msp430.0`
//! and a colour mixer on `lm3630a_led`. Intensity is written to the white
//! LED. Warmth is written to the mixer, inverted. KOReader's values:
//! - profile: `frontend/device/kobo/device.lua:532-553`
//! - natural light scale (`nl_min` 0, `nl_max` 10, `nl_inverted`): `device.lua:549-551`
//! - warmth scale 0 to 100 mapped to 0 to `nl_max`: `frontend/device/generic/powerd.lua:226-250`
//! - mixer write `nl_max - warmth` when inverted: `frontend/device/sysfs_light.lua:79-86`
//! - ramp delay 25 ms for `lm3630a_led` on MTK: `frontend/device/kobo/device.lua:811-817` and `powerd.lua:138-144`
//! - ramp down and up: `frontend/device/kobo/powerd.lua:352-429`
//! - default intensity 20 when nothing is stored: `frontend/device/kobo/powerd.lua:132`
//!
//! KOReader prefers an ioctl (241 on `/dev/ntx_io`, `base/ffi/kobolight.lua:31`)
//! for intensity. Kaia writes the sysfs file instead, the path KOReader falls
//! back to (`sysfs_light.lua:76`). This crate contains no `unsafe`, so the
//! ioctl is not ported. Unverified on hardware.

use std::io;
use std::time::Duration;

use crate::error::PlatformError;
use crate::sysfs::{Root, SysfsWriter};

/// Ramp delay for the `lm3630a_led` mixer on MTK (`device.lua:811-817`).
pub const RAMP_DELAY_LM3630: Duration = Duration::from_millis(25);

/// Paths and scales for one frontlight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontlightConfig {
    /// White LED brightness file (`device.lua:547`).
    pub intensity_path: String,
    /// Colour mixer file (`device.lua:548`).
    pub mixer_path: String,
    /// Largest native warmth value (`device.lua:550`).
    pub nl_max: u8,
    /// Whether the mixer counts from cold to warm (`device.lua:551`).
    pub nl_inverted: bool,
    /// Pause between ramp steps.
    pub ramp_delay: Duration,
    /// Intensity used after a start with the light off (`powerd.lua:132`).
    pub default_intensity: u8,
}

impl FrontlightConfig {
    /// The Libra Colour (`KoboMonza`, `device.lua:532-553`).
    pub fn monza() -> Self {
        FrontlightConfig {
            intensity_path: "/sys/class/backlight/mxc_msp430.0/brightness".to_string(),
            mixer_path: "/sys/class/backlight/lm3630a_led/color".to_string(),
            nl_max: 10,
            nl_inverted: true,
            ramp_delay: RAMP_DELAY_LM3630,
            default_intensity: 20,
        }
    }
}

/// Intensity values written by a ramp down from `from` to 0.
///
/// Each value is `floor(previous * 0.75)`, never below 0, and the last value
/// is always 0 (`powerd.lua:352-363`). A ramp from 100 writes
/// `75, 56, 42, 31, 23, 17, 12, 9, 6, 4, 3, 2, 1, 0`.
pub fn ramp_down_values(from: u8) -> Vec<u8> {
    let mut values = Vec::new();
    let mut current = (f64::from(from) * 0.75).floor() as u8;
    while current > 0 {
        values.push(current);
        current = (f64::from(current) * 0.75).floor() as u8;
    }
    values.push(0);
    values
}

/// Intensity values written by a ramp up from 0 to `to`.
///
/// Each value is `ceil(previous * 1.5)` (1 after 0), capped at 100, and the
/// last value is always `to` (`powerd.lua:415-429`). A ramp to 20 writes
/// `1, 2, 3, 5, 8, 12, 18, 20`.
pub fn ramp_up_values(to: u8) -> Vec<u8> {
    let mut values = Vec::new();
    let mut current: u8 = 0;
    loop {
        let next = if current == 0 {
            1
        } else {
            (f64::from(current) * 1.5).ceil().min(100.0) as u8
        };
        if next >= to {
            values.push(to);
            return values;
        }
        values.push(next);
        current = next;
    }
}

/// Frontlight control in the Kaia shape.
pub trait Frontlight {
    /// Whether the light is on.
    fn is_on(&self) -> bool;
    /// Intensity 0 to 100. Returns 0 while the light is off, as KOReader's `frontlightIntensity` does.
    fn intensity(&self) -> u8;
    /// Sets intensity. Values above 100 are clamped. 0 turns the light off.
    fn set_intensity(&mut self, value: u8) -> Result<(), PlatformError>;
    /// Warmth 0 (cold) to 100 (warm).
    fn warmth(&self) -> u8;
    /// Sets warmth. Values above 100 are clamped.
    fn set_warmth(&mut self, value: u8) -> Result<(), PlatformError>;
    /// Ramps the light off, or back on to the last intensity.
    fn toggle(&mut self) -> Result<(), PlatformError>;
}

/// A writer that discards every write. Used on desktop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NullWriter;

impl SysfsWriter for NullWriter {
    fn write(&mut self, _device_path: &str, _value: &str) -> io::Result<()> {
        Ok(())
    }
}

/// Frontlight state machine that writes through a [`SysfsWriter`].
///
/// The Libra Colour uses `SysfsWriter = SystemIo`. Desktop uses [`NullWriter`]
/// and keeps the state in memory.
#[derive(Debug)]
pub struct SysfsFrontlight<W: SysfsWriter> {
    cfg: FrontlightConfig,
    writer: W,
    on: bool,
    /// The last set intensity, which KOReader keeps as `fl_intensity`.
    intensity: u8,
    /// The intensity the hardware currently shows, `hw_intensity` in KOReader.
    hw: u8,
    warmth: u8,
}

impl<W: SysfsWriter> SysfsFrontlight<W> {
    /// Starts from known values without touching the hardware.
    pub fn new(cfg: FrontlightConfig, writer: W, hw_intensity: u8, warmth: u8) -> Self {
        let on = hw_intensity > 0;
        let intensity = if on { hw_intensity } else { cfg.default_intensity };
        SysfsFrontlight {
            cfg,
            writer,
            on,
            intensity,
            hw: hw_intensity,
            warmth: warmth.min(100),
        }
    }

    /// Reads the current brightness and mixer values under `root`. A file that
    /// cannot be read counts as 0.
    ///
    /// The mixer value is read back. KOReader does not read it (`powerd.lua:279-281`
    /// keeps warmth in memory), so this read is unverified on hardware.
    pub fn open(cfg: FrontlightConfig, writer: W, root: &Root) -> Self {
        let hw = root
            .read_int(&cfg.intensity_path)
            .and_then(|v| u8::try_from(v).ok())
            .map(|v| v.min(100))
            .unwrap_or(0);
        let warmth = root
            .read_int(&cfg.mixer_path)
            .and_then(|raw| {
                let raw = u8::try_from(raw).ok()?;
                let native = if cfg.nl_inverted {
                    cfg.nl_max.checked_sub(raw)?
                } else {
                    raw
                };
                Some(warmth_from_native(native, cfg.nl_max))
            })
            .unwrap_or(0);
        Self::new(cfg, writer, hw, warmth)
    }

    fn write_intensity(&mut self, value: u8) -> Result<(), PlatformError> {
        self.writer
            .write(&self.cfg.intensity_path, &value.to_string())
            .map_err(|e| PlatformError::io(self.cfg.intensity_path.clone(), e))
    }

    /// Ramp steps for a toggle, run with the configured delay between writes.
    fn ramp(&mut self, values: &[u8]) -> Result<(), PlatformError> {
        for value in values {
            self.write_intensity(*value)?;
            if !self.cfg.ramp_delay.is_zero() {
                std::thread::sleep(self.cfg.ramp_delay);
            }
        }
        Ok(())
    }
}

/// Maps a native warmth value to the 0 to 100 scale. `powerd.lua:231-237` (`fromNativeWarmth`).
fn warmth_from_native(native: u8, nl_max: u8) -> u8 {
    let scaled = u32::from(native) * 100 / u32::from(nl_max.max(1));
    scaled.min(100) as u8
}

/// Maps a 0 to 100 warmth to the native scale, rounding half up. `powerd.lua:231-233`.
fn warmth_to_native(warmth: u8, nl_max: u8) -> u8 {
    let native = (u32::from(warmth) * u32::from(nl_max) + 50) / 100;
    native.min(u32::from(nl_max)) as u8
}

impl<W: SysfsWriter> Frontlight for SysfsFrontlight<W> {
    fn is_on(&self) -> bool {
        self.on
    }

    fn intensity(&self) -> u8 {
        if self.on {
            self.intensity
        } else {
            0
        }
    }

    /// Writes the white LED (`powerd.lua:256-270`). Intensity 0 turns the light off.
    fn set_intensity(&mut self, value: u8) -> Result<(), PlatformError> {
        let value = value.min(100);
        self.write_intensity(value)?;
        self.intensity = value;
        self.hw = value;
        self.on = value > 0;
        Ok(())
    }

    /// Writes the mixer (`sysfs_light.lua:79-86`). KOReader writes it even when the light is off.
    fn set_warmth(&mut self, value: u8) -> Result<(), PlatformError> {
        let value = value.min(100);
        let native = warmth_to_native(value, self.cfg.nl_max);
        let written = if self.cfg.nl_inverted {
            self.cfg.nl_max - native
        } else {
            native
        };
        self.writer
            .write(&self.cfg.mixer_path, &written.to_string())
            .map_err(|e| PlatformError::io(self.cfg.mixer_path.clone(), e))?;
        self.warmth = value;
        Ok(())
    }

    fn warmth(&self) -> u8 {
        self.warmth
    }

    /// Ramps down when on, and ramps up to the last intensity when off
    /// (`powerd.lua:365-466`). Turning on with intensity 0 does nothing.
    fn toggle(&mut self) -> Result<(), PlatformError> {
        if self.on {
            if self.hw > 0 {
                let values = ramp_down_values(self.hw);
                self.ramp(&values)?;
            }
            self.hw = 0;
            self.on = false;
        } else if self.intensity > 0 {
            let values = ramp_up_values(self.intensity);
            self.ramp(&values)?;
            self.hw = self.intensity;
            self.on = true;
        }
        Ok(())
    }
}

/// Desktop frontlight: state in memory, no hardware.
pub fn desktop_frontlight() -> SysfsFrontlight<NullWriter> {
    let cfg = FrontlightConfig {
        intensity_path: String::new(),
        mixer_path: String::new(),
        nl_max: 100,
        nl_inverted: false,
        ramp_delay: Duration::ZERO,
        default_intensity: 20,
    };
    SysfsFrontlight::new(cfg, NullWriter, 0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_down_from_100_matches_powerd_lua() {
        assert_eq!(
            ramp_down_values(100),
            vec![75, 56, 42, 31, 23, 17, 12, 9, 6, 4, 3, 2, 1, 0]
        );
    }

    #[test]
    fn ramp_down_from_zero_is_just_zero() {
        assert_eq!(ramp_down_values(0), vec![0]);
    }

    #[test]
    fn ramp_up_to_20_matches_powerd_lua() {
        assert_eq!(ramp_up_values(20), vec![1, 2, 3, 5, 8, 12, 18, 20]);
    }

    #[test]
    fn ramp_up_to_one_is_one_step() {
        assert_eq!(ramp_up_values(1), vec![1]);
    }

    #[test]
    fn warmth_mapping_matches_generic_powerd() {
        // toNativeWarmth: Math.round(v / 10). Half rounds up.
        assert_eq!(warmth_to_native(0, 10), 0);
        assert_eq!(warmth_to_native(4, 10), 0);
        assert_eq!(warmth_to_native(5, 10), 1);
        assert_eq!(warmth_to_native(55, 10), 6);
        assert_eq!(warmth_to_native(100, 10), 10);
        // fromNativeWarmth: native * 10.
        assert_eq!(warmth_from_native(0, 10), 0);
        assert_eq!(warmth_from_native(7, 10), 70);
        assert_eq!(warmth_from_native(10, 10), 100);
    }
}
