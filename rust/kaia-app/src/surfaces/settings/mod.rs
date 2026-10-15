//! Settings: a root with a search field and the sections, and one surface per
//! section (see [`section`]). The root is [`SettingsSurface`], a unit struct so
//! the route table can build it with no state. A search replaces the root with
//! a [`SettingsRoot`] that carries the query.
//!
//! Module map: `keys` (the settings keys), `commands` (action ids), `facts`
//! (statistics, the KOReader folder, the library folders), `sections` (the rows
//! of each section), `entries` (the row widgets), `section` (the section
//! surface), `quick` (the quick settings sheet), `sleep` (the sleep screen)
//! and `experiments` (the experimental feature flags).

pub mod commands;
pub mod entries;
pub mod experiments;
pub mod facts;
pub mod keys;
pub mod quick;
pub mod reset;
pub mod section;
pub mod sections;
pub mod sleep;
pub mod storage;
pub mod widgets;

use kaia_events::Value;
use kaia_gfx::Size;
use kaia_i18n::{format as tr_format, tr};
use kaia_ui::components::{Column, CrossAlign, Expanded};
use kaia_ui::composite::{ScrollList, SettingsDialog, SettingsIndexRow};
use kaia_ui::controls::{OnScreenKeyboard, TextField, ENGLISH};
use kaia_ui::{Theme, WidgetPod, Window};

use crate::services::Services;
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{body, root_window as window};
use crate::surfaces::Route;
use commands::{Command, INDEX_SECTIONS};
use section::SettingsSectionSurface;

/// The Settings root. See the module docs.
pub struct SettingsSurface;

impl Surface for SettingsSurface {
    fn route(&self) -> Route {
        Route::Settings
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        SettingsRoot::default().build(env)
    }

    fn on_command(&mut self, cmd: &str, args: &[Value], _env: &mut SurfaceEnv<'_>) -> Outcome {
        handle(cmd, args)
    }
}

/// The Settings root with a search query. `typing` shows the text field and
/// the keyboard. Without it the root shows a search button and the sections.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsRoot {
    pub query: String,
    pub typing: bool,
}

impl Surface for SettingsRoot {
    fn route(&self) -> Route {
        Route::Settings
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(root_window(env.services, env.screen, self))
    }

    fn on_command(&mut self, cmd: &str, args: &[Value], _env: &mut SurfaceEnv<'_>) -> Outcome {
        handle(cmd, args)
    }
}

fn handle(cmd: &str, args: &[Value]) -> Outcome {
    match commands::parse(cmd) {
        Some(Command::Section(name)) => {
            Outcome::Navigate(Nav::Push(Box::new(SettingsSectionSurface::new(name))))
        }
        Some(Command::SearchOpen) => {
            let query = current_query(args);
            Outcome::Navigate(Nav::Replace(Box::new(SettingsRoot { query, typing: true })))
        }
        Some(Command::SearchClose) => Outcome::Navigate(Nav::Replace(Box::new(SettingsRoot {
            query: current_query(args),
            typing: false,
        }))),
        Some(Command::Search) => Outcome::Navigate(Nav::Replace(Box::new(SettingsRoot {
            query: current_query(args),
            typing: false,
        }))),
        _ => Outcome::NotMine,
    }
}

/// The text a field event carries, trimmed. Empty when the event has no text.
fn current_query(args: &[Value]) -> String {
    args.first()
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// Writes `settings.json`. A failure is kept as a notice; the in-memory settings stay.
pub fn save(env: &mut SurfaceEnv<'_>) {
    let path = env.services.settings_path();
    if let Err(e) = env.services.settings.flush(&path) {
        env.notice(tr_format(&tr("cannot save settings: %1"), &[&e.to_string()]));
    }
}

/// The searchable items: the section and the English item title. Search matches the
/// title the user sees, see [`item_title`].
pub const ITEMS: &[(&str, &str)] = &[
    ("home", "Continue reading"),
    ("home", "Next in series"),
    ("home", "More by author"),
    ("home", "Recent"),
    ("home", "Other books in progress"),
    ("fonts", "Font size"),
    ("fonts", "Font family"),
    ("reading", "Tap zones"),
    ("reading", "Full refresh every"),
    ("language-dictionaries", "Dictionary folder"),
    ("display", "Frontlight"),
    ("display", "Warmth"),
    ("display", "Night mode"),
    ("display", "Color rendering"),
    ("display", "Large print"),
    ("display", "Screen rotation"),
    ("language-dictionaries", "Language"),
    ("library", "Book folders"),
    ("library", "Scan now"),
    ("library", "Show finished books"),
    ("network", "Server address"),
    ("network", "Username"),
    ("network", "Password"),
    ("network", "Test connection"),
    ("network", "OPDS catalogs"),
    ("network", "Calibre wireless"),
    ("statistics", "Today"),
    ("statistics", "This week"),
    ("statistics", "Books"),
    ("device-info", "Storage"),
    ("device", "Sleep after"),
    ("device", "Sleep screen"),
    ("device", "Screen lock"),
    ("extensions", "KOReader plugins"),
    ("extensions", "Safe mode"),
    ("data", "Import from KOReader"),
    ("data", "Export notes"),
    ("about", "License"),
    ("about", "Source"),
    ("advanced", "Gesture zones"),
    ("advanced", "Hardware keys"),
    ("advanced", "Auto suspend"),
    ("advanced", "Backup and safe mode"),
    ("datetime", "Date and time"),
    ("advanced", "Experimental features"),
    ("advanced", "Reset settings"),
    ("advanced", "Erase all Kaia data"),
];

/// The items whose title, in the language in use, contains `query` (case-insensitive),
/// in catalogue order.
pub fn search(query: &str) -> Vec<(&'static str, &'static str)> {
    let q = query.to_lowercase();
    ITEMS
        .iter()
        .copied()
        .filter(|(_, item)| item_title(item).to_lowercase().contains(&q))
        .collect()
}

/// The title of a search item in the language in use. Each English title in
/// [`ITEMS`] has an arm here. A title without an arm stays English.
fn item_title(english: &str) -> String {
    match english {
        "Continue reading" => tr("Continue reading"),
        "Next in series" => tr("Next in series"),
        "More by author" => tr("More by author"),
        "Recent" => tr("Recent"),
        "Other books in progress" => tr("Other books in progress"),
        "Font size" => tr("Font size"),
        "Font family" => tr("Font family"),
        "Tap zones" => tr("Tap zones"),
        "Full refresh every" => tr("Full refresh every"),
        "Dictionary folder" => tr("Dictionary folder"),
        "Frontlight" => tr("Frontlight"),
        "Warmth" => tr("Warmth"),
        "Night mode" => tr("Night mode"),
        "Color rendering" => tr("Color rendering"),
        "Large print" => tr("Large print"),
        "Screen rotation" => tr("Screen rotation"),
        "Language" => tr("Language"),
        "Book folders" => tr("Book folders"),
        "Scan now" => tr("Scan now"),
        "Show finished books" => tr("Show finished books"),
        "Server address" => tr("Server address"),
        "Username" => tr("Username"),
        "Password" => tr("Password"),
        "Test connection" => tr("Test connection"),
        "OPDS catalogs" => tr("OPDS catalogs"),
        "Calibre wireless" => tr("Calibre wireless"),
        "Today" => tr("Today"),
        "This week" => tr("This week"),
        "Books" => tr("Books"),
        "Storage" => tr("Storage"),
        "Sleep after" => tr("Sleep after"),
        "Sleep screen" => tr("Sleep screen"),
        "Screen lock" => tr("Screen lock"),
        "KOReader plugins" => tr("KOReader plugins"),
        "Safe mode" => tr("Safe mode"),
        "Import from KOReader" => tr("Import from KOReader"),
        "Export notes" => tr("Export notes"),
        "License" => tr("License"),
        "Source" => tr("Source"),
        "Gesture zones" => tr("Gesture zones"),
        "Hardware keys" => tr("Hardware keys"),
        "Auto suspend" => tr("Auto suspend"),
        "Backup and safe mode" => tr("Backup and safe mode"),
        "Date and time" => tr("Date and time"),
        "Experimental features" => tr("Experimental features"),
        "Reset settings" => tr("Reset settings"),
        "Erase all Kaia data" => tr("Erase all Kaia data"),
        other => other.to_string(),
    }
}

/// One line of the root list.
#[derive(Clone)]
enum RootItem {
    Section(&'static str),
    Hit {
        section: &'static str,
        title: &'static str,
        index: usize,
    },
}

fn root_window(services: &Services, screen: Size, root: &SettingsRoot) -> Window {
    let mut children = Vec::new();
    if root.typing {
        children.push(
            WidgetPod::new(
                TextField::new()
                    .with_text(root.query.clone())
                    .placeholder(tr("Search settings"))
                    .on_submit("settings:search"),
            )
            .focusable(true)
            .with_key("settings-search-field"),
        );
    }
    if root.query.is_empty() {
        let items: Vec<RootItem> = INDEX_SECTIONS
            .iter()
            .map(|name| RootItem::Section(name))
            .collect();
        children.push(list(items));
    } else {
        let hits = search(&root.query);
        if hits.is_empty() {
            children.push(body(
                services,
                tr_format(&tr("No setting matches \u{201c}%1\u{201d}."), &[&root.query]),
            ));
        } else {
            let items: Vec<RootItem> = hits
                .into_iter()
                .enumerate()
                .map(|(index, (section, title))| RootItem::Hit {
                    section,
                    title,
                    index,
                })
                .collect();
            children.push(list(items));
        }
    }
    if root.typing {
        children.push(WidgetPod::new(
            OnScreenKeyboard::new(&ENGLISH).on_hide("settings:search:close"),
        ));
    }
    let content = WidgetPod::new(Column::new(children).cross_align(CrossAlign::Stretch));
    let dialog = SettingsDialog::new(tr("Settings"), tr("Settings"), content, "settings:close")
        .search("settings:search:open");
    window(services, screen, dialog)
}

fn list(items: Vec<RootItem>) -> WidgetPod {
    WidgetPod::new(Expanded::new(
        WidgetPod::new(ScrollList::new(items, build_item)).with_key("settings-index"),
        1,
    ))
}

fn build_item(item: &RootItem, _theme: &Theme) -> WidgetPod {
    match item {
        RootItem::Section(name) => WidgetPod::new(
            SettingsIndexRow::new(sections::title(name), format!("settings:{name}"))
                .first(*name == INDEX_SECTIONS[0]),
        )
        .with_key(format!("settings-{name}"))
        .focusable(true),
        RootItem::Hit {
            section,
            title,
            index,
        } => WidgetPod::new(
            SettingsIndexRow::new(
                tr_format(&tr("%1: %2"), &[&sections::title(section), &item_title(title)]),
                format!("settings:{section}"),
            )
            .first(*index == 0),
        )
        .with_key(format!("settings-hit-{index}"))
        .focusable(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_titles_in_any_section() {
        let hits = search("refresh");
        assert_eq!(hits, vec![("reading", "Full refresh every")]);
        assert!(search("zzz").is_empty());
        assert!(search("OPDS").iter().any(|(s, _)| *s == "network"));
    }

    #[test]
    fn every_search_item_names_a_known_section() {
        for (section, _) in ITEMS {
            assert!(commands::SECTIONS.contains(section), "{section}");
        }
    }
}

mod kobo;

mod lua_import;
