//! Manual clock editors. Draft values never enter settings.json.
use chrono::{Datelike, Local, NaiveDate, NaiveTime, TimeZone, Timelike};
use kaia_events::Value;
use kaia_i18n::tr;
use kaia_platform::{
    plan::{OnError, Outcome as PlanOutcome, Plan, Step},
    Kind,
};
use kaia_ui::{composite::SettingsDialog, WidgetPod, Window};

use super::{
    entries::{Entry, StepUnit},
    widgets::SettingsPages,
};
use crate::surface::{Outcome, Surface, SurfaceEnv};
use crate::surfaces::{common::root_window, Route};

pub struct ClockSurface {
    date: bool,
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    status: String,
}
impl ClockSurface {
    pub fn new(date: bool) -> Self {
        let now = Local::now();
        Self {
            date,
            year: now.year(),
            month: now.month(),
            day: now.day(),
            hour: now.hour(),
            minute: now.minute(),
            status: String::new(),
        }
    }
    fn rows(&self) -> Vec<Entry> {
        let values = if self.date {
            vec![
                ("clock.year", tr("Year"), self.year as f64, 1970., 2099.),
                ("clock.month", tr("Month"), self.month as f64, 1., 12.),
                ("clock.day", tr("Day"), self.day as f64, 1., 31.),
            ]
        } else {
            vec![
                ("clock.hour", tr("Hour"), self.hour as f64, 0., 23.),
                ("clock.minute", tr("Minute"), self.minute as f64, 0., 59.),
            ]
        };
        let mut rows: Vec<_> = values
            .into_iter()
            .map(|(key, label, value, min, max)| Entry::Stepper {
                key,
                label,
                value,
                min,
                max,
                step: 1.,
                unit: StepUnit::KoreaderUnits,
            })
            .collect();
        if !self.status.is_empty() {
            rows.push(Entry::Text(self.status.clone()));
        }
        rows.push(Entry::Button {
            key: "clock-save",
            label: tr("Save"),
            action: "clock:save".into(),
        });
        rows.push(Entry::Button {
            key: "clock-cancel",
            label: tr("Cancel"),
            action: "back".into(),
        });
        rows
    }
    fn timestamp(&self) -> Option<i64> {
        let now = Local::now();
        let date = if self.date {
            NaiveDate::from_ymd_opt(self.year, self.month, self.day)?
        } else {
            now.date_naive()
        };
        let time = if self.date {
            now.time()
        } else {
            NaiveTime::from_hms_opt(self.hour, self.minute, 0)?
        };
        // Reject both skipped and ambiguous wall times instead of choosing an offset silently.
        Local
            .from_local_datetime(&date.and_time(time))
            .single()
            .map(|d| d.timestamp())
    }
}
/// UTC arguments keep the process time zone out of BusyBox's parsing.
pub fn clock_plan(timestamp: i64) -> Option<Plan> {
    let date = chrono::DateTime::from_timestamp(timestamp, 0)?;
    let text = date.format("%Y-%m-%d %H:%M:%S").to_string();
    Some(Plan::new(vec![
        Step::run("date", &["-u", "-s", &text], None, &[], "manual clock editor").on_error(OnError::Abort),
        Step::run("hwclock", &["-u", "-w"], None, &[], "manual clock editor").on_error(OnError::Abort),
    ]))
}
impl Surface for ClockSurface {
    fn route(&self) -> Route {
        Route::SettingsSection {
            name: if self.date { "date-editor" } else { "time-editor" }.into(),
        }
    }
    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(root_window(
            env.services,
            env.screen,
            SettingsDialog::new(
                tr("Settings"),
                if self.date { tr("Date") } else { tr("Time") },
                WidgetPod::new(SettingsPages::new(self.rows(), 0)),
                "settings:close",
            )
            .back("back")
            .content_header(),
        ))
    }
    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        if let Some(key) = cmd.strip_prefix("settings:set:clock.") {
            let Some(value) = args.first().and_then(Value::as_i64) else {
                return Outcome::Handled;
            };
            match key {
                "year" if (1970..=2099).contains(&value) => self.year = value as i32,
                "month" if (1..=12).contains(&value) => self.month = value as u32,
                "day" if (1..=31).contains(&value) => self.day = value as u32,
                "hour" if (0..=23).contains(&value) => self.hour = value as u32,
                "minute" if (0..=59).contains(&value) => self.minute = value as u32,
                _ => return Outcome::Handled,
            }
            self.status.clear();
            return Outcome::Rebuild;
        }
        if cmd != "clock:save" {
            return Outcome::NotMine;
        }
        let Some(plan) = self.timestamp().and_then(clock_plan) else {
            self.status = tr("Choose a valid date and an unambiguous local time.");
            return Outcome::Rebuild;
        };
        // SystemIo runs host commands even with a fake root. Never set a host clock in tests.
        if !cfg!(target_os = "linux")
            || !matches!(env.services.platform.kind, Kind::Kobo(_))
            || env.services.platform.io_handle().root().base() != std::path::Path::new("/")
        {
            self.status = tr("Desktop preview. The system clock is unchanged.");
            return Outcome::Rebuild;
        }
        self.status = if env.services.platform.run_plan(&plan) == PlanOutcome::Completed {
            if super::keys::network_time(&env.services.settings) {
                tr("Clock saved. Network time is still on.")
            } else {
                tr("Clock saved.")
            }
        } else {
            tr("Could not finish setting the clock. Check the date and time before retrying.")
        };
        Outcome::Rebuild
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_validation_and_utc_plan() {
        let mut editor = ClockSurface::new(true);
        editor.year = 2024;
        editor.month = 2;
        editor.day = 29;
        assert!(editor.timestamp().is_some());
        editor.year = 2023;
        assert!(editor.timestamp().is_none());
        let plan = clock_plan(0).unwrap();
        assert_eq!(plan.steps.len(), 2);
        assert!(
            matches!(&plan.steps[0].action, kaia_platform::plan::Action::Run { program, args, .. } if program == "date" && args == &["-u", "-s", "1970-01-01 00:00:00"])
        );
        assert_eq!(plan.steps[0].on_error, OnError::Abort);
    }
}
