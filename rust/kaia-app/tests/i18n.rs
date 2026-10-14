//! The UI language: the catalogs, the Language page of Settings, and the rebuild
//! after a change. The catalogs under `tests/fixtures/locales` stand in for KOReader's
//! `l10n` folder and Kaia's `l10n-kaia` folder, so the test runs without the
//! translations submodule. One test reads the real French catalog when it is there.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use kaia_app::i18n;
use kaia_app::surfaces::settings::entries::Entry;
use kaia_app::surfaces::settings::sections;
use kaia_app::{App, HeadlessBackend, Route};
use kaia_i18n::{tr, tr_n};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/locales")
}

/// The languages on the Language page as `(label, checked)`.
fn language_choices(app: &App<HeadlessBackend>) -> Vec<(String, bool)> {
    sections::entries("language", app.services(), 0)
        .into_iter()
        .filter_map(|entry| match entry {
            Entry::Choice { label, selected, .. } => Some((label, selected)),
            _ => None,
        })
        .collect()
}

#[test]
fn shared_and_kaia_catalogs_translate_and_fall_back() {
    i18n::activate(&fixture_root(), "fr");
    // A msgid that KOReader also has: its translation applies.
    assert_eq!(tr("Settings"), "Paramètres");
    // Kaia's own catalog.
    assert_eq!(tr("Scan now"), "Analyser maintenant");
    // In neither catalog: English.
    assert_eq!(tr("Show finished books"), "Show finished books");
    assert_eq!(tr_n("%1 book", "%1 books", 1), "%1 livre");
    assert_eq!(tr_n("%1 book", "%1 books", 3), "%1 livres");
    i18n::activate(&fixture_root(), "en");
    assert_eq!(tr("Settings"), "Settings");
}

#[test]
fn koreader_catalog_translates_shared_strings() {
    let root = repo_root();
    if !root.join("l10n/fr/koreader.po").is_file() {
        eprintln!("skipped: the l10n submodule is not checked out");
        return;
    }
    i18n::activate(&root, "fr");
    assert_eq!(tr("Settings"), "Paramètres");
    assert_eq!(tr("Language"), "Langue");
    i18n::activate(&root, "en");
}

#[test]
fn a_compiled_catalog_alone_is_offered_and_translates() {
    // The Kobo package ships compiled catalogs (`.mo`) only. This fixture has just
    // `l10n/de/koreader.mo`, compiled with msgfmt from `source/de.po`.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/locales-mo");
    assert!(i18n::available(&root).contains(&("de", "Deutsch")));
    i18n::activate(&root, "de");
    assert_eq!(tr("Settings"), "Einstellungen");
    assert_eq!(tr_n("%1 book", "%1 books", 1), "%1 Buch");
    assert_eq!(tr_n("%1 book", "%1 books", 2), "%1 Bücher");
    i18n::activate(&root, "en");
    assert_eq!(tr("Settings"), "Settings");
}

#[test]
fn choosing_a_language_rebuilds_the_settings_screens() {
    let dir = scratch("i18n-switch");
    let mut app = make_app(&dir, vec![]);
    app.start().unwrap();
    // Only the fixture languages are on offer, so the list is short and fixed.
    app.services_mut().catalog_root = fixture_root();
    app.command("nav:settings", &[]);
    app.command("settings:display", &[]);
    app.command("settings:language", &[]);
    app.process(Vec::new()).unwrap();
    assert!(matches!(app.top_route(), Some(Route::SettingsSection { name }) if name == "language"));
    assert_eq!(
        language_choices(&app),
        vec![("English".to_string(), true), ("Français".to_string(), false)]
    );
    let english = app.frame().data().to_vec();

    app.command("settings:language:fr", &[]);
    app.process(Vec::new()).unwrap();
    assert_eq!(app.services().language, "fr");
    assert_eq!(sections::title("language"), "Langue");
    assert_eq!(tr("Settings"), "Paramètres");
    assert_eq!(
        language_choices(&app),
        vec![("English".to_string(), false), ("Français".to_string(), true)]
    );
    assert_ne!(
        app.frame().data(),
        english.as_slice(),
        "the language page was painted again"
    );

    app.command("settings:language:en", &[]);
    app.process(Vec::new()).unwrap();
    assert_eq!(app.services().language, "en");
    assert_eq!(
        app.frame().data(),
        english.as_slice(),
        "English screens come back unchanged"
    );
}
