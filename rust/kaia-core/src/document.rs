use serde::{Deserialize, Serialize};

use crate::error::{check_lower_hex, is_lower_hex, ValidationError};

/// True when `s` is a valid `partial_md5` (32 lowercase hex characters).
pub fn is_partial_md5(s: &str) -> bool {
    is_lower_hex(s, 32)
}

/// Identity of a book. `partial_md5` is the primary key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentId {
    /// KOReader `util.partialMD5`: 32 lowercase hex characters.
    pub partial_md5: String,
    /// File size in bytes, used to detect a changed file.
    pub file_size: u64,
    /// Display only. Never used for matching.
    pub title: Option<String>,
}

impl DocumentId {
    /// Checks the hash format.
    pub fn validate(&self) -> Result<(), ValidationError> {
        check_lower_hex("doc.partial_md5", &self.partial_md5, 32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(md5: &str) -> DocumentId {
        DocumentId {
            partial_md5: md5.to_string(),
            file_size: 1,
            title: None,
        }
    }

    #[test]
    fn accepts_32_lowercase_hex() {
        assert!(doc("0123456789abcdef0123456789abcdef").validate().is_ok());
    }

    #[test]
    fn rejects_bad_md5() {
        for bad in [
            "",
            "0123",
            "0123456789ABCDEF0123456789abcdef",
            "0123456789abcdef0123456789abcdeg",
        ] {
            let err = doc(bad).validate().unwrap_err();
            assert_eq!(err.field, "doc.partial_md5", "input {bad:?}");
        }
    }

    #[test]
    fn serializes_title_as_null_when_absent() {
        let json = serde_json::to_value(doc("0123456789abcdef0123456789abcdef")).unwrap();
        assert_eq!(json["title"], serde_json::Value::Null);
        assert_eq!(json["file_size"], 1);
    }
}
