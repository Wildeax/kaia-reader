//! The timing around suspend, as pure state machines.
//!
//! KOReader orders the suspend steps with `UIManager:scheduleIn` and cancels
//! them with `UIManager:unschedule`. The plans in [`crate::power`] hold the
//! device writes. This module holds the order and the timers, so the app can
//! drive them from its event scheduler (`kaia-events`) and tests can check the
//! order without a clock.
//!
//! Two machines:
//!
//! - [`screensaver_suspend_wait`] and [`SUSPEND_WAIT_TIMEOUT`]: how long to wait
//!   after the screensaver is drawn before the suspend sequence starts.
//! - [`SuspendFlow`]: the suspend sequence itself, the wait between its two
//!   halves, and the unexpected wakeup guard.
//!
//! # How the app drives [`SuspendFlow`]
//!
//! The flow never sleeps and never runs a plan. Every method returns a list of
//! [`Effect`]s, and the app carries them out:
//!
//! - [`Effect::Schedule`]: start a one-shot timer. When it fires, call the
//!   method named in the [`Timer`] docs.
//! - [`Effect::Cancel`]: drop that timer if it is still pending.
//! - [`Effect::PrepareSuspend`]: run [`PowerManager::prepare_suspend`](crate::power::PowerManager::prepare_suspend)
//!   and report the outcome with [`SuspendFlow::prepare_done`].
//! - [`Effect::RunFinishSuspend`]: run [`PowerManager::finish_suspend`](crate::power::PowerManager::finish_suspend)
//!   and report with [`SuspendFlow::finish_done`] when it returns. The kernel
//!   returns from the `mem` write on wakeup, so this call returns after resume.
//! - [`Effect::GiveUp`]: send the `UnexpectedWakeupLimit` event so the idle
//!   timer can take over, as `Kobo:checkUnexpectedWakeup` does.
//!
//! The app must call [`SuspendFlow::resume`] on every resume, and on a Power
//! press while a finish timer is pending ([`SuspendFlow::finish_pending`]).
//! That cancel is what keeps a Power press in the 2 second window from being
//! lost (KOReader issue #12325; `device.lua:1405-1409`, `1484`).
//!
//! Sources, all in `frontend/device/`: `generic/device.lua:84` (`suspend_wait_timeout`),
//! `generic/device.lua:393-402` (`getScreensaverSuspendWaitTimeout`),
//! `generic/device.lua:406-409` (`rescheduleSuspend`), `kobo/device.lua:1238-1243`
//! (`rescheduleSuspend` override), `1245-1249` (`scheduleUnexpectedWakeupGuard`),
//! `1251-1280` (`checkUnexpectedWakeup`), `1361-1409` (`suspend`), `1412-1473`
//! (`_doSuspend`), `1476-1514` (`resume`) and `1516` (`usbPlugOut`). Unverified on hardware.

use std::time::Duration;

use crate::plan::Outcome;
use crate::power::PRE_SUSPEND_DELAY;

/// How long to wait after the screensaver call before suspending
/// (`generic/device.lua:84`, `suspend_wait_timeout = 15`). KOReader waits this long
/// because `Screensaver:show()` only enqueues the refresh, and a suspend that
/// starts too soon fails.
pub const SUSPEND_WAIT_TIMEOUT: Duration = Duration::from_secs(15);

/// The delay before the unexpected wakeup check (`kobo/device.lua:1248`).
pub const UNEXPECTED_WAKEUP_CHECK_DELAY: Duration = Duration::from_secs(15);

/// The delay before going back to sleep after a scheduled alarm wakeup
/// (`kobo/device.lua:1263`). The scheduled action needs time to finish first.
pub const SCHEDULED_WAKEUP_SLEEP_DELAY: Duration = Duration::from_secs(30);

/// The number of unexpected wakeups after which the guard gives up
/// (`kobo/device.lua:1267`, `> 20`).
pub const UNEXPECTED_WAKEUP_LIMIT: u32 = 20;

/// The suspend wait when the screensaver flashes the panel for anti-ghosting:
/// `suspend_wait_timeout`, plus the flash window, plus a 2 second margin
/// (`generic/device.lua:396-401`, `getScreensaverSuspendWaitTimeout`). The flash
/// window is 0.5 s before the first flash, `delay_ms` between flashes and 0.5 s
/// after the last. With no flashes the result is 17 seconds.
pub fn screensaver_suspend_wait(extra_flash_count: u32, delay_ms: u32) -> Duration {
    let flash_window = if extra_flash_count > 0 {
        Duration::from_millis(500)
            + Duration::from_millis(u64::from(extra_flash_count - 1) * u64::from(delay_ms))
            + Duration::from_millis(500)
    } else {
        Duration::ZERO
    };
    SUSPEND_WAIT_TIMEOUT + flash_window + Duration::from_secs(2)
}

/// A one-shot timer that the flow asks the app to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timer {
    /// Starts the suspend sequence. When it fires, call [`SuspendFlow::suspend_timer_fired`].
    /// This is `Kobo:suspend` scheduled by `rescheduleSuspend`.
    Suspend,
    /// Ends the wait between the two halves of suspend. When it fires, call
    /// [`SuspendFlow::finish_timer_fired`]. This is `Kobo:_doSuspend`.
    FinishSuspend,
    /// The unexpected wakeup check. When it fires, call [`SuspendFlow::check_timer_fired`].
    /// This is `Kobo:checkUnexpectedWakeup`.
    UnexpectedWakeupCheck,
}

impl Timer {
    const fn index(self) -> usize {
        match self {
            Timer::Suspend => 0,
            Timer::FinishSuspend => 1,
            Timer::UnexpectedWakeupCheck => 2,
        }
    }
}

/// Something the app must do, in the order returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Start a one-shot timer. A timer of the same kind that is still running was
    /// cancelled by an earlier [`Effect::Cancel`] in the same list.
    Schedule {
        /// Which timer.
        timer: Timer,
        /// The delay.
        delay: Duration,
    },
    /// Stop this timer. Only emitted when the flow believes it is pending.
    Cancel(Timer),
    /// Run `PowerManager::prepare_suspend`, then call [`SuspendFlow::prepare_done`].
    PrepareSuspend,
    /// Run `PowerManager::finish_suspend`, then call [`SuspendFlow::finish_done`].
    RunFinishSuspend,
    /// Broadcast `UnexpectedWakeupLimit`. The guard has stopped.
    GiveUp,
}

/// How the prepare plan ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrepareResult {
    /// Every step ran.
    Done,
    /// The plan was skipped (MTK while charging). KOReader goes straight to the guard.
    Skipped,
    /// The `state-extended` write failed. KOReader retries through the guard.
    Failed,
}

impl PrepareResult {
    /// Maps the outcome of running the prepare plan.
    pub fn from_outcome(outcome: &Outcome) -> Self {
        match outcome {
            Outcome::Completed => PrepareResult::Done,
            Outcome::Skipped(_) => PrepareResult::Skipped,
            Outcome::Failed { .. } => PrepareResult::Failed,
        }
    }
}

/// What the scheduled-alarm check found when the guard timer fired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeupCause {
    /// An alarm that Kaia set fired and its action ran
    /// (`wakeup_mgr:isWakeupAlarmScheduled() and wakeup_mgr:wakeupAction(30)`).
    ScheduledAlarm,
    /// Nothing explains the wakeup.
    Unexpected,
}

/// The suspend sequence and the unexpected wakeup guard.
///
/// See the module documentation for how the app drives it. The timers are
/// tracked so that [`Effect::Cancel`] appears only for a timer that is pending,
/// and [`SuspendFlow::finish_pending`] answers the Power press question.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SuspendFlow {
    pending: [bool; 3],
    /// `unexpected_wakeup_count` (`kobo/device.lua`, reset by `resume`).
    unexpected_wakeups: u32,
}

impl SuspendFlow {
    /// A flow with nothing scheduled.
    pub fn new() -> Self {
        SuspendFlow::default()
    }

    /// Whether this timer is pending.
    pub fn is_pending(&self, timer: Timer) -> bool {
        self.pending[timer.index()]
    }

    /// Whether the wait between the two halves of suspend is running. The app asks
    /// this on a Power press; [`SuspendFlow::resume`] cancels it.
    pub fn finish_pending(&self) -> bool {
        self.is_pending(Timer::FinishSuspend)
    }

    /// The number of unexpected wakeups counted since the last resume.
    pub fn unexpected_wakeups(&self) -> u32 {
        self.unexpected_wakeups
    }

    /// Schedules the suspend sequence after the screensaver, dropping every timer
    /// first (`Kobo:rescheduleSuspend`, `kobo/device.lua:1238-1243`, so that the
    /// guard does not trip before the new suspend).
    ///
    /// Pass [`SUSPEND_WAIT_TIMEOUT`] for the default, or the result of
    /// [`screensaver_suspend_wait`] when the screensaver flashes the panel.
    pub fn reschedule_suspend(&mut self, wait: Duration) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.cancel(Timer::Suspend, &mut effects);
        self.cancel(Timer::FinishSuspend, &mut effects);
        self.cancel(Timer::UnexpectedWakeupCheck, &mut effects);
        self.schedule(Timer::Suspend, wait, &mut effects);
        effects
    }

    /// The [`Timer::Suspend`] timer fired. Starts the sequence.
    pub fn suspend_timer_fired(&mut self) -> Vec<Effect> {
        self.pending[Timer::Suspend.index()] = false;
        self.start_suspend()
    }

    /// Starts the suspend sequence now (`Kobo:suspend`, `kobo/device.lua:1361`).
    /// [`SuspendFlow::suspend_timer_fired`] and [`SuspendFlow::check_timer_fired`] call this.
    pub fn start_suspend(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        // A finish timer still pending means two suspends overlapped. KOReader
        // logs it as a bug and drops the old one.
        self.cancel(Timer::FinishSuspend, &mut effects);
        self.cancel(Timer::UnexpectedWakeupCheck, &mut effects);
        effects.push(Effect::PrepareSuspend);
        effects
    }

    /// The prepare plan ended. On success the wait starts. Otherwise the guard
    /// arms, so the device tries again later (`kobo/device.lua:1362-1403`).
    pub fn prepare_done(&mut self, result: PrepareResult) -> Vec<Effect> {
        let mut effects = Vec::new();
        match result {
            PrepareResult::Done => self.schedule(Timer::FinishSuspend, PRE_SUSPEND_DELAY, &mut effects),
            PrepareResult::Skipped | PrepareResult::Failed => self.arm_guard(&mut effects),
        }
        effects
    }

    /// The [`Timer::FinishSuspend`] timer fired. Runs the second half.
    pub fn finish_timer_fired(&mut self) -> Vec<Effect> {
        self.pending[Timer::FinishSuspend.index()] = false;
        vec![Effect::RunFinishSuspend]
    }

    /// The second half returned, which means the device woke up or the kernel
    /// refused to suspend. The guard arms either way (`kobo/device.lua:1471`).
    pub fn finish_done(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.arm_guard(&mut effects);
        effects
    }

    /// A resume or a Power press. Resets the wakeup counter and cancels every
    /// pending timer, including the wait between the two halves of suspend
    /// (`Kobo:resume`, `kobo/device.lua:1477-1485`).
    ///
    /// Call it on every resume. Also call it when a Power press arrives while
    /// [`SuspendFlow::finish_pending`] is true, or the press is lost
    /// (KOReader issue #12325).
    pub fn resume(&mut self) -> Vec<Effect> {
        self.unexpected_wakeups = 0;
        let mut effects = Vec::new();
        self.cancel(Timer::UnexpectedWakeupCheck, &mut effects);
        self.cancel(Timer::Suspend, &mut effects);
        self.cancel(Timer::FinishSuspend, &mut effects);
        effects
    }

    /// The [`Timer::UnexpectedWakeupCheck`] timer fired
    /// (`Kobo:checkUnexpectedWakeup`, `kobo/device.lua:1249-1278`).
    ///
    /// With a scheduled alarm, the device goes back to sleep after
    /// [`SCHEDULED_WAKEUP_SLEEP_DELAY`]. Otherwise the wakeup is unexpected: after
    /// [`UNEXPECTED_WAKEUP_LIMIT`] of them the guard gives up, and before that
    /// the suspend sequence starts again.
    pub fn check_timer_fired(&mut self, cause: WakeupCause) -> Vec<Effect> {
        self.pending[Timer::UnexpectedWakeupCheck.index()] = false;
        let mut effects = Vec::new();
        // Another event, such as a sleep cover close, may have scheduled a suspend too.
        self.cancel(Timer::Suspend, &mut effects);
        match cause {
            WakeupCause::ScheduledAlarm => {
                self.schedule(Timer::Suspend, SCHEDULED_WAKEUP_SLEEP_DELAY, &mut effects);
            }
            WakeupCause::Unexpected if self.unexpected_wakeups > UNEXPECTED_WAKEUP_LIMIT => {
                effects.push(Effect::GiveUp);
            }
            WakeupCause::Unexpected => effects.extend(self.start_suspend()),
        }
        effects
    }

    /// The charger was unplugged. Rewinds the counter to 1 when it is above 0,
    /// because power savings matter again (`Kobo:usbPlugOut`). Only a resume
    /// resets it to 0.
    pub fn usb_plugged_out(&mut self) {
        if self.unexpected_wakeups > 0 {
            self.unexpected_wakeups = 1;
        }
    }

    fn arm_guard(&mut self, effects: &mut Vec<Effect>) {
        self.unexpected_wakeups += 1;
        self.schedule(
            Timer::UnexpectedWakeupCheck,
            UNEXPECTED_WAKEUP_CHECK_DELAY,
            effects,
        );
    }

    fn schedule(&mut self, timer: Timer, delay: Duration, effects: &mut Vec<Effect>) {
        self.pending[timer.index()] = true;
        effects.push(Effect::Schedule { timer, delay });
    }

    fn cancel(&mut self, timer: Timer, effects: &mut Vec<Effect>) {
        if std::mem::take(&mut self.pending[timer.index()]) {
            effects.push(Effect::Cancel(timer));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(timer: Timer, secs: u64) -> Effect {
        Effect::Schedule {
            timer,
            delay: Duration::from_secs(secs),
        }
    }

    #[test]
    fn screensaver_wait_is_15_s_plus_margin_without_flashes() {
        assert_eq!(screensaver_suspend_wait(0, 500), Duration::from_secs(17));
    }

    #[test]
    fn screensaver_wait_adds_the_flash_window() {
        // 3 flashes, 400 ms apart: 0.5 + 2 * 0.4 + 0.5 = 1.8 s on top of 17 s.
        assert_eq!(screensaver_suspend_wait(3, 400), Duration::from_millis(18_800));
        // One flash has no gap: 0.5 + 0.5 = 1 s.
        assert_eq!(screensaver_suspend_wait(1, 400), Duration::from_secs(18));
    }

    #[test]
    fn screensaver_then_suspend_waits_15_s_before_prepare() {
        let mut flow = SuspendFlow::new();
        assert_eq!(
            flow.reschedule_suspend(SUSPEND_WAIT_TIMEOUT),
            vec![schedule(Timer::Suspend, 15)]
        );
        assert!(flow.is_pending(Timer::Suspend));
        assert_eq!(flow.suspend_timer_fired(), vec![Effect::PrepareSuspend]);
        assert!(!flow.is_pending(Timer::Suspend));
    }

    #[test]
    fn rescheduling_replaces_the_pending_suspend_and_the_guard() {
        let mut flow = SuspendFlow::new();
        flow.reschedule_suspend(SUSPEND_WAIT_TIMEOUT);
        flow.finish_done();
        assert!(flow.is_pending(Timer::UnexpectedWakeupCheck));
        // A second screensaver (for example a sleep cover close) restarts the wait.
        let effects = flow.reschedule_suspend(Duration::from_secs(17));
        assert_eq!(
            effects,
            vec![
                Effect::Cancel(Timer::Suspend),
                Effect::Cancel(Timer::UnexpectedWakeupCheck),
                schedule(Timer::Suspend, 17),
            ]
        );
    }

    #[test]
    fn full_suspend_sequence_in_order() {
        let mut flow = SuspendFlow::new();
        flow.reschedule_suspend(SUSPEND_WAIT_TIMEOUT);
        assert_eq!(flow.suspend_timer_fired(), vec![Effect::PrepareSuspend]);
        // The 2 s wait is a timer, not a sleep.
        assert_eq!(
            flow.prepare_done(PrepareResult::Done),
            vec![schedule(Timer::FinishSuspend, 2)]
        );
        assert!(flow.finish_pending());
        assert_eq!(flow.finish_timer_fired(), vec![Effect::RunFinishSuspend]);
        assert!(!flow.finish_pending());
        // The write returned: the device woke up. The guard arms.
        assert_eq!(
            flow.finish_done(),
            vec![schedule(Timer::UnexpectedWakeupCheck, 15)]
        );
        assert_eq!(flow.unexpected_wakeups(), 1);
    }

    #[test]
    fn power_press_during_the_2_s_wait_cancels_the_suspend() {
        let mut flow = SuspendFlow::new();
        flow.reschedule_suspend(SUSPEND_WAIT_TIMEOUT);
        flow.suspend_timer_fired();
        flow.prepare_done(PrepareResult::Done);
        assert!(flow.finish_pending());
        // The press arrives before the 2 s elapse. The app calls resume().
        assert_eq!(flow.resume(), vec![Effect::Cancel(Timer::FinishSuspend)]);
        assert!(!flow.finish_pending());
        assert_eq!(flow.unexpected_wakeups(), 0);
    }

    #[test]
    fn skipped_or_failed_prepare_arms_the_guard_without_a_wait() {
        for result in [PrepareResult::Skipped, PrepareResult::Failed] {
            let mut flow = SuspendFlow::new();
            flow.reschedule_suspend(SUSPEND_WAIT_TIMEOUT);
            flow.suspend_timer_fired();
            assert_eq!(
                flow.prepare_done(result),
                vec![schedule(Timer::UnexpectedWakeupCheck, 15)]
            );
            assert!(!flow.finish_pending());
        }
    }

    #[test]
    fn overlapping_suspend_drops_the_old_wait() {
        let mut flow = SuspendFlow::new();
        flow.suspend_timer_fired();
        flow.prepare_done(PrepareResult::Done);
        assert_eq!(
            flow.start_suspend(),
            vec![Effect::Cancel(Timer::FinishSuspend), Effect::PrepareSuspend]
        );
    }

    #[test]
    fn unexpected_wakeup_puts_the_device_back_to_sleep() {
        let mut flow = SuspendFlow::new();
        flow.finish_done();
        let effects = flow.check_timer_fired(WakeupCause::Unexpected);
        assert_eq!(effects, vec![Effect::PrepareSuspend]);
        assert!(!flow.is_pending(Timer::UnexpectedWakeupCheck));
    }

    #[test]
    fn scheduled_alarm_wakeup_sleeps_again_after_30_s() {
        let mut flow = SuspendFlow::new();
        flow.finish_done();
        let effects = flow.check_timer_fired(WakeupCause::ScheduledAlarm);
        assert_eq!(effects, vec![schedule(Timer::Suspend, 30)]);
    }

    #[test]
    fn check_cancels_a_suspend_scheduled_by_another_event() {
        let mut flow = SuspendFlow::new();
        flow.reschedule_suspend(SUSPEND_WAIT_TIMEOUT);
        flow.finish_done();
        let effects = flow.check_timer_fired(WakeupCause::ScheduledAlarm);
        assert_eq!(
            effects,
            vec![Effect::Cancel(Timer::Suspend), schedule(Timer::Suspend, 30)]
        );
    }

    #[test]
    fn guard_gives_up_after_20_unexpected_wakeups() {
        let mut flow = SuspendFlow::new();
        // Twenty rounds of: suspend, wake up unexpectedly, check, try again.
        for round in 1..=UNEXPECTED_WAKEUP_LIMIT {
            flow.finish_done();
            assert_eq!(flow.unexpected_wakeups(), round);
            let effects = flow.check_timer_fired(WakeupCause::Unexpected);
            assert_eq!(effects, vec![Effect::PrepareSuspend], "round {round}");
        }
        // The 21st wakeup is over the limit.
        flow.finish_done();
        assert_eq!(flow.unexpected_wakeups(), 21);
        assert_eq!(
            flow.check_timer_fired(WakeupCause::Unexpected),
            vec![Effect::GiveUp]
        );
        assert!(!flow.is_pending(Timer::UnexpectedWakeupCheck));
    }

    #[test]
    fn a_real_resume_resets_the_counter_and_the_guard() {
        let mut flow = SuspendFlow::new();
        flow.finish_done();
        flow.finish_done();
        assert_eq!(flow.unexpected_wakeups(), 2);
        assert_eq!(flow.resume(), vec![Effect::Cancel(Timer::UnexpectedWakeupCheck)]);
        assert_eq!(flow.unexpected_wakeups(), 0);
        assert!(flow.resume().is_empty());
    }

    #[test]
    fn unplugging_rewinds_the_counter_to_one() {
        let mut flow = SuspendFlow::new();
        flow.usb_plugged_out();
        assert_eq!(flow.unexpected_wakeups(), 0);
        for _ in 0..7 {
            flow.finish_done();
        }
        flow.usb_plugged_out();
        assert_eq!(flow.unexpected_wakeups(), 1);
    }

    #[test]
    fn prepare_result_maps_plan_outcomes() {
        assert_eq!(
            PrepareResult::from_outcome(&Outcome::Completed),
            PrepareResult::Done
        );
        assert_eq!(
            PrepareResult::from_outcome(&Outcome::Skipped("charging")),
            PrepareResult::Skipped
        );
        assert_eq!(
            PrepareResult::from_outcome(&Outcome::Failed {
                step: 0,
                recovered: false
            }),
            PrepareResult::Failed
        );
    }
}
