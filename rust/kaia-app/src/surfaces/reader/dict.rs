//! Dictionary lookup for the selected word. StarDict folders are read from
//! Kaia's `dict` folder in the data directory and from KOReader's
//! `data/dict` folder on the Kobo, when either exists.

use std::path::{Path, PathBuf};

use crate::services::Services;
use kaia_dict::{Library, LookupOptions};

pub fn lookup(word: &str, services: &mut Services) -> String {
    if services.dictionary.is_none() {
        let dirs: [PathBuf; 2] = [
            services.paths.data_dir.join("dict"),
            services.paths.books_dir.join(".adds/koreader/data/dict"),
        ];
        services.dictionary = dirs
            .iter()
            .filter_map(|dir| Library::open_dir(dir).ok())
            .find(|lib| !lib.dictionaries().is_empty());
    }
    let Some(library) = services.dictionary.as_ref() else {
        return install_hint(&services.paths.data_dir);
    };
    let names = |key: &str| -> Vec<String> {
        services
            .settings
            .read(key)
            .as_table()
            .into_iter()
            .flat_map(|table| table.values())
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    };
    let order = names("dictionary.order");
    let options = LookupOptions {
        dictionaries: (!order.is_empty()).then_some(order),
        disabled: names("dictionary.disabled").into_iter().collect(),
        ..LookupOptions::default()
    };
    let blocks: Vec<_> = library
        .lookup_with(word, &options)
        .into_iter()
        .map(|entry| {
            format!(
                "{} ({})\n{}",
                entry.headword,
                entry.dict_name,
                entry.definition.display_text()
            )
        })
        .collect();
    if blocks.is_empty() {
        format!("No entry for \"{word}\" in the installed dictionaries.")
    } else {
        blocks.join("\n\n")
    }
}

/// The sheet text when no dictionary is installed: what to copy and where.
pub fn install_hint(data_dir: &Path) -> String {
    format!(
        "No dictionary installed. Copy a StarDict dictionary (.ifo, .idx and .dict files) into {}.",
        data_dir.join("dict").display()
    )
}
