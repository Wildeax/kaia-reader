//! Reflowable text layout: the settings the reader owns for EPUB, FB2, HTML
//! and other CREngine or MuPDF reflow formats.

use kaia_doc::{Layout, Margins};
use kaia_gfx::Size;

/// The hyphenation dictionary that CREngine ships. KOReader's default for
/// English books.
pub const HYPHENATION_DICT: &str = "English_US.pattern";

/// CSS added when text is justified. Appended after the book's own styles.
const JUSTIFY_CSS: &str = "body { text-align: justify !important; }\n";

/// CREngine base weight of regular text, in CSS units.
pub const REGULAR_FONT_WEIGHT: u32 = 400;

/// The font weight choices, as KOReader's `font_base_weight` values in
/// `frontend/ui/data/creoptions.lua`. Index 2 (value 0) is regular. Each value
/// gives a weight of 400 plus the value times [`CSS_UNITS_PER_WEIGHT_VALUE`].
pub const FONT_WEIGHT_VALUES: [f32; 7] = [-1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 3.0];

/// CSS weight units per `font_base_weight` value. KOReader's
/// `CreDocument:setFontBaseWeight` uses 100, so +1 is Medium (500) and +3 is
/// Bold (700). The steps are at least 50 units apart, past the 25-unit gap at
/// which CREngine fakes a weight.
///
/// Device question, not settled: Nickel's own slider runs from 0 to 120 steps
/// in a unit that is not known yet (PARITY T2-8, Q26). Whether Kaia should
/// follow that scale is for a device check. Until then this constant and the
/// choices above decide the weights.
pub const CSS_UNITS_PER_WEIGHT_VALUE: f64 = 100.0;

/// CREngine base weight for a KOReader font weight value, in CSS units. Zero
/// gives 400. Values that are not finite give 400, and the result stays in the
/// engine range of 1 to 999.
pub fn weight_for_value(value: f64) -> u32 {
    if !value.is_finite() {
        return REGULAR_FONT_WEIGHT;
    }
    let css = f64::from(REGULAR_FONT_WEIGHT) + value * CSS_UNITS_PER_WEIGHT_VALUE;
    css.round().clamp(1.0, 999.0) as u32
}

/// Reflowable text settings. Font size and margins are engine pixels.
/// The app converts KOReader units before passing them here, using
/// `ceil(units * min(viewport width, viewport height) / 600)`.
/// For a Libra Colour viewport, 22 font units become 47 pixels.
/// MuPDF reflow uses the same numbers as points.
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
    /// KOReader font weight value, one of [`FONT_WEIGHT_VALUES`]. 0 is regular.
    pub font_weight: f32,
}

impl Default for TextLayout {
    /// Defaults for a 600-pixel reference viewport. Callers must scale font
    /// size and margins for larger panels before opening a book.
    /// Line spacing is a ratio, so KOReader's 100 percent is 1.0.
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
            font_weight: 0.0,
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
            font_weight: Some(self.css_weight()),
            css_extra: if self.justify {
                JUSTIFY_CSS.to_string()
            } else {
                String::new()
            },
        }
    }

    /// The CREngine base weight for `font_weight`, in CSS units.
    pub fn css_weight(&self) -> u32 {
        weight_for_value(f64::from(self.font_weight))
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
        if !self.font_weight.is_finite() {
            return Err("font weight must be a number".to_string());
        }
        let m = self.margins;
        let margins = [m.top, m.right, m.bottom, m.left];
        if margins.iter().any(|v| !v.is_finite() || *v < 0.0) {
            return Err("margins must be zero or positive numbers".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_regular_and_the_stock_choices_map_by_100_units() {
        assert_eq!(weight_for_value(0.0), 400);
        assert_eq!(weight_for_value(-1.0), 300);
        assert_eq!(weight_for_value(-0.5), 350);
        assert_eq!(weight_for_value(0.5), 450);
        assert_eq!(weight_for_value(1.0), 500);
        assert_eq!(weight_for_value(1.5), 550);
        assert_eq!(weight_for_value(3.0), 700);
    }

    #[test]
    fn every_choice_is_past_the_fake_weight_gap_from_its_neighbours() {
        let weights: Vec<u32> = FONT_WEIGHT_VALUES
            .iter()
            .map(|v| weight_for_value(f64::from(*v)))
            .collect();
        for pair in weights.windows(2) {
            assert!(pair[1] - pair[0] >= 50, "{pair:?} is closer than 50 units");
        }
    }

    #[test]
    fn values_that_are_not_numbers_or_are_huge_stay_in_the_engine_range() {
        assert_eq!(weight_for_value(f64::NAN), 400);
        assert_eq!(weight_for_value(f64::INFINITY), 400);
        assert_eq!(weight_for_value(-50.0), 1);
        assert_eq!(weight_for_value(50.0), 999);
    }

    #[test]
    fn default_layout_is_regular() {
        assert_eq!(TextLayout::default().css_weight(), 400);
        assert_eq!(
            TextLayout::default()
                .to_doc_layout(Size { w: 600, h: 800 })
                .font_weight,
            Some(400)
        );
    }
}
