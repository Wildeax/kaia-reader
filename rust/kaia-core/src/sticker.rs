use serde::{Deserialize, Serialize};

use crate::anchor::Anchor;
use crate::document::DocumentId;
use crate::error::{check_finite, check_lower_hex, ValidationError};
use crate::limits::{ROTATION_ABS_MAX, TEXT_MAX_CHARS};

/// A sticker on a book page.
///
/// `id`, `created_at` and `updated_at` are assigned by the store. Input without
/// `id` inserts a new sticker, input with `id` updates the stored one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sticker {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub doc: DocumentId,
    pub anchor: Anchor,
    /// SHA-256 of the image asset, or `null` for text-only stickers.
    #[serde(default)]
    pub asset: Option<String>,
    /// Caption or text sticker body, at most 2000 chars.
    #[serde(default)]
    pub text: Option<String>,
    pub size: StickerSize,
    /// Degrees, -180..=180.
    #[serde(default)]
    pub rotation: i16,
    pub state: StickerState,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub updated_at: i64,
}

impl Sticker {
    /// Checks every rule that does not need the store.
    pub fn validate(&self) -> Result<(), ValidationError> {
        self.doc.validate()?;
        self.anchor.validate()?;
        if let Some(sha) = &self.asset {
            check_lower_hex("asset", sha, 64)?;
        }
        if let Some(text) = &self.text {
            if text.chars().count() > TEXT_MAX_CHARS {
                return Err(ValidationError::new(
                    "text",
                    format!("must be at most {TEXT_MAX_CHARS} chars"),
                ));
            }
        }
        self.size.validate()?;
        if !(-ROTATION_ABS_MAX..=ROTATION_ABS_MAX).contains(&self.rotation) {
            return Err(ValidationError::new(
                "rotation",
                format!(
                    "must be -{ROTATION_ABS_MAX}..={ROTATION_ABS_MAX}, got {}",
                    self.rotation
                ),
            ));
        }
        Ok(())
    }
}

/// Sticker size as fractions of page width and height. Each is in (0, 1].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StickerSize {
    pub w: f32,
    pub h: f32,
}

impl StickerSize {
    /// Checks that `w` and `h` are finite and in (0, 1].
    pub fn validate(&self) -> Result<(), ValidationError> {
        check_open_unit("size.w", self.w)?;
        check_open_unit("size.h", self.h)
    }
}

fn check_open_unit(field: &'static str, v: f32) -> Result<(), ValidationError> {
    check_finite(field, v)?;
    if v > 0.0 && v <= 1.0 {
        Ok(())
    } else {
        Err(ValidationError::new(field, format!("must be in (0, 1], got {v}")))
    }
}

/// Sticker state. The JSON and SQL values are the snake case strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StickerState {
    Attached,
    NeedsReview,
    Orphaned,
}

impl StickerState {
    /// The string stored in SQL and used in JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            StickerState::Attached => "attached",
            StickerState::NeedsReview => "needs_review",
            StickerState::Orphaned => "orphaned",
        }
    }

    /// Parses the string form. Returns `None` for unknown values.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "attached" => Some(StickerState::Attached),
            "needs_review" => Some(StickerState::NeedsReview),
            "orphaned" => Some(StickerState::Orphaned),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::anchor::{Offset, Placement, TextQuote};

    const MD5: &str = "0123456789abcdef0123456789abcdef";

    fn base() -> Sticker {
        Sticker {
            id: None,
            doc: DocumentId {
                partial_md5: MD5.into(),
                file_size: 100,
                title: Some("Book".into()),
            },
            anchor: Anchor::Reflowable {
                locator: "/body/p[2]".into(),
                quote: TextQuote {
                    exact: "quoted".into(),
                    prefix: "a ".into(),
                    suffix: " b".into(),
                },
                offset: Offset { dx: 0.0, dy: -1.2 },
                placement: Placement::Margin,
            },
            asset: None,
            text: Some("hello".into()),
            size: StickerSize { w: 0.1, h: 0.07 },
            rotation: 0,
            state: StickerState::Attached,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn base_is_valid() {
        assert!(base().validate().is_ok());
    }

    #[test]
    fn text_limit_is_chars() {
        let mut s = base();
        s.text = Some("x".repeat(2000));
        assert!(s.validate().is_ok());
        s.text = Some("x".repeat(2001));
        assert_eq!(s.validate().unwrap_err().field, "text");
        s.text = Some("é".repeat(2000));
        assert!(s.validate().is_ok());
        s.text = None;
        assert!(s.validate().is_ok());
    }

    #[test]
    fn asset_must_be_sha256_hex() {
        let mut s = base();
        s.asset = Some("b".repeat(64));
        assert!(s.validate().is_ok());
        s.asset = Some("B".repeat(64));
        assert_eq!(s.validate().unwrap_err().field, "asset");
    }

    #[test]
    fn size_is_open_at_zero_and_closed_at_one() {
        let mut s = base();
        s.size = StickerSize { w: 1.0, h: 1.0 };
        assert!(s.validate().is_ok());
        s.size = StickerSize { w: 0.0, h: 0.5 };
        assert_eq!(s.validate().unwrap_err().field, "size.w");
        s.size = StickerSize { w: 0.5, h: -0.1 };
        assert_eq!(s.validate().unwrap_err().field, "size.h");
        s.size = StickerSize { w: 1.01, h: 0.5 };
        assert_eq!(s.validate().unwrap_err().field, "size.w");
        s.size = StickerSize { w: f32::NAN, h: 0.5 };
        assert_eq!(s.validate().unwrap_err().field, "size.w");
        s.size = StickerSize {
            w: 0.5,
            h: f32::INFINITY,
        };
        assert_eq!(s.validate().unwrap_err().field, "size.h");
    }

    #[test]
    fn rotation_range() {
        let mut s = base();
        for ok in [-180, 0, 180] {
            s.rotation = ok;
            assert!(s.validate().is_ok(), "rotation {ok}");
        }
        for bad in [-181, 181, i16::MIN, i16::MAX] {
            s.rotation = bad;
            assert_eq!(s.validate().unwrap_err().field, "rotation", "rotation {bad}");
        }
    }

    #[test]
    fn document_and_anchor_errors_propagate() {
        let mut s = base();
        s.doc.partial_md5 = "short".into();
        assert_eq!(s.validate().unwrap_err().field, "doc.partial_md5");
        let mut s = base();
        s.anchor = Anchor::Fixed {
            page: 0,
            x: 0.0,
            y: 0.0,
        };
        assert_eq!(s.validate().unwrap_err().field, "anchor.page");
    }

    #[test]
    fn state_strings() {
        assert_eq!(
            serde_json::to_string(&StickerState::NeedsReview).unwrap(),
            r#""needs_review""#
        );
        assert_eq!(
            serde_json::to_string(&StickerState::Attached).unwrap(),
            r#""attached""#
        );
        assert_eq!(
            serde_json::to_string(&StickerState::Orphaned).unwrap(),
            r#""orphaned""#
        );
        assert_eq!(
            StickerState::parse("needs_review"),
            Some(StickerState::NeedsReview)
        );
        assert_eq!(StickerState::parse("Attached"), None);
        assert_eq!(StickerState::Orphaned.as_str(), "orphaned");
    }

    #[test]
    fn input_without_id_or_timestamps_parses() {
        let json = r#"{
            "doc": {"partial_md5": "0123456789abcdef0123456789abcdef", "file_size": 5, "title": null},
            "anchor": {"type": "book"},
            "asset": null,
            "text": "note",
            "size": {"w": 0.2, "h": 0.2},
            "state": "attached"
        }"#;
        let s: Sticker = serde_json::from_str(json).unwrap();
        assert_eq!(s.id, None);
        assert_eq!(s.rotation, 0);
        assert_eq!(s.created_at, 0);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn serialized_shape_matches_spec() {
        let mut s = base();
        s.id = Some("00000000-0000-4000-8000-000000000000".into());
        s.created_at = 1760000000;
        s.updated_at = 1760000001;
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["id"], "00000000-0000-4000-8000-000000000000");
        assert_eq!(v["anchor"]["type"], "reflowable");
        assert_eq!(v["anchor"]["quote"]["exact"], "quoted");
        assert!((v["anchor"]["offset"]["dy"].as_f64().unwrap() + 1.2).abs() < 1e-6);
        assert_eq!(v["asset"], serde_json::Value::Null);
        assert!((v["size"]["w"].as_f64().unwrap() - 0.1).abs() < 1e-6);
        assert_eq!(v["state"], "attached");
        assert_eq!(v["rotation"], 0);
        let back: Sticker = serde_json::from_value(v).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn id_is_omitted_when_absent() {
        let v = serde_json::to_value(base()).unwrap();
        assert!(v.get("id").is_none());
    }
}
