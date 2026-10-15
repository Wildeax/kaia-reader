//! Commands of the settings surface. Every id starts with `settings:`.
//!
//! - `settings:<section>` opens a section (see [`SECTIONS`]).
//! - `settings:set:<key>` writes the argument of a toggle, stepper or text
//!   field to `settings.json`. The key is one of `keys.rs`.
//! - `settings:pick:<key>` writes the option a radio group chose (argument: the
//!   option index, see [`options_for`]).
//! - `settings:light:<frontlight|warmth>` sets the platform value (argument: 0 to 100).
//! - `settings:fields:open` and `settings:fields:close` show and hide the
//!   keyboard of the Network and sync section's text fields.
//! - `settings:search:open` shows the keyboard for the search field.
//! - `settings:search:close` hides it (the keyboard's Hide key).
//! - `settings:search` runs the search on the text the field holds.
//! - `settings:sync:test` calls `SyncClient::health`. It runs only when tapped.
//! - `settings:data:import` and `settings:data:export` run the Data actions.
//! - `settings:quick:<name>` is a control of the quick settings sheet. The app
//!   passes these ids to [`super::quick::command`].
//! - `settings:language` opens the language page. `settings:language:<tag>`
//!   shows the screens in that language.
//! - `settings:dictionaries` opens the Dictionary settings page.
//! - `settings:timezone` opens the Time zone page. `settings:timezone:<name>`
//!   stores the zone `name`. An empty name stores the system zone.
//! - `settings:pin:<set|change|off>` starts a screen lock flow in the device
//!   section. The pad then sends `pin:digit:<n>`, `pin:delete`, `pin:submit` and `back`.
//!   See [`crate::surfaces::pin`].
//! - `settings:reset:settings` and `settings:reset:erase` open the confirmation pages
//!   of Advanced (see [`super::reset`]). Those pages send `settings:reset:type`,
//!   `settings:reset:run` and `settings:reset:cancel`.
//! - `settings:dicts:<toggle|up|down>:<index>` changes one dictionary. `index` is
//!   its place in the lookup order. The change is `reader::dict::Change`.

use kaia_events::Value;

use super::{experiments, keys};
use crate::surfaces::reader::dict::Change;

/// The sections, in the order the root lists them.
pub const SECTIONS: [&str; 11] = [
    "home",
    "reading",
    "display",
    "library",
    "network",
    "statistics",
    "device",
    "extensions",
    "data",
    "about",
    "advanced",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    Frontlight,
    Warmth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Section(String),
    FieldsOpen,
    FieldsClose,
    Set(String),
    Pick(String),
    Light(Light),
    SearchOpen,
    SearchClose,
    Search,
    SyncTest,
    Import,
    ExportNotes,
    Quick(String),
    Language,
    LanguagePick(String),
    /// Opens the Dictionary settings page.
    Dictionaries,
    DictionaryDownloads,
    DictionaryInstall(String),
    /// Changes the dictionary at `index` in lookup order.
    Dictionary {
        index: usize,
        change: Change,
    },
    /// Opens the Time zone page.
    TimeZone,
    /// Stores the zone called `name`. An empty name stores the system zone.
    TimeZonePick(String),
}

/// Parses a settings action id. None for every other id, and for an unknown section.
pub fn parse(id: &str) -> Option<Command> {
    let rest = id.strip_prefix("settings:")?;
    match rest {
        // A page of Advanced. It is not in the root list.
        "experimental" => return Some(Command::Section("experimental".into())),
        "search" => return Some(Command::Search),
        "search:open" => return Some(Command::SearchOpen),
        "fields:open" => return Some(Command::FieldsOpen),
        "fields:close" => return Some(Command::FieldsClose),
        "search:close" => return Some(Command::SearchClose),
        "sync:test" => return Some(Command::SyncTest),
        "data:import" => return Some(Command::Import),
        "data:export" => return Some(Command::ExportNotes),
        "language" => return Some(Command::Language),
        "dictionaries" => return Some(Command::Dictionaries),
        "dictionary-downloads" => return Some(Command::DictionaryDownloads),
        "timezone" => return Some(Command::TimeZone),
        _ => {}
    }
    if let Some(id) = rest.strip_prefix("dictionary-install:") {
        return Some(Command::DictionaryInstall(id.into()));
    }
    if let Some(name) = rest.strip_prefix("timezone:") {
        return Some(Command::TimeZonePick(name.to_string()));
    }
    if let Some(action) = rest.strip_prefix("dicts:") {
        let (change, index) = action.split_once(':')?;
        let change = match change {
            "toggle" => Change::Toggle,
            "up" => Change::Up,
            "down" => Change::Down,
            _ => return None,
        };
        let index = index.parse().ok()?;
        return Some(Command::Dictionary { index, change });
    }
    if let Some(key) = rest.strip_prefix("set:") {
        return (!key.is_empty()).then(|| Command::Set(key.to_string()));
    }
    if let Some(key) = rest.strip_prefix("pick:") {
        return (!key.is_empty()).then(|| Command::Pick(key.to_string()));
    }
    if let Some(name) = rest.strip_prefix("light:") {
        return match name {
            "frontlight" => Some(Command::Light(Light::Frontlight)),
            "warmth" => Some(Command::Light(Light::Warmth)),
            _ => None,
        };
    }
    if let Some(name) = rest.strip_prefix("quick:") {
        return (!name.is_empty()).then(|| Command::Quick(name.to_string()));
    }
    if let Some(tag) = rest.strip_prefix("language:") {
        return (!tag.is_empty()).then(|| Command::LanguagePick(tag.to_string()));
    }
    SECTIONS
        .contains(&rest)
        .then(|| Command::Section(rest.to_string()))
}

/// The options of a radio group, as `(stored value, label)`. An empty stored
/// value removes the key. None for a key with no fixed radio group. The reading
/// font list changes at run time, see [`font_options`].
pub fn options_for(key: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match key {
        keys::SYNC_FORWARD | keys::SYNC_BACKWARD => Some(&[
            ("prompt", "Ask first"),
            ("silent", "Use synced position"),
            ("never", "Never"),
        ]),
        keys::READING_TAP_ZONES => Some(&keys::TAP_ZONE_PRESETS),
        keys::READING_PAGE_TURN_WAVEFORM => Some(&keys::PAGE_TURN_WAVEFORMS),
        keys::DEVICE_SLEEP_SCREEN => Some(&keys::SLEEP_SCREENS),
        _ => None,
    }
}

/// The reading font choices that do not change: Engine default and the built-in
/// faces, as `(stored value, label)`.
const BUILTIN_FONT_OPTIONS: [(&str, &str); 5] = [
    ("", "Engine default"),
    ("Noto Serif", "Noto Serif"),
    ("Noto Sans", "Noto Sans"),
    ("Atkinson Hyperlegible", "Atkinson Hyperlegible"),
    ("OpenDyslexic", "OpenDyslexic"),
];

/// The reading font radio options as `(stored value, label)`: Engine default,
/// the built-in faces, then the user faces. The user faces come from the engine.
pub fn font_options() -> Vec<(String, String)> {
    font_options_with(&kaia_doc::cre::user_font_faces())
}

/// [`font_options`] with the user faces given. The reader's font list uses the same faces.
pub fn font_options_with(user: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = BUILTIN_FONT_OPTIONS
        .iter()
        .map(|(value, label)| (value.to_string(), label.to_string()))
        .collect();
    out.extend(
        extra_font_faces(user)
            .into_iter()
            .map(|face| (face.clone(), face)),
    );
    out
}

/// The user faces that the built-in list does not already show, sorted by name
/// and without repeats. A name matches a built-in face without regard to case.
pub fn extra_font_faces(user: &[String]) -> Vec<String> {
    let builtin: Vec<&str> = BUILTIN_FONT_OPTIONS
        .iter()
        .map(|(value, _)| *value)
        .filter(|value| !value.is_empty())
        .collect();
    let mut faces: Vec<String> = user
        .iter()
        .filter(|face| !builtin.iter().any(|b| b.eq_ignore_ascii_case(face)))
        .cloned()
        .collect();
    faces.sort_by_key(|face| (face.to_lowercase(), face.clone()));
    faces.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    faces
}

/// Registers the fonts that appeared in the user folder since the last scan.
/// Call it when a font list opens. A failed scan keeps the fonts already registered.
pub fn rescan_user_fonts() {
    let _ = kaia_doc::cre::rescan_fonts();
}

/// The value to store for radio option `index` of `key`. None when the key or index is unknown.
pub fn value_for_pick(key: &str, index: usize) -> Option<Value> {
    if matches!(key, keys::DEVICE_SLEEP_MINUTES | keys::DEVICE_POWER_OFF_MINUTES) {
        return keys::IDLE_MINUTES
            .get(index)
            .map(|n| Value::Number(f64::from(*n)));
    }
    if key == keys::READING_FONT_FAMILY {
        return font_options()
            .get(index)
            .map(|(stored, _)| Value::from(stored.as_str()));
    }
    let (stored, _) = options_for(key)?.get(index)?;
    Some(Value::from(*stored))
}

/// The keys a `settings:set:` id may write. Other keys are not stored.
pub fn is_known_key(key: &str) -> bool {
    if key == crate::dictionary_downloads::ALLOW_KEY {
        return true;
    }
    keys::EXPERIMENTAL_ENABLED == key
        || experiments::is_flag_key(experiments::FLAGS, key)
        || matches!(
            key,
            keys::DISPLAY_LARGE_PRINT
                | keys::READING_INVERT_PAGE_BUTTONS
                | keys::DEVICE_SLEEP_COVER
                | keys::DEVICE_PAGE_BUTTON_WAKE
                | keys::DEVICE_POWER_OFF_MINUTES
                | keys::SYNC_ENABLED
                | keys::SYNC_AUTO
                | keys::SYNC_INVALID_CERTS
                | keys::SYNC_FORWARD
                | keys::SYNC_BACKWARD
                | keys::READING_FONT_SIZE
                | keys::READING_FULL_REFRESH_PAGES
                | keys::DISPLAY_NIGHT_MODE
                | keys::DISPLAY_COLOR_RENDERING
                | keys::DISPLAY_ROTATION_LOCK
                | keys::DISPLAY_NATURAL_LIGHT_ENABLED
                | keys::DISPLAY_NATURAL_LIGHT_BEDTIME
                | keys::DISPLAY_NATURAL_LIGHT_DAY_WARMTH
                | keys::DISPLAY_NATURAL_LIGHT_BEDTIME_WARMTH
                | keys::LIBRARY_SHOW_FINISHED
                | keys::HOME_SHOW_CURRENT
                | keys::HOME_SHOW_NEXT_IN_SERIES
                | keys::HOME_SHOW_MORE_BY_AUTHOR
                | keys::HOME_SHOW_RECENT
                | keys::HOME_SHOW_IN_PROGRESS
                | keys::SYNC_SERVER
                | keys::SYNC_USERNAME
                | keys::SYNC_PASSWORD
                | keys::DEVICE_SLEEP_MINUTES
                | keys::READING_FONT_FAMILY
                | keys::READING_TAP_ZONES
                | keys::READING_PAGE_TURN_WAVEFORM
                | keys::READING_FOOTER_PAGE
                | keys::READING_FOOTER_PERCENT
                | keys::READING_FOOTER_TIME_CHAPTER
                | keys::READING_FOOTER_TIME_BOOK
                | keys::READING_FOOTER_CLOCK
                | keys::READING_FOOTER_BATTERY
                | keys::DEVICE_SLEEP_SCREEN
                | keys::DEVICE_NETWORK_TIME
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_and_controls() {
        assert_eq!(
            parse("settings:reading"),
            Some(Command::Section("reading".into()))
        );
        assert_eq!(parse("settings:nope"), None);
        assert_eq!(
            parse("settings:set:reading.font_size"),
            Some(Command::Set("reading.font_size".into()))
        );
        assert_eq!(parse("settings:search"), Some(Command::Search));
        assert_eq!(parse("settings:search:open"), Some(Command::SearchOpen));
        assert_eq!(parse("settings:sync:test"), Some(Command::SyncTest));
        assert_eq!(parse("settings:data:import"), Some(Command::Import));
        assert_eq!(
            parse("settings:light:warmth"),
            Some(Command::Light(Light::Warmth))
        );
        assert_eq!(parse("nav:settings"), None);
    }

    #[test]
    fn parses_the_dictionary_page_and_its_changes() {
        assert_eq!(parse("settings:dictionaries"), Some(Command::Dictionaries));
        assert_eq!(
            parse("settings:dicts:down:3"),
            Some(Command::Dictionary {
                index: 3,
                change: Change::Down
            })
        );
        assert_eq!(
            parse("settings:dicts:toggle:0"),
            Some(Command::Dictionary {
                index: 0,
                change: Change::Toggle
            })
        );
        assert_eq!(parse("settings:dicts:sideways:0"), None);
        assert_eq!(parse("settings:dicts:up:first"), None);
        assert_eq!(parse("settings:dicts:up"), None);
    }

    #[test]
    fn parses_the_time_zone_page_and_its_picks() {
        assert_eq!(parse("settings:timezone"), Some(Command::TimeZone));
        assert_eq!(
            parse("settings:timezone:America/Bogota"),
            Some(Command::TimeZonePick("America/Bogota".into()))
        );
        assert_eq!(
            parse("settings:timezone:"),
            Some(Command::TimeZonePick(String::new()))
        );
    }

    #[test]
    fn radio_picks_map_to_stored_values() {
        assert_eq!(
            value_for_pick(keys::READING_FONT_FAMILY, 0),
            Some(Value::from(""))
        );
        assert_eq!(
            value_for_pick(keys::READING_TAP_ZONES, 2),
            Some(Value::from("page_only"))
        );
        assert_eq!(value_for_pick(keys::READING_TAP_ZONES, 9), None);
        assert_eq!(
            value_for_pick(keys::READING_PAGE_TURN_WAVEFORM, 1),
            Some(Value::from("faster"))
        );
    }

    #[test]
    fn reading_font_picks_include_the_kaia_faces() {
        let faces: Vec<String> = font_options_with(&[])
            .into_iter()
            .map(|(value, _)| value)
            .collect();
        assert_eq!(
            faces,
            [
                "",
                "Noto Serif",
                "Noto Sans",
                "Atkinson Hyperlegible",
                "OpenDyslexic"
            ]
        );
        assert!(options_for(keys::READING_FONT_FAMILY).is_none());
        assert_eq!(
            value_for_pick(keys::READING_FONT_FAMILY, 3),
            Some(Value::from("Atkinson Hyperlegible"))
        );
        assert_eq!(
            value_for_pick(keys::READING_FONT_FAMILY, 4),
            Some(Value::from("OpenDyslexic"))
        );
    }

    #[test]
    fn user_faces_follow_the_built_in_faces_once_each() {
        let user = [
            "Zeta Mono",
            "opendyslexic",
            "Alpha Sans",
            "zeta mono",
            "Atkinson Hyperlegible",
        ]
        .map(str::to_string);
        let values: Vec<String> = font_options_with(&user)
            .into_iter()
            .map(|(value, _)| value)
            .collect();
        assert_eq!(
            values,
            [
                "",
                "Noto Serif",
                "Noto Sans",
                "Atkinson Hyperlegible",
                "OpenDyslexic",
                "Alpha Sans",
                "Zeta Mono",
            ]
        );
    }
}
