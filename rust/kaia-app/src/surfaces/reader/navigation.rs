//! Validation for the reader's page and percent entry.

pub(super) fn entered_page(text: &str, percent: bool, count: usize) -> Option<u32> {
    if count == 0 {
        return None;
    }
    let value: f64 = text.trim().trim_end_matches('%').trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    if percent {
        if !(0.0..=100.0).contains(&value) {
            return None;
        }
        Some(((value / 100.0 * count as f64).round() as i64 - 1).clamp(0, count as i64 - 1) as u32)
    } else if value >= 1.0 && value <= count as f64 && value.fract() == 0.0 {
        Some(value as u32 - 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_page_and_percent_boundaries() {
        for (input, expected) in [
            ("1", Some(0)),
            ("10", Some(9)),
            ("0", None),
            ("11", None),
            ("1.5", None),
        ] {
            assert_eq!(entered_page(input, false, 10), expected);
        }
        for (input, expected) in [
            ("0", Some(0)),
            ("50%", Some(4)),
            ("100", Some(9)),
            ("-1", None),
            ("101", None),
            ("NaN", None),
            ("inf", None),
            ("", None),
        ] {
            assert_eq!(entered_page(input, true, 10), expected);
        }
        assert_eq!(entered_page("1", false, 0), None);
    }
}
