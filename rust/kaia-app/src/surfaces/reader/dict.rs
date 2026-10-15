//! Dictionary lookup for the selected word. StarDict folders are read from
//! Kaia's `dict` folder in the data directory and from KOReader's
//! `data/dict` folder on the Kobo. Nickel dictionaries are read from `.kobo/dict`.
//!
//! The settings `dictionary.order` (names, first to last) and
//! `dictionary.disabled` (names that are off) set the order and the on or off
//! state. A book's `preferred_dictionaries` go before the settings order. A
//! preferred dictionary is used even when the settings turn it off, as KOReader does.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::services::Services;
use kaia_dict::{Library, LookupOptions};
use kaia_events::Value;
use kaia_i18n::{format as tr_format, tr};
use kaia_storage::{Store, StoreError};

/// Settings key of the dictionary order: names, first to last.
pub const ORDER_KEY: &str = "dictionary.order";
/// Settings key of the dictionaries that are off: names.
pub const DISABLED_KEY: &str = "dictionary.disabled";

/// What a lookup gives the sheet: the text, and the dictionary of the first
/// definition, which the sheet offers to use first for the book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub text: String,
    pub first: Option<String>,
}

/// Opens StarDict folders and the local Kobo dictionary folder once. Call this before [`installed`] or [`lookup`].
pub fn load(services: &mut Services) {
    if services.dictionary.is_none() {
        let mut library = Library::default();
        for dir in folders(services) {
            if let Ok(source) = Library::open_dir(dir) {
                library.append(source);
            }
        }
        library.add_kobo_dir(&services.paths.books_dir.join(".kobo/dict"));
        services.dictionary = Some(library);
    }
}

/// The folders searched for dictionaries, in order: Kaia's, then KOReader's.
fn folders(services: &Services) -> [PathBuf; 3] {
    [
        services.paths.data_dir.join("dict"),
        crate::lua_import::find(&services.paths)
            .unwrap_or_else(|| services.paths.data_dir.clone())
            .join("data/dict"),
        services.paths.books_dir.join(".adds/koreader/data/dict"),
    ]
}

/// The folder the loaded dictionaries come from, or Kaia's folder when none are
/// loaded. Call [`load`] first.
pub fn folder(services: &Services) -> PathBuf {
    let folders = folders(services);
    let first = services
        .dictionary
        .as_ref()
        .and_then(|lib| lib.dictionaries().first().map(|d| d.path().to_path_buf()));
    first
        .and_then(|path| folders.iter().find(|folder| path.starts_with(folder)).cloned())
        .unwrap_or_else(|| folders[0].clone())
}

/// The names of the installed dictionaries, in the order the settings give.
/// Call [`load`] first.
pub fn installed(services: &Services) -> Vec<String> {
    services
        .dictionary
        .as_ref()
        .map(Library::names)
        .unwrap_or_default()
}

/// The names in a settings list, in order. Other values are skipped.
fn names(services: &Services, key: &str) -> Vec<String> {
    services
        .settings
        .read(key)
        .as_table()
        .into_iter()
        .flat_map(|table| table.values())
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

/// The names of `installed` in lookup order: those in `order` first, in that
/// order, then the rest by name. A name listed twice keeps its first place, and
/// a name in `order` that is not installed is left out.
pub fn order_names(installed: &[String], order: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in order {
        if installed.contains(name) && !out.contains(name) {
            out.push(name.clone());
        }
    }
    let mut rest: Vec<String> = installed
        .iter()
        .filter(|name| !out.contains(name))
        .cloned()
        .collect();
    rest.sort();
    rest.dedup();
    out.extend(rest);
    out
}

/// The installed dictionary names in the order the settings give.
pub fn ordered(services: &Services) -> Vec<String> {
    order_names(&installed(services), &names(services, ORDER_KEY))
}

/// Saves the dictionary order in the settings. The caller flushes the settings.
pub fn save_order(services: &mut Services, order: &[String]) {
    services.settings.save(
        ORDER_KEY,
        Value::array(order.iter().map(|n| Value::from(n.as_str()))),
    );
}

/// Saves the names of the dictionaries that are off. An empty list clears the key.
pub fn save_disabled(services: &mut Services, disabled: &[String]) {
    if disabled.is_empty() {
        services.settings.del(DISABLED_KEY);
    } else {
        services.settings.save(
            DISABLED_KEY,
            Value::array(disabled.iter().map(|n| Value::from(n.as_str()))),
        );
    }
}

/// The names that are off in the settings.
pub fn disabled(services: &Services) -> Vec<String> {
    names(services, DISABLED_KEY)
}

/// What a row of the Dictionary settings does to its dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// Turns the dictionary on or off.
    Toggle,
    /// Moves it one place up the lookup order.
    Up,
    /// Moves it one place down the lookup order.
    Down,
}

/// Applies `change` to the dictionary at `index` in lookup order, then saves the
/// order and the off list. The caller flushes the settings. Returns false when
/// nothing changes. That happens when no dictionary has `index`, or when the
/// dictionary is already first (for up) or last (for down).
pub fn apply(services: &mut Services, index: usize, change: Change) -> bool {
    load(services);
    let mut order = ordered(services);
    if index >= order.len() {
        return false;
    }
    match change {
        Change::Toggle => {
            let name = order[index].clone();
            let mut off = disabled(services);
            if off.contains(&name) {
                off.retain(|n| n != &name);
            } else {
                off.push(name);
            }
            save_disabled(services, &off);
        }
        Change::Up if index > 0 => order.swap(index, index - 1),
        Change::Down if index + 1 < order.len() => order.swap(index, index + 1),
        _ => return false,
    }
    save_order(services, &order);
    true
}

/// The lookup options for one word: the book's preferred dictionaries, then the
/// settings order. A preferred dictionary is kept even when the settings turn it off.
fn options(services: &Services, preferred: &[String]) -> LookupOptions {
    let mut order: Vec<String> = Vec::new();
    for name in preferred.iter().cloned().chain(names(services, ORDER_KEY)) {
        if !order.contains(&name) {
            order.push(name);
        }
    }
    let mut disabled: HashSet<String> = names(services, DISABLED_KEY).into_iter().collect();
    for name in preferred {
        disabled.remove(name);
    }
    LookupOptions {
        dictionaries: (!order.is_empty()).then_some(order),
        disabled,
        ..LookupOptions::default()
    }
}

/// Looks `word` up in the installed dictionaries, with the book's `preferred`
/// dictionaries first.
pub fn lookup(word: &str, services: &mut Services, preferred: &[String]) -> Answer {
    load(services);
    let options = options(services, preferred);
    let Some(library) = services.dictionary.as_ref() else {
        return Answer {
            text: install_hint(&services.paths.data_dir),
            first: None,
        };
    };
    if library.names().is_empty() {
        return Answer {
            text: if library
                .errors()
                .iter()
                .any(|(_, e)| e.to_string().to_lowercase().contains("encrypt"))
            {
                tr("This Kobo dictionary is encrypted. Open Dictionaries in Settings for free dictionary downloads.")
            } else {
                tr("No dictionary installed. Open Dictionaries in Settings for free dictionary downloads.")
            },
            first: None,
        };
    }
    let entries = library.lookup_with(word, &options);
    let first = entries.first().map(|entry| entry.dict_name.clone());
    let text = if entries.is_empty() {
        tr_format(&tr("No entry for \"%1\" in the installed dictionaries."), &[word])
    } else {
        entries
            .iter()
            .map(|entry| {
                format!(
                    "{} ({})\n{}",
                    entry.headword,
                    entry.dict_name,
                    entry.definition.display_text()
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };
    Answer { text, first }
}

/// Opens Kaia's word list, in the `kaia` folder of the data directory.
pub fn word_store(services: &Services) -> Result<Store, StoreError> {
    Store::open(&services.paths.data_dir.join("kaia"))
}

/// The sheet text when no dictionary is installed: what to copy and where.
pub fn install_hint(data_dir: &Path) -> String {
    let folder = data_dir.join("dict").display().to_string();
    tr_format(
        &tr("No dictionary installed. Copy a StarDict dictionary (.ifo, .idx and .dict files) into %1."),
        &[folder.as_str()],
    )
}

/// Keep the vocabulary sentence separate from the plugin's surrounding words.
pub(super) struct LookupContext {
    pub sentence: Option<String>,
    #[cfg(feature = "lua")]
    pub surrounding: Option<(String, String)>,
}

/// Snapshot the lookup context before the native selection clears.
#[cfg(feature = "lua")]
pub(super) fn plugin_context(word: &str, context: Option<&LookupContext>) -> Value {
    let Some((before, after)) = context.and_then(|c| c.surrounding.as_ref()) else {
        return Value::Nil;
    };
    Value::Table(
        [
            ("before".into(), Value::from(before.clone())),
            ("after".into(), Value::from(after.clone())),
            (
                "selected_text".into(),
                Value::Table([("text".into(), Value::from(word))].into()),
            ),
        ]
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names_of(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_order_list_comes_first_and_the_rest_follow_by_name() {
        let installed = names_of(&["Zeta", "Alpha", "Mid"]);
        let order = names_of(&["Mid", "Zeta"]);
        assert_eq!(
            order_names(&installed, &order),
            names_of(&["Mid", "Zeta", "Alpha"])
        );
    }

    #[test]
    fn names_that_are_not_installed_are_left_out_and_repeats_keep_the_first_place() {
        let installed = names_of(&["A", "B"]);
        let order = names_of(&["Gone", "B", "A", "B"]);
        assert_eq!(order_names(&installed, &order), names_of(&["B", "A"]));
        assert_eq!(order_names(&installed, &[]), names_of(&["A", "B"]));
    }
}
