//! Suspend, resume, standby and wakeup alarms.
//!
//! Each sequence is a [`Plan`] built from the device state. Nothing runs
//! until the caller passes the plan to [`crate::plan::execute`] or
//! [`crate::Platform::run_plan`].
//!
//! KOReader sources (`frontend/device/kobo/device.lua` unless noted):
//! - suspend: `Kobo:suspend` at 1361, `Kobo:_doSuspend` at 1412, `Kobo:resume` at 1476.
//! - standby: `Kobo:standby` at 1282.
//! - standby state: `standby` by default (156), `mem` on MTK (955-957).
//! - `checkStandby` at 35-50.
//! - wakeup alarm: `WakeupMgr:addTask` in `frontend/device/wakeupmgr.lua:69-111`,
//!   which calls `base/ffi/rtc.lua` (`setWakeupAlarm`, `unsetWakeupAlarm`).
//!   The alarm is set with the `RTC_WKALM_SET` ioctl on `/dev/rtc0`, which
//!   replaces an alarm that is set. If the ioctl fails, the plan writes `0`
//!   and then the epoch to `/sys/class/rtc/rtc0/wakealarm`; a sysfs write
//!   alone fails with EBUSY while an alarm is set. See [`crate::plan::Action::SetWakeAlarm`].
//! - clock resync after resume: `RTC:HCToSys` in `powerd.lua:515`.
//!
//! # Timing belongs to the caller
//!
//! No plan here sleeps for longer than a few milliseconds. KOReader waits
//! between steps with `UIManager:scheduleIn`, so input keeps flowing. The
//! delays that need that are returned as constants for the caller to schedule,
//! and [`crate::suspend::SuspendFlow`] is the state machine that orders them.
//! The suspend sequence is two plans with a wait between them:
//!
//! 1. [`PowerManager::prepare_suspend`]: Wi-Fi off and `state-extended` to 1.
//! 2. Wait [`PRE_SUSPEND_DELAY`] (2 s) on the event scheduler.
//! 3. [`PowerManager::finish_suspend`]: `sync`, then the write of `mem`.
//!
//! The app must cancel step 2 when a Power press (or any Resume) arrives
//! during the wait, as `Kobo:resume` does (`device.lua:1484`, issue #12325).
//! Without the cancel, the device suspends right after the user pressed the
//! button to keep it awake. [`crate::suspend::SuspendFlow::resume`] does the
//! cancel and returns the timer to drop.
//!
//! Power key and sleep cover handling is in [`crate::events`].
//!
//! Not ported here:
//! - the frontlight ramp before suspend (`powerd.lua:473-490`). The caller
//!   turns the light off with `Frontlight::begin_toggle` first.
//! - the `pmic_rtc` alarm chain (`wakeupmgr.lua:79-94`). That RTC is on i.MX5
//!   boards, not the Libra Colour.
//! - KOReader's `enableCPUCores`. Its `wakeup_count` handshake is commented out
//!   unconditionally in `device.lua:1416-1436`, so suspend does not use it here.

use std::path::PathBuf;
use std::time::Duration;

use crate::network::disable_wifi_steps;
use crate::plan::{OnError, Plan, Step};
use crate::sysfs::Root;

/// `/sys/power/state-extended`. Set to 1 while subsystems sleep (`device.lua:1395`).
pub const STATE_EXTENDED: &str = "/sys/power/state-extended";
/// `/sys/power/state`. The kernel enters suspend or standby here (`device.lua:1322`, `1442`).
pub const POWER_STATE: &str = "/sys/power/state";
/// RTC wakeup alarm in sysfs form. The fallback when the ioctl fails.
pub const RTC_WAKEALARM: &str = "/sys/class/rtc/rtc0/wakealarm";
/// The RTC device node for the alarm and clock ioctls (`base/ffi/rtc.lua:26`, `dev_rtc`).
pub const RTC_DEVICE: &str = "/dev/rtc0";
/// The wait between `state-extended` and the suspend write (`device.lua:1409`,
/// `UIManager:scheduleIn(2, self._doSuspend, self)`). The caller schedules it.
pub const PRE_SUSPEND_DELAY: Duration = Duration::from_secs(2);
/// Neonode IR grid knobs, written `a` on resume (`device.lua:967-975`, `1509`).
const IR_GRID_KNOBS: [&str; 2] = [
    "/sys/devices/virtual/input/input1/neocmd",
    "/sys/devices/platform/imx-i2c.1/i2c-1/1-0050/neocmd",
];
/// Charging LED brightness knobs, in KOReader's order (`device.lua:919-925`).
const CHARGING_LED_KNOBS: [&str; 3] = [
    "/sys/class/leds/LED/brightness",
    "/sys/class/leds/GLED/brightness",
    "/sys/class/leds/bd71828-green-led/brightness",
];

/// Facts that decide whether a sequence runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PowerContext {
    /// Whether Wi-Fi is on. Suspend turns it off first (`device.lua:1379-1385`).
    pub wifi_on: bool,
    /// Whether the charger is connected. MTK skips suspend and standby while charging (`device.lua:1370-1375`, `1285-1288`).
    pub charging: bool,
}

/// Facts that the resume sequence needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ResumeContext {
    /// Charging LED state to set after resume. `None` leaves the LED alone (`device.lua:1513`).
    pub charging_led: Option<bool>,
    /// Copy the RTC time to the system clock after resume (`RTC:HCToSys`,
    /// `powerd.lua:515`). Off by default. Unverified on hardware: the call needs
    /// root and a working RTC driver.
    pub sync_system_clock: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KoboPower {
    install_dir: PathBuf,
    env: Vec<(String, String)>,
    mtk: bool,
    standby_state: &'static str,
    can_standby: bool,
    governor_knob: Option<String>,
    default_governor: Option<String>,
    ir_grid_knob: Option<String>,
    charging_led_knob: Option<String>,
}

/// Suspend, resume, standby and wakeup plans. On desktop every plan is skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerManager {
    kobo: Option<KoboPower>,
}

const NO_POWER_MANAGEMENT: &str = "power management is not available on this platform";

impl PowerManager {
    /// A manager for desktop, where every plan is skipped.
    pub fn none() -> Self {
        PowerManager { kobo: None }
    }

    /// Reads the device state under `root`. `install_dir` holds the Wi-Fi scripts,
    /// and `env` is passed to them. `mtk` is true for the Libra Colour.
    pub fn kobo(root: &Root, install_dir: PathBuf, env: Vec<(String, String)>, mtk: bool) -> Self {
        let standby_state = if mtk { "mem" } else { "standby" };
        // checkStandby (device.lua:35-50): the state file must list the target state.
        let can_standby = root
            .read_text(POWER_STATE)
            .is_some_and(|states| states.split_whitespace().any(|s| s == standby_state));
        // device.lua:769-773: policy0 when the cpufreq layout has it, otherwise cpu0.
        let governor_knob = if root.exists("/sys/devices/system/cpu/cpufreq/policy0") {
            "/sys/devices/system/cpu/cpufreq/policy0/scaling_governor".to_string()
        } else {
            "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor".to_string()
        };
        // getCPUGovernor (device.lua:66-80): userspace is reported as conservative.
        let default_governor = root.read_line(&governor_knob).map(|g| {
            if g == "userspace" {
                "conservative".to_string()
            } else {
                g
            }
        });
        let ir_grid_knob = IR_GRID_KNOBS
            .iter()
            .find(|k| root.exists(k))
            .map(|k| k.to_string());
        let charging_led_knob = CHARGING_LED_KNOBS
            .iter()
            .find(|k| root.exists(k))
            .map(|k| k.to_string());
        // device.lua:776-779 ("NOP unsupported methods"): without a default governor, no governor writes.
        let governor_knob = default_governor.as_ref().map(|_| governor_knob);
        PowerManager {
            kobo: Some(KoboPower {
                install_dir,
                env,
                mtk,
                standby_state,
                can_standby,
                governor_knob,
                default_governor,
                ir_grid_knob,
                charging_led_knob,
            }),
        }
    }

    /// Whether the kernel offers the standby state.
    pub fn can_standby(&self) -> bool {
        self.kobo.as_ref().is_some_and(|k| k.can_standby)
    }

    /// The first half of suspend: Wi-Fi off, then `state-extended` set to 1.
    ///
    /// The plan does not wait. KOReader waits 2 seconds before the second half
    /// (`device.lua:1409`) on its event loop, so a Power press in that window
    /// is still handled (#12325). The caller schedules [`PRE_SUSPEND_DELAY`],
    /// then runs [`PowerManager::finish_suspend`], and cancels the wait when a
    /// Power press or Resume arrives first. [`crate::suspend::SuspendFlow`]
    /// holds that order.
    ///
    /// KOReader source: `Kobo:suspend` (`device.lua:1361-1409`). The frontlight
    /// and the Suspend event come first (`powerd.lua:473-490`,
    /// `generic/device.lua:412-478`). If the `state-extended` write fails, the
    /// plan stops with a failed outcome, and the caller must not run the second
    /// half (KOReader retries through the unexpected wakeup guard,
    /// `device.lua:1397-1403`). Unverified on hardware.
    pub fn prepare_suspend(&self, ctx: PowerContext) -> Plan {
        let Some(k) = &self.kobo else {
            return Plan::skipped(NO_POWER_MANAGEMENT);
        };
        if k.mtk && ctx.charging {
            return Plan::skipped("MTK suspend while charging would hang the kernel");
        }
        let mut steps = Vec::new();
        if ctx.wifi_on {
            steps.extend(disable_wifi_steps(&k.install_dir, &k.env));
        }
        steps.push(
            Step::write(STATE_EXTENDED, "1", "frontend/device/kobo/device.lua:1395").on_error(OnError::Abort),
        );
        Plan::new(steps)
    }

    /// The second half of suspend: `sync`, then write `mem` to `/sys/power/state`.
    /// Run it [`PRE_SUSPEND_DELAY`] after [`PowerManager::prepare_suspend`], unless
    /// a Power press or Resume cancelled the wait.
    ///
    /// If the write fails, `state-extended` goes back to 0 (`device.lua:1442-1464`).
    /// The write returns when the kernel resumes. Whatever the outcome, the
    /// caller must restore subsystems and the UI. Kaia returns to the UI on every
    /// return, including an immediate return, instead of retrying unattended.
    /// Unverified on hardware.
    pub fn finish_suspend(&self) -> Plan {
        if self.kobo.is_none() {
            return Plan::skipped(NO_POWER_MANAGEMENT);
        }
        Plan::new(vec![
            Step::run("sync", &[], None, &[], "frontend/device/kobo/device.lua:1413"),
            Step::write(POWER_STATE, "mem", "frontend/device/kobo/device.lua:1442").on_error(
                OnError::Recover(vec![Step::write(
                    STATE_EXTENDED,
                    "0",
                    "frontend/device/kobo/device.lua:1464",
                )]),
            ),
        ])
    }

    /// Resume: `state-extended` back to 0, wait 100 ms, then the IR grid knob and the charging LED.
    /// With [`ResumeContext::sync_system_clock`], the RTC time is copied to the system clock last.
    ///
    /// KOReader source: `Kobo:resume` (`device.lua:1476-1514`) and
    /// `KoboPowerD:afterResume` (`powerd.lua:515`, `RTC:HCToSys()`). Cancelling a
    /// pending suspend (`device.lua:1484`) is the caller's job, see the module
    /// documentation. Unverified on hardware.
    pub fn resume(&self, ctx: ResumeContext) -> Plan {
        let Some(k) = &self.kobo else {
            return Plan::skipped(NO_POWER_MANAGEMENT);
        };
        let mut steps = vec![
            Step::write(STATE_EXTENDED, "0", "frontend/device/kobo/device.lua:1493"),
            Step::sleep(
                Duration::from_millis(100),
                "frontend/device/kobo/device.lua:1501 (usleep 100000)",
            ),
        ];
        if let Some(knob) = &k.ir_grid_knob {
            steps.push(Step::write(knob, "a", "frontend/device/kobo/device.lua:1509"));
        }
        if let (Some(knob), Some(on)) = (&k.charging_led_knob, ctx.charging_led) {
            steps.push(Step::write(
                knob,
                if on { "1" } else { "0" },
                "frontend/device/kobo/device.lua:1582-1587 (_LinuxChargingLEDToggle, toggleChargingLED)",
            ));
        }
        if ctx.sync_system_clock {
            steps.push(Step::rtc_to_system_clock(
                "frontend/device/kobo/powerd.lua:515 (afterResume, RTC:HCToSys); base/ffi/rtc.lua:352",
            ));
        }
        Plan::new(steps)
    }

    /// Sets the RTC alarm `seconds` after `now_epoch` (UTC seconds).
    ///
    /// KOReader source: `WakeupMgr:addTask` (`frontend/device/wakeupmgr.lua:69-111`)
    /// and `RTC:setWakeupAlarm` (`base/ffi/rtc.lua:102`). The plan uses the
    /// `RTC_WKALM_SET` ioctl, which replaces an alarm that is set, and falls back
    /// to sysfs (write `0`, then the epoch) when the ioctl fails. Unverified on hardware.
    pub fn schedule_wakeup(&self, seconds: u64, now_epoch: u64) -> Plan {
        if self.kobo.is_none() {
            return Plan::skipped(NO_POWER_MANAGEMENT);
        }
        Plan::new(vec![Step::set_wake_alarm(
            now_epoch.saturating_add(seconds),
            "frontend/device/wakeupmgr.lua:69-111 (addTask); base/ffi/rtc.lua:102 (setWakeupAlarm)",
        )])
    }

    /// Clears the alarm, syncs storage and asks init to power off on MTK.
    pub fn power_off(&self) -> Plan {
        let Some(kobo) = &self.kobo else {
            return Plan::skipped(NO_POWER_MANAGEMENT);
        };
        let source = "frontend/device/kobo/device.lua:1529-1540 (powerOff)";
        let mut plan = self.clear_wakeup();
        let mut sync = Step::run("sync", &[], None, &[], source);
        sync.on_error = OnError::Abort;
        plan.steps.push(sync);
        let mut off = Step::run(
            "poweroff",
            if kobo.mtk { &[] } else { &["-f"] },
            None,
            &[],
            source,
        );
        off.on_error = OnError::Abort;
        plan.steps.push(off);
        plan
    }

    /// Clears the RTC alarm. Run it on power off, as `Kobo:powerOff` does, and
    /// when a scheduled wakeup is cancelled.
    ///
    /// KOReader source: `RTC:unsetWakeupAlarm` (`base/ffi/rtc.lua:154`) through
    /// `Kobo:powerOff` (`device.lua:1531`). The plan uses `RTC_AIE_OFF` and falls
    /// back to writing `0` to the sysfs `wakealarm` file. Unverified on hardware.
    pub fn clear_wakeup(&self) -> Plan {
        if self.kobo.is_none() {
            return Plan::skipped(NO_POWER_MANAGEMENT);
        }
        Plan::new(vec![Step::clear_wake_alarm(
            "frontend/device/kobo/device.lua:1531 (powerOff); base/ffi/rtc.lua:154 (unsetWakeupAlarm)",
        )])
    }

    /// Standby, which keeps the CPU awake and powers down the rest.
    ///
    /// KOReader source: `Kobo:standby` (`device.lua:1282-1359`). Skipped on MTK
    /// while charging, and when the kernel does not offer the state. With
    /// `max_duration`, the wakeup alarm is set first. Unverified on hardware.
    pub fn standby(&self, ctx: PowerContext, max_duration: Option<u64>, now_epoch: u64) -> Plan {
        let Some(k) = &self.kobo else {
            return Plan::skipped(NO_POWER_MANAGEMENT);
        };
        if k.mtk && ctx.charging {
            return Plan::skipped("MTK standby while charging would hang the kernel");
        }
        if !k.can_standby {
            return Plan::skipped("the kernel does not offer the standby state");
        }
        let mut plan = Plan::default();
        if let Some(knob) = &k.governor_knob {
            plan.steps.push(Step::write(
                knob,
                "performance",
                "frontend/device/kobo/device.lua:1293, 1636-1637 (performanceCPUGovernor)",
            ));
        }
        if let Some(seconds) = max_duration {
            plan = plan.then(self.schedule_wakeup(seconds, now_epoch));
        }
        plan.steps.push(Step::sleep(
            Duration::from_millis(90),
            "frontend/device/kobo/device.lua:1320 (usleep 90000)",
        ));
        plan.steps.push(Step::write(
            POWER_STATE,
            k.standby_state,
            "frontend/device/kobo/device.lua:1322",
        ));
        if let (Some(knob), Some(governor)) = (&k.governor_knob, &k.default_governor) {
            plan.steps.push(Step::write(
                knob,
                governor,
                "frontend/device/kobo/device.lua:1358 (tickAfterNext defaultCPUGovernor)",
            ));
        }
        plan
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Action;
    use std::path::Path;

    fn kobo_manager(root: &Root, env: Vec<(String, String)>) -> PowerManager {
        PowerManager::kobo(root, PathBuf::from("/mnt/onboard/.adds/kaia"), env, true)
    }

    fn write_file(base: &Path, rel: &str, text: &str) {
        let path = base.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn power_off_clears_alarm_then_syncs_then_calls_init_on_mtk() {
        let dir = tempfile::tempdir().unwrap();
        let pm = PowerManager::kobo(&Root::new(dir.path()), dir.path().into(), vec![], true);
        let plan = pm.power_off();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].action, Action::ClearWakeAlarm);
        assert!(
            matches!(&plan.steps[1].action, Action::Run { program, args, .. } if program == "sync" && args.is_empty())
        );
        assert!(
            matches!(&plan.steps[2].action, Action::Run { program, args, .. } if program == "poweroff" && args.is_empty())
        );
        assert_eq!(plan.steps[1].on_error, OnError::Abort);
        assert_eq!(plan.steps[2].on_error, OnError::Abort);
        assert!(PowerManager::none().power_off().is_skipped());
    }

    #[test]
    fn desktop_plans_are_skipped() {
        let pm = PowerManager::none();
        assert!(pm.prepare_suspend(PowerContext::default()).is_skipped());
        assert!(pm.finish_suspend().is_skipped());
        assert!(pm.clear_wakeup().is_skipped());
        assert!(pm.resume(ResumeContext::default()).is_skipped());
        assert!(pm.standby(PowerContext::default(), None, 0).is_skipped());
        assert!(pm.schedule_wakeup(10, 0).is_skipped());
        assert!(!pm.can_standby());
    }

    #[test]
    fn mtk_skips_suspend_and_standby_while_charging() {
        let dir = tempfile::tempdir().unwrap();
        let pm = kobo_manager(&Root::new(dir.path()), vec![]);
        let charging = PowerContext {
            wifi_on: false,
            charging: true,
        };
        assert!(pm.prepare_suspend(charging).is_skipped());
        assert!(pm.standby(charging, Some(60), 0).is_skipped());
    }

    #[test]
    fn standby_needs_mem_in_power_state() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!kobo_manager(&Root::new(dir.path()), vec![]).can_standby());
        write_file(dir.path(), "sys/power/state", "freeze mem disk\n");
        assert!(kobo_manager(&Root::new(dir.path()), vec![]).can_standby());
    }

    #[test]
    fn suspend_step_list_is_exact() {
        let dir = tempfile::tempdir().unwrap();
        let pm = kobo_manager(&Root::new(dir.path()), vec![]);
        let first = pm.prepare_suspend(PowerContext::default());
        // The 2 s wait is not a step: the caller schedules PRE_SUSPEND_DELAY.
        assert_eq!(first.steps.len(), 1);
        assert_eq!(
            first.steps[0].action,
            Action::Write {
                path: STATE_EXTENDED.to_string(),
                value: "1".to_string()
            }
        );
        assert_eq!(first.steps[0].on_error, OnError::Abort);
        assert!(first.steps.iter().all(|s| !matches!(s.action, Action::Sleep(_))));
        assert_eq!(PRE_SUSPEND_DELAY, Duration::from_secs(2));

        let second = pm.finish_suspend();
        assert_eq!(second.steps.len(), 2);
        assert_eq!(
            second.steps[0].action,
            Action::Run {
                program: "sync".to_string(),
                args: vec![],
                cwd: None,
                env: vec![]
            }
        );
        assert_eq!(
            second.steps[1].action,
            Action::Write {
                path: POWER_STATE.to_string(),
                value: "mem".to_string()
            }
        );
        match &second.steps[1].on_error {
            OnError::Recover(steps) => {
                assert_eq!(steps.len(), 1);
                assert_eq!(
                    steps[0].action,
                    Action::Write {
                        path: STATE_EXTENDED.to_string(),
                        value: "0".to_string()
                    }
                );
            }
            other => panic!("expected Recover, got {other:?}"),
        }
    }

    #[test]
    fn suspend_with_wifi_turns_it_off_first() {
        let dir = tempfile::tempdir().unwrap();
        let env = vec![("INTERFACE".to_string(), "eth0".to_string())];
        let pm = kobo_manager(&Root::new(dir.path()), env.clone());
        let plan = pm.prepare_suspend(PowerContext {
            wifi_on: true,
            charging: false,
        });
        assert_eq!(plan.steps.len(), 3);
        match &plan.steps[0].action {
            Action::Run {
                program, cwd, env: e, ..
            } => {
                assert_eq!(program, "/mnt/onboard/.adds/kaia/release-ip.sh");
                assert_eq!(cwd.as_deref(), Some(Path::new("/mnt/onboard/.adds/kaia")));
                assert_eq!(e, &env);
            }
            other => panic!("expected release-ip.sh, got {other:?}"),
        }
        match &plan.steps[1].action {
            Action::Run { program, .. } => assert_eq!(program, "/mnt/onboard/.adds/kaia/disable-wifi.sh"),
            other => panic!("expected disable-wifi.sh, got {other:?}"),
        }
        assert_eq!(
            plan.steps[2].action,
            Action::Write {
                path: STATE_EXTENDED.to_string(),
                value: "1".to_string()
            }
        );
    }

    #[test]
    fn resume_sequence_with_ir_grid_and_led_knobs() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "sys/class/leds/LED/brightness", "0");
        write_file(
            dir.path(),
            "sys/devices/platform/imx-i2c.1/i2c-1/1-0050/neocmd",
            "",
        );
        let pm = kobo_manager(&Root::new(dir.path()), vec![]);
        let plan = pm.resume(ResumeContext {
            charging_led: Some(true),
            sync_system_clock: false,
        });
        let actions: Vec<Action> = plan.steps.iter().map(|s| s.action.clone()).collect();
        assert_eq!(
            actions,
            vec![
                Action::Write {
                    path: STATE_EXTENDED.to_string(),
                    value: "0".to_string()
                },
                Action::Sleep(Duration::from_millis(100)),
                Action::Write {
                    path: "/sys/devices/platform/imx-i2c.1/i2c-1/1-0050/neocmd".to_string(),
                    value: "a".to_string()
                },
                Action::Write {
                    path: "/sys/class/leds/LED/brightness".to_string(),
                    value: "1".to_string()
                },
            ]
        );
    }

    #[test]
    fn standby_sequence_with_wakeup_and_governor() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "sys/power/state", "mem");
        write_file(
            dir.path(),
            "sys/devices/system/cpu/cpu0/cpufreq/scaling_governor",
            "userspace",
        );
        let pm = kobo_manager(&Root::new(dir.path()), vec![]);
        let plan = pm.standby(PowerContext::default(), Some(300), 1_000);
        let actions: Vec<Action> = plan.steps.iter().map(|s| s.action.clone()).collect();
        let knob = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor".to_string();
        assert_eq!(
            actions,
            vec![
                Action::Write {
                    path: knob.clone(),
                    value: "performance".to_string()
                },
                Action::SetWakeAlarm { epoch: 1300 },
                Action::Sleep(Duration::from_millis(90)),
                Action::Write {
                    path: POWER_STATE.to_string(),
                    value: "mem".to_string()
                },
                Action::Write {
                    path: knob,
                    value: "conservative".to_string()
                },
            ]
        );
    }

    #[test]
    fn resume_can_resync_the_system_clock_last() {
        let dir = tempfile::tempdir().unwrap();
        let pm = kobo_manager(&Root::new(dir.path()), vec![]);
        let plain = pm.resume(ResumeContext::default());
        assert!(plain.steps.iter().all(|s| s.action != Action::RtcToSystemClock));
        let plan = pm.resume(ResumeContext {
            charging_led: None,
            sync_system_clock: true,
        });
        assert_eq!(plan.steps.last().unwrap().action, Action::RtcToSystemClock);
        assert_eq!(plan.steps.last().unwrap().on_error, OnError::Ignore);
    }

    #[test]
    fn wakeup_and_clear_are_single_alarm_steps() {
        let dir = tempfile::tempdir().unwrap();
        let pm = kobo_manager(&Root::new(dir.path()), vec![]);
        let set = pm.schedule_wakeup(600, 2_000);
        assert_eq!(set.steps.len(), 1);
        assert_eq!(set.steps[0].action, Action::SetWakeAlarm { epoch: 2_600 });
        let clear = pm.clear_wakeup();
        assert_eq!(clear.steps.len(), 1);
        assert_eq!(clear.steps[0].action, Action::ClearWakeAlarm);
    }
}
