//! Timestamps in KOReader's local `YYYY-MM-DD HH:MM:SS` form.

use chrono::Local;

/// The current local time as `YYYY-MM-DD HH:MM:SS`.
pub(crate) fn now_datetime() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// The current local date as `YYYY-MM-DD`.
pub(crate) fn today() -> String {
    now_datetime()[..10].to_string()
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
