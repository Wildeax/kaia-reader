use std::fmt;

/// A value that breaks a rule in `STORAGE.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Name of the offending field, for example `"quote.exact"`.
    pub field: &'static str,
    /// Human-readable reason.
    pub reason: String,
}

impl ValidationError {
    pub fn new(field: &'static str, reason: impl Into<String>) -> Self {
        Self {
            field,
            reason: reason.into(),
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid {}: {}", self.field, self.reason)
    }
}

impl std::error::Error for ValidationError {}

/// True when `s` has exactly `len` lowercase hex characters.
pub(crate) fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Checks that `s` has exactly `len` lowercase hex characters.
pub(crate) fn check_lower_hex(field: &'static str, s: &str, len: usize) -> Result<(), ValidationError> {
    if is_lower_hex(s, len) {
        Ok(())
    } else {
        Err(ValidationError::new(
            field,
            format!("expected {len} lowercase hex characters"),
        ))
    }
}

/// Checks that `value` is finite.
pub(crate) fn check_finite(field: &'static str, value: f32) -> Result<(), ValidationError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ValidationError::new(field, "must be a finite number"))
    }
}

/// Checks that `value` is finite and within `[min, max]`.
pub(crate) fn check_range(
    field: &'static str,
    value: f32,
    min: f32,
    max: f32,
) -> Result<(), ValidationError> {
    check_finite(field, value)?;
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(ValidationError::new(
            field,
            format!("must be in {min}..={max}, got {value}"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_field() {
        let err = ValidationError::new("size.w", "must be in (0, 1]");
        assert_eq!(err.to_string(), "invalid size.w: must be in (0, 1]");
    }

    #[test]
    fn lower_hex_rules() {
        let ok = "0123456789abcdef0123456789abcdef";
        assert!(check_lower_hex("f", ok, 32).is_ok());
        assert!(check_lower_hex("f", &ok.to_uppercase(), 32).is_err());
        assert!(check_lower_hex("f", &ok[1..], 32).is_err());
        assert!(check_lower_hex("f", &format!("{ok}0"), 32).is_err());
        assert!(check_lower_hex("f", "g123456789abcdef0123456789abcdef", 32).is_err());
        assert!(check_lower_hex("f", "", 32).is_err());
    }

    #[test]
    fn finite_and_range_rules() {
        assert!(check_finite("x", 1.5).is_ok());
        assert!(check_finite("x", f32::NAN).is_err());
        assert!(check_finite("x", f32::INFINITY).is_err());
        assert!(check_range("x", 0.0, 0.0, 1.0).is_ok());
        assert!(check_range("x", 1.0, 0.0, 1.0).is_ok());
        assert!(check_range("x", 1.01, 0.0, 1.0).is_err());
        assert!(check_range("x", -0.01, 0.0, 1.0).is_err());
        assert!(check_range("x", f32::NAN, 0.0, 1.0).is_err());
    }
}
