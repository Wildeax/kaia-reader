//! The frontlight level read at open, and the writes that follow, against a fake
//! device tree. On the Libra Colour `brightness` reads 0 while the light is on,
//! so the level comes from `actual_brightness` beside it.

mod common;

use common::{get, put, FakeIo};
use kaia_platform::frontlight::{Frontlight, FrontlightConfig, HardwareLevels, SysfsFrontlight};
use kaia_platform::sysfs::Root;

const BRIGHTNESS: &str = "/sys/class/backlight/mxc_msp430.0/brightness";
const ACTUAL: &str = "/sys/class/backlight/mxc_msp430.0/actual_brightness";
const MIXER: &str = "/sys/class/backlight/lm3630a_led/color";

/// A tree with the LED files. `actual` is left out when `None`. The mixer reads
/// color 1, as it did on the device.
fn tree(brightness: &str, actual: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), BRIGHTNESS, brightness);
    if let Some(actual) = actual {
        put(dir.path(), ACTUAL, actual);
    }
    put(dir.path(), MIXER, "1\n");
    dir
}

fn open(dir: &tempfile::TempDir, saved_warmth: Option<u8>) -> SysfsFrontlight<FakeIo> {
    SysfsFrontlight::open(
        FrontlightConfig::monza(),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        saved_warmth,
    )
}

#[test]
fn level_comes_from_actual_brightness_when_brightness_reads_zero() {
    // The device state from the report: brightness 0, actual_brightness 26.
    let dir = tree("0\n", Some("26\n"));
    let fl = open(&dir, None);
    assert!(fl.is_on());
    assert_eq!(fl.intensity(), 26);
    assert!(
        fl.writer().writes.is_empty(),
        "opening must not write the LED, so the light stays on"
    );
    assert_eq!(get(dir.path(), BRIGHTNESS), "0\n");
}

#[test]
fn level_falls_back_to_brightness_when_actual_brightness_is_missing() {
    let dir = tree("26\n", None);
    let fl = open(&dir, None);
    assert!(fl.is_on());
    assert_eq!(fl.intensity(), 26);
}

#[test]
fn level_falls_back_to_brightness_when_actual_brightness_is_unreadable() {
    let dir = tree("26\n", Some("not a number\n"));
    let fl = open(&dir, None);
    assert_eq!(fl.intensity(), 26);
}

#[test]
fn level_is_zero_when_both_files_read_zero() {
    let dir = tree("0\n", Some("0\n"));
    let fl = open(&dir, None);
    assert!(!fl.is_on());
    assert_eq!(fl.intensity(), 0);
}

#[test]
fn intensity_writes_go_to_brightness_and_not_to_actual_brightness() {
    let dir = tree("0\n", Some("26\n"));
    let mut fl = open(&dir, None);
    fl.set_intensity(40).unwrap();
    assert_eq!(
        fl.writer().writes,
        vec![(BRIGHTNESS.to_string(), "40".to_string())]
    );
    assert_eq!(get(dir.path(), BRIGHTNESS), "40");
    assert_eq!(
        get(dir.path(), ACTUAL),
        "26\n",
        "the read-back file is not written"
    );
    assert_eq!(fl.intensity(), 40);
}

#[test]
fn warmth_90_is_written_as_color_1_and_reports_90() {
    // KOReader's Monza scale: nl_max 10, inverted (`device.lua:547-551`). Warmth
    // 90 is native 9, which the inverted mixer writes as 10 - 9 = 1. Color 1 on
    // the device therefore reads as native 9, warmth 90.
    let dir = tree("0\n", Some("0\n"));
    let fl = open(&dir, Some(90));
    assert_eq!(fl.warmth(), 90);
    assert_eq!(fl.writer().writes, vec![(MIXER.to_string(), "1".to_string())]);
    assert_eq!(get(dir.path(), MIXER), "1");
}

#[test]
fn startup_keeps_the_hardware_level_and_reads_color_1_as_warmth_90() {
    // Nickel left the light at 26 with color 1. Opening reports both and writes neither file.
    let dir = tree("0\n", Some("26\n"));
    let fl = open(&dir, None);
    assert_eq!(
        fl.hardware_levels(),
        Some(HardwareLevels {
            intensity: 26,
            warmth: Some(90),
        })
    );
    assert_eq!(fl.intensity(), 26);
    assert_eq!(fl.warmth(), 90);
    assert!(fl.writer().writes.is_empty());
    assert_eq!(get(dir.path(), BRIGHTNESS), "0\n");
    assert_eq!(get(dir.path(), MIXER), "1\n");
}
