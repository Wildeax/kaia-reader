//! Reflowable text layout: the settings the reader owns for EPUB, FB2, HTML
//! and other CREngine or MuPDF reflow formats.

use kaia_doc::{Layout, Margins};
use kaia_gfx::Size;

/// The hyphenation dictionary that CREngine ships. KOReader's default for
/// English books. It is the last choice when neither the book nor the UI
/// language has a pattern.
pub const HYPHENATION_DICT: &str = "English_US.pattern";

/// One language with a hyphenation pattern in the hyph-utf8 set that CREngine ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HyphenationLanguage {
    /// The KOReader language tag, such as `es` or `en-US`. The override saves it.
    pub tag: &'static str,
    /// Other tags that books use for the same language, such as `spa` for `es`.
    pub aliases: &'static [&'static str],
    /// The pattern file in `data/hyph`, such as `Spanish.pattern`.
    pub pattern: &'static str,
    /// The English name, which the list shows through `kaia_i18n::tr`.
    pub name: &'static str,
}

/// Every language that has a hyphenation pattern. The data is copied from the
/// `LANGUAGES` table in `frontend/apps/reader/modules/readertypography.lua`
/// (KOReader v2026.07.1), in the same order. The `pattern` values are the file
/// names that KOReader's `languages.json` lists in `base/thirdparty/kpvcrlib/
/// crengine/cr3gui/data/hyph`. Chinese, Japanese and Korean have no pattern
/// and are left out.
#[rustfmt::skip]
pub const HYPHENATION_LANGUAGES: &[HyphenationLanguage] = &[
    HyphenationLanguage { tag: "hy", aliases: &["arm", "hye", "hyw"], pattern: "Armenian.pattern", name: "Armenian" },
    HyphenationLanguage { tag: "eu", aliases: &[], pattern: "Basque.pattern", name: "Basque" },
    HyphenationLanguage { tag: "bg", aliases: &["bul"], pattern: "Bulgarian.pattern", name: "Bulgarian" },
    HyphenationLanguage { tag: "ca", aliases: &["cat"], pattern: "Catalan.pattern", name: "Catalan" },
    HyphenationLanguage { tag: "hr", aliases: &[], pattern: "Croatian.pattern", name: "Croatian" },
    HyphenationLanguage { tag: "cs", aliases: &["ces"], pattern: "Czech.pattern", name: "Czech" },
    HyphenationLanguage { tag: "da", aliases: &["dan"], pattern: "Danish.pattern", name: "Danish" },
    HyphenationLanguage { tag: "nl", aliases: &["nld"], pattern: "Dutch.pattern", name: "Dutch" },
    HyphenationLanguage { tag: "en-GB", aliases: &[], pattern: "English_GB.pattern", name: "English (UK)" },
    HyphenationLanguage { tag: "en-US", aliases: &["en", "eng"], pattern: "English_US.pattern", name: "English (US)" },
    HyphenationLanguage { tag: "eo", aliases: &["epo"], pattern: "Esperanto.pattern", name: "Esperanto" },
    HyphenationLanguage { tag: "et", aliases: &["est"], pattern: "Estonian.pattern", name: "Estonian" },
    HyphenationLanguage { tag: "fi", aliases: &["fin"], pattern: "Finnish.pattern", name: "Finnish" },
    HyphenationLanguage { tag: "fr", aliases: &["fra", "fre"], pattern: "French.pattern", name: "French" },
    HyphenationLanguage { tag: "fur", aliases: &[], pattern: "Friulian.pattern", name: "Friulian" },
    HyphenationLanguage { tag: "gl", aliases: &["glg"], pattern: "Galician.pattern", name: "Galician" },
    HyphenationLanguage { tag: "ka", aliases: &[], pattern: "Georgian.pattern", name: "Georgian" },
    HyphenationLanguage { tag: "de", aliases: &["deu"], pattern: "German.pattern", name: "German" },
    HyphenationLanguage { tag: "el", aliases: &["ell"], pattern: "Greek.pattern", name: "Greek" },
    HyphenationLanguage { tag: "hu", aliases: &["hun"], pattern: "Hungarian.pattern", name: "Hungarian" },
    HyphenationLanguage { tag: "is", aliases: &["isl"], pattern: "Icelandic.pattern", name: "Icelandic" },
    HyphenationLanguage { tag: "ga", aliases: &["gle"], pattern: "Irish.pattern", name: "Irish" },
    HyphenationLanguage { tag: "it", aliases: &["ita"], pattern: "Italian.pattern", name: "Italian" },
    HyphenationLanguage { tag: "la", aliases: &["lat"], pattern: "Latin.pattern", name: "Latin" },
    HyphenationLanguage { tag: "la-lit", aliases: &["lat-lit"], pattern: "Latin_liturgical.pattern", name: "Latin (liturgical)" },
    HyphenationLanguage { tag: "lv", aliases: &["lav"], pattern: "Latvian.pattern", name: "Latvian" },
    HyphenationLanguage { tag: "lt", aliases: &["lit"], pattern: "Lithuanian.pattern", name: "Lithuanian" },
    HyphenationLanguage { tag: "mk", aliases: &[], pattern: "Macedonian.pattern", name: "Macedonian" },
    HyphenationLanguage { tag: "ml", aliases: &["mal"], pattern: "Malayalam.pattern", name: "Malayalam" },
    HyphenationLanguage { tag: "no", aliases: &["nor"], pattern: "Norwegian.pattern", name: "Norwegian" },
    HyphenationLanguage { tag: "oc", aliases: &["oci"], pattern: "Occitan.pattern", name: "Occitan" },
    HyphenationLanguage { tag: "pl", aliases: &["pol"], pattern: "Polish.pattern", name: "Polish" },
    HyphenationLanguage { tag: "pms", aliases: &[], pattern: "Piedmontese.pattern", name: "Piedmontese" },
    HyphenationLanguage { tag: "pt", aliases: &["por"], pattern: "Portuguese.pattern", name: "Portuguese" },
    HyphenationLanguage { tag: "pt-BR", aliases: &[], pattern: "Portuguese_BR.pattern", name: "Portuguese (BR)" },
    HyphenationLanguage { tag: "rm", aliases: &["roh"], pattern: "Romansh.pattern", name: "Romansh" },
    HyphenationLanguage { tag: "ro", aliases: &["ron"], pattern: "Romanian.pattern", name: "Romanian" },
    HyphenationLanguage { tag: "ru", aliases: &["rus"], pattern: "Russian.pattern", name: "Russian" },
    HyphenationLanguage { tag: "ru-GB", aliases: &[], pattern: "Russian_EnGB.pattern", name: "Russian + English (UK)" },
    HyphenationLanguage { tag: "ru-US", aliases: &[], pattern: "Russian_EnUS.pattern", name: "Russian + English (US)" },
    HyphenationLanguage { tag: "sr", aliases: &["srp"], pattern: "Serbian.pattern", name: "Serbian" },
    HyphenationLanguage { tag: "sk", aliases: &["slk"], pattern: "Slovak.pattern", name: "Slovak" },
    HyphenationLanguage { tag: "sl", aliases: &["slv"], pattern: "Slovenian.pattern", name: "Slovenian" },
    HyphenationLanguage { tag: "es", aliases: &["spa"], pattern: "Spanish.pattern", name: "Spanish" },
    HyphenationLanguage { tag: "sv", aliases: &["swe"], pattern: "Swedish.pattern", name: "Swedish" },
    HyphenationLanguage { tag: "tr", aliases: &["tur"], pattern: "Turkish.pattern", name: "Turkish" },
    HyphenationLanguage { tag: "uk", aliases: &["ukr"], pattern: "Ukrainian.pattern", name: "Ukrainian" },
    HyphenationLanguage { tag: "cy", aliases: &["cym"], pattern: "Welsh.pattern", name: "Welsh" },
    HyphenationLanguage { tag: "zu", aliases: &["zul"], pattern: "Zulu.pattern", name: "Zulu" },
];

/// Finds the pattern entry for a language tag from a book or a UI language.
/// The tag is matched without case, with `_` read as `-`, first in full and then
/// by its primary subtag. So `es-MX` takes Spanish and `en-AU` takes English (US),
/// as `en` is an alias of English (US). Empty and unknown tags give None.
pub fn language_for_tag(tag: &str) -> Option<&'static HyphenationLanguage> {
    let norm = tag.trim().replace('_', "-").to_ascii_lowercase();
    if norm.is_empty() {
        return None;
    }
    let find = |key: &str| {
        HYPHENATION_LANGUAGES.iter().find(|lang| {
            lang.tag.eq_ignore_ascii_case(key) || lang.aliases.iter().any(|a| a.eq_ignore_ascii_case(key))
        })
    };
    find(&norm).or_else(|| norm.split('-').next().and_then(find))
}

/// The pattern file for a hyphenation choice. The first tag that names a pattern wins:
/// `choice` (the override, None for "Book language"), then `book` (the `dc:language`
/// of the book), then `ui` (the UI language). Without any of them, English (US).
/// A book language with no pattern, such as Japanese, falls through to the UI language.
pub fn hyphenation_pattern(choice: Option<&str>, book: Option<&str>, ui: Option<&str>) -> &'static str {
    [choice, book, ui]
        .into_iter()
        .flatten()
        .find_map(|tag| language_for_tag(tag))
        .map_or(HYPHENATION_DICT, |lang| lang.pattern)
}

/// The first language of a `dc:language` value. Some books list several, as
/// `en fr`, or `en, fr`. Kaia uses the first one.
pub fn primary_book_language(dc_language: &str) -> Option<&str> {
    dc_language
        .split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .find(|part| !part.is_empty())
}

/// CSS added when text is justified. Appended after the book's own styles.
const JUSTIFY_CSS: &str = "body, p, div, li, blockquote { text-align: justify !important; }\n";

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
    /// Hyphenate words. The pattern comes from [`hyphenation_pattern`].
    pub hyphenation: bool,
    /// The language tag the reader chose for hyphenation, from
    /// [`HYPHENATION_LANGUAGES`]. None means the book language.
    pub hyphenation_lang: Option<String>,
    /// Justify lines.
    pub justify: bool,
    /// Force left alignment. False with justify false preserves book alignment.
    pub align_left: bool,
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
            hyphenation_lang: None,
            justify: false,
            align_left: false,
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
            css_extra: if self.align_left {
                "body, p, div, li, blockquote { text-align: left !important; }\n".to_string()
            } else if self.justify {
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

#[cfg(test)]
mod hyphenation_tests {
    use super::*;

    fn pattern(choice: Option<&str>, book: Option<&str>, ui: Option<&str>) -> &'static str {
        hyphenation_pattern(choice, book, ui)
    }

    #[test]
    fn book_language_tags_and_aliases_find_their_pattern() {
        assert_eq!(language_for_tag("es").unwrap().pattern, "Spanish.pattern");
        assert_eq!(language_for_tag("fr").unwrap().pattern, "French.pattern");
        assert_eq!(language_for_tag("fra").unwrap().pattern, "French.pattern");
        assert_eq!(language_for_tag("spa").unwrap().pattern, "Spanish.pattern");
        assert_eq!(
            language_for_tag("pt-BR").unwrap().pattern,
            "Portuguese_BR.pattern"
        );
        assert_eq!(language_for_tag("pt-PT").unwrap().pattern, "Portuguese.pattern");
        assert_eq!(
            language_for_tag("la-lit").unwrap().pattern,
            "Latin_liturgical.pattern"
        );
    }

    #[test]
    fn tags_are_matched_without_case_and_with_underscores() {
        assert_eq!(language_for_tag("ES").unwrap().tag, "es");
        assert_eq!(language_for_tag("fr_FR").unwrap().tag, "fr");
        assert_eq!(language_for_tag("en-us").unwrap().tag, "en-US");
        assert_eq!(language_for_tag(" de ").unwrap().tag, "de");
    }

    #[test]
    fn a_region_falls_back_to_its_primary_language() {
        assert_eq!(language_for_tag("es-MX").unwrap().tag, "es");
        assert_eq!(language_for_tag("en-AU").unwrap().tag, "en-US");
        assert_eq!(language_for_tag("de-AT").unwrap().tag, "de");
    }

    #[test]
    fn unknown_empty_and_patternless_tags_give_none() {
        assert!(language_for_tag("").is_none());
        assert!(language_for_tag("   ").is_none());
        assert!(language_for_tag("xx").is_none());
        assert!(language_for_tag("tlh-Klingon").is_none());
        assert!(language_for_tag("ja").is_none(), "Japanese has no pattern");
        assert!(language_for_tag("zh-CN").is_none(), "Chinese has no pattern");
        assert!(language_for_tag("ko").is_none(), "Korean has no pattern");
    }

    #[test]
    fn choice_beats_book_beats_ui_beats_english() {
        assert_eq!(pattern(Some("fr"), Some("es"), Some("de")), "French.pattern");
        assert_eq!(pattern(None, Some("es"), Some("de")), "Spanish.pattern");
        assert_eq!(pattern(None, None, Some("de")), "German.pattern");
        assert_eq!(pattern(None, None, None), HYPHENATION_DICT);
    }

    #[test]
    fn an_unknown_choice_falls_back_to_the_book_language() {
        assert_eq!(pattern(Some("xx"), Some("es"), None), "Spanish.pattern");
    }

    #[test]
    fn a_book_without_a_pattern_uses_the_ui_language() {
        assert_eq!(pattern(None, Some("ja"), Some("fr")), "French.pattern");
        assert_eq!(pattern(None, Some("ja"), None), HYPHENATION_DICT);
    }

    #[test]
    fn the_book_language_takes_its_first_value() {
        assert_eq!(primary_book_language("fr"), Some("fr"));
        assert_eq!(primary_book_language("en fr"), Some("en"));
        assert_eq!(primary_book_language("es, en"), Some("es"));
        assert_eq!(primary_book_language(" ;de"), Some("de"));
        assert_eq!(primary_book_language(""), None);
        assert_eq!(primary_book_language(" , "), None);
    }

    #[test]
    fn the_table_lists_each_pattern_once_in_koreader_order() {
        let mut files: Vec<&str> = HYPHENATION_LANGUAGES.iter().map(|l| l.pattern).collect();
        assert_eq!(files.len(), 49);
        files.sort_unstable();
        files.dedup();
        assert_eq!(files.len(), 49, "a pattern file appears twice");
        let mut tags: Vec<&str> = HYPHENATION_LANGUAGES.iter().map(|l| l.tag).collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), HYPHENATION_LANGUAGES.len(), "a tag appears twice");
    }
}
