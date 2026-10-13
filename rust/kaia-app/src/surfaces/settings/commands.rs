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

use kaia_events::Value;

use super::keys;

/// The sections, in the order the root lists them.
pub const SECTIONS: [&str; 10] = [
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
}

/// Parses a settings action id. None for every other id, and for an unknown section.
pub fn parse(id: &str) -> Option<Command> {
    let rest = id.strip_prefix("settings:")?;
    match rest {
        "search" => return Some(Command::Search),
        "search:open" => return Some(Command::SearchOpen),
        "fields:open" => return Some(Command::FieldsOpen),
        "fields:close" => return Some(Command::FieldsClose),
        "search:close" => return Some(Command::SearchClose),
        "sync:test" => return Some(Command::SyncTest),
        "data:import" => return Some(Command::Import),
        "data:export" => return Some(Command::ExportNotes),
        _ => {}
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
    SECTIONS
        .contains(&rest)
        .then(|| Command::Section(rest.to_string()))
}

/// The options of a radio group, as `(stored value, label)`. An empty stored
/// value removes the key. None for a key with no radio group.
pub fn options_for(key: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match key {
        keys::SYNC_FORWARD | keys::SYNC_BACKWARD => Some(&[
            ("prompt", "Ask first"),
            ("silent", "Use synced position"),
            ("never", "Never"),
        ]),
        keys::READING_FONT_FAMILY => Some(&[
            ("", "Engine default"),
            ("Noto Serif", "Noto Serif"),
            ("Noto Sans", "Noto Sans"),
        ]),
        keys::READING_TAP_ZONES => Some(&keys::TAP_ZONE_PRESETS),
        keys::DEVICE_SLEEP_SCREEN => Some(&keys::SLEEP_SCREENS),
        _ => None,
    }
}

/// The value to store for radio option `index` of `key`. None when the key or index is unknown.
pub fn value_for_pick(key: &str, index: usize) -> Option<Value> {
    let (stored, _) = options_for(key)?.get(index)?;
    Some(Value::from(*stored))
}

/// The keys a `settings:set:` id may write. Other keys are not stored.
pub fn is_known_key(key: &str) -> bool {
    matches!(
        key,
        keys::SYNC_ENABLED
            | keys::SYNC_INVALID_CERTS
            | keys::SYNC_FORWARD
            | keys::SYNC_BACKWARD
            | keys::READING_FONT_SIZE
            | keys::READING_FULL_REFRESH_PAGES
            | keys::DISPLAY_NIGHT_MODE
            | keys::DISPLAY_COLOR_RENDERING
            | keys::DISPLAY_ROTATION_LOCK
            | keys::LIBRARY_SHOW_FINISHED
            | keys::SYNC_SERVER
            | keys::SYNC_USERNAME
            | keys::SYNC_PASSWORD
            | keys::DEVICE_SLEEP_MINUTES
            | keys::READING_FONT_FAMILY
            | keys::READING_TAP_ZONES
            | keys::DEVICE_SLEEP_SCREEN
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
    }
}
