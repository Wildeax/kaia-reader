//! The contents of each settings section. [`entries`] reads the settings, the
//! platform and the device folders and returns the rows of one section.

use std::path::PathBuf;

use kaia_platform::platform::Kind;
use kaia_stats::DayTotal;

use super::commands::{options_for, Light};
use super::entries::Entry;
use super::facts::{self, duration, KoreaderFolder, StatsView};
use super::keys;
use crate::services::Services;

/// The title of a section.
pub fn title(name: &str) -> &'static str {
    match name {
        "reading" => "Reading",
        "display" => "Display and light",
        "library" => "Library",
        "network" => "Network and sync",
        "statistics" => "Statistics",
        "device" => "Device",
        "extensions" => "Extensions",
        "plugin-menus" => "Plugins",
        "data" => "Data",
        "about" => "About",
        "advanced" => "Advanced",
        _ if name.starts_with("plugin:") => "Plugin",
        _ => "Settings",
    }
}

/// Whether the device is a Kobo, where the frontlight and warmth work.
pub fn has_frontlight(services: &Services) -> bool {
    matches!(services.platform.kind, Kind::Kobo(_))
}

/// The rows of section `name` at Unix time `now`.
pub fn entries(name: &str, services: &Services, now: i64) -> Vec<Entry> {
    match name {
        "reading" => reading(services),
        "display" => display(services),
        "library" => library(services),
        "network" => network(services),
        "statistics" => statistics(services, now),
        "device" => device(services),
        "extensions" => extensions(services),
        "plugin-menus" => reader_menus(services),
        "data" => data(services),
        "about" => about(),
        "advanced" => advanced(),
        _ if name.starts_with("plugin:") => plugin_entries(&name[7..], services),
        _ => Vec::new(),
    }
}

fn reading(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let family = keys::text_or(s, keys::READING_FONT_FAMILY, "");
    let family_index = options_for(keys::READING_FONT_FAMILY)
        .and_then(|opts| opts.iter().position(|(v, _)| *v == family))
        .unwrap_or(0);
    let zones = keys::text_or(s, keys::READING_TAP_ZONES, "default");
    let dictionaries = services.paths.data_dir.join("dict");
    let found = facts::dictionary_count(&services.paths.data_dir);
    vec![
        Entry::Heading("Text".into()),
        Entry::Radio {
            key: keys::READING_FONT_FAMILY,
            options: labels(keys::READING_FONT_FAMILY),
            selected: family_index,
        },
        Entry::Stepper {
            key: keys::READING_FONT_SIZE,
            label: format!(
                "Font size, {} px",
                crate::surfaces::reader::koreader_px(
                    kaia_gfx::Size::new(services.theme.scale.width, services.theme.scale.height),
                    keys::number_or(s, keys::READING_FONT_SIZE, keys::DEFAULT_FONT_SIZE)
                )
            ),
            value: keys::number_or(s, keys::READING_FONT_SIZE, keys::DEFAULT_FONT_SIZE),
            min: 12.0,
            max: 40.0,
            step: 1.0,
            unit: " KOReader units",
        },
        Entry::Heading("Page turns".into()),
        Entry::Radio {
            key: keys::READING_TAP_ZONES,
            options: labels(keys::READING_TAP_ZONES),
            selected: keys::preset_index(&keys::TAP_ZONE_PRESETS, &zones),
        },
        Entry::Stepper {
            key: keys::READING_FULL_REFRESH_PAGES,
            label: "Full refresh every".into(),
            value: keys::number_or(
                s,
                keys::READING_FULL_REFRESH_PAGES,
                keys::DEFAULT_FULL_REFRESH_PAGES,
            ),
            min: 1.0,
            max: 50.0,
            step: 1.0,
            unit: " pages",
        },
        Entry::Heading("Dictionary".into()),
        Entry::Row {
            primary: "Dictionary folder".into(),
            secondary: Some(dictionaries.display().to_string()),
            value: Some(format!("{found} found")),
            action: None,
        },
    ]
}

fn display(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let mut out = vec![Entry::Heading("Light".into())];
    if has_frontlight(services) {
        out.push(Entry::Slider {
            which: Light::Frontlight,
            label: "Frontlight".into(),
            value: f64::from(services.platform.frontlight.intensity()),
            enabled: true,
        });
        out.push(Entry::Slider {
            which: Light::Warmth,
            label: "Warmth".into(),
            value: f64::from(services.platform.frontlight.warmth()),
            enabled: true,
        });
    } else {
        out.push(Entry::Text(
            "The frontlight and warmth work on the Kobo only. This is a desktop build.".into(),
        ));
    }
    out.push(Entry::Heading("Screen".into()));
    out.push(Entry::Toggle {
        key: keys::DISPLAY_NIGHT_MODE,
        label: "Night mode".into(),
        on: keys::bool_or(s, keys::DISPLAY_NIGHT_MODE, false),
    });
    out.push(Entry::Toggle {
        key: keys::DISPLAY_COLOR_RENDERING,
        label: "Color rendering".into(),
        on: keys::bool_or(s, keys::DISPLAY_COLOR_RENDERING, true),
    });
    out.push(Entry::Text(
        "Color rendering applies on the color panel only. Grayscale panels ignore it.".into(),
    ));
    out.push(Entry::Text(
        "Automatic rotation is not available yet. The screen keeps its current orientation.".into(),
    ));
    out
}

fn library(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let mut out = vec![Entry::Heading("Book folders".into())];
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
        label: "Scan now".into(),
        action: "scan".into(),
    });
    out.push(Entry::Heading("Shelves".into()));
    out.push(Entry::Toggle {
        key: keys::LIBRARY_SHOW_FINISHED,
        label: "Show finished books".into(),
        on: keys::bool_or(s, keys::LIBRARY_SHOW_FINISHED, true),
    });
    out
}

fn network(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    let mut out = vec![
        Entry::Heading("Progress sync".into()),
        Entry::Toggle {
            key: keys::SYNC_ENABLED,
            label: "Sync reading progress".into(),
            on: s.is_true(keys::SYNC_ENABLED),
        },
        Entry::Toggle {
            key: keys::SYNC_INVALID_CERTS,
            label: "Accept self-signed certificates".into(),
            on: s.is_true(keys::SYNC_INVALID_CERTS),
        },
        Entry::Heading("Sync to a newer state".into()),
        Entry::Radio {
            key: keys::SYNC_FORWARD,
            options: labels(keys::SYNC_FORWARD),
            selected: keys::preset_index(
                options_for(keys::SYNC_FORWARD).unwrap(),
                &keys::text_or(s, keys::SYNC_FORWARD, "prompt"),
            ),
        },
        Entry::Heading("Sync to an older state".into()),
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
            label: "Server address".into(),
            value: keys::text(s, keys::SYNC_SERVER),
            password: false,
        },
        Entry::Field {
            key: keys::SYNC_USERNAME,
            label: "Username".into(),
            value: keys::text(s, keys::SYNC_USERNAME),
            password: false,
        },
        Entry::Field {
            key: keys::SYNC_PASSWORD,
            label: "Password".into(),
            value: keys::text(s, keys::SYNC_PASSWORD),
            password: true,
        },
        Entry::Button {
            key: "settings-sync-test",
            label: "Test connection".into(),
            action: "settings:sync:test".into(),
        },
        Entry::Text(
            "Turn on sync to send progress on close and sleep, and read progress from other devices.".into(),
        ),
        Entry::Heading("OPDS catalogs".into()),
    ];
    for catalog in kaia_opds::default_catalogs() {
        out.push(Entry::Row {
            primary: catalog.title,
            secondary: Some(catalog.url),
            value: None,
            action: None,
        });
    }
    for (title, url) in facts::user_catalogs(s) {
        out.push(Entry::Row {
            primary: title,
            secondary: Some(url),
            value: Some("yours".into()),
            action: None,
        });
    }
    out
}

fn statistics(services: &Services, now: i64) -> Vec<Entry> {
    match facts::stats_view(&services.paths.data_dir, now) {
        None => vec![Entry::Text(
            "No reading recorded yet. Statistics appear after the first book is read.".into(),
        )],
        Some(view) => statistics_rows(&view),
    }
}

fn statistics_rows(view: &StatsView) -> Vec<Entry> {
    let mut out = vec![
        Entry::Heading("Reading time".into()),
        Entry::Row {
            primary: "Today".into(),
            secondary: Some(format!("{} pages", view.today.pages)),
            value: Some(duration(view.today.seconds)),
            action: None,
        },
        Entry::Row {
            primary: "This week".into(),
            secondary: Some(format!("{} pages", view.week.pages)),
            value: Some(duration(view.week.seconds)),
            action: None,
        },
        Entry::Heading("Last seven days".into()),
    ];
    let longest = view.days.iter().map(|d| d.seconds).max().unwrap_or(0).max(1);
    for day in &view.days {
        out.push(day_bar(day, longest));
    }
    out.push(Entry::Heading("Books".into()));
    for book in &view.books {
        let title = if book.title.trim().is_empty() {
            "Untitled".to_string()
        } else {
            book.title.clone()
        };
        out.push(Entry::Row {
            primary: title,
            secondary: Some(format!("{} pages", book.distinct_pages)),
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

fn device(services: &Services) -> Vec<Entry> {
    let s = &services.settings;
    vec![
        Entry::Heading("Sleep".into()),
        Entry::Stepper {
            key: keys::DEVICE_SLEEP_MINUTES,
            label: "Sleep after".into(),
            value: keys::number_or(s, keys::DEVICE_SLEEP_MINUTES, keys::DEFAULT_SLEEP_MINUTES),
            min: 1.0,
            max: 120.0,
            step: 1.0,
            unit: " min",
        },
        Entry::Heading("Sleep screen".into()),
        Entry::Radio {
            key: keys::DEVICE_SLEEP_SCREEN,
            options: labels(keys::DEVICE_SLEEP_SCREEN),
            selected: keys::preset_index(
                &keys::SLEEP_SCREENS,
                &keys::text_or(s, keys::DEVICE_SLEEP_SCREEN, "cover"),
            ),
        },
        Entry::Text("The cover of the book you are reading, or a blank screen.".into()),
    ]
}

fn data(services: &Services) -> Vec<Entry> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut out = vec![Entry::Heading("Import from KOReader".into())];
    match facts::find_koreader(&services.paths, home.as_deref()) {
        None => out.push(Entry::Text(
            "No KOReader folder found. Kaia looks in the books folder and in the standard KOReader folders."
                .into(),
        )),
        Some(folder) => {
            out.extend(found_rows(&folder));
            out.push(Entry::Button {
                key: "settings-import",
                label: "Import from KOReader".into(),
                action: "settings:data:import".into(),
            });
        }
    }
    out.push(Entry::Heading("Notes".into()));
    out.push(Entry::Button {
        key: "settings-export",
        label: "Export notes".into(),
        action: "settings:data:export".into(),
    });
    out.push(Entry::Text(
        "Writes every highlight, note and bookmark to the exports folder in Markdown and JSON.".into(),
    ));
    out
}

fn found_rows(folder: &KoreaderFolder) -> Vec<Entry> {
    let yes_no = |found: bool| if found { "found" } else { "not found" };
    vec![
        Entry::Row {
            primary: "KOReader folder".into(),
            secondary: Some(folder.path.display().to_string()),
            value: None,
            action: None,
        },
        Entry::Row {
            primary: "Reading history".into(),
            secondary: Some("history.lua".into()),
            value: Some(yes_no(folder.history).into()),
            action: None,
        },
        Entry::Row {
            primary: "Collections".into(),
            secondary: Some("collection.lua".into()),
            value: Some(yes_no(folder.collections).into()),
            action: None,
        },
        Entry::Row {
            primary: "Statistics".into(),
            secondary: Some("settings/statistics.sqlite3".into()),
            value: Some(yes_no(folder.statistics).into()),
            action: None,
        },
    ]
}

fn about() -> Vec<Entry> {
    vec![
        Entry::Row {
            primary: "Kaia Reader".into(),
            secondary: Some("A reader for the Kobo Libra Colour".into()),
            value: Some(format!("version {}", env!("CARGO_PKG_VERSION"))),
            action: None,
        },
        Entry::Row {
            primary: "License".into(),
            secondary: Some("GNU Affero General Public License v3.0 (AGPL-3.0)".into()),
            value: None,
            action: None,
        },
        Entry::Row {
            primary: "Source".into(),
            secondary: Some("https://github.com/Wildeax/kaia-reader".into()),
            value: None,
            action: None,
        },
        Entry::Heading("Credits".into()),
        Entry::Text(
            "Kaia Reader is a fork of KOReader, base version v2026.07.1 (commit 9192014d8). \
             KOReader is free software under AGPL-3.0, written by the KOReader developers. \
             Kaia keeps its document engines, its Kobo support and its file formats."
                .into(),
        ),
    ]
}

fn advanced() -> Vec<Entry> {
    let items = [
        (
            "Gesture zones",
            "Custom zone layouts and gesture bindings. Choose a preset in Reading.",
        ),
        (
            "Hardware keys",
            "Which keys turn pages. Not configurable in Kaia yet.",
        ),
        (
            "Auto suspend",
            "Suspend after a set time with no input. The sleep timeout is in Device.",
        ),
        (
            "Calibre wireless",
            "Calibre over the network. Not built in Kaia yet.",
        ),
        ("Backup", "Copy the data folder to keep a backup."),
        (
            "Date and time",
            "Kaia uses the clock of the device. Set it in the device settings.",
        ),
    ];
    let mut out = vec![Entry::Text(
        "These KOReader options have no Kaia control yet. Each line says what the option does.".into(),
    )];
    for (primary, secondary) in items {
        out.push(Entry::Row {
            primary: primary.into(),
            secondary: Some(secondary.into()),
            value: None,
            action: None,
        });
    }
    out
}

/// The labels of a radio group, in the order of [`options_for`].
fn labels(key: &str) -> Vec<String> {
    options_for(key)
        .map(|opts| opts.iter().map(|(_, label)| (*label).to_string()).collect())
        .unwrap_or_default()
}

fn extensions(services: &Services) -> Vec<Entry> {
    #[cfg(not(feature = "lua"))]
    {
        let _ = services;
        vec![
            Entry::Row {
                primary: "Native statistics".into(),
                secondary: None,
                value: None,
                action: Some("settings:statistics".into()),
            },
            Entry::Text("This build has no Lua plugin support.".into()),
        ]
    }
    #[cfg(feature = "lua")]
    {
        let p = &services.plugins;
        let mut entries = vec![
            Entry::Row {
                primary: "Native statistics".into(),
                secondary: None,
                value: None,
                action: Some("settings:statistics".into()),
            },
            Entry::Toggle {
                key: crate::plugins::ENABLED,
                label: "Enable KOReader plugins".into(),
                on: services.settings.is_true(crate::plugins::ENABLED),
            },
            Entry::Toggle {
                key: crate::plugins::SAFE_MODE,
                label: "Safe mode, bundled plugins only".into(),
                on: p.startup_safe_mode || services.settings.is_true(crate::plugins::SAFE_MODE),
            },
        ];
        if let Some(error) = p.error() {
            entries.push(Entry::Text(format!("Plugin runtime failed: {error}")));
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
        vec![Entry::Text("This build has no Lua plugin support.".into())]
    }
    #[cfg(feature = "lua")]
    {
        if !services.plugins.running() {
            return vec![Entry::Text(
                "Enable KOReader plugins in Settings, Extensions.".into(),
            )];
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
