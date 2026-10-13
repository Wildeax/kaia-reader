//! Timestamps in KOReader's shape: `YYYY-MM-DD HH:MM:SS`. Kaia writes UTC,
//! because the crate has no time zone database. KOReader writes local time,
//! so imported times can differ by the device's offset. The strings still sort
//! in time order.

use std::time::{SystemTime, UNIX_EPOCH};

/// The current time as `YYYY-MM-DD HH:MM:SS` (UTC).
pub(crate) fn now_datetime() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    format_datetime(secs)
}

/// The current date as `YYYY-MM-DD` (UTC).
pub(crate) fn today() -> String {
    now_datetime()[..10].to_string()
}

/// Formats unix seconds as `YYYY-MM-DD HH:MM:SS` (UTC).
pub(crate) fn format_datetime(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_known_instants() {
        assert_eq!(format_datetime(0), "1970-01-01 00:00:00");
        assert_eq!(format_datetime(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(format_datetime(1_760_000_000), "2025-10-09 08:53:20");
    }
}
