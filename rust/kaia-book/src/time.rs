//! Timestamps in KOReader's local `YYYY-MM-DD HH:MM:SS` form.

use chrono::{Local, TimeZone};

/// The current local time as `YYYY-MM-DD HH:MM:SS`.
pub(crate) fn now_datetime() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// The current local date as `YYYY-MM-DD`.
pub(crate) fn today() -> String {
    now_datetime()[..10].to_string()
}

/// The local calendar date of a unix time, as `YYYY-MM-DD`. Empty when the time
/// is out of the range the calendar holds.
pub fn local_date(unix: i64) -> String {
    Local
        .timestamp_opt(unix, 0)
        .single()
        .map(|t| t.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_uses_local_calendar_time() {
        assert_eq!(
            now_datetime(),
            Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
        );
    }
}
