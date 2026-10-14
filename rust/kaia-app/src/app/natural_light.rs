//! Natural light targets use local clock time. Fades use the app clock.

use std::time::Duration;

use chrono::{Local, NaiveDateTime, Timelike};
use kaia_events::Value;
use kaia_i18n::{format as tr_format, tr};

use super::App;
use crate::backend::Backend;
use crate::surfaces::settings::keys;

const DAY: u32 = 24 * 60 * 60;
const MORNING: u32 = 5 * 60 * 60;
const DAY_START: u32 = 7 * 60 * 60;
const EVENING: u32 = 18 * 60 * 60;
const HALF_HOUR: u32 = 30 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Schedule {
    bedtime_minutes: u16,
    day: u8,
    bedtime: u8,
}

impl Schedule {
    fn read(settings: &kaia_events::Settings) -> Self {
        Self {
            bedtime_minutes: number(settings, keys::DISPLAY_NATURAL_LIGHT_BEDTIME, 1380.0, 1439.0) as u16,
            day: number(settings, keys::DISPLAY_NATURAL_LIGHT_DAY_WARMTH, 0.0, 100.0) as u8,
            bedtime: number(settings, keys::DISPLAY_NATURAL_LIGHT_BEDTIME_WARMTH, 92.0, 100.0) as u8,
        }
    }

    /// Returns the target now and seconds until the next clock boundary.
    fn at(self, seconds: u32) -> (u8, u32) {
        let seconds = seconds % DAY;
        let mut bedtime = u32::from(self.bedtime_minutes) * 60;
        if bedtime >= DAY || (MORNING..=EVENING).contains(&bedtime) {
            bedtime = 23 * 60 * 60;
        }
        if bedtime < MORNING {
            bedtime += DAY;
        }
        let evening_time = if seconds < MORNING { seconds + DAY } else { seconds };
        let fraction = if (DAY_START..EVENING).contains(&seconds) {
            0.0
        } else if (MORNING..DAY_START).contains(&seconds) {
            f64::from(DAY_START - seconds) / f64::from(DAY_START - MORNING)
        } else if evening_time < bedtime {
            f64::from(evening_time - EVENING) / f64::from(bedtime - EVENING)
        } else {
            1.0
        };
        let warmth =
            (f64::from(self.day) + (f64::from(self.bedtime) - f64::from(self.day)) * fraction).round() as u8;
        let next = if (DAY_START..EVENING).contains(&seconds) {
            EVENING
        } else {
            let half_hour = (seconds / HALF_HOUR + 1) * HALF_HOUR;
            let bed_today = bedtime % DAY;
            if bed_today > seconds {
                half_hour.min(bed_today)
            } else {
                half_hour
            }
        };
        (warmth, next - seconds)
    }
}

fn number(settings: &kaia_events::Settings, key: &str, default: f64, max: f64) -> f64 {
    let value = keys::number_or(settings, key, default);
    if value.is_finite() {
        value.round().clamp(0.0, max)
    } else {
        default
    }
}

struct Fade {
    start: Duration,
    duration: Duration,
    from: u8,
    to: u8,
}

pub(super) struct State {
    local_now: Box<dyn Fn() -> NaiveDateTime>,
    revision: u64,
    config: Option<Schedule>,
    next_boundary: Option<NaiveDateTime>,
    paused_until: Option<NaiveDateTime>,
    last_local: Option<NaiveDateTime>,
    fade: Option<Fade>,
    wake: Duration,
}

impl State {
    pub(super) fn new(revision: u64) -> Self {
        Self {
            local_now: Box::new(|| Local::now().naive_local()),
            revision,
            config: None,
            next_boundary: None,
            paused_until: None,
            last_local: None,
            fade: None,
            wake: Duration::MAX,
        }
    }

    pub(super) fn wait(&self, now: Duration) -> Duration {
        self.wake.saturating_sub(now)
    }
}

impl<B: Backend> App<B> {
    /// Supplies local date and time for natural light. Tests can advance it with the app clock.
    pub fn set_natural_light_clock(&mut self, now: impl Fn() -> NaiveDateTime + 'static) {
        self.natural_light.local_now = Box::new(now);
        self.natural_light.next_boundary = None;
        self.natural_light.last_local = None;
    }

    pub(super) fn natural_light_due(&mut self) {
        let inactive = self.power_off_requested() || self.sleeping();
        let state = &mut self.natural_light;
        let light = &mut self.services.platform.frontlight;
        let revision = light.warmth_revision();
        let manual = revision != state.revision;
        state.revision = revision;
        let settings = &mut self.services.settings;
        if !settings.is_true(keys::DISPLAY_NATURAL_LIGHT_ENABLED) {
            state.config = None;
            state.fade = None;
            state.next_boundary = None;
            state.paused_until = None;
            state.wake = Duration::MAX;
            return;
        }
        let local = (state.local_now)();
        let now = self.ctx.scheduler.now();
        let schedule = Schedule::read(settings);
        let (target, seconds) = schedule.at(local.num_seconds_from_midnight());
        let boundary = local.with_nanosecond(0).unwrap() + chrono::Duration::seconds(i64::from(seconds));
        let clock_back = state.last_local.is_some_and(|previous| local < previous);
        state.last_local = Some(local);
        if manual || (clock_back && state.paused_until.is_some()) {
            state.paused_until = Some(boundary);
            state.fade = None;
        }
        if state.paused_until.is_some_and(|until| local >= until) {
            state.paused_until = None;
            state.next_boundary = None;
        }
        if clock_back || state.config != Some(schedule) {
            state.next_boundary = None;
            state.fade = None;
        }
        state.config = Some(schedule);
        // Check wall clock changes at least once a minute, even during the daytime hold.
        state.wake = now + Duration::from_secs(60);
        if !light.is_on() || !light.target_is_on() || inactive {
            state.fade = None;
            state.next_boundary = None;
            return;
        }
        if let Some(until) = state.paused_until {
            state.wake = state.wake.min(now + (until - local).to_std().unwrap_or_default());
            return;
        }
        if state.next_boundary.is_none_or(|next| local >= next) {
            state.next_boundary = Some(boundary);
            let from = light.warmth();
            state.fade = (from != target).then(|| Fade {
                start: now,
                duration: Duration::from_secs_f64(
                    (60.0 * f64::from(from.abs_diff(target))
                        / f64::from(schedule.day.abs_diff(schedule.bedtime).max(1)))
                    .min(60.0),
                ),
                from,
                to: target,
            });
        }
        if let Some(next) = state.next_boundary {
            state.wake = state.wake.min(now + (next - local).to_std().unwrap_or_default());
        }
        let Some(fade) = &state.fade else { return };
        let elapsed = now.saturating_sub(fade.start).min(fade.duration);
        let total = u128::from(fade.from.abs_diff(fade.to));
        let distance = (elapsed.as_nanos() * total / fade.duration.as_nanos()) as u8;
        let value = if fade.to > fade.from {
            fade.from + distance
        } else {
            fade.from - distance
        };
        if value != light.warmth() {
            if let Err(error) = light.set_warmth(value) {
                self.notices.push(tr_format(
                    &tr("Cannot set natural light: %1"),
                    &[&error.to_string()],
                ));
                state.fade = None;
                return;
            }
            state.revision = light.warmth_revision();
            settings.save("display.warmth", Value::Number(f64::from(value)));
        }
        if elapsed >= fade.duration {
            state.fade = None;
        } else {
            let offset = (fade.duration.as_nanos() * (u128::from(distance) + 1)).div_ceil(total);
            state.wake = state.wake.min(fade.start + Duration::from_nanos(offset as u64));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schedule(bedtime_minutes: u16) -> Schedule {
        Schedule {
            bedtime_minutes,
            day: 0,
            bedtime: 92,
        }
    }

    #[test]
    fn natural_light_stock_boundaries_and_half_hours() {
        let s = schedule(23 * 60);
        for (hour, minute, warmth, next) in [
            (5, 0, 92, 1800),
            (6, 0, 46, 1800),
            (7, 0, 0, 11 * 3600),
            (18, 0, 0, 1800),
            (18, 30, 9, 1800),
            (20, 30, 46, 1800),
            (23, 0, 92, 1800),
            (23, 59, 92, 60),
        ] {
            assert_eq!(s.at(hour * 3600 + minute * 60), (warmth, next));
        }
    }

    #[test]
    fn natural_light_daytime_bedtimes_fall_back_to_2300() {
        for bedtime in [5 * 60, 7 * 60, 12 * 60, 18 * 60, 1440] {
            assert_eq!(schedule(bedtime).at(21 * 3600), schedule(23 * 60).at(21 * 3600));
        }
    }

    #[test]
    fn natural_light_bedtime_wraps_past_midnight() {
        let s = schedule(30);
        assert_eq!(s.at(18 * 3600), (0, 1800));
        assert_eq!(s.at(21 * 3600 + 15 * 60).0, 46);
        assert_eq!(s.at(0), (85, 1800));
        assert_eq!(s.at(30 * 60), (92, 1800));
        assert_eq!(s.at(5 * 3600), (92, 1800));
        assert_eq!(s.at(7 * 3600), (0, 11 * 3600));
        assert_eq!(schedule(0).at(0), (92, 1800));
    }

    #[test]
    fn natural_light_includes_bedtime_between_half_hours() {
        assert_eq!(schedule(23 * 60 + 15).at(23 * 3600).1, 900);
        assert_eq!(schedule(23 * 60 + 15).at(23 * 3600 + 900), (92, 900));
    }

    #[test]
    fn natural_light_handles_equal_and_reversed_targets() {
        let s = Schedule {
            bedtime_minutes: 1380,
            day: 80,
            bedtime: 20,
        };
        assert_eq!(s.at(6 * 3600).0, 50);
        assert_eq!(s.at(7 * 3600).0, 80);
        assert_eq!(s.at(23 * 3600).0, 20);
        assert_eq!(Schedule { bedtime: 80, ..s }.at(6 * 3600).0, 80);
    }
}
