//! Battery, frontlight, power, Wi-Fi and detection against a fake device tree.

mod common;

use std::path::PathBuf;
use std::time::Duration;

use common::{get, put, FakeIo, Run};
use kaia_platform::battery::{Battery, KoboBattery, MONZA_BATTERY_DIR};
use kaia_platform::frontlight::{Frontlight, FrontlightConfig, SysfsFrontlight, NTX_IO};
use kaia_platform::network::{KoboNetwork, Network};
use kaia_platform::plan::{execute, Outcome};
use kaia_platform::power::{
    PowerContext, PowerManager, ResumeContext, POWER_STATE, RTC_WAKEALARM, STATE_EXTENDED,
};
use kaia_platform::sysfs::Root;
use kaia_platform::usb::handover_plan;
use kaia_platform::{Env, Kind, Model, Paths, Platform};

const BAT: &str = MONZA_BATTERY_DIR;
const BRIGHTNESS: &str = "/sys/class/backlight/mxc_msp430.0/brightness";
const ACTUAL: &str = "/sys/class/backlight/mxc_msp430.0/actual_brightness";
const MIXER: &str = "/sys/class/backlight/lm3630a_led/color";

fn battery_tree(status: &str, capacity: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), &format!("{BAT}/status"), status);
    put(dir.path(), &format!("{BAT}/capacity"), capacity);
    dir
}

#[test]
fn battery_capacity_and_status_paths() {
    let dir = battery_tree("Charging\n", "87\n");
    let bat = KoboBattery::new(Root::new(dir.path()), BAT);
    assert_eq!(bat.capacity(), Some(87));
    assert_eq!(bat.is_charging(), Some(true));
    assert_eq!(bat.is_charged(), Some(false));
}

#[test]
fn battery_discharging_and_full() {
    let dir = battery_tree("Discharging\n", "40\n");
    let bat = KoboBattery::new(Root::new(dir.path()), BAT);
    assert_eq!(bat.is_charging(), Some(false));
    assert_eq!(bat.is_charged(), Some(false));

    put(dir.path(), &format!("{BAT}/status"), "Full\n");
    assert_eq!(bat.is_charging(), Some(true));
    assert_eq!(bat.is_charged(), Some(true));
}

#[test]
fn battery_not_charging_counts_as_charged_only_at_100() {
    let dir = battery_tree("Not charging\n", "100\n");
    let bat = KoboBattery::new(Root::new(dir.path()), BAT);
    assert_eq!(bat.is_charged(), Some(true));
    put(dir.path(), &format!("{BAT}/capacity"), "99\n");
    assert_eq!(bat.is_charged(), Some(false));
}

#[test]
fn battery_missing_or_bad_values_are_none() {
    let dir = tempfile::tempdir().unwrap();
    let bat = KoboBattery::new(Root::new(dir.path()), BAT);
    assert_eq!(bat.capacity(), None);
    // A missing status file reads as charging, like KOReader's isChargingHW.
    assert_eq!(bat.is_charging(), Some(true));
    assert_eq!(bat.is_charged(), None);

    let dir = battery_tree("Charging\n", "150\n");
    let bat = KoboBattery::new(Root::new(dir.path()), BAT);
    assert_eq!(bat.capacity(), None);
}

fn frontlight_tree(brightness: &str, mixer: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), BRIGHTNESS, brightness);
    put(dir.path(), MIXER, mixer);
    dir
}

/// Runs a toggle ramp to the end without waiting.
fn toggle<F: Frontlight>(fl: &mut F) {
    if let Some(ramp) = fl.begin_toggle() {
        ramp.run(fl, &mut |_| {}).unwrap();
    }
}

fn fast(mut cfg: FrontlightConfig) -> FrontlightConfig {
    cfg.ramp_delay = Duration::ZERO;
    cfg
}

#[test]
fn frontlight_open_reads_the_level_and_warmth_back_and_writes_nothing() {
    // Brightness 20 and mixer color 5. On the inverted Monza scale color 5 is
    // native 5, which is warmth 50. Opening keeps both and writes neither file.
    let dir = frontlight_tree("20\n", "5\n");
    let fl = SysfsFrontlight::open(
        fast(FrontlightConfig::monza()),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        None,
    );
    assert!(fl.is_on());
    assert_eq!(fl.intensity(), 20);
    assert_eq!(fl.warmth(), 50);
    assert!(fl.writer().writes.is_empty());
    assert_eq!(get(dir.path(), BRIGHTNESS), "20\n");
    assert_eq!(get(dir.path(), MIXER), "5\n");
}

#[test]
fn detect_kobo_keeps_the_hardware_light_and_writes_nothing() {
    // Nickel left the light at 26 with brightness reading 0, and color 1.
    let dir = frontlight_tree("0\n", "1\n");
    put(dir.path(), ACTUAL, "26\n");
    put(
        dir.path(),
        "/mnt/onboard/.kobo/version",
        "N3-0-0,4.42.23303,x,y,z,w,v,320\n",
    );
    let env = Env {
        product: Some("monza".to_string()),
        ..Default::default()
    };
    let platform = Platform::detect_in(dir.path(), &env).unwrap();
    assert_eq!(platform.frontlight.intensity(), 26);
    assert_eq!(platform.frontlight.warmth(), 90);
    assert!(platform.frontlight.is_on());
    assert_eq!(get(dir.path(), BRIGHTNESS), "0\n");
    assert_eq!(get(dir.path(), MIXER), "1\n");
    assert_eq!(get(dir.path(), ACTUAL), "26\n");
}

#[test]
fn frontlight_writes_the_saved_warmth_on_open() {
    let dir = frontlight_tree("20\n", "5\n");
    let fl = SysfsFrontlight::open(
        fast(FrontlightConfig::monza()),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        Some(80),
    );
    assert_eq!(fl.warmth(), 80);
    // Native 8, written inverted as 10 - 8.
    assert_eq!(get(dir.path(), MIXER), "2");
}

#[test]
fn frontlight_uses_the_ntx_ioctl_when_the_node_opens() {
    let dir = frontlight_tree("0\n", "5\n");
    let mut io = FakeIo::new(dir.path());
    io.ntx_open = true;
    let mut fl = SysfsFrontlight::open(fast(FrontlightConfig::monza()), io, &Root::new(dir.path()), None);
    fl.set_intensity(33).unwrap();
    assert_eq!(fl.writer().ntx_calls, vec![(NTX_IO.to_string(), 33)]);
    // The sysfs file is untouched.
    assert_eq!(get(dir.path(), BRIGHTNESS), "0\n");
}

#[test]
fn frontlight_off_at_start_uses_default_intensity() {
    let dir = frontlight_tree("0\n", "10\n");
    let fl = SysfsFrontlight::open(
        fast(FrontlightConfig::monza()),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        None,
    );
    assert!(!fl.is_on());
    assert_eq!(fl.intensity(), 0);
    assert_eq!(fl.warmth(), 0);
}

#[test]
fn set_intensity_writes_white_led_file() {
    let dir = frontlight_tree("0\n", "0\n");
    let mut fl = SysfsFrontlight::open(
        fast(FrontlightConfig::monza()),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        None,
    );
    fl.set_intensity(45).unwrap();
    assert_eq!(get(dir.path(), BRIGHTNESS), "45");
    assert!(fl.is_on());
    assert_eq!(fl.intensity(), 45);
    fl.set_intensity(250).unwrap();
    assert_eq!(get(dir.path(), BRIGHTNESS), "100");
    fl.set_intensity(0).unwrap();
    assert!(!fl.is_on());
    assert_eq!(fl.intensity(), 0);
}

#[test]
fn set_warmth_inverts_the_mixer_value() {
    let dir = frontlight_tree("10\n", "0\n");
    let mut fl = SysfsFrontlight::open(
        fast(FrontlightConfig::monza()),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        None,
    );
    // Warm (100) is nl_min 0 on the mixer. Cold (0) is nl_max 10.
    fl.set_warmth(100).unwrap();
    assert_eq!(get(dir.path(), MIXER), "0");
    fl.set_warmth(0).unwrap();
    assert_eq!(get(dir.path(), MIXER), "10");
    fl.set_warmth(55).unwrap();
    // 55 rounds to native 6, which is written as 10 - 6 = 4.
    assert_eq!(get(dir.path(), MIXER), "4");
    assert_eq!(fl.warmth(), 55);
}

#[test]
fn toggle_off_ramps_the_file_to_zero_and_on_restores() {
    let dir = frontlight_tree("100\n", "5\n");
    let mut fl = SysfsFrontlight::open(
        fast(FrontlightConfig::monza()),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        None,
    );
    assert_eq!(fl.intensity(), 100);
    toggle(&mut fl);
    assert!(!fl.is_on());
    assert_eq!(fl.intensity(), 0);
    assert_eq!(get(dir.path(), BRIGHTNESS), "0");

    toggle(&mut fl);
    assert!(fl.is_on());
    assert_eq!(fl.intensity(), 100);
    assert_eq!(get(dir.path(), BRIGHTNESS), "100");
}

#[test]
fn toggle_on_after_start_with_light_off_uses_default_intensity() {
    // powerd.lua:132: a light that starts off comes back at the default intensity (20).
    let dir = frontlight_tree("0\n", "5\n");
    let mut fl = SysfsFrontlight::open(
        fast(FrontlightConfig::monza()),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        None,
    );
    toggle(&mut fl);
    assert!(fl.is_on());
    assert_eq!(fl.intensity(), 20);
    assert_eq!(get(dir.path(), BRIGHTNESS), "20");
}

#[test]
fn toggle_on_with_zero_intensity_does_nothing() {
    let dir = frontlight_tree("30\n", "5\n");
    let mut fl = SysfsFrontlight::open(
        fast(FrontlightConfig::monza()),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        None,
    );
    fl.set_intensity(0).unwrap();
    toggle(&mut fl);
    assert!(!fl.is_on());
    assert_eq!(get(dir.path(), BRIGHTNESS), "0");
}

#[test]
fn write_failure_is_reported_with_the_path() {
    // No mixer file: writes must not create it.
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), BRIGHTNESS, "0\n");
    let mut fl = SysfsFrontlight::open(
        fast(FrontlightConfig::monza()),
        FakeIo::new(dir.path()),
        &Root::new(dir.path()),
        None,
    );
    let err = fl.set_warmth(30).unwrap_err();
    assert!(err.to_string().contains(MIXER), "{err}");
    assert!(!dir.path().join(MIXER.trim_start_matches('/')).exists());
}

#[test]
fn suspend_then_resume_against_fake_tree() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), POWER_STATE, "freeze mem\n");
    put(dir.path(), STATE_EXTENDED, "0");
    put(
        dir.path(),
        "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
        "ondemand",
    );
    let root = Root::new(dir.path());
    let pm = PowerManager::kobo(&root, PathBuf::from("/mnt/onboard/.adds/kaia"), vec![], true);
    let mut io = FakeIo::new(dir.path());

    let first = pm.prepare_suspend(PowerContext::default());
    let outcome = execute(&first, &mut io, &mut |_| {});
    assert_eq!(outcome, Outcome::Completed);
    assert_eq!(get(dir.path(), STATE_EXTENDED), "1");

    let mut slept = Vec::new();
    let second = pm.finish_suspend();
    let outcome = execute(&second, &mut io, &mut |d| slept.push(d));
    assert_eq!(outcome, Outcome::Completed);
    assert_eq!(get(dir.path(), POWER_STATE), "mem");
    assert_eq!(io.runs.len(), 1);
    assert_eq!(io.runs[0].program, "sync");

    let resume = pm.resume(ResumeContext::default());
    let mut slept = Vec::new();
    execute(&resume, &mut io, &mut |d| slept.push(d));
    assert_eq!(get(dir.path(), STATE_EXTENDED), "0");
    assert_eq!(slept, vec![Duration::from_millis(100)]);
}

#[test]
fn refused_suspend_resets_state_extended() {
    // /sys/power/state is missing, so the kernel "refuses" the write.
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), STATE_EXTENDED, "1");
    let root = Root::new(dir.path());
    let pm = PowerManager::kobo(&root, PathBuf::from("/x"), vec![], true);
    let mut io = FakeIo::new(dir.path());
    let outcome = execute(&pm.finish_suspend(), &mut io, &mut |_| {});
    assert_eq!(
        outcome,
        Outcome::Failed {
            step: 1,
            recovered: true
        }
    );
    assert_eq!(get(dir.path(), STATE_EXTENDED), "0");
}

#[test]
fn standby_writes_wakealarm_and_mem() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), POWER_STATE, "mem");
    put(dir.path(), RTC_WAKEALARM, "0");
    let root = Root::new(dir.path());
    let pm = PowerManager::kobo(&root, PathBuf::from("/x"), vec![], true);
    let mut io = FakeIo::new(dir.path());
    let plan = pm.standby(PowerContext::default(), Some(600), 2_000);
    let mut slept = Vec::new();
    assert_eq!(
        execute(&plan, &mut io, &mut |d| slept.push(d)),
        Outcome::Completed
    );
    assert_eq!(get(dir.path(), RTC_WAKEALARM), "2600");
    assert_eq!(get(dir.path(), POWER_STATE), "mem");
    assert_eq!(slept, vec![Duration::from_millis(90)]);
}

#[test]
fn wifi_state_and_default_route_from_fake_tree() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::new(dir.path());
    let net = KoboNetwork::new(
        root.clone(),
        FakeIo::new(dir.path()),
        PathBuf::from("/mnt/onboard/.adds/kaia"),
        "eth0",
        vec![],
    );
    assert!(!net.is_wifi_on());
    assert!(!net.is_connected());

    std::fs::create_dir_all(dir.path().join("sys/class/net/eth0")).unwrap();
    put(
        dir.path(),
        "/proc/net/route",
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\n\
         eth0\t0000A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\n\
         eth0\t00000000\t0100A8C0\t0003\t0\t0\t0\t00000000\n",
    );
    assert!(net.is_wifi_on());
    assert!(net.is_connected());
}

#[test]
fn enable_and_disable_run_the_install_scripts_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let install = PathBuf::from("/mnt/onboard/.adds/kaia");
    let env = vec![("INTERFACE".to_string(), "eth0".to_string())];
    let mut io = FakeIo::new(dir.path());
    let net = KoboNetwork::new(
        Root::new(dir.path()),
        FakeIo::new(dir.path()),
        install.clone(),
        "eth0",
        env.clone(),
    );

    assert_eq!(
        execute(&net.enable_plan(), &mut io, &mut |_| {}),
        Outcome::Completed
    );
    assert_eq!(
        io.runs,
        vec![Run {
            program: "/mnt/onboard/.adds/kaia/enable-wifi.sh".to_string(),
            args: vec![],
            cwd: Some(install.clone()),
            env: env.clone(),
        }]
    );

    io.runs.clear();
    assert_eq!(
        execute(&net.disable_plan(), &mut io, &mut |_| {}),
        Outcome::Completed
    );
    let programs: Vec<&str> = io.runs.iter().map(|r| r.program.as_str()).collect();
    assert_eq!(
        programs,
        vec![
            "/mnt/onboard/.adds/kaia/release-ip.sh",
            "/mnt/onboard/.adds/kaia/disable-wifi.sh"
        ]
    );
}

#[test]
fn failing_enable_script_is_an_error() {
    // A runner whose commands all report failure. Writes go to the fake tree.
    struct Failing(FakeIo);
    impl kaia_platform::sysfs::SysfsWriter for Failing {
        fn write(&mut self, p: &str, v: &str) -> std::io::Result<()> {
            self.0.write(p, v)
        }
    }
    impl kaia_platform::sysfs::Runner for Failing {
        fn run(
            &mut self,
            _p: &str,
            _a: &[String],
            _c: Option<&std::path::Path>,
            _e: &[(String, String)],
        ) -> std::io::Result<bool> {
            Ok(false)
        }
        fn output(&mut self, _p: &str, _a: &[&str]) -> std::io::Result<String> {
            Ok(String::new())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut net = KoboNetwork::new(
        Root::new(dir.path()),
        Failing(FakeIo::new(dir.path())),
        PathBuf::from("/x"),
        "eth0",
        vec![],
    );
    assert!(net.enable_wifi().is_err());
}

#[test]
fn usb_handover_runs_in_order_and_recovers() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::kobo(&Root::new(dir.path()));
    let plan = handover_plan(&paths, "de");
    let mut io = FakeIo::new(dir.path());
    assert_eq!(execute(&plan, &mut io, &mut |_| {}), Outcome::Completed);
    let programs: Vec<&str> = io.runs.iter().map(|r| r.program.as_str()).collect();
    assert_eq!(programs.last(), Some(&"rm"));
    assert_eq!(programs.len(), 6);
    assert_eq!(io.runs[3].env, vec![("LANGUAGE".to_string(), "de".to_string())]);

    // A failed unpack runs the cleanup list, then stops.
    let mut io = FakeIo::new(dir.path());
    io.failing_programs
        .push(paths.install_dir.join("tar").to_string_lossy().into_owned());
    let outcome = execute(&plan, &mut io, &mut |_| {});
    assert_eq!(
        outcome,
        Outcome::Failed {
            step: 2,
            recovered: true
        }
    );
    let programs: Vec<&str> = io.runs.iter().map(|r| r.program.as_str()).collect();
    let tar = paths.install_dir.join("tar").to_string_lossy().into_owned();
    assert_eq!(programs, vec!["mkdir", "mount", tar.as_str(), "umount", "rm"]);
}

#[test]
fn detect_kobo_reads_version_and_model() {
    let dir = tempfile::tempdir().unwrap();
    put(
        dir.path(),
        "/mnt/onboard/.kobo/version",
        "N3-0-0,4.38.23171,4.38.23171,4.38.23171,x,y,z,320\n",
    );
    put(dir.path(), &format!("{BAT}/capacity"), "63\n");
    put(dir.path(), &format!("{BAT}/status"), "Discharging\n");
    let env = Env {
        product: Some("monza".to_string()),
        ..Default::default()
    };
    let platform = Platform::detect_in(dir.path(), &env).unwrap();
    assert_eq!(platform.kind, Kind::Kobo(Model::LibraColour));
    assert_eq!(platform.device.model_id, "Kobo_monza");
    assert_eq!(platform.device.firmware.as_deref(), Some("4.38.23171"));
    assert_eq!(platform.device.product_id, "320");
    assert!(platform.device.has_color_screen);
    assert_eq!(platform.device.dpi, 300);
    assert_eq!(platform.battery.capacity(), Some(63));
    assert_eq!(platform.battery.is_charging(), Some(false));
    assert_eq!(
        platform.paths.install_dir,
        dir.path().join("mnt/onboard/.adds/kaia")
    );
    assert!(!platform.power.can_standby());
}

#[test]
fn detect_kobo_writes_the_saved_warmth_from_the_env() {
    let dir = frontlight_tree("15\n", "9\n");
    put(
        dir.path(),
        "/mnt/onboard/.kobo/version",
        "N3-0-0,4.38.23171,x,y,z,w,v,320\n",
    );
    let env = Env {
        product: Some("monza".to_string()),
        saved_warmth: Some(40),
        ..Default::default()
    };
    let platform = Platform::detect_in(dir.path(), &env).unwrap();
    assert_eq!(platform.frontlight.warmth(), 40);
    // Native 4, written inverted as 10 - 4.
    assert_eq!(get(dir.path(), MIXER), "6");
    assert_eq!(platform.frontlight.intensity(), 15);
}

#[test]
fn detect_kobo_rejects_other_models() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "/mnt/onboard/.kobo/version", "x\n");
    let env = Env {
        product: Some("spaBW".to_string()),
        ..Default::default()
    };
    let err = Platform::detect_in(dir.path(), &env)
        .err()
        .expect("unsupported model");
    assert!(err.to_string().contains("spaBW"), "{err}");
}

#[test]
fn detect_without_marker_is_desktop() {
    let dir = tempfile::tempdir().unwrap();
    let env = Env {
        home: Some(PathBuf::from("/home/reader")),
        ..Default::default()
    };
    let mut platform = Platform::detect_in(dir.path(), &env).unwrap();
    assert_eq!(platform.kind, Kind::Desktop);
    let expected = if cfg!(target_os = "macos") {
        PathBuf::from("/home/reader/Library/Application Support/kaia")
    } else {
        PathBuf::from("/home/reader/.local/share/kaia")
    };
    assert_eq!(platform.paths.data_dir, expected);
    assert_eq!(platform.paths.books_dir, PathBuf::from("/home/reader/Books"));
    assert!(platform.battery.capacity().is_none());
    assert!(!platform.frontlight.is_on());
    assert!(platform.network.scan().is_err());
    assert!(platform
        .power
        .prepare_suspend(PowerContext::default())
        .is_skipped());
}

fn wake_manager(dir: &std::path::Path) -> PowerManager {
    PowerManager::kobo(&Root::new(dir), PathBuf::from("/x"), vec![], true)
}

#[test]
fn wakeup_uses_the_rtc_ioctl_and_leaves_sysfs_alone() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), RTC_WAKEALARM, "0");
    let pm = wake_manager(dir.path());
    let mut io = FakeIo::new(dir.path());
    io.rtc_ioctl_works = true;
    let plan = pm.schedule_wakeup(600, 2_000);
    assert_eq!(execute(&plan, &mut io, &mut |_| {}), Outcome::Completed);
    assert_eq!(io.rtc_alarms, vec![2_600]);
    assert!(io.writes.is_empty(), "no sysfs write when the ioctl works");
    assert_eq!(get(dir.path(), RTC_WAKEALARM), "0");
}

#[test]
fn wakeup_falls_back_to_sysfs_and_clears_the_old_alarm_first() {
    // An alarm is already set, so a plain write would fail with EBUSY.
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), RTC_WAKEALARM, "1500\n");
    let pm = wake_manager(dir.path());
    let mut io = FakeIo::new(dir.path());
    io.wakealarm_busy_while_set = true;
    let plan = pm.schedule_wakeup(600, 2_000);
    assert_eq!(execute(&plan, &mut io, &mut |_| {}), Outcome::Completed);
    assert_eq!(io.rtc_alarms, vec![2_600], "the ioctl was tried first");
    assert_eq!(
        io.writes,
        vec![
            (RTC_WAKEALARM.to_string(), "0".to_string()),
            (RTC_WAKEALARM.to_string(), "2600".to_string()),
        ]
    );
    assert_eq!(get(dir.path(), RTC_WAKEALARM), "2600");
}

#[test]
fn clear_wakeup_uses_aie_off_then_sysfs() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), RTC_WAKEALARM, "2600");
    let pm = wake_manager(dir.path());

    let mut io = FakeIo::new(dir.path());
    io.rtc_ioctl_works = true;
    execute(&pm.clear_wakeup(), &mut io, &mut |_| {});
    assert_eq!(io.rtc_disables, 1);
    assert!(io.writes.is_empty());
    assert_eq!(get(dir.path(), RTC_WAKEALARM), "2600");

    let mut io = FakeIo::new(dir.path());
    execute(&pm.clear_wakeup(), &mut io, &mut |_| {});
    assert_eq!(io.rtc_disables, 1);
    assert_eq!(get(dir.path(), RTC_WAKEALARM), "0");
}

#[test]
fn resume_resyncs_the_system_clock_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), STATE_EXTENDED, "1");
    let pm = wake_manager(dir.path());

    let mut io = FakeIo::new(dir.path());
    execute(&pm.resume(ResumeContext::default()), &mut io, &mut |_| {});
    assert_eq!(io.rtc_clock_syncs, 0);

    let mut io = FakeIo::new(dir.path());
    io.rtc_ioctl_works = true;
    let plan = pm.resume(ResumeContext {
        charging_led: None,
        sync_system_clock: true,
    });
    assert_eq!(execute(&plan, &mut io, &mut |_| {}), Outcome::Completed);
    assert_eq!(io.rtc_clock_syncs, 1);

    // A failed resync does not stop the resume.
    let mut io = FakeIo::new(dir.path());
    assert_eq!(execute(&plan, &mut io, &mut |_| {}), Outcome::Completed);
    assert_eq!(io.rtc_clock_syncs, 1);
}

#[test]
fn prepare_suspend_has_no_sleep_so_the_caller_can_schedule_the_wait() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), STATE_EXTENDED, "0");
    let pm = wake_manager(dir.path());
    let mut io = FakeIo::new(dir.path());
    let mut slept = Vec::new();
    let outcome = execute(&pm.prepare_suspend(PowerContext::default()), &mut io, &mut |d| {
        slept.push(d)
    });
    assert_eq!(outcome, Outcome::Completed);
    assert!(slept.is_empty());
    assert_eq!(get(dir.path(), STATE_EXTENDED), "1");
}

#[test]
fn monza_page_button_wake_stays_disabled_without_a_known_knob() {
    let dir = tempfile::tempdir().unwrap();
    put(dir.path(), "/mnt/onboard/.kobo/version", "320");
    let platform = Platform::detect_in(
        dir.path(),
        &Env {
            product: Some("monza".into()),
            ..Env::default()
        },
    )
    .unwrap();
    assert_eq!(platform.kind, Kind::Kobo(Model::LibraColour));
    assert!(!platform.supports_page_button_wake());
}
