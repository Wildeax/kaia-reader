//! The UI language: the catalog the screens read, and the languages on offer.
//!
//! Source strings are English. A string with a translation in the catalog of the
//! language in use shows that translation. A string with none shows English.
//! Catalogs are gettext files: KOReader's `l10n/<tag>/koreader.po`, shared with
//! KOReader, and Kaia's own `l10n-kaia/<tag>/kaia.po`. `kaia_i18n` keeps the
//! catalog per thread, so the app sets it on the UI thread (see [`activate`]).

use std::path::{Path, PathBuf};

use kaia_i18n::Catalog;
use kaia_platform::platform::Kind;
use kaia_platform::Platform;

/// The settings key that holds the language tag. Unset means the system language.
pub const SETTING: &str = "ui.language";

/// The tag of the source language. It has no catalog.
pub const ENGLISH: &str = "en";

/// Languages with a native name, as KOReader names them in `frontend/ui/language.lua`.
/// A language is on offer when its catalog exists under the catalog root. English
/// is always on offer and is not in this table. Add a row when a catalog comes in.
pub const NAMES: &[(&str, &str)] = &[
    ("en_GB", "English (United Kingdom)"),
    ("ca", "Català"),
    ("cs", "Čeština"),
    ("da", "Dansk"),
    ("de", "Deutsch"),
    ("eo", "Esperanto"),
    ("es", "Español"),
    ("eu", "Euskara"),
    ("fi", "Suomi"),
    ("fr", "Français"),
    ("ga", "Gaeilge"),
    ("gl", "Galego"),
    ("he", "עִבְרִית"),
    ("hr", "Hrvatski"),
    ("hu", "Magyar"),
    ("id", "Bahasa Indonesia"),
    ("it_IT", "Italiano"),
    ("lt_LT", "Lietuvių"),
    ("lv", "Latviešu"),
    ("nl_NL", "Nederlands"),
    ("nb_NO", "Norsk bokmål"),
    ("pl", "Polski"),
    ("pt_PT", "Português"),
    ("pt_BR", "Português do Brasil"),
    ("ro", "Română"),
    ("ro_MD", "Română (Moldova)"),
    ("sk", "Slovenčina"),
    ("sl", "Slovenščina"),
    ("sv", "Svenska"),
    ("th", "ภาษาไทย"),
    ("tr", "Türkçe"),
    ("vi", "Tiếng Việt"),
    ("ar", "عربى"),
    ("bg_BG", "Български"),
    ("bn", "বাংলা"),
    ("el", "Ελληνικά"),
    ("fa", "فارسی"),
    ("hi", "हिन्दी"),
    ("ja", "日本語"),
    ("ka", "ქართული"),
    ("ko_KR", "한국어"),
    ("ru", "Русский"),
    ("sr", "Српски"),
    ("uk", "Українська"),
    ("zh_CN", "简体中文"),
    ("zh_TW", "中文（台灣）"),
];

/// The folder that holds `l10n` and `l10n-kaia`. The desktop build reads the copy in
/// this repository. A Kobo reads the copy in its install folder.
pub fn catalog_root(platform: &Platform) -> PathBuf {
    match platform.kind {
        Kind::Kobo(_) => platform.paths.install_dir.clone(),
        Kind::Desktop => PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")),
    }
}

/// The languages on offer under `root`, English first, as `(tag, name in that language)`.
/// A language is on offer when its catalog exists as a `.po` or a `.mo` file.
pub fn available(root: &Path) -> Vec<(&'static str, &'static str)> {
    let mut on_offer = vec![(ENGLISH, "English")];
    on_offer.extend(
        NAMES
            .iter()
            .copied()
            .filter(|(tag, _)| kaia_i18n::has_catalog(root, tag)),
    );
    on_offer
}

/// The tag to show. It is `setting` when that is on offer. Otherwise it is the
/// first system locale that is on offer, and English when none is.
pub fn resolve(setting: Option<&str>, on_offer: &[(&str, &str)]) -> String {
    let offered = |tag: &str| on_offer.iter().any(|(t, _)| *t == tag);
    if let Some(tag) = setting.filter(|tag| offered(tag)) {
        return tag.to_string();
    }
    system_locales()
        .iter()
        .flat_map(|locale| candidates(locale))
        .find(|tag| offered(tag))
        .unwrap_or_else(|| ENGLISH.to_string())
}

/// The locales the environment asks for, in order. The first of `LC_ALL`,
/// `LC_MESSAGES`, `LANGUAGE` and `LANG` that is set decides. `LANGUAGE` may list
/// several locales, separated by colons.
fn system_locales() -> Vec<String> {
    for name in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        if let Ok(value) = std::env::var(name) {
            if !value.is_empty() {
                return value.split(':').map(str::to_string).collect();
            }
        }
    }
    Vec::new()
}

/// The tags for a locale such as `pt_BR.UTF-8`, most specific first: `pt_BR`, then `pt`.
fn candidates(locale: &str) -> Vec<String> {
    let tag = locale.split(['.', '@']).next().unwrap_or("").replace('-', "_");
    let mut out = vec![tag.clone()];
    if let Some((base, _)) = tag.split_once('_') {
        out.push(base.to_string());
    }
    out
}

/// Loads the catalog of `tag` from `root` and makes it the catalog of this thread.
/// A catalog that cannot be read leaves English in place and logs the reason.
pub fn activate(root: &Path, tag: &str) {
    let catalog = if tag == ENGLISH {
        Catalog::default()
    } else {
        Catalog::load(root, tag).unwrap_or_else(|error| {
            eprintln!("kaia: cannot read the {tag} catalog, showing English: {error}");
            Catalog::default()
        })
    };
    kaia_i18n::set_current(catalog);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_drop_encoding_and_modifier() {
        assert_eq!(candidates("pt_BR.UTF-8"), vec!["pt_BR", "pt"]);
        assert_eq!(candidates("de_DE@euro"), vec!["de_DE", "de"]);
        assert_eq!(candidates("sr-RS"), vec!["sr_RS", "sr"]);
        assert_eq!(candidates("fr"), vec!["fr"]);
    }

    #[test]
    fn resolve_prefers_the_setting_when_it_is_on_offer() {
        let on_offer = [(ENGLISH, "English"), ("fr", "Français")];
        assert_eq!(resolve(Some("fr"), &on_offer), "fr");
        assert_eq!(resolve(Some("xx"), &on_offer), resolve(None, &on_offer));
    }

    #[test]
    fn names_have_no_duplicate_tags() {
        let mut tags: Vec<&str> = NAMES.iter().map(|(tag, _)| *tag).collect();
        tags.sort_unstable();
        tags.dedup();
        assert_eq!(tags.len(), NAMES.len());
        assert!(NAMES.iter().all(|(tag, _)| *tag != ENGLISH));
    }
}
