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
//! | `reading.font_weight.<family>` | number | 0 | Text weight of one font family, as a KOReader font weight value: -1, -0.5, 0, 0.5, 1, 1.5 or 3. Zero is regular. `<family>` is the face name, or `(book)` for the book's own font. See [`font_weight_key`]. |
//! | `reading.tap_zones` | string | `default` | Tap zone preset: `default`, `left_handed` or `page_only`. |
//! | `reading.full_refresh_pages` | number | 0 | E Ink full refresh cadence. `0` means at each chapter. `1` to `10` means every N page turns, and each chapter start is also a full refresh. A value above 10 reads as 10. See [`full_refresh_pages`]. |
//! | `reading.page_turn_waveform` | string | `cleaner` | E Ink waveform of a page turn: `cleaner` (gray with ghost reduction) or `faster` (fast black and white). Full refreshes keep their waveform. See [`page_turn`]. |
//! | `reading.highlight_shade` | number | 0.2 | Shade of a box highlight, from 0 to 1 in steps of 0.1, as KOReader's "Gray highlight opacity". A higher value is darker. The reader's More sheet shows it as a percent. See [`highlight_shade`]. |
//! | `reading.footer_page` | bool | true | Reader footer shows the page number, as `Page 3 of 200`. |
//! | `reading.footer_percent` | bool | false | Reader footer shows the reading percent. |
//! | `reading.footer_time_chapter` | bool | false | Reader footer shows the time left in the chapter, or in the book before the first chapter. |
//! | `reading.footer_time_book` | bool | false | Reader footer shows the time left in the book. |
//! | `reading.footer_clock` | bool | false | Reader footer shows the clock, read when the page redraws or the reader opens its bars. |
//! | `reading.footer_battery` | bool | false | Reader footer shows the battery level, read when the page redraws or the reader opens its bars. |
//! | `display.night_mode` | bool | false | Night mode: light text on a dark page. |
//! | `display.color_rendering` | bool | true | Color rendering on a color panel. |
//! | `display.rotation_lock` | bool | false | Ignore automatic orientation changes. |
//! | `display.natural_light_enabled` | bool | false | Adjust warmth on the local clock schedule. |
//! | `display.natural_light_bedtime` | number | 1380 | Bedtime in minutes after midnight, default 23:00. Values from 05:00 through 18:00 use 23:00. |
//! | `display.natural_light_day_warmth` | number | 0 | Daytime warmth, from 0 to 100. |
//! | `display.natural_light_bedtime_warmth` | number | 92 | Bedtime warmth, from 0 to 100. |
//! | `library.folders` | array of strings | the books folder | Folders the library scan reads. |
//! | `library.show_finished` | bool | true | Shows books marked as finished in the library. |
//! | `library.layout` | string | `covers` | Layout of the Books tab: `covers` or `list`. Kept between runs, also while another tab is open. |
//! | `sync.server` | string | empty | Progress sync server address, `http://` or `https://`. |
//! | `sync.username` | string | empty | Progress sync account name. |
//! | `sync.userkey` | string | empty | MD5 hex of the progress sync password, the key KOReader stores. The password itself is never stored. |
//! | `sync.password` | string | unset | Old plain password. The startup migration replaces it with `sync.userkey` and removes it. |
//! | `opds.catalogs` | array of tables | empty | User OPDS catalogs: `{ title = ..., url = ... }`. |
//! | `device.sleep_minutes` | number | 15 | Minutes of no input before the device sleeps. |
//! | `ui.language` | string | unset (system language, else English) | Language tag of the screens, such as `fr`. Set from the language page. |
//! | `device.sleep_screen` | string | `cover` | Sleep screen: `cover` (the current book) or `blank`. |
//! | `device.time_zone` | string | unset (system zone) | IANA name of the zone for Kaia's clock, such as `America/Bogota`. Set from the Time zone page. Kaia sets `TZ` at startup, so the footer clock, natural light and statistics dates follow it. An unknown name counts as unset. See `kaia_platform::timezone`. |
//! | `display.large_print` | bool | false | Large print: every font size times `theme::LARGE_PRINT_SCALE`. |
//! | `network.airplane_mode` | bool | false | Airplane mode: Wi-Fi stays off and Kaia does not turn it on. Set from quick settings. |
//! | `experimental.enabled` | bool | false | Master switch of the experimental features. See `experiments`. |
//! | `experimental.<id>` | bool | false | One experimental flag. It works only when the master switch is on. |
//!
//! `display.warmth` stores the last warmth value and restores it at startup.
//! The platform holds the current frontlight intensity and Wi-Fi state.
//! `sync.enabled` defaults to false. `sync.accept_invalid_certs` defaults to false.
//! `sync.forward` defaults to `prompt`; `sync.backward` defaults to `never`.
//!
//! Text fields save as the user types, and the Settings surface flushes the file
//! when the section closes. The password field is the exception: typing into it
//! stores `sync.userkey`, never the text (see `sync::store_password`).

use kaia_display::waveform::PageTurn;
use kaia_events::Settings;

pub const READING_FONT_FAMILY: &str = "reading.font_family";
pub const READING_FONT_SIZE: &str = "reading.font_size";
/// Prefix of the per-family text weight keys. See [`font_weight_key`].
pub const READING_FONT_WEIGHT_PREFIX: &str = "reading.font_weight.";
/// The family segment of the text weight key for the book's own font.
pub const FONT_WEIGHT_BOOK_FONT: &str = "(book)";
pub const READING_TAP_ZONES: &str = "reading.tap_zones";
pub const READING_FULL_REFRESH_PAGES: &str = "reading.full_refresh_pages";
/// Waveform of a reader page turn. See [`page_turn`].
pub const READING_PAGE_TURN_WAVEFORM: &str = "reading.page_turn_waveform";
/// Shade of a box highlight. See [`highlight_shade`].
pub const READING_HIGHLIGHT_SHADE: &str = "reading.highlight_shade";
/// Reader footer items, each on or off. See the Footer sheet of the reader.
pub const READING_FOOTER_PAGE: &str = "reading.footer_page";
pub const READING_FOOTER_PERCENT: &str = "reading.footer_percent";
pub const READING_FOOTER_TIME_CHAPTER: &str = "reading.footer_time_chapter";
pub const READING_FOOTER_TIME_BOOK: &str = "reading.footer_time_book";
pub const READING_FOOTER_CLOCK: &str = "reading.footer_clock";
pub const READING_FOOTER_BATTERY: &str = "reading.footer_battery";
pub const DISPLAY_NIGHT_MODE: &str = "display.night_mode";
pub const DISPLAY_COLOR_RENDERING: &str = "display.color_rendering";
pub const DISPLAY_NATURAL_LIGHT_ENABLED: &str = "display.natural_light_enabled";
pub const DISPLAY_NATURAL_LIGHT_BEDTIME: &str = "display.natural_light_bedtime";
pub const DISPLAY_NATURAL_LIGHT_DAY_WARMTH: &str = "display.natural_light_day_warmth";
pub const DISPLAY_NATURAL_LIGHT_BEDTIME_WARMTH: &str = "display.natural_light_bedtime_warmth";
/// Manual orientation in degrees, restored at startup.
pub const DISPLAY_ROTATION: &str = "display.rotation";
/// Swap the bezel page keys. Default false.
pub const READING_INVERT_PAGE_BUTTONS: &str = "reading.invert_page_buttons";
/// Accept sleep cover events. Default true.
pub const DEVICE_SLEEP_COVER: &str = "device.sleep_cover";
/// Wake on a page button where supported. Default false.
pub const DEVICE_PAGE_BUTTON_WAKE: &str = "device.page_button_wake";
/// Idle power-off timeout in minutes. Zero means never and is the default.
pub const DEVICE_POWER_OFF_MINUTES: &str = "device.power_off_minutes";
pub const DISPLAY_ROTATION_LOCK: &str = "display.rotation_lock";
pub const LIBRARY_FOLDERS: &str = "library.folders";
pub const LIBRARY_SHOW_FINISHED: &str = "library.show_finished";
pub const SYNC_ENABLED: &str = "sync.enabled";
pub const SYNC_INVALID_CERTS: &str = "sync.accept_invalid_certs";
pub const SYNC_FORWARD: &str = "sync.forward";
pub const SYNC_BACKWARD: &str = "sync.backward";
pub const SYNC_SERVER: &str = "sync.server";
pub const SYNC_USERNAME: &str = "sync.username";
/// The password field's id. Typed text is stored under `SYNC_USERKEY`, not here.
/// This key is also the old plain password that the startup migration removes.
pub const SYNC_PASSWORD: &str = "sync.password";
/// MD5 hex of the sync password, the value KOReader stores as `userkey`.
pub const SYNC_USERKEY: &str = "sync.userkey";
pub const OPDS_CATALOGS: &str = "opds.catalogs";
pub const DEVICE_SLEEP_MINUTES: &str = "device.sleep_minutes";
pub const DEVICE_SLEEP_SCREEN: &str = "device.sleep_screen";
/// IANA name of the zone for Kaia's clock. Unset means the system zone.
pub const DEVICE_TIME_ZONE: &str = "device.time_zone";
pub const UI_LANGUAGE: &str = crate::i18n::SETTING;
pub const DISPLAY_LARGE_PRINT: &str = "display.large_print";
pub const NETWORK_AIRPLANE_MODE: &str = "network.airplane_mode";
pub const EXPERIMENTAL_ENABLED: &str = "experimental.enabled";

/// Default reading font size in KOReader units. It becomes 47 px on the Libra Colour.
pub const DEFAULT_FONT_SIZE: f64 = 22.0;
/// Default full refresh cadence: zero, which means at each chapter.
pub const DEFAULT_FULL_REFRESH_PAGES: f64 = 0.0;
/// The largest page count the full refresh choice offers. Larger saved values read as this.
pub const FULL_REFRESH_PAGES_MAX: u32 = 10;
/// Default sleep timeout in minutes.
pub const DEFAULT_SLEEP_MINUTES: f64 = 15.0;
/// KOReader's default highlight opacity, `highlight_lighten_factor`. Kaia draws the
/// default shade as it drew boxes before the setting existed. See `page::shade_tint`.
pub const DEFAULT_HIGHLIGHT_SHADE: f64 = 0.2;

/// The tap zone presets, in the order the Reading section lists them.
pub const TAP_ZONE_PRESETS: [(&str, &str); 3] = [
    ("default", "Default"),
    ("left_handed", "Left-handed"),
    ("page_only", "Page turn only"),
];

/// The page turn waveform choices, in the order the Reading section lists them.
/// The first is the default.
pub const PAGE_TURN_WAVEFORMS: [(&str, &str); 2] =
    [("cleaner", "Cleaner page turns"), ("faster", "Faster page turns")];

/// The sleep screen choices, in the order the Device section lists them.
pub const SLEEP_SCREENS: [(&str, &str); 2] = [("cover", "Book cover"), ("blank", "Blank")];

/// Idle timeout choices. Zero means never.
pub const IDLE_MINUTES: [u32; 7] = [0, 5, 10, 15, 30, 45, 60];

pub fn bool_or(settings: &Settings, key: &str, default: bool) -> bool {
    settings.read(key).as_bool().unwrap_or(default)
}

/// Whether airplane mode is on. Kaia must not turn Wi-Fi on while it is.
pub fn airplane_mode(settings: &Settings) -> bool {
    bool_or(settings, NETWORK_AIRPLANE_MODE, false)
}

pub fn number_or(settings: &Settings, key: &str, default: f64) -> f64 {
    settings.read(key).as_f64().unwrap_or(default)
}

/// The full refresh cadence from `reading.full_refresh_pages`. Zero means a full
/// refresh at each chapter start. From 1 to 10, the value is the page count, and
/// a larger value reads as 10. A negative value reads as zero. The Settings row
/// and the reader both read through here, so they show and do the same thing.
pub fn full_refresh_pages(settings: &Settings) -> u32 {
    let n = number_or(settings, READING_FULL_REFRESH_PAGES, DEFAULT_FULL_REFRESH_PAGES);
    if n >= 1.0 {
        n.min(f64::from(FULL_REFRESH_PAGES_MAX)).floor() as u32
    } else {
        0
    }
}

/// The waveform of a reader page turn from `reading.page_turn_waveform`. Unset, or a
/// value that is not on offer, is `cleaner`. The app reads it for each partial
/// refresh while the reader is on top.
pub fn page_turn(settings: &Settings) -> PageTurn {
    match text_or(settings, READING_PAGE_TURN_WAVEFORM, PAGE_TURN_WAVEFORMS[0].0).as_str() {
        "faster" => PageTurn::Faster,
        _ => PageTurn::Cleaner,
    }
}

/// The box highlight shade from `reading.highlight_shade`, from 0 to 1. Unset is
/// [`DEFAULT_HIGHLIGHT_SHADE`]. A value outside the range reads as the nearest end.
pub fn highlight_shade(settings: &Settings) -> f64 {
    number_or(settings, READING_HIGHLIGHT_SHADE, DEFAULT_HIGHLIGHT_SHADE).clamp(0.0, 1.0)
}

/// The shade for a percent the More sheet offers. The sheet steps by 10 percent,
/// which is KOReader's step of 0.1. The result is rounded to that step.
pub fn shade_from_percent(percent: f64) -> f64 {
    (percent / 10.0).round().clamp(0.0, 10.0) / 10.0
}

/// The settings key of a face's text weight. `None` is the book's own font.
pub fn font_weight_key(family: Option<&str>) -> String {
    format!(
        "{READING_FONT_WEIGHT_PREFIX}{}",
        family.unwrap_or(FONT_WEIGHT_BOOK_FONT)
    )
}

/// The saved text weight of a face, as a KOReader font weight value. Unset is 0, which is regular.
pub fn font_weight(settings: &Settings, family: Option<&str>) -> f64 {
    number_or(settings, &font_weight_key(family), 0.0)
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

/// The zone stored under `device.time_zone`. None when the key is unset or names a
/// zone that is not on offer. Both mean the system zone.
pub fn time_zone(settings: &Settings) -> Option<&'static kaia_platform::timezone::Zone> {
    kaia_platform::timezone::find(settings.read(DEVICE_TIME_ZONE).as_str()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_events::Value;

    /// The cadence for a stored value, or for no stored value when `value` is None.
    fn cadence(value: Option<f64>) -> u32 {
        // The file does not exist, so nothing is read from disk and nothing is written.
        let mut settings = Settings::open(std::path::Path::new("/nonexistent/kaia-keys-test/settings.json"));
        if let Some(v) = value {
            settings.save(READING_FULL_REFRESH_PAGES, Value::from(v));
        }
        full_refresh_pages(&settings)
    }

    #[test]
    fn full_refresh_pages_reads_chapter_pages_and_clamps() {
        assert_eq!(cadence(None), 0, "an unset key is each chapter");
        assert_eq!(cadence(Some(0.0)), 0, "zero is each chapter");
        assert_eq!(cadence(Some(1.0)), 1);
        assert_eq!(
            cadence(Some(6.0)),
            6,
            "a page count saved before this change keeps its value"
        );
        assert_eq!(cadence(Some(10.0)), 10);
        assert_eq!(cadence(Some(25.0)), 10, "a saved value above 10 reads as 10");
        assert_eq!(cadence(Some(2.7)), 2);
        assert_eq!(cadence(Some(-3.0)), 0, "a negative value reads as each chapter");
    }

    #[test]
    fn page_turn_is_cleaner_unless_faster_is_saved() {
        let mut settings = Settings::open(std::path::Path::new("/nonexistent/kaia-keys-test/settings.json"));
        assert_eq!(page_turn(&settings), PageTurn::Cleaner, "unset is the default");
        settings.save(READING_PAGE_TURN_WAVEFORM, Value::from("faster"));
        assert_eq!(page_turn(&settings), PageTurn::Faster);
        settings.save(READING_PAGE_TURN_WAVEFORM, Value::from("unknown"));
        assert_eq!(
            page_turn(&settings),
            PageTurn::Cleaner,
            "a value not on offer is the default"
        );
        assert_eq!(
            PAGE_TURN_WAVEFORMS[0].0, "cleaner",
            "the first choice is the default"
        );
    }

    #[test]
    fn highlight_shade_is_the_default_when_unset_and_stays_in_range() {
        let mut settings = Settings::open(std::path::Path::new("/nonexistent/kaia-keys-test/settings.json"));
        assert_eq!(highlight_shade(&settings), 0.2, "unset is KOReader's default");
        settings.save(READING_HIGHLIGHT_SHADE, Value::from(0.6));
        assert_eq!(highlight_shade(&settings), 0.6);
        settings.save(READING_HIGHLIGHT_SHADE, Value::from(3.0));
        assert_eq!(highlight_shade(&settings), 1.0, "above one reads as one");
        settings.save(READING_HIGHLIGHT_SHADE, Value::from(-1.0));
        assert_eq!(highlight_shade(&settings), 0.0, "below zero reads as zero");
    }

    #[test]
    fn shade_from_percent_rounds_to_tenths() {
        assert_eq!(shade_from_percent(20.0), 0.2);
        assert_eq!(shade_from_percent(20.000000000000004), 0.2, "stepper float drift");
        assert_eq!(shade_from_percent(70.0), 0.7);
        assert_eq!(shade_from_percent(-10.0), 0.0);
        assert_eq!(shade_from_percent(140.0), 1.0);
    }

    #[test]
    fn font_weight_is_kept_per_family_and_regular_when_unset() {
        let mut settings = Settings::open(std::path::Path::new("/nonexistent/kaia-keys-test/settings.json"));
        assert_eq!(font_weight(&settings, Some("Noto Serif")), 0.0);
        settings.save(font_weight_key(Some("Noto Serif")), Value::from(1.5));
        assert_eq!(font_weight(&settings, Some("Noto Serif")), 1.5);
        assert_eq!(
            font_weight(&settings, Some("Noto Sans")),
            0.0,
            "another family keeps its own"
        );
        assert_eq!(font_weight_key(None), "reading.font_weight.(book)");
    }
}
