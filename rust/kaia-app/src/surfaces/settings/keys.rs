//! The keys of `kaia_events::Settings`, stored in `<data>/settings.json`.
//!
//! Every setting in the Settings surface reads and writes one of these keys.
//! A key that is not set takes the default in the table. The app writes the
//! file with [`kaia_events::Settings::flush`]; nothing else writes these keys.
//!
//! | Key | Type | Default | Meaning |
//! |---|---|---|---|
//! | `reading.font_family` | string | unset (engine default) | Reading font family name. |
//! | `reading.font_size` | number | 22 | Default font size in KOReader units. The app scales it to the panel width. |
//! | `reading.tap_zones` | string | `default` | Tap zone preset: `default`, `left_handed` or `page_only`. |
//! | `reading.full_refresh_pages` | number | 6 | E Ink full refresh every this many page turns. |
//! | `display.night_mode` | bool | false | Night mode: light text on a dark page. |
//! | `display.color_rendering` | bool | true | Color rendering on a color panel. |
//! | `display.rotation_lock` | bool | false | Reserved. Automatic rotation is not available. |
//! | `library.folders` | array of strings | the books folder | Folders the library scan reads. |
//! | `library.show_finished` | bool | true | Shows books marked as finished in the library. |
//! | `sync.server` | string | empty | Progress sync server address, `http://` or `https://`. |
//! | `sync.username` | string | empty | Progress sync account name. |
//! | `sync.password` | string | empty | Progress sync password, stored as typed. |
//! | `opds.catalogs` | array of tables | empty | User OPDS catalogs: `{ title = ..., url = ... }`. |
//! | `device.sleep_minutes` | number | 15 | Minutes of no input before the device sleeps. |
//! | `ui.language` | string | unset (system language, else English) | Language tag of the screens, such as `fr`. Set from the language page. |
//! | `device.sleep_screen` | string | `cover` | Sleep screen: `cover` (the current book) or `blank`. |
//!
//! `display.warmth` stores the last warmth value and restores it at startup.
//! The platform holds the current frontlight intensity and Wi-Fi state.
//! `sync.enabled` defaults to false. `sync.accept_invalid_certs` defaults to false.
//! `sync.forward` defaults to `prompt`; `sync.backward` defaults to `never`.
//!
//! Keys with a text value that the user types (`sync.*`) are saved as the
//! user types. The Settings surface flushes the file when the section closes.

use kaia_events::Settings;

pub const READING_FONT_FAMILY: &str = "reading.font_family";
pub const READING_FONT_SIZE: &str = "reading.font_size";
pub const READING_TAP_ZONES: &str = "reading.tap_zones";
pub const READING_FULL_REFRESH_PAGES: &str = "reading.full_refresh_pages";
pub const DISPLAY_NIGHT_MODE: &str = "display.night_mode";
pub const DISPLAY_COLOR_RENDERING: &str = "display.color_rendering";
pub const DISPLAY_ROTATION_LOCK: &str = "display.rotation_lock";
pub const LIBRARY_FOLDERS: &str = "library.folders";
pub const LIBRARY_SHOW_FINISHED: &str = "library.show_finished";
pub const SYNC_ENABLED: &str = "sync.enabled";
pub const SYNC_INVALID_CERTS: &str = "sync.accept_invalid_certs";
pub const SYNC_FORWARD: &str = "sync.forward";
pub const SYNC_BACKWARD: &str = "sync.backward";
pub const SYNC_SERVER: &str = "sync.server";
pub const SYNC_USERNAME: &str = "sync.username";
pub const SYNC_PASSWORD: &str = "sync.password";
pub const OPDS_CATALOGS: &str = "opds.catalogs";
pub const DEVICE_SLEEP_MINUTES: &str = "device.sleep_minutes";
pub const DEVICE_SLEEP_SCREEN: &str = "device.sleep_screen";
pub const UI_LANGUAGE: &str = crate::i18n::SETTING;

/// Default reading font size in KOReader units. It becomes 47 px on the Libra Colour.
pub const DEFAULT_FONT_SIZE: f64 = 22.0;
/// Default full refresh interval in page turns.
pub const DEFAULT_FULL_REFRESH_PAGES: f64 = 6.0;
/// Default sleep timeout in minutes.
pub const DEFAULT_SLEEP_MINUTES: f64 = 15.0;

/// The tap zone presets, in the order the Reading section lists them.
pub const TAP_ZONE_PRESETS: [(&str, &str); 3] = [
    ("default", "Default"),
    ("left_handed", "Left-handed"),
    ("page_only", "Page turn only"),
];

/// The sleep screen choices, in the order the Device section lists them.
pub const SLEEP_SCREENS: [(&str, &str); 2] = [("cover", "Book cover"), ("blank", "Blank")];

pub fn bool_or(settings: &Settings, key: &str, default: bool) -> bool {
    settings.read(key).as_bool().unwrap_or(default)
}

pub fn number_or(settings: &Settings, key: &str, default: f64) -> f64 {
    settings.read(key).as_f64().unwrap_or(default)
}

/// The string at `key`, or `default` when the key is unset or not a string.
pub fn text_or(settings: &Settings, key: &str, default: &str) -> String {
    settings.read(key).as_str().unwrap_or(default).to_string()
}

/// The string at `key`, or the empty string.
pub fn text(settings: &Settings, key: &str) -> String {
    text_or(settings, key, "")
}

/// The index of the preset `value` in `presets`, or 0 for an unknown value.
pub fn preset_index(presets: &[(&str, &str)], value: &str) -> usize {
    presets.iter().position(|(id, _)| *id == value).unwrap_or(0)
}
