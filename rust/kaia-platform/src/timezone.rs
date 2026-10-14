//! Time zones for Kaia's own clock.
//!
//! Kaia sets the process `TZ` variable to a POSIX rule from [`ZONES`]. Chrono's
//! `Local` and libc read the rule directly, so the zone works whether or not
//! the device ships `/usr/share/zoneinfo`. The rules give the current offset
//! and the current daylight saving rule. Past changes to a zone are not modeled.
//!
//! Kaia never changes the system zone. `/etc/localtime`, `date` and Nickel's
//! settings stay as they are. Without a saved zone, the process keeps the `TZ`
//! it started with, or the system zone when it has none.
//!
//! KOReader sets no `TZ` on Kobo, and its Kobo code reads no zoneinfo files.
//! Whether the Libra Colour ships `/usr/share/zoneinfo` is not verified. POSIX
//! rules work either way, so the table uses them.

/// One zone of the picker. The names are IANA names, the values Kaia stores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    /// IANA name, such as `America/Bogota`. This is the stored value.
    pub name: &'static str,
    /// Offset from UTC in minutes, outside daylight saving time.
    pub standard_offset_minutes: i32,
    /// POSIX `TZ` rule with the same offsets and daylight saving dates.
    pub rule: &'static str,
}

/// The zones on offer, in alphabetical order. The picker sorts them by offset.
#[rustfmt::skip]
pub const ZONES: &[Zone] = &[
    Zone { name: "Africa/Johannesburg", standard_offset_minutes: 120, rule: "SAST-2" },
    Zone { name: "Africa/Lagos", standard_offset_minutes: 60, rule: "WAT-1" },
    Zone { name: "Africa/Nairobi", standard_offset_minutes: 180, rule: "EAT-3" },
    Zone { name: "America/Anchorage", standard_offset_minutes: -540, rule: "AKST9AKDT,M3.2.0,M11.1.0" },
    Zone { name: "America/Argentina/Buenos_Aires", standard_offset_minutes: -180, rule: "<-03>3" },
    Zone { name: "America/Bogota", standard_offset_minutes: -300, rule: "<-05>5" },
    Zone { name: "America/Caracas", standard_offset_minutes: -240, rule: "<-04>4" },
    Zone { name: "America/Chicago", standard_offset_minutes: -360, rule: "CST6CDT,M3.2.0,M11.1.0" },
    Zone { name: "America/Denver", standard_offset_minutes: -420, rule: "MST7MDT,M3.2.0,M11.1.0" },
    Zone { name: "America/Halifax", standard_offset_minutes: -240, rule: "AST4ADT,M3.2.0,M11.1.0" },
    Zone { name: "America/Lima", standard_offset_minutes: -300, rule: "<-05>5" },
    Zone { name: "America/Los_Angeles", standard_offset_minutes: -480, rule: "PST8PDT,M3.2.0,M11.1.0" },
    Zone { name: "America/Mexico_City", standard_offset_minutes: -360, rule: "CST6" },
    Zone { name: "America/New_York", standard_offset_minutes: -300, rule: "EST5EDT,M3.2.0,M11.1.0" },
    Zone { name: "America/Phoenix", standard_offset_minutes: -420, rule: "MST7" },
    Zone { name: "America/Sao_Paulo", standard_offset_minutes: -180, rule: "<-03>3" },
    Zone { name: "America/St_Johns", standard_offset_minutes: -210, rule: "NST3:30NDT,M3.2.0,M11.1.0" },
    Zone { name: "America/Toronto", standard_offset_minutes: -300, rule: "EST5EDT,M3.2.0,M11.1.0" },
    Zone { name: "America/Vancouver", standard_offset_minutes: -480, rule: "PST8PDT,M3.2.0,M11.1.0" },
    Zone { name: "Asia/Bangkok", standard_offset_minutes: 420, rule: "<+07>-7" },
    Zone { name: "Asia/Dhaka", standard_offset_minutes: 360, rule: "<+06>-6" },
    Zone { name: "Asia/Dubai", standard_offset_minutes: 240, rule: "<+04>-4" },
    Zone { name: "Asia/Hong_Kong", standard_offset_minutes: 480, rule: "HKT-8" },
    Zone { name: "Asia/Jakarta", standard_offset_minutes: 420, rule: "WIB-7" },
    Zone { name: "Asia/Karachi", standard_offset_minutes: 300, rule: "PKT-5" },
    Zone { name: "Asia/Kathmandu", standard_offset_minutes: 345, rule: "<+0545>-5:45" },
    Zone { name: "Asia/Kolkata", standard_offset_minutes: 330, rule: "IST-5:30" },
    Zone { name: "Asia/Manila", standard_offset_minutes: 480, rule: "PST-8" },
    Zone { name: "Asia/Seoul", standard_offset_minutes: 540, rule: "KST-9" },
    Zone { name: "Asia/Shanghai", standard_offset_minutes: 480, rule: "CST-8" },
    Zone { name: "Asia/Singapore", standard_offset_minutes: 480, rule: "<+08>-8" },
    Zone { name: "Asia/Tokyo", standard_offset_minutes: 540, rule: "JST-9" },
    Zone { name: "Australia/Adelaide", standard_offset_minutes: 570, rule: "ACST-9:30ACDT,M10.1.0,M4.1.0/3" },
    Zone { name: "Australia/Brisbane", standard_offset_minutes: 600, rule: "AEST-10" },
    Zone { name: "Australia/Melbourne", standard_offset_minutes: 600, rule: "AEST-10AEDT,M10.1.0,M4.1.0/3" },
    Zone { name: "Australia/Perth", standard_offset_minutes: 480, rule: "AWST-8" },
    Zone { name: "Australia/Sydney", standard_offset_minutes: 600, rule: "AEST-10AEDT,M10.1.0,M4.1.0/3" },
    Zone { name: "Europe/Amsterdam", standard_offset_minutes: 60, rule: "CET-1CEST,M3.5.0,M10.5.0/3" },
    Zone { name: "Europe/Athens", standard_offset_minutes: 120, rule: "EET-2EEST,M3.5.0/3,M10.5.0/4" },
    Zone { name: "Europe/Helsinki", standard_offset_minutes: 120, rule: "EET-2EEST,M3.5.0/3,M10.5.0/4" },
    Zone { name: "Europe/Berlin", standard_offset_minutes: 60, rule: "CET-1CEST,M3.5.0,M10.5.0/3" },
    Zone { name: "Europe/Istanbul", standard_offset_minutes: 180, rule: "<+03>-3" },
    Zone { name: "Europe/Lisbon", standard_offset_minutes: 0, rule: "WET0WEST,M3.5.0/1,M10.5.0" },
    Zone { name: "Europe/London", standard_offset_minutes: 0, rule: "GMT0BST,M3.5.0/1,M10.5.0" },
    Zone { name: "Europe/Madrid", standard_offset_minutes: 60, rule: "CET-1CEST,M3.5.0,M10.5.0/3" },
    Zone { name: "Europe/Moscow", standard_offset_minutes: 180, rule: "MSK-3" },
    Zone { name: "Europe/Paris", standard_offset_minutes: 60, rule: "CET-1CEST,M3.5.0,M10.5.0/3" },
    Zone { name: "Europe/Rome", standard_offset_minutes: 60, rule: "CET-1CEST,M3.5.0,M10.5.0/3" },
    Zone { name: "Europe/Warsaw", standard_offset_minutes: 60, rule: "CET-1CEST,M3.5.0,M10.5.0/3" },
    Zone { name: "Pacific/Auckland", standard_offset_minutes: 720, rule: "NZST-12NZDT,M9.5.0,M4.1.0/3" },
    Zone { name: "Pacific/Honolulu", standard_offset_minutes: -600, rule: "HST10" },
    Zone { name: "UTC", standard_offset_minutes: 0, rule: "UTC0" },
];

/// The zone with `name`, or `None` for a name that is not on offer.
pub fn find(name: &str) -> Option<&'static Zone> {
    ZONES.iter().find(|zone| zone.name == name)
}

/// The zones in the order the picker lists them: by standard offset, then by name.
pub fn sorted() -> Vec<&'static Zone> {
    let mut zones: Vec<&'static Zone> = ZONES.iter().collect();
    zones.sort_by(|a, b| {
        a.standard_offset_minutes
            .cmp(&b.standard_offset_minutes)
            .then_with(|| a.name.cmp(b.name))
    });
    zones
}

/// The standard offset as `UTC+05:45` or `UTC-05:00`.
pub fn offset_label(zone: &Zone) -> String {
    let minutes = zone.standard_offset_minutes;
    let sign = if minutes < 0 { '-' } else { '+' };
    let abs = minutes.abs();
    format!("UTC{sign}{:02}:{:02}", abs / 60, abs % 60)
}

/// Sets the process `TZ` to the rule of `zone`. With no zone, the variable is
/// left as it is, which is the system zone.
///
/// Call it once at startup, before any thread starts and before any local time
/// is read. `std::env::set_var` races with C code that reads the environment
/// from another thread, such as SQLite or the HTTP client. A zone chosen while
/// the app runs therefore takes effect at the next start.
pub fn apply(zone: Option<&Zone>) {
    if let Some(zone) = zone {
        std::env::set_var("TZ", zone.rule);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_rules_are_plain() {
        let mut names: Vec<&str> = ZONES.iter().map(|z| z.name).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count);
        for zone in ZONES {
            assert!(!zone.rule.is_empty(), "{}", zone.name);
            assert!(!zone.rule.contains(char::is_whitespace), "{}", zone.name);
        }
    }

    #[test]
    fn sorted_puts_offsets_first_then_names() {
        let zones = sorted();
        assert_eq!(zones.len(), ZONES.len());
        for pair in zones.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                (a.standard_offset_minutes, a.name) <= (b.standard_offset_minutes, b.name),
                "{} before {}",
                a.name,
                b.name
            );
        }
        assert_eq!(zones.first().unwrap().name, "Pacific/Honolulu");
        assert_eq!(zones.last().unwrap().name, "Pacific/Auckland");
    }

    #[test]
    fn find_knows_only_the_table() {
        assert_eq!(find("America/Bogota").unwrap().rule, "<-05>5");
        assert!(find("Mars/Olympus").is_none());
        assert!(find("").is_none());
        assert!(find("america/bogota").is_none());
    }

    #[test]
    fn offset_labels_show_sign_and_minutes() {
        assert_eq!(offset_label(find("America/Bogota").unwrap()), "UTC-05:00");
        assert_eq!(offset_label(find("Asia/Kathmandu").unwrap()), "UTC+05:45");
        assert_eq!(offset_label(find("UTC").unwrap()), "UTC+00:00");
    }
}
