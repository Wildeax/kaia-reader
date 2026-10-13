//! Reflowable text layout: the settings the reader owns for EPUB, FB2, HTML
//! and other CREngine or MuPDF reflow formats.

use kaia_doc::{Layout, Margins};
use kaia_gfx::Size;

/// The hyphenation dictionary that CREngine ships. KOReader's default for
/// English books.
pub const HYPHENATION_DICT: &str = "English_US.pattern";

/// CSS added when text is justified. Appended after the book's own styles.
const JUSTIFY_CSS: &str = "body { text-align: justify !important; }\n";

/// Reflowable text settings. Sizes are in device pixels for CREngine, which is
/// the unit KOReader stores. MuPDF layouts read the same numbers as points.
#[derive(Debug, Clone, PartialEq)]
pub struct TextLayout {
    /// Font face name, such as `Noto Serif`. None keeps the engine default.
    pub font_family: Option<String>,
    /// Font size in pixels (the em size).
    pub font_size: f32,
    /// Page margins in pixels.
    pub margins: Margins,
    /// Line height as a multiple of the font size. 1.0 is the engine's normal spacing.
    pub line_spacing: f32,
    /// Hyphenate words with [`HYPHENATION_DICT`].
    pub hyphenation: bool,
    /// Justify lines.
    pub justify: bool,
    /// Keep the book's own CSS. When false, the engine ignores the book's styles.
    pub embedded_styles: bool,
}

impl Default for TextLayout {
    /// KOReader's defaults: 22 px text, 10 px side margins, 15 px top and
    /// bottom margins, normal line spacing, hyphenation on, the book's styles on.
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 22.0,
            margins: Margins {
                top: 15.0,
                right: 10.0,
                bottom: 15.0,
                left: 10.0,
            },
            line_spacing: 1.0,
            hyphenation: true,
            justify: false,
            embedded_styles: true,
        }
    }
}

impl TextLayout {
    /// The layout that `kaia-doc` applies, for a page of the given size.
    pub(crate) fn to_doc_layout(&self, page: Size) -> Layout {
        Layout {
            width: page.w as f32,
            height: page.h as f32,
            em: self.font_size,
            margins: Some(self.margins),
            line_height: Some(self.line_spacing),
            font_family: self.font_family.clone(),
            css_extra: if self.justify {
                JUSTIFY_CSS.to_string()
            } else {
                String::new()
            },
        }
    }

    /// Checks the values that the engines reject, so the error comes before a layout job.
    pub(crate) fn validate(&self) -> Result<(), String> {
        let positive = |v: f32| v.is_finite() && v > 0.0;
        if !positive(self.font_size) {
            return Err("font size must be a positive number".to_string());
        }
        if !(self.line_spacing.is_finite() && self.line_spacing > 0.0) {
            return Err("line spacing must be a positive number".to_string());
        }
        let m = self.margins;
        let margins = [m.top, m.right, m.bottom, m.left];
        if margins.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err("margins must be zero or positive numbers".to_string());
        }
        Ok(())
    }
}
