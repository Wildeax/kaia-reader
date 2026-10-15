//! The contents of each settings section. [`entries`] reads the settings, the
//! platform and the device folders and returns the rows of one section.

use std::path::{Path, PathBuf};

use kaia_i18n::{format as tr_format, tr};
use kaia_platform::platform::Kind;
use kaia_stats::DayTotal;

use super::commands::{font_options, options_for, rescan_user_fonts, Light};
use super::entries::{Entry, StepUnit};
use super::experiments;
use super::facts::{self, duration, KoreaderFolder, StatsView};
use super::keys;
use crate::services::Services;
use crate::surfaces::reader::dict;

/// The title of a section.
pub fn title(name: &str) -> String {
    match name {
        "home" => tr("Home"),
        "reading" => tr("Reading"),
        "dictionaries" => tr("Dictionary settings"),
        "dictionary-downloads" => tr("Download dictionaries"),
        "display" => tr("Display and light"),
        "library" => tr("Library"),
        "network" => tr("Network and sync"),
        "statistics" => tr("Statistics"),
        "device" => tr("Device"),
        "storage" => tr("Storage"),
        "extensions" => tr("Extensions"),
        "plugin-menus" => tr("Plugins"),
        "data" => tr("Data"),
        "about" => tr("About"),
        "advanced" => tr("Advanced"),
        "language" => tr("Language"),
        "timezone" => tr("Time zone"),
        _ if name.starts_with("plugin:") => tr("Plugin"),
        _ => tr("Settings"),
    }
}

/// Whether the device is a Kobo, where the frontlight and warmth work.
pub fn has_frontlight(services: &Services) -> bool {
    matches!(services.platform.kind, Kind::Kobo(_))
}

/// The rows of section `name` at Unix time `now`.
pub fn entries(name: &str, services: &Services, now: i64) -> Vec<Entry> {
    match name {
        "home" => home(services),
        "reading" => reading(services),
        "dictionaries" => dictionary_page(services),
        "dictionary-downloads" => download_page(services),
        "display" => display(services),
        "library" => library(services),
        "network" => network(services),
        "statistics" => statistics(services, now),
        "device" => device(services),
        "storage" => super::storage::entries(services),
        "extensions" => extensions(services),
        "plugin-menus" => reader_menus(services),
        "language" => language(services),
        "timezone" => time_zone_page(services),
        "data" => data(services),
        "about" => about(),
        "advanced" => advanced(),
        "experimental" => experimental(services),
        _ if name.starts_with("plugin:") => plugin_entries(&name[7..], services),
        _ => Vec::new(),
    }
}

fn reading(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let family = keys::text_or(s, keys::READING_FONT_FAMILY, "");
    // Fonts copied into the user folder since the last visit show up in the list.
    rescan_user_fonts();
    let faces = font_options();
    let family_index = faces.iter().position(|(v, _)| *v == family).unwrap_or(0);
    let zones = keys::text_or(s, keys::READING_TAP_ZONES, "default");
    let page_turn = keys::text_or(
        s,
        keys::READING_PAGE_TURN_WAVEFORM,
        keys::PAGE_TURN_WAVEFORMS[0].0,
    );
    // The section loads the dictionaries before it builds. See `section::lists_dictionaries`.
    let dictionaries = dict::folder(services);
    let found = dict::installed(services).len();
    let mut out = vec![
        Entry::Heading(tr("Text")),
        Entry::Radio {
            key: keys::READING_FONT_FAMILY,
            options: faces.iter().map(|(_, label)| option_label(label)).collect(),
            selected: family_index,
        },
        Entry::Text(tr_format(
            &tr("Add .ttf or .otf files to %1. Kaia finds them when you open this section or the font list. A deleted font stays listed until Kaia restarts."),
            &[&font_folder_label(&services.paths.data_dir)],
        )),
        Entry::Stepper {
            key: keys::READING_FONT_SIZE,
            label: tr_format(
                &tr("Font size, %1 px"),
                &[&crate::surfaces::reader::koreader_px(
                    kaia_gfx::Size::new(services.theme.scale.width, services.theme.scale.height),
                    keys::number_or(s, keys::READING_FONT_SIZE, keys::DEFAULT_FONT_SIZE),
                )
                .to_string()],
            ),
            value: keys::number_or(s, keys::READING_FONT_SIZE, keys::DEFAULT_FONT_SIZE),
            min: 12.0,
            max: 40.0,
            step: 1.0,
            unit: StepUnit::KoreaderUnits,
        },
        Entry::Heading(tr("Page turns")),
        Entry::Toggle {
            key: keys::READING_INVERT_PAGE_BUTTONS,
            label: tr("Invert page buttons"),
            on: keys::bool_or(s, keys::READING_INVERT_PAGE_BUTTONS, false),
        },
        Entry::Radio {
            key: keys::READING_TAP_ZONES,
            options: labels(keys::READING_TAP_ZONES),
            selected: keys::preset_index(&keys::TAP_ZONE_PRESETS, &zones),
        },
        Entry::Toggle {
            key: keys::READING_DOUBLE_TAP_LOOKUP,
            label: tr("Double tap a word to look it up"),
            on: keys::double_tap_lookup(s),
        },
        Entry::Text(tr(
            "When this is on, a single tap turns the page 0.3 seconds after the tap, so the reader can tell it from a double tap.",
        )),
        Entry::Stepper {
            key: keys::READING_FULL_REFRESH_PAGES,
            label: tr("Full refresh every"),
            value: f64::from(keys::full_refresh_pages(s)),
            min: 0.0,
            max: f64::from(keys::FULL_REFRESH_PAGES_MAX),
            step: 1.0,
            unit: StepUnit::ChapterOrPages,
        },
        Entry::Radio {
            key: keys::READING_PAGE_TURN_WAVEFORM,
            options: labels(keys::READING_PAGE_TURN_WAVEFORM),
            selected: keys::preset_index(&keys::PAGE_TURN_WAVEFORMS, &page_turn),
        },
        Entry::Heading(tr("Dictionary")),
        Entry::Row {
            primary: tr("Dictionary folder"),
            // The last two parts only. See `facts::short_path`.
            secondary: Some(facts::short_path(&dictionaries)),
            value: Some(tr_format(&tr("%1 found"), &[&found.to_string()])),
            action: None,
        },
    ];
    if found > 0
        || services
            .dictionary
            .as_ref()
            .is_some_and(|lib| !lib.errors().is_empty())
    {
        out.push(Entry::Row {
            primary: tr("Dictionary settings"),
            secondary: None,
            value: None,
            action: Some("settings:dictionaries".into()),
        });
    }
    download_controls(services, &mut out);
    out
}

fn download_controls(services: &Services, out: &mut Vec<Entry>) {
    let on = crate::dictionary_downloads::allowed(&services.settings);
    out.push(Entry::Toggle {
        key: crate::dictionary_downloads::ALLOW_KEY,
        label: tr("Allow dictionary downloads"),
        on,
    });
    out.push(Entry::Text(tr("Dictionary downloads fetch files from the internet. Each file must match its pinned SHA-256 checksum.")));
    if on {
        out.push(Entry::Row {
            primary: tr("Download dictionaries"),
            secondary: None,
            value: None,
            action: Some("settings:dictionary-downloads".into()),
        });
    }
}

fn download_page(services: &Services) -> Vec<Entry> {
    if !crate::dictionary_downloads::allowed(&services.settings) {
        return Vec::new();
    }
    let busy = services.dictionary_downloads.busy();
    let mut out = vec![Entry::Text(tr(
        "These dictionaries come from the internet. Tap a dictionary to download and install it.",
    ))];
    if busy {
        out.push(Entry::Text(tr("Downloading dictionary…")));
    }
    for entry in kaia_dict::download::catalog() {
        let installed = services.paths.data_dir.join("dict").join(&entry.id).exists();
        out.push(Entry::Row {
            primary: entry.name.clone(),
            secondary: Some(format!(
                "{} → {} · {:.1} MB · {}",
                entry.lang_in,
                entry.lang_out,
                entry.size as f64 / 1_000_000.0,
                entry.license
            )),
            value: Some(if installed {
                tr("Installed")
            } else {
                tr("Download")
            }),
            action: (!installed && !busy).then(|| format!("settings:dictionary-install:{}", entry.id)),
        });
    }
    out
}

/// The Dictionary settings page. It lists the installed dictionaries in lookup order,
/// each with its switch and move buttons. With no dictionary installed, the page
/// shows the install hint instead. Call `dict::load` first.
fn dictionary_page(services: &Services) -> Vec<Entry> {
    let names = dict::ordered(services);
    let off = dict::disabled(services);
    let mut out = vec![Entry::Text(tr(
        "Dictionaries are searched in this order. A dictionary that is off is skipped.",
    ))];
    if names.is_empty() {
        out.push(Entry::Text(dict::install_hint(&services.paths.data_dir)));
    }
    if let Some(library) = &services.dictionary {
        for (path, error) in library.errors() {
            out.push(Entry::Text(tr_format(
                &tr("Skipped dictionary %1: %2"),
                &[
                    &path.file_name().unwrap_or_default().to_string_lossy(),
                    &error.to_string(),
                ],
            )));
        }
    }
    for (index, name) in names.into_iter().enumerate() {
        let enabled = !off.contains(&name);
        out.push(Entry::Dictionary { index, name, enabled });
    }
    download_controls(services, &mut out);
    out
}

fn display(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let mut out = vec![Entry::Heading(tr("Light"))];
    if has_frontlight(services) {
        out.push(Entry::Slider {
            which: Light::Frontlight,
            label: tr("Frontlight"),
            value: f64::from(services.platform.frontlight.intensity()),
            enabled: true,
        });
        out.push(Entry::Slider {
            which: Light::Warmth,
            label: tr("Warmth"),
            value: f64::from(services.platform.frontlight.warmth()),
            enabled: true,
        });
        out.push(Entry::Toggle {
            key: keys::DISPLAY_NATURAL_LIGHT_ENABLED,
            label: tr("Natural light schedule"),
            on: keys::bool_or(s, keys::DISPLAY_NATURAL_LIGHT_ENABLED, false),
        });
        out.push(Entry::Stepper {
            key: keys::DISPLAY_NATURAL_LIGHT_BEDTIME,
            label: tr("Bedtime"),
            value: keys::number_or(s, keys::DISPLAY_NATURAL_LIGHT_BEDTIME, 1380.0),
            min: 0.0,
            max: 1410.0,
            step: 30.0,
            unit: StepUnit::ClockTime,
        });
        for (key, label, default) in [
            (keys::DISPLAY_NATURAL_LIGHT_DAY_WARMTH, tr("Daytime warmth"), 0.0),
            (
                keys::DISPLAY_NATURAL_LIGHT_BEDTIME_WARMTH,
                tr("Bedtime warmth"),
                92.0,
            ),
        ] {
            out.push(Entry::Stepper {
                key,
                label,
                value: keys::number_or(s, key, default),
                min: 0.0,
                max: 100.0,
                step: 1.0,
                unit: StepUnit::Percent,
            });
        }
        out.push(Entry::Text(tr(
            "If you change the warmth by hand, the schedule waits until its next step. A bedtime between 05:00 and 18:00 counts as 23:00.",
        )));
    } else {
        out.push(Entry::Text(tr(
            "The frontlight and warmth work on the Kobo only. This is a desktop build.",
        )));
    }
    out.push(Entry::Heading(tr("Screen")));
    out.push(Entry::Toggle {
        key: keys::DISPLAY_LARGE_PRINT,
        label: tr("Large print"),
        on: keys::bool_or(s, keys::DISPLAY_LARGE_PRINT, false),
    });
    let percent = ((kaia_ui::theme::LARGE_PRINT_SCALE - 1.0) * 100.0).round();
    out.push(Entry::Text(tr_format(
        &tr("Makes the text on every screen %1% larger."),
        &[&percent.to_string()],
    )));
    out.push(Entry::Toggle {
        key: keys::DISPLAY_NIGHT_MODE,
        label: tr("Night mode"),
        on: keys::bool_or(s, keys::DISPLAY_NIGHT_MODE, false),
    });
    out.push(Entry::Toggle {
        key: keys::DISPLAY_COLOR_RENDERING,
        label: tr("Color rendering"),
        on: keys::bool_or(s, keys::DISPLAY_COLOR_RENDERING, true),
    });
    out.push(Entry::Text(tr(
        "Color rendering applies on the color panel only. Grayscale panels ignore it.",
    )));
    out.push(Entry::Toggle {
        key: keys::DISPLAY_ROTATION_LOCK,
        label: tr("Lock orientation"),
        on: keys::bool_or(s, keys::DISPLAY_ROTATION_LOCK, false),
    });
    out.push(Entry::Heading(tr("Orientation")));
    out.push(Entry::Radio {
        key: keys::DISPLAY_ROTATION,
        options: super::quick::orientation_labels(),
        selected: (keys::number_or(
            s,
            keys::DISPLAY_ROTATION,
            f64::from(services.platform.device.startup_rotation_degrees()),
        ) / 90.0) as usize,
    });
    out.push(Entry::Heading(tr("Language")));
    let current = crate::i18n::available(&services.catalog_root)
        .into_iter()
        .find(|(tag, _)| *tag == services.language)
        .map_or(services.language.clone(), |(_, name)| name.to_string());
    out.push(Entry::Row {
        primary: current,
        secondary: None,
        value: None,
        action: Some("settings:language".into()),
    });
    out
}

/// The languages on offer, one row each. The row of the language in use has a checkbox.
fn language(services: &Services) -> Vec<Entry> {
    crate::i18n::available(&services.catalog_root)
        .into_iter()
        .map(|(tag, name)| Entry::Choice {
            label: name.to_string(),
            selected: tag == services.language,
            action: format!("settings:language:{tag}"),
        })
        .collect()
}

/// The Time zone page. "System default" comes first. The zones follow, by standard
/// offset and then by name, and the zone in use has a checkbox. The zone applies
/// at the next start.
fn time_zone_page(services: &Services) -> Vec<Entry> {
    let current = keys::time_zone(&services.settings).map(|zone| zone.name);
    let mut out = vec![
        Entry::Text(tr(
            "Kaia uses this zone for the clock, natural light and reading statistics. A change applies after a restart.",
        )),
        Entry::Choice {
            label: tr("System default"),
            selected: current.is_none(),
            action: "settings:timezone:".into(),
        },
    ];
    out.extend(
        kaia_platform::timezone::sorted()
            .into_iter()
            .map(|zone| Entry::Choice {
                label: format!("{}  {}", kaia_platform::timezone::offset_label(zone), zone.name),
                selected: current == Some(zone.name),
                action: format!("settings:timezone:{}", zone.name),
            }),
    );
    out
}

fn library(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let mut out = vec![Entry::Heading(tr("Book folders"))];
    for folder in facts::library_folders(s, &services.paths.books_dir) {
        out.push(Entry::Row {
            primary: folder,
            secondary: None,
            value: None,
            action: None,
        });
    }
    out.push(Entry::Button {
        key: "settings-scan",
        label: tr("Scan now"),
        action: "scan".into(),
    });
    out.push(Entry::Heading(tr("Shelves")));
    out.push(Entry::Toggle {
        key: keys::LIBRARY_SHOW_FINISHED,
        label: tr("Show finished books"),
        on: keys::bool_or(s, keys::LIBRARY_SHOW_FINISHED, true),
    });
    out
}

/// The Home page: one switch per Home section. A switch that is off hides its
/// section on Home. The books stay in the library.
fn home(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let mut out = vec![Entry::Heading(tr("Sections"))];
    for (key, label) in [
        (keys::HOME_SHOW_CURRENT, tr("Continue reading")),
        (keys::HOME_SHOW_NEXT_IN_SERIES, tr("Next in series")),
        (keys::HOME_SHOW_MORE_BY_AUTHOR, tr("More by author")),
        (keys::HOME_SHOW_RECENT, tr("Recent")),
        (keys::HOME_SHOW_IN_PROGRESS, tr("Other books in progress")),
    ] {
        out.push(Entry::Toggle {
            key,
            label,
            on: keys::bool_or(s, key, true),
        });
    }
    out.push(Entry::Text(tr(
        "When every section is off, Home shows a short note and a button to the Library.",
    )));
    out
}

fn network(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let mut out = vec![
        Entry::Heading(tr("Progress sync")),
        Entry::Toggle {
            key: keys::SYNC_ENABLED,
            label: tr("Sync reading progress"),
            on: s.is_true(keys::SYNC_ENABLED),
        },
        Entry::Toggle {
            key: keys::SYNC_AUTO,
            label: tr("Automatically keep documents in sync"),
            on: s.is_true(keys::SYNC_AUTO),
        },
        Entry::Toggle {
            key: keys::SYNC_INVALID_CERTS,
            label: tr("Accept self-signed certificates"),
            on: s.is_true(keys::SYNC_INVALID_CERTS),
        },
        Entry::Heading(tr("Sync to a newer state")),
        Entry::Radio {
            key: keys::SYNC_FORWARD,
            options: labels(keys::SYNC_FORWARD),
            selected: keys::preset_index(
                options_for(keys::SYNC_FORWARD).unwrap(),
                &keys::text_or(s, keys::SYNC_FORWARD, "prompt"),
            ),
        },
        Entry::Heading(tr("Sync to an older state")),
        Entry::Radio {
            key: keys::SYNC_BACKWARD,
            options: labels(keys::SYNC_BACKWARD),
            selected: keys::preset_index(
                options_for(keys::SYNC_BACKWARD).unwrap(),
                &keys::text_or(s, keys::SYNC_BACKWARD, "never"),
            ),
        },
        Entry::Field {
            key: keys::SYNC_SERVER,
            label: tr("Server address"),
            value: keys::text(s, keys::SYNC_SERVER),
            password: false,
        },
        Entry::Field {
            key: keys::SYNC_USERNAME,
            label: tr("Username"),
            value: keys::text(s, keys::SYNC_USERNAME),
            password: false,
        },
        Entry::Field {
            key: keys::SYNC_PASSWORD,
            label: tr("Password"),
            // The password is never stored, so the field starts empty.
            value: String::new(),
            password: true,
        },
        Entry::Button {
            key: "settings-sync-test",
            label: tr("Test connection"),
            action: "settings:sync:test".into(),
        },
        Entry::Text(tr(
            "Turn on sync to send progress on close and sleep. With automatic sync on, a book also reads progress from other devices when it opens.",
        )),
    ];
    out.extend([
        Entry::Heading(tr("WebDAV notes export")),
        Entry::Toggle { key: crate::webdav::ENABLED, label: tr("Export notes to WebDAV"), on: s.is_true(crate::webdav::ENABLED) },
        Entry::Text(tr("Sends Markdown and JSON on Export and book close. Nothing is read back. HTTPS is required; HTTP is not allowed. The password is stored as plain text in a separate file that only your user account can read.")),
        Entry::Field { key: crate::webdav::SERVER, label: tr("WebDAV server URL"), value: keys::text(s, crate::webdav::SERVER), password: false },
        Entry::Field { key: crate::webdav::USERNAME, label: tr("WebDAV username"), value: keys::text(s, crate::webdav::USERNAME), password: false },
        Entry::Field { key: crate::webdav::PASSWORD, label: tr("WebDAV password"), value: String::new(), password: true },
        Entry::Field { key: crate::webdav::FOLDER, label: tr("WebDAV folder path"), value: keys::text(s, crate::webdav::FOLDER), password: false },
        Entry::Button { key: "settings-webdav-test", label: tr("Test connection"), action: "settings:webdav:test".into() },
        Entry::Button { key: "settings-webdav-export", label: tr("Export now"), action: "settings:webdav:export".into() },
    ]);
    out.splice(
        0..0,
        [
            Entry::Row {
                primary: tr("OPDS catalogs"),
                secondary: Some(tr("Browse catalogs and download books")),
                value: None,
                action: Some("settings:catalogs".into()),
            },
            Entry::Row {
                primary: tr("Calibre wireless"),
                secondary: Some(tr("Receive books from Calibre")),
                value: None,
                action: Some("settings:calibre".into()),
            },
        ],
    );
    out
}

fn statistics(services: &Services, now: i64) -> Vec<Entry> {
    let finished = facts::finished_books(&services.library);
    match facts::stats_view(&services.paths.data_dir, now) {
        None => vec![
            Entry::Text(tr(
                "No reading recorded yet. Statistics appear after the first book is read.",
            )),
            Entry::Heading(tr("Books")),
            finished_row(finished),
        ],
        Some(view) => statistics_rows(&view, finished),
    }
}

/// The count of finished books, from the library index. See `facts::finished_books`.
fn finished_row(finished: usize) -> Entry {
    Entry::Row {
        primary: tr("Books finished"),
        secondary: None,
        value: Some(finished.to_string()),
        action: None,
    }
}

fn statistics_rows(view: &StatsView, finished: usize) -> Vec<Entry> {
    let mut out = vec![
        Entry::Heading(tr("Reading time")),
        Entry::Row {
            primary: tr("Today"),
            secondary: Some(facts::page_count(view.today.pages)),
            value: Some(duration(view.today.seconds)),
            action: None,
        },
        Entry::Row {
            primary: tr("This week"),
            secondary: Some(facts::page_count(view.week.pages)),
            value: Some(duration(view.week.seconds)),
            action: None,
        },
        Entry::Row {
            primary: tr("All time"),
            secondary: Some(facts::page_count(view.all.pages)),
            value: Some(duration(view.all.seconds)),
            action: None,
        },
        Entry::Heading(tr("Last seven days")),
    ];
    let longest = view.days.iter().map(|d| d.seconds).max().unwrap_or(0).max(1);
    for day in &view.days {
        out.push(day_bar(day, longest));
    }
    out.push(Entry::Heading(tr("Books")));
    out.push(finished_row(finished));
    for book in &view.books {
        let title = if book.title.trim().is_empty() {
            tr("Untitled")
        } else {
            book.title.clone()
        };
        out.push(Entry::Row {
            primary: title,
            secondary: Some(facts::page_count(book.distinct_pages)),
            value: Some(duration(book.seconds)),
            action: None,
        });
    }
    out
}

fn day_bar(day: &DayTotal, longest: i64) -> Entry {
    Entry::Bar {
        // `MM-DD`: the year is in the title of the section, and the row stays short.
        label: day.date.get(5..).unwrap_or(&day.date).to_string(),
        detail: duration(day.seconds),
        fraction: day.seconds as f64 / longest as f64,
    }
}

fn idle_choices(s: &kaia_events::Settings, key: &'static str, default: f64) -> Entry {
    minutes_choices(s, key, default, &keys::IDLE_MINUTES, tr("Never"))
}

/// A radio of minute choices. The zero choice shows `zero`, and the others show minutes.
fn minutes_choices(
    s: &kaia_events::Settings,
    key: &'static str,
    default: f64,
    choices: &[u32],
    zero: String,
) -> Entry {
    let value = keys::number_or(s, key, default);
    Entry::Radio {
        key,
        options: choices
            .iter()
            .map(|n| {
                if *n == 0 {
                    zero.clone()
                } else {
                    tr_format(&tr("%1 min"), &[&n.to_string()])
                }
            })
            .collect(),
        selected: choices.iter().position(|n| f64::from(*n) == value).unwrap_or(0),
    }
}

/// The screen lock rows: set a PIN when none is set, or change and turn off the PIN.
/// The PIN file is `.kaia/pin`, and the text says how to remove a lost PIN.
fn screen_lock(services: &Services) -> Vec<Entry> {
    if crate::surfaces::pin::store::is_on(&services.paths.data_dir) {
        vec![
            Entry::Heading(tr("Screen lock")),
            Entry::Text(tr(
                "On. Kaia asks for the PIN when the device wakes and when Kaia starts.",
            )),
            Entry::Button {
                key: "settings-pin-change",
                label: tr("Change PIN"),
                action: "settings:pin:change".into(),
            },
            Entry::Button {
                key: "settings-pin-off",
                label: tr("Turn off"),
                action: "settings:pin:off".into(),
            },
            Entry::Text(tr(
                "To reset a forgotten PIN, connect the Kobo to a computer and delete the file .kaia/pin.",
            )),
        ]
    } else {
        vec![
            Entry::Heading(tr("Screen lock")),
            Entry::Text(tr("Off. Set a PIN to ask for it when the device wakes.")),
            Entry::Button {
                key: "settings-pin-set",
                label: tr("Set PIN"),
                action: "settings:pin:set".into(),
            },
        ]
    }
}

fn device(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let mut out = vec![
        Entry::Button {
            key: "settings-storage",
            label: tr("Storage"),
            action: "settings:storage".into(),
        },
        Entry::Heading(tr("Standby after")),
        minutes_choices(s, keys::DEVICE_STANDBY_MINUTES, 0.0, &keys::STANDBY_MINUTES, tr("Off")),
        Entry::Text(tr(
            "Standby keeps Kaia awake and idle, so any input wakes it at once. The device still sleeps after the time under Sleep after.",
        )),
        Entry::Heading(tr("Sleep after")),
        idle_choices(s, keys::DEVICE_SLEEP_MINUTES, keys::DEFAULT_SLEEP_MINUTES),
        Entry::Heading(tr("Power off after")),
        idle_choices(s, keys::DEVICE_POWER_OFF_MINUTES, 0.0),
        Entry::Toggle {
            key: keys::DEVICE_SLEEP_COVER,
            label: tr("Sleep cover"),
            on: keys::bool_or(s, keys::DEVICE_SLEEP_COVER, true),
        },
        Entry::DeviceToggle {
            key: keys::DEVICE_PAGE_BUTTON_WAKE,
            label: tr("Wake with page buttons"),
            on: services.platform.supports_page_button_wake()
                && keys::bool_or(s, keys::DEVICE_PAGE_BUTTON_WAKE, false),
            enabled: services.platform.supports_page_button_wake(),
        },
        Entry::Heading(tr("Sleep screen")),
        Entry::Radio {
            key: keys::DEVICE_SLEEP_SCREEN,
            options: labels(keys::DEVICE_SLEEP_SCREEN),
            selected: keys::preset_index(
                &keys::SLEEP_SCREENS,
                &keys::text_or(s, keys::DEVICE_SLEEP_SCREEN, "cover"),
            ),
        },
        Entry::Text(tr(
            "Book cover shows the book you are reading. Book cover with details also shows its title, author and progress, and uses the last book you opened when no book is open.",
        )),
    ];
    out.extend(screen_lock(services));
    out.extend([
        Entry::Heading(tr("Time zone")),
        Entry::Row {
            primary: keys::time_zone(s).map_or_else(|| tr("System default"), |zone| zone.name.to_string()),
            secondary: None,
            value: None,
            action: Some("settings:timezone".into()),
        },
        Entry::Heading(tr("Network time")),
        Entry::Toggle {
            key: keys::DEVICE_NETWORK_TIME,
            label: tr("Set the clock from pool.ntp.org"),
            on: keys::network_time(s),
        },
        Entry::Text(tr(
            "Needs Wi-Fi on. Syncs at most every six hours. Off by default. Kaia contacts pool.ntp.org only when this is on.",
        )),
    ]);
    out
}

fn data(services: &Services) -> Vec<Entry> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut out = vec![Entry::Heading(tr("Import from KOReader"))];
    match facts::find_koreader(&services.paths, home.as_deref()) {
        None => out.push(Entry::Text(tr(
            "No KOReader folder found. Kaia looks in the books folder and in the standard KOReader folders.",
        ))),
        Some(folder) => {
            out.extend(found_rows(&folder));
            out.push(Entry::Button {
                key: "settings-import",
                label: tr("Import from KOReader"),
                action: "settings:data:import".into(),
            });
        }
    }
    if crate::lua_import::find(&services.paths).is_some() {
        out.push(Entry::Button {
            key: "settings-import-lua",
            label: tr("Import from Kaia (Lua app)"),
            action: "settings:data:lua".into(),
        });
    }
    if facts::kobo_database(&services.paths).is_some() {
        out.push(Entry::Button {
            key: "settings-import-kobo",
            label: tr("Import from Kobo"),
            action: "settings:data:kobo".into(),
        });
    }
    out.push(Entry::Heading(tr("Notes")));
    out.push(Entry::Button {
        key: "settings-export",
        label: tr("Export notes"),
        action: "settings:data:export".into(),
    });
    out.push(Entry::Text(tr(
        "Writes every highlight, note and bookmark to the exports folder in Markdown and JSON.",
    )));
    out
}

fn found_rows(folder: &KoreaderFolder) -> Vec<Entry> {
    let yes_no = |found: bool| if found { tr("found") } else { tr("not found") };
    vec![
        Entry::Row {
            primary: tr("KOReader folder"),
            secondary: Some(folder.path.display().to_string()),
            value: None,
            action: None,
        },
        Entry::Row {
            primary: tr("Reading history"),
            secondary: Some("history.lua".into()),
            value: Some(yes_no(folder.history)),
            action: None,
        },
        Entry::Row {
            primary: tr("Collections"),
            secondary: Some("settings/collection.lua".into()),
            value: Some(yes_no(folder.collections)),
            action: None,
        },
        Entry::Row {
            primary: tr("Statistics"),
            secondary: Some("settings/statistics.sqlite3".into()),
            value: Some(yes_no(folder.statistics)),
            action: None,
        },
    ]
}

fn about() -> Vec<Entry> {
    vec![
        Entry::Row {
            primary: tr("Kaia Reader"),
            secondary: Some(tr("A reader for the Kobo Libra Colour")),
            value: Some(tr_format(&tr("version %1"), &[env!("CARGO_PKG_VERSION")])),
            action: None,
        },
        Entry::Row {
            primary: tr("License"),
            secondary: Some(tr("GNU Affero General Public License v3.0 (AGPL-3.0)")),
            value: None,
            action: None,
        },
        Entry::Row {
            primary: tr("Source"),
            secondary: Some("https://github.com/Wildeax/kaia-reader".into()),
            value: None,
            action: None,
        },
        Entry::Heading(tr("Credits")),
        Entry::Text(tr(
            "Kaia Reader is a fork of KOReader, base version v2026.07.1 (commit 9192014d8). \
             KOReader is free software under AGPL-3.0, written by the KOReader developers. \
             Kaia keeps its document engines, its Kobo support and its file formats.",
        )),
    ]
}

/// The Experimental features page. See `experiments` for the flags.
fn experimental(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let mut out = vec![
        Entry::Toggle {
            key: keys::EXPERIMENTAL_ENABLED,
            label: tr("Experimental features"),
            on: keys::bool_or(s, keys::EXPERIMENTAL_ENABLED, false),
        },
        Entry::Text(tr(
            "Experimental features can change or break. A flag works only while this switch is on.",
        )),
    ];
    out.extend(experiments::entries(experiments::FLAGS, s));
    out
}

fn advanced() -> Vec<Entry> {
    let items = [
        (
            tr("Gesture zones"),
            tr("Zones and gestures. Choose a preset in Reading."),
        ),
        (
            tr("Hardware keys"),
            tr("Which keys turn pages. Not configurable in Kaia yet."),
        ),
        (
            tr("Auto suspend"),
            tr("Suspend after no input. The timeout is in Device."),
        ),
        (tr("Backup"), tr("Copy the data folder to keep a backup.")),
        (
            tr("Date and time"),
            tr("Uses the device clock. Set it on the device."),
        ),
    ];
    let mut out = vec![
        Entry::Row {
            primary: tr("Experimental features"),
            secondary: Some(tr("Off by default. Flags for features that are not ready.")),
            value: None,
            action: Some("settings:experimental".into()),
        },
        Entry::Text(tr(
            "These KOReader options have no Kaia control yet. Each line says what the option does.",
        )),
    ];
    for (primary, secondary) in items {
        out.push(Entry::Row {
            primary,
            secondary: Some(secondary),
            value: None,
            action: None,
        });
    }
    out.push(Entry::Heading(tr("Reset")));
    out.push(Entry::Text(tr("Reset settings sets every setting back to its default. Reading data, books, fonts and dictionaries stay.")));
    out.push(Entry::Button {
        key: "settings-reset-settings",
        label: tr("Reset settings"),
        action: "settings:reset:settings".into(),
    });
    out.push(Entry::Text(tr("Erase all Kaia data deletes everything Kaia keeps in the .kaia folder. Book files, the Kobo and KOReader folders and the Lua app are not touched.")));
    out.push(Entry::Button {
        key: "settings-erase-data",
        label: tr("Erase all Kaia data"),
        action: "settings:reset:erase".into(),
    });
    out
}

/// The user font folder as the Settings text shows it: the data folder's name
/// and `fonts`, such as `.kaia/fonts`.
fn font_folder_label(data_dir: &Path) -> String {
    let name = data_dir
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    format!("{name}/fonts")
}

/// The labels of a radio group, in the order of [`options_for`].
fn labels(key: &str) -> Vec<String> {
    options_for(key)
        .map(|opts| opts.iter().map(|(_, label)| option_label(label)).collect())
        .unwrap_or_default()
}

/// The text of a radio option label in the language in use. A label with no
/// translation, such as a font name, stays as it is.
fn option_label(label: &str) -> String {
    match label {
        "Engine default" => tr("Engine default"),
        "Ask first" => tr("Ask first"),
        "Use synced position" => tr("Use synced position"),
        "Never" => tr("Never"),
        "Default" => tr("Default"),
        "Left-handed" => tr("Left-handed"),
        "Page turn only" => tr("Page turn only"),
        "Cleaner page turns" => tr("Cleaner page turns"),
        "Faster page turns" => tr("Faster page turns"),
        "Book cover" => tr("Book cover"),
        "Book cover with details" => tr("Book cover with details"),
        "Blank" => tr("Blank"),
        other => other.to_string(),
    }
}

fn extensions(services: &Services) -> Vec<Entry> {
    #[cfg(not(feature = "lua"))]
    {
        let _ = services;
        vec![
            Entry::Row {
                primary: tr("Native statistics"),
                secondary: None,
                value: None,
                action: Some("settings:statistics".into()),
            },
            Entry::Text(tr("This build has no Lua plugin support.")),
        ]
    }
    #[cfg(feature = "lua")]
    {
        let p = &services.plugins;
        let mut entries = vec![
            Entry::Row {
                primary: tr("Native statistics"),
                secondary: None,
                value: None,
                action: Some("settings:statistics".into()),
            },
            Entry::Toggle {
                key: crate::plugins::ENABLED,
                label: tr("Enable KOReader plugins"),
                on: services.settings.is_true(crate::plugins::ENABLED),
            },
            Entry::Toggle {
                key: crate::plugins::SAFE_MODE,
                label: tr("Safe mode, bundled plugins only"),
                on: p.startup_safe_mode || services.settings.is_true(crate::plugins::SAFE_MODE),
            },
        ];
        if let Some(error) = p.error() {
            entries.push(Entry::Text(tr_format(&tr("Plugin runtime failed: %1"), &[error])));
        }
        for plugin in &p.entries {
            entries.push(Entry::Row {
                primary: plugin.name.clone(),
                secondary: Some(plugin.state.clone()),
                value: None,
                action: Some(format!("plugins:detail:{}", plugin.name)),
            });
        }
        entries
    }
}

fn plugin_entries(name: &str, services: &Services) -> Vec<Entry> {
    #[cfg(not(feature = "lua"))]
    {
        let _ = (name, services);
        vec![]
    }
    #[cfg(feature = "lua")]
    {
        let p = &services.plugins;
        let Some(plugin) = p.entries.iter().find(|p| p.name == name) else {
            return vec![];
        };
        let mut entries = vec![
            Entry::PluginToggle {
                key: format!("plugins.on.{name}"),
                label: name.into(),
                on: plugin.enabled,
            },
            Entry::Text(plugin.state.clone()),
        ];
        for menu in p
            .menus
            .iter()
            .filter(|m| p.owners.get(&m.id).is_some_and(|o| o == name))
        {
            entries.push(Entry::Row {
                primary: menu.text.clone(),
                secondary: None,
                value: None,
                action: Some(format!("plugins:menu:{}", menu.id)),
            });
        }
        entries
    }
}

fn reader_menus(services: &Services) -> Vec<Entry> {
    #[cfg(not(feature = "lua"))]
    {
        let _ = services;
        vec![Entry::Text(tr("This build has no Lua plugin support."))]
    }
    #[cfg(feature = "lua")]
    {
        if !services.plugins.running() {
            return vec![Entry::Text(tr(
                "Enable KOReader plugins in Settings, Extensions.",
            ))];
        }
        services
            .plugins
            .menus
            .iter()
            .map(|m| Entry::Row {
                primary: m.text.clone(),
                secondary: None,
                value: None,
                action: Some(format!("plugins:menu:{}", m.id)),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_has_a_title() {
        for name in super::super::commands::SECTIONS {
            assert_ne!(title(name), "Settings", "{name}");
        }
    }
}
