use serde::{Deserialize, Serialize};

use crate::error::{check_range, ValidationError};
use crate::limits::{OFFSET_ABS_MAX, QUOTE_CONTEXT_MAX_CHARS, QUOTE_EXACT_MAX_CHARS};

/// Where a sticker sits in the book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Anchor {
    /// EPUB, FB2, HTML and other CREngine formats.
    Reflowable {
        /// CREngine xpointer, stored verbatim.
        locator: String,
        quote: TextQuote,
        offset: Offset,
    },
    /// PDF, DjVu, CBZ and other paged formats.
    Fixed {
        /// 1-based page number.
        page: u32,
        /// Top-left in unrotated, uncropped page space, as fractions of the page.
        x: f32,
        y: f32,
    },
    /// Attached to the book as a whole.
    Book {},
}

impl Anchor {
    /// Checks every limit for the variant.
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Anchor::Reflowable { quote, offset, .. } => {
                quote.validate()?;
                offset.validate()
            }
            Anchor::Fixed { page, x, y } => {
                if *page < 1 {
                    return Err(ValidationError::new("anchor.page", "page numbers start at 1"));
                }
                check_range("anchor.x", *x, 0.0, 1.0)?;
                check_range("anchor.y", *y, 0.0, 1.0)
            }
            Anchor::Book {} => Ok(()),
        }
    }
}

/// Text around a reflowable anchor, used to re-anchor after reflow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextQuote {
    pub exact: String,
    pub prefix: String,
    pub suffix: String,
}

impl TextQuote {
    /// `exact` is 1..=1000 chars. `prefix` and `suffix` are 0..=64 chars each.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let exact = self.exact.chars().count();
        if !(1..=QUOTE_EXACT_MAX_CHARS).contains(&exact) {
            return Err(ValidationError::new(
                "anchor.quote.exact",
                format!("must be 1..={QUOTE_EXACT_MAX_CHARS} chars, got {exact}"),
            ));
        }
        if self.prefix.chars().count() > QUOTE_CONTEXT_MAX_CHARS {
            return Err(ValidationError::new(
                "anchor.quote.prefix",
                format!("must be at most {QUOTE_CONTEXT_MAX_CHARS} chars"),
            ));
        }
        if self.suffix.chars().count() > QUOTE_CONTEXT_MAX_CHARS {
            return Err(ValidationError::new(
                "anchor.quote.suffix",
                format!("must be at most {QUOTE_CONTEXT_MAX_CHARS} chars"),
            ));
        }
        Ok(())
    }
}

/// Placement relative to the anchor rect, in units of the rect height.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Offset {
    pub dx: f32,
    pub dy: f32,
}

impl Offset {
    /// `dx` and `dy` are finite and within `-50..=50`.
    pub fn validate(&self) -> Result<(), ValidationError> {
        check_range("anchor.offset.dx", self.dx, -OFFSET_ABS_MAX, OFFSET_ABS_MAX)?;
        check_range("anchor.offset.dy", self.dy, -OFFSET_ABS_MAX, OFFSET_ABS_MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quote(exact: &str, prefix: &str, suffix: &str) -> TextQuote {
        TextQuote {
            exact: exact.into(),
            prefix: prefix.into(),
            suffix: suffix.into(),
        }
    }

    fn reflowable(q: TextQuote, dx: f32, dy: f32) -> Anchor {
        Anchor::Reflowable {
            locator: "/body/p[1]".into(),
            quote: q,
            offset: Offset { dx, dy },
        }
    }

    fn field_of(a: &Anchor) -> &'static str {
        a.validate().unwrap_err().field
    }

    #[test]
    fn exact_length_bounds() {
        assert!(quote("a", "", "").validate().is_ok());
        assert!(quote(&"x".repeat(1000), "", "").validate().is_ok());
        assert_eq!(
            quote("", "", "").validate().unwrap_err().field,
            "anchor.quote.exact"
        );
        let err = quote(&"x".repeat(1001), "", "").validate().unwrap_err();
        assert_eq!(err.field, "anchor.quote.exact");
    }

    #[test]
    fn exact_counts_chars_not_bytes() {
        // 1000 two-byte chars is 2000 bytes but still valid.
        assert!(quote(&"é".repeat(1000), "", "").validate().is_ok());
        assert!(quote(&"é".repeat(1001), "", "").validate().is_err());
    }

    #[test]
    fn context_length_bounds() {
        assert!(quote("a", &"p".repeat(64), &"s".repeat(64)).validate().is_ok());
        assert_eq!(
            quote("a", &"p".repeat(65), "").validate().unwrap_err().field,
            "anchor.quote.prefix"
        );
        assert_eq!(
            quote("a", "", &"s".repeat(65)).validate().unwrap_err().field,
            "anchor.quote.suffix"
        );
        // Multi-byte context counted in chars: 64 chars of 3 bytes each is fine.
        assert!(quote("a", &"€".repeat(64), "").validate().is_ok());
        assert!(quote("a", &"€".repeat(65), "").validate().is_err());
    }

    #[test]
    fn offset_range_and_finiteness() {
        assert!(reflowable(quote("a", "", ""), 50.0, -50.0).validate().is_ok());
        assert_eq!(
            field_of(&reflowable(quote("a", "", ""), 50.5, 0.0)),
            "anchor.offset.dx"
        );
        assert_eq!(
            field_of(&reflowable(quote("a", "", ""), 0.0, -50.5)),
            "anchor.offset.dy"
        );
        assert_eq!(
            field_of(&reflowable(quote("a", "", ""), f32::NAN, 0.0)),
            "anchor.offset.dx"
        );
        assert_eq!(
            field_of(&reflowable(quote("a", "", ""), 0.0, f32::INFINITY)),
            "anchor.offset.dy"
        );
    }

    #[test]
    fn fixed_page_and_fractions() {
        assert!(Anchor::Fixed {
            page: 1,
            x: 0.0,
            y: 1.0
        }
        .validate()
        .is_ok());
        assert_eq!(
            field_of(&Anchor::Fixed {
                page: 0,
                x: 0.1,
                y: 0.1
            }),
            "anchor.page"
        );
        assert_eq!(
            field_of(&Anchor::Fixed {
                page: 1,
                x: 1.1,
                y: 0.1
            }),
            "anchor.x"
        );
        assert_eq!(
            field_of(&Anchor::Fixed {
                page: 1,
                x: 0.1,
                y: -0.1
            }),
            "anchor.y"
        );
        assert_eq!(
            field_of(&Anchor::Fixed {
                page: 1,
                x: f32::NAN,
                y: 0.1
            }),
            "anchor.x"
        );
    }

    #[test]
    fn book_is_always_valid() {
        assert!(Anchor::Book {}.validate().is_ok());
    }

    #[test]
    fn serializes_with_type_tag_first() {
        let a = reflowable(quote("hi", "a", "b"), 0.0, -1.2);
        let json = serde_json::to_string(&a).unwrap();
        assert!(
            json.starts_with(r#"{"type":"reflowable","locator":"/body/p[1]""#),
            "{json}"
        );
        assert_eq!(
            serde_json::to_string(&Anchor::Book {}).unwrap(),
            r#"{"type":"book"}"#
        );
        let fixed: Anchor = serde_json::from_str(r#"{"type":"fixed","page":3,"x":0.5,"y":0.25}"#).unwrap();
        assert_eq!(
            fixed,
            Anchor::Fixed {
                page: 3,
                x: 0.5,
                y: 0.25
            }
        );
        let book: Anchor = serde_json::from_str(r#"{"type":"book"}"#).unwrap();
        assert_eq!(book, Anchor::Book {});
    }

    #[test]
    fn rejects_unknown_type_tag() {
        assert!(serde_json::from_str::<Anchor>(r#"{"type":"pdf"}"#).is_err());
    }
}
