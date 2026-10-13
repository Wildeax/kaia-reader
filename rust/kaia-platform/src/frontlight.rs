//! Frontlight intensity and warmth for the Libra Colour.
//!
//! The Libra Colour (codename `monza`) has a white LED on `mxc_msp430.0`
//! and a colour mixer on `lm3630a_led`. Intensity is written to the white
//! LED. Warmth is written to the mixer, inverted. KOReader's values:
//! - profile: `frontend/device/kobo/device.lua:532-553`
//! - natural light scale (`nl_min` 0, `nl_max` 10, `nl_inverted`): `device.lua:549-551`
//! - warmth scale 0 to 100 mapped to 0 to `nl_max`: `frontend/device/generic/powerd.lua:226-250`
//! - mixer write `nl_max - warmth` when inverted: `frontend/device/sysfs_light.lua:79-86`
//! - ramp delay 25 ms and `delay_ramp_start` for `lm3630a_led` on MTK: `frontend/device/kobo/device.lua:811-817` and `powerd.lua:138-144`
//! - ramp down and up: `frontend/device/kobo/powerd.lua:352-429`
//! - default intensity 20 when nothing is stored: `frontend/device/kobo/powerd.lua:132`
//!
//! # Brightness path
//!
//! KOReader writes brightness with ioctl 241 on `/dev/ntx_io` when the device
//! has a mixer and the node opens, because it is much faster than sysfs
//! (`powerd.lua:160-166`, `sysfs_light.lua:73-77`, `base/ffi/kobolight.lua`).
//! [`SysfsFrontlight::open`] probes the node through [`SysfsWriter::ntx_available`].
//! When it opens, every intensity write goes to the ioctl first. A failed ioctl
//! switches to the sysfs file for good. Warmth always goes through the mixer
//! file. Unverified on hardware: whether ioctl 241 behaves the same on the
//! Libra Colour's MTK kernel is a device check.
//!
//! # Ramps
//!
//! A toggle is a [`Ramp`]: a list of timed writes. Nothing here sleeps.
//! [`Frontlight::begin_toggle`] returns the ramp, and the caller schedules each
//! [`RampStep`] on its event loop and calls [`Frontlight::apply_ramp_step`], as
//! KOReader's `UIManager:scheduleIn` chain does (`powerd.lua:352-466`).
//!
//! # Warmth at startup
//!
//! KOReader does not read the mixer back. It keeps the saved warmth in
//! memory and writes it at startup (`powerd.lua:186-191`). [`SysfsFrontlight::open`]
//! takes the saved value from the caller and does the same.

use std::io;
use std::time::Duration;

use crate::error::PlatformError;
use crate::sysfs::{Root, SysfsWriter};

/// Ramp delay for the `lm3630a_led` mixer on MTK (`device.lua:811-817`).
pub const RAMP_DELAY_LM3630: Duration = Duration::from_millis(25);

/// The `ntx_io` character device (`base/ffi/kobolight.lua:44`).
pub const NTX_IO: &str = "/dev/ntx_io";

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
    /// Start a ramp on the next UI frame instead of at once, so it does not
    /// interleave with screen refreshes (`delay_ramp_start`, `device.lua:811-817`,
    /// `powerd.lua:383`, `451`). Set for the `lm3630a_led` mixer on MTK.
    pub delay_ramp_start: bool,
    /// The `ntx_io` device node for the brightness ioctl, or `None` to use only sysfs.
    pub ntx_io_path: Option<String>,
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
            delay_ramp_start: true,
            ntx_io_path: Some(NTX_IO.to_string()),
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

/// Which way a ramp goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RampDirection {
    /// From the current intensity down to 0.
    Off,
    /// From 0 up to the saved intensity.
    On,
}

/// One timed write of a ramp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RampStep {
    /// How long to wait after the previous step before this one. The first step
    /// has no delay, and the caller starts it when the ramp starts.
    pub delay: Duration,
    /// The intensity to write, 0 to 100.
    pub intensity: u8,
    /// Whether this is the final step. After it, [`Frontlight::is_on`] changes.
    pub last: bool,
}

/// A frontlight toggle as a list of timed writes. Nothing in it sleeps; the
/// caller schedules the steps (see the module documentation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ramp {
    /// Which way it goes.
    pub direction: RampDirection,
    /// Wait for the next UI frame before the first step (`delay_ramp_start`).
    pub defer_start: bool,
    /// The writes, in order. Never empty.
    pub steps: Vec<RampStep>,
}

impl Ramp {
    /// The time from the first step to the last, without the deferred start.
    pub fn duration(&self) -> Duration {
        self.steps.iter().map(|s| s.delay).sum()
    }

    /// Runs the whole ramp in the calling thread, calling `wait` for every delay.
    /// Stops at the first write error. This is for tests and for callers that have
    /// no event loop; an app with one schedules the steps instead.
    pub fn run<F>(&self, light: &mut F, wait: &mut dyn FnMut(Duration)) -> Result<(), PlatformError>
    where
        F: Frontlight + ?Sized,
    {
        for step in &self.steps {
            if !step.delay.is_zero() {
                wait(step.delay);
            }
            if let Err(e) = light.apply_ramp_step(step) {
                light.cancel_ramp();
                return Err(e);
            }
        }
        Ok(())
    }
}

/// Frontlight control in the Kaia shape.
pub trait Frontlight {
    /// Whether the light is on.
    fn is_on(&self) -> bool;
    /// Intensity 0 to 100. Returns 0 while the light is off, as KOReader's `frontlightIntensity` does.
    fn intensity(&self) -> u8;
    /// Sets intensity. Values above 100 are clamped. 0 turns the light off.
    /// A ramp in progress is cancelled (`setIntensityHW` stops it in KOReader).
    fn set_intensity(&mut self, value: u8) -> Result<(), PlatformError>;
    /// Warmth 0 (cold) to 100 (warm).
    fn warmth(&self) -> u8;
    /// Sets warmth. Values above 100 are clamped.
    fn set_warmth(&mut self, value: u8) -> Result<(), PlatformError>;
    /// Starts a toggle: a ramp off when the light is on, a ramp up to the saved
    /// intensity when it is off. Returns the ramp for the caller to schedule, or
    /// `None` when nothing is left to do (toggling on at intensity 0, or a light
    /// that is already at 0). Toggling during a ramp reverses it from the current level.
    fn begin_toggle(&mut self) -> Option<Ramp>;
    /// Writes one step of the ramp from [`Frontlight::begin_toggle`]. The final
    /// step sets [`Frontlight::is_on`].
    fn apply_ramp_step(&mut self, step: &RampStep) -> Result<(), PlatformError>;
    /// Forgets the ramp in progress, as `_stopFrontlightRamp` does. The caller drops
    /// its scheduled steps. The light stays at the last written level.
    fn cancel_ramp(&mut self);
    /// Whether a ramp has started and not finished.
    fn is_ramping(&self) -> bool;
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
    /// The ramp in progress, if any.
    ramping: Option<RampDirection>,
    /// Whether intensity goes through the `ntx_io` ioctl.
    use_ioctl: bool,
}

impl<W: SysfsWriter> SysfsFrontlight<W> {
    /// Starts from known values without touching the hardware. Intensity uses
    /// the sysfs file; [`SysfsFrontlight::open`] turns the ioctl on.
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
            ramping: None,
            use_ioctl: false,
        }
    }

    /// Reads the current brightness under `root`, probes the `ntx_io` node, and
    /// writes `saved_warmth` to the mixer when given. A brightness file that
    /// cannot be read counts as 0.
    ///
    /// The mixer is not read back. The caller holds the saved warmth, as
    /// KOReader's settings do (`powerd.lua:186-191`, `setWarmth(self.fl_warmth, true)`).
    /// With `None` the warmth reads 0 and the mixer is left alone until the
    /// caller calls [`Frontlight::set_warmth`]. KOReader writes the saved value
    /// only when the light is on at startup. Kaia writes it in both cases so the
    /// mixer is right when the light comes on later. A failed write keeps the
    /// value in memory.
    pub fn open(cfg: FrontlightConfig, writer: W, root: &Root, saved_warmth: Option<u8>) -> Self {
        let hw = root
            .read_int(&cfg.intensity_path)
            .and_then(|v| u8::try_from(v).ok())
            .map(|v| v.min(100))
            .unwrap_or(0);
        let mut light = Self::new(cfg, writer, hw, saved_warmth.unwrap_or(0));
        if let Some(path) = light.cfg.ntx_io_path.clone() {
            light.use_ioctl = light.writer.ntx_available(&path);
        }
        if let Some(warmth) = saved_warmth {
            let _ = light.set_warmth(warmth);
        }
        light
    }

    /// Whether intensity writes currently go through the `ntx_io` ioctl.
    pub fn uses_ioctl(&self) -> bool {
        self.use_ioctl
    }

    /// The writer, for callers that need to inspect it (tests).
    pub fn writer(&self) -> &W {
        &self.writer
    }

    fn write_intensity(&mut self, value: u8) -> Result<(), PlatformError> {
        if self.use_ioctl {
            if let Some(path) = self.cfg.ntx_io_path.clone() {
                if self.writer.ntx_set_brightness(&path, value).is_ok() {
                    return Ok(());
                }
            }
            // The ioctl failed. KOReader would assert; Kaia falls back to the file
            // KOReader uses without the ioctl and stays there.
            self.use_ioctl = false;
        }
        self.writer
            .write(&self.cfg.intensity_path, &value.to_string())
            .map_err(|e| PlatformError::io(self.cfg.intensity_path.clone(), e))
    }

    fn ramp(&self, direction: RampDirection, values: Vec<u8>) -> Ramp {
        let count = values.len();
        let steps = values
            .into_iter()
            .enumerate()
            .map(|(i, intensity)| RampStep {
                delay: if i == 0 {
                    Duration::ZERO
                } else {
                    self.cfg.ramp_delay
                },
                intensity,
                last: i + 1 == count,
            })
            .collect();
        Ramp {
            direction,
            defer_start: self.cfg.delay_ramp_start,
            steps,
        }
    }
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
        self.ramping = None;
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

    /// The ramps of `powerd.lua:365-466`. Turning on with intensity 0 does nothing.
    fn begin_toggle(&mut self) -> Option<Ramp> {
        let direction = match self.ramping {
            Some(RampDirection::Off) => RampDirection::On,
            Some(RampDirection::On) => RampDirection::Off,
            None if self.on => RampDirection::Off,
            None => RampDirection::On,
        };
        match direction {
            RampDirection::Off => {
                if self.hw == 0 {
                    self.on = false;
                    self.ramping = None;
                    return None;
                }
                self.ramping = Some(direction);
                Some(self.ramp(direction, ramp_down_values(self.hw)))
            }
            RampDirection::On => {
                if self.intensity == 0 {
                    self.ramping = None;
                    return None;
                }
                // A reversed ramp starts above the level it is at now.
                let hw = self.hw;
                let values: Vec<u8> = ramp_up_values(self.intensity)
                    .into_iter()
                    .filter(|&v| v > hw)
                    .collect();
                if values.is_empty() {
                    self.ramping = None;
                    self.on = true;
                    return None;
                }
                self.ramping = Some(direction);
                Some(self.ramp(direction, values))
            }
        }
    }

    fn apply_ramp_step(&mut self, step: &RampStep) -> Result<(), PlatformError> {
        self.write_intensity(step.intensity)?;
        self.hw = step.intensity;
        if step.last {
            if let Some(direction) = self.ramping.take() {
                self.on = direction == RampDirection::On;
            }
        }
        Ok(())
    }

    fn cancel_ramp(&mut self) {
        self.ramping = None;
    }

    fn is_ramping(&self) -> bool {
        self.ramping.is_some()
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
        delay_ramp_start: false,
        ntx_io_path: None,
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
    }

    /// Records every call so tests can check the order and the path.
    #[derive(Default)]
    struct Rec {
        writes: Vec<(String, String)>,
        ioctls: Vec<(String, u8)>,
        ntx: bool,
        ntx_fails: bool,
    }

    impl SysfsWriter for Rec {
        fn write(&mut self, device_path: &str, value: &str) -> io::Result<()> {
            self.writes.push((device_path.to_string(), value.to_string()));
            Ok(())
        }

        fn ntx_available(&mut self, _device_path: &str) -> bool {
            self.ntx
        }

        fn ntx_set_brightness(&mut self, device_path: &str, value: u8) -> io::Result<()> {
            if self.ntx_fails {
                return Err(io::Error::other("ioctl refused"));
            }
            self.ioctls.push((device_path.to_string(), value));
            Ok(())
        }
    }

    const BRIGHTNESS: &str = "/sys/class/backlight/mxc_msp430.0/brightness";
    const MIXER: &str = "/sys/class/backlight/lm3630a_led/color";

    fn tree(brightness: &str, mixer: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, text) in [(BRIGHTNESS, brightness), (MIXER, mixer)] {
            let full = dir.path().join(path.trim_start_matches('/'));
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, text).unwrap();
        }
        dir
    }

    fn open(rec: Rec, brightness: &str, saved: Option<u8>) -> SysfsFrontlight<Rec> {
        let dir = tree(brightness, "9\n");
        SysfsFrontlight::open(FrontlightConfig::monza(), rec, &Root::new(dir.path()), saved)
    }

    #[test]
    fn open_does_not_read_warmth_back_from_the_mixer() {
        // The mixer file says 9. With no saved value, warmth stays 0 and nothing is written.
        let fl = open(Rec::default(), "20\n", None);
        assert_eq!(fl.warmth(), 0);
        assert!(fl.writer().writes.is_empty());
        assert_eq!(fl.intensity(), 20);
    }

    #[test]
    fn open_writes_the_saved_warmth_to_the_mixer() {
        // 70 maps to native 7, written inverted as 10 - 7.
        let fl = open(Rec::default(), "20\n", Some(70));
        assert_eq!(fl.warmth(), 70);
        assert_eq!(fl.writer().writes, vec![(MIXER.to_string(), "3".to_string())]);
    }

    #[test]
    fn open_writes_the_saved_warmth_with_the_light_off_too() {
        let fl = open(Rec::default(), "0\n", Some(100));
        assert!(!fl.is_on());
        assert_eq!(fl.writer().writes, vec![(MIXER.to_string(), "0".to_string())]);
    }

    #[test]
    fn brightness_goes_through_the_ioctl_when_ntx_io_opens() {
        let rec = Rec {
            ntx: true,
            ..Rec::default()
        };
        let mut fl = open(rec, "0\n", None);
        assert!(fl.uses_ioctl());
        fl.set_intensity(45).unwrap();
        assert_eq!(fl.writer().ioctls, vec![(NTX_IO.to_string(), 45)]);
        assert!(fl.writer().writes.is_empty());
        // Warmth never uses the ioctl.
        fl.set_warmth(50).unwrap();
        assert_eq!(fl.writer().writes, vec![(MIXER.to_string(), "5".to_string())]);
    }

    #[test]
    fn brightness_uses_sysfs_when_ntx_io_does_not_open() {
        let mut fl = open(Rec::default(), "0\n", None);
        assert!(!fl.uses_ioctl());
        fl.set_intensity(45).unwrap();
        assert!(fl.writer().ioctls.is_empty());
        assert_eq!(
            fl.writer().writes,
            vec![(BRIGHTNESS.to_string(), "45".to_string())]
        );
    }

    #[test]
    fn a_failing_ioctl_falls_back_to_sysfs_for_good() {
        let rec = Rec {
            ntx: true,
            ntx_fails: true,
            ..Rec::default()
        };
        let mut fl = open(rec, "0\n", None);
        fl.set_intensity(30).unwrap();
        assert!(!fl.uses_ioctl());
        fl.set_intensity(31).unwrap();
        assert_eq!(
            fl.writer().writes,
            vec![
                (BRIGHTNESS.to_string(), "30".to_string()),
                (BRIGHTNESS.to_string(), "31".to_string()),
            ]
        );
    }

    #[test]
    fn toggle_off_is_a_list_of_timed_steps_and_writes_nothing_yet() {
        let mut fl = open(Rec::default(), "100\n", None);
        let ramp = fl.begin_toggle().unwrap();
        assert!(fl.writer().writes.is_empty(), "begin_toggle must not write");
        assert_eq!(ramp.direction, RampDirection::Off);
        assert!(ramp.defer_start, "lm3630a on MTK delays the ramp start");
        let values: Vec<u8> = ramp.steps.iter().map(|s| s.intensity).collect();
        assert_eq!(values, ramp_down_values(100));
        assert_eq!(ramp.steps[0].delay, Duration::ZERO);
        assert!(ramp.steps[1..].iter().all(|s| s.delay == RAMP_DELAY_LM3630));
        assert_eq!(ramp.steps.iter().filter(|s| s.last).count(), 1);
        assert!(ramp.steps.last().unwrap().last);
        assert_eq!(ramp.duration(), RAMP_DELAY_LM3630 * (values.len() as u32 - 1));
        // Nothing changed until the steps run.
        assert!(fl.is_on());
        assert!(fl.is_ramping());
    }

    #[test]
    fn running_the_ramp_writes_every_step_and_flips_the_state() {
        let mut fl = open(Rec::default(), "100\n", None);
        let ramp = fl.begin_toggle().unwrap();
        let mut waits = Vec::new();
        ramp.run(&mut fl, &mut |d| waits.push(d)).unwrap();
        assert!(!fl.is_on());
        assert!(!fl.is_ramping());
        assert_eq!(fl.intensity(), 0);
        assert_eq!(waits.len(), ramp.steps.len() - 1);
        let written: Vec<&str> = fl.writer().writes.iter().map(|(_, v)| v.as_str()).collect();
        assert_eq!(written.last(), Some(&"0"));
        assert_eq!(written.len(), ramp.steps.len());

        let up = fl.begin_toggle().unwrap();
        assert_eq!(up.direction, RampDirection::On);
        up.run(&mut fl, &mut |_| {}).unwrap();
        assert!(fl.is_on());
        assert_eq!(fl.intensity(), 100);
    }

    #[test]
    fn toggle_on_ramps_up_to_the_saved_intensity() {
        let mut fl = open(Rec::default(), "0\n", None);
        let ramp = fl.begin_toggle().unwrap();
        let values: Vec<u8> = ramp.steps.iter().map(|s| s.intensity).collect();
        assert_eq!(values, vec![1, 2, 3, 5, 8, 12, 18, 20]);
    }

    #[test]
    fn toggle_during_a_ramp_reverses_it_from_the_current_level() {
        let mut fl = open(Rec::default(), "100\n", None);
        let down = fl.begin_toggle().unwrap();
        // Three steps ran: 75, 56, 42.
        for step in &down.steps[..3] {
            fl.apply_ramp_step(step).unwrap();
        }
        assert!(fl.is_on(), "the light counts as on until the ramp ends");
        let up = fl.begin_toggle().unwrap();
        assert_eq!(up.direction, RampDirection::On);
        // Every step is above the 42 the light is at, ending at the saved 100.
        assert!(up.steps.iter().all(|s| s.intensity > 42));
        assert_eq!(up.steps.last().unwrap().intensity, 100);
    }

    #[test]
    fn cancel_ramp_keeps_the_level_and_the_state() {
        let mut fl = open(Rec::default(), "100\n", None);
        let down = fl.begin_toggle().unwrap();
        fl.apply_ramp_step(&down.steps[0]).unwrap();
        fl.cancel_ramp();
        assert!(!fl.is_ramping());
        assert!(fl.is_on());
        // The next toggle starts a new ramp down from the 75 that was written.
        let again = fl.begin_toggle().unwrap();
        assert_eq!(again.direction, RampDirection::Off);
        assert_eq!(again.steps[0].intensity, 56);
    }

    #[test]
    fn set_intensity_cancels_a_ramp() {
        let mut fl = open(Rec::default(), "100\n", None);
        fl.begin_toggle().unwrap();
        fl.set_intensity(40).unwrap();
        assert!(!fl.is_ramping());
    }

    #[test]
    fn toggle_with_nothing_to_do_returns_none() {
        let mut fl = open(Rec::default(), "30\n", None);
        fl.set_intensity(0).unwrap();
        assert!(fl.begin_toggle().is_none());
        assert!(!fl.is_on());
    }

    #[test]
    fn a_failed_step_cancels_the_ramp() {
        struct Failing;
        impl SysfsWriter for Failing {
            fn write(&mut self, _: &str, _: &str) -> io::Result<()> {
                Err(io::Error::other("busy"))
            }
        }
        let mut fl = SysfsFrontlight::new(FrontlightConfig::monza(), Failing, 50, 0);
        let ramp = fl.begin_toggle().unwrap();
        assert!(ramp.run(&mut fl, &mut |_| {}).is_err());
        assert!(!fl.is_ramping());
        assert!(fl.is_on());
    }

    #[test]
    fn desktop_config_has_no_ioctl_and_no_deferred_start() {
        let mut fl = desktop_frontlight();
        fl.set_intensity(50).unwrap();
        let ramp = fl.begin_toggle().unwrap();
        assert!(!ramp.defer_start);
        assert!(!fl.uses_ioctl());
    }
}
