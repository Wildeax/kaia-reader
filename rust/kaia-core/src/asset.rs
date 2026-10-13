use serde::{Deserialize, Serialize};

use crate::error::{check_lower_hex, is_lower_hex, ValidationError};
use crate::limits::{ASSET_MAX_BYTES, ASSET_MAX_DIM};

/// True when `s` is a lowercase hex SHA-256 digest (64 characters).
pub fn is_sha256(s: &str) -> bool {
    is_lower_hex(s, 64)
}

/// A sticker image stored in `assets/`, named by its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetRef {
    /// Lowercase hex SHA-256 of the file bytes.
    pub sha256: String,
    /// `image/png` or `image/jpeg`.
    pub mime: String,
    pub width: u32,
    pub height: u32,
    /// Encoded size in bytes.
    pub bytes: u64,
    #[serde(default)]
    pub source_name: Option<String>,
    /// Free text. May be empty.
    #[serde(default)]
    pub license: String,
}

impl AssetRef {
    /// Checks the hash, MIME type, dimensions and size.
    pub fn validate(&self) -> Result<(), ValidationError> {
        check_lower_hex("asset.sha256", &self.sha256, 64)?;
        if !matches!(self.mime.as_str(), "image/png" | "image/jpeg") {
            return Err(ValidationError::new(
                "asset.mime",
                "must be image/png or image/jpeg",
            ));
        }
        if !(1..=ASSET_MAX_DIM).contains(&self.width) {
            return Err(ValidationError::new(
                "asset.width",
                format!("must be 1..={ASSET_MAX_DIM}"),
            ));
        }
        if !(1..=ASSET_MAX_DIM).contains(&self.height) {
            return Err(ValidationError::new(
                "asset.height",
                format!("must be 1..={ASSET_MAX_DIM}"),
            ));
        }
        if !(1..=ASSET_MAX_BYTES).contains(&self.bytes) {
            return Err(ValidationError::new(
                "asset.bytes",
                format!("must be 1..={ASSET_MAX_BYTES}"),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> AssetRef {
        AssetRef {
            sha256: "a".repeat(64),
            mime: "image/png".into(),
            width: 64,
            height: 64,
            bytes: 2048,
            source_name: Some("star.png".into()),
            license: String::new(),
        }
    }

    #[test]
    fn valid_asset_passes() {
        assert!(valid().validate().is_ok());
    }

    #[test]
    fn sha256_must_be_64_lowercase_hex() {
        let mut a = valid();
        a.sha256 = "A".repeat(64);
        assert_eq!(a.validate().unwrap_err().field, "asset.sha256");
        a.sha256 = "a".repeat(63);
        assert_eq!(a.validate().unwrap_err().field, "asset.sha256");
    }

    #[test]
    fn mime_is_png_or_jpeg() {
        let mut a = valid();
        a.mime = "image/jpeg".into();
        assert!(a.validate().is_ok());
        a.mime = "image/gif".into();
        assert_eq!(a.validate().unwrap_err().field, "asset.mime");
    }

    #[test]
    fn dimensions_are_bounded() {
        let mut a = valid();
        a.width = 2048;
        a.height = 2048;
        assert!(a.validate().is_ok());
        a.width = 2049;
        assert_eq!(a.validate().unwrap_err().field, "asset.width");
        a.width = 1;
        a.height = 0;
        assert_eq!(a.validate().unwrap_err().field, "asset.height");
    }

    #[test]
    fn size_is_bounded() {
        let mut a = valid();
        a.bytes = ASSET_MAX_BYTES;
        assert!(a.validate().is_ok());
        a.bytes = ASSET_MAX_BYTES + 1;
        assert_eq!(a.validate().unwrap_err().field, "asset.bytes");
        a.bytes = 0;
        assert_eq!(a.validate().unwrap_err().field, "asset.bytes");
    }

    #[test]
    fn license_may_be_empty_and_source_name_null() {
        let json = r#"{"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","mime":"image/png","width":1,"height":1,"bytes":10}"#;
        let a: AssetRef = serde_json::from_str(json).unwrap();
        assert_eq!(a.license, "");
        assert_eq!(a.source_name, None);
        assert!(a.validate().is_ok());
    }
}
