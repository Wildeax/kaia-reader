//! Settings: a root with a search field and the sections, and one surface per
//! section (see [`section`]). The root is [`SettingsSurface`], a unit struct so
//! the route table can build it with no state. A search replaces the root with
//! a [`SettingsRoot`] that carries the query.
//!
//! Module map: `keys` (the settings keys), `commands` (action ids), `facts`
//! (statistics, the KOReader folder, the library folders), `sections` (the rows
//! of each section), `entries` (the row widgets), `section` (the section
//! surface), `quick` (the quick settings sheet) and `sleep` (the sleep screen).

pub mod commands;
pub mod entries;
pub mod facts;
pub mod keys;
pub mod quick;
pub mod section;
pub mod sections;
pub mod sleep;

use kaia_events::Value;
use kaia_gfx::Size;
use kaia_ui::components::Expanded;
use kaia_ui::composite::{ListRow, PagedList};
use kaia_ui::controls::{ButtonStyle, OnScreenKeyboard, TextField, ENGLISH};
use kaia_ui::{Theme, WidgetPod, Window};

use crate::services::Services;
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{body, button, page, root_window as window, title};
use crate::surfaces::Route;
use commands::{Command, SECTIONS};
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
        env.notice(format!("cannot save settings: {e}"));
    }
}

/// The searchable items: the section and the item title. Search matches the title.
pub const ITEMS: &[(&str, &str)] = &[
    ("reading", "Font size"),
    ("reading", "Font family"),
    ("reading", "Tap zones"),
    ("reading", "Full refresh every"),
    ("reading", "Dictionary folder"),
    ("display", "Frontlight"),
    ("display", "Warmth"),
    ("display", "Night mode"),
    ("display", "Color rendering"),
    ("display", "Screen rotation"),
    ("library", "Book folders"),
    ("library", "Scan now"),
    ("library", "Show finished books"),
    ("network", "Server address"),
    ("network", "Username"),
    ("network", "Password"),
    ("network", "Test connection"),
    ("network", "OPDS catalogs"),
    ("statistics", "Today"),
    ("statistics", "This week"),
    ("statistics", "Books"),
    ("device", "Sleep after"),
    ("device", "Sleep screen"),
    ("extensions", "KOReader plugins"),
    ("extensions", "Safe mode"),
    ("data", "Import from KOReader"),
    ("data", "Export notes"),
    ("about", "License"),
    ("about", "Source"),
    ("advanced", "Gesture zones"),
    ("advanced", "Hardware keys"),
    ("advanced", "Auto suspend"),
    ("advanced", "Calibre wireless"),
    ("advanced", "Backup and safe mode"),
    ("advanced", "Date and time"),
];

/// The items whose title contains `query` (case-insensitive), in catalogue order.
pub fn search(query: &str) -> Vec<(&'static str, &'static str)> {
    let q = query.to_lowercase();
    ITEMS
        .iter()
        .copied()
        .filter(|(_, item)| item.to_lowercase().contains(&q))
        .collect()
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
    let mut children = vec![title(services, "Settings")];
    if root.typing {
        children.push(
            WidgetPod::new(
                TextField::new()
                    .with_text(root.query.clone())
                    .placeholder("Search settings")
                    .on_submit("settings:search"),
            )
            .focusable(true)
            .with_key("settings-search-field"),
        );
    } else {
        let label = if root.query.is_empty() {
            "Search settings".to_string()
        } else {
            format!("Search: {}", root.query)
        };
        children.push(button(
            ButtonStyle::Secondary,
            &label,
            "settings:search:open",
            "settings-search",
        ));
    }
    if root.query.is_empty() {
        let items: Vec<RootItem> = SECTIONS.iter().map(|name| RootItem::Section(name)).collect();
        children.push(list(items));
    } else {
        let hits = search(&root.query);
        if hits.is_empty() {
            children.push(body(
                services,
                format!("No setting matches \u{201c}{}\u{201d}.", root.query),
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
    children.push(
        WidgetPod::new(kaia_ui::composite::NavBar::new(
            kaia_ui::composite::NavItem::Settings,
        ))
        .with_key("nav-bar"),
    );
    window(services, screen, page(services, children))
}

fn list(items: Vec<RootItem>) -> WidgetPod {
    WidgetPod::new(Expanded::new(
        WidgetPod::new(PagedList::new(items, build_item)),
        1,
    ))
}

fn build_item(item: &RootItem, _theme: &Theme) -> WidgetPod {
    match item {
        RootItem::Section(name) => WidgetPod::new(
            ListRow::new(sections::title(name))
                .chevron()
                .on_action(format!("settings:{name}")),
        )
        .with_key(format!("settings-{name}")),
        RootItem::Hit {
            section,
            title,
            index,
        } => WidgetPod::new(
            ListRow::new(*title)
                .secondary(sections::title(section))
                .chevron()
                .on_action(format!("settings:{section}")),
        )
        .with_key(format!("settings-hit-{index}")),
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
            assert!(SECTIONS.contains(section), "{section}");
        }
    }
}
