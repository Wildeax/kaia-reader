//! Experimental features. One list in code names the flags. The Settings page
//! `experimental` (opened from Advanced) shows a master switch and one switch
//! per flag.
//!
//! A flag is on when the master switch and the flag's own switch are both on.
//! The master switch is `experimental.enabled`, and a flag is stored under
//! `experimental.<id>`. Both default to off.
//!
//! To add a flag:
//! 1. Add a `Flag` to [`FLAGS`]. Its `key` is `experimental.<id>`, with the id
//!    in lowercase letters, digits and dashes.
//! 2. Add an arm to [`title`] that returns the label as `tr("...")`. The string
//!    extraction reads literal `tr` calls only, so the label must appear there.
//! 3. Call [`enabled`] where the feature starts, and keep the old path when it returns false.
//! 4. Run `tools/kaia/i18n/extract.py` to refresh the catalogue.

use kaia_events::Settings;
use kaia_i18n::tr;

use super::entries::Entry;
use super::keys;

/// One experimental flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Flag {
    /// The id, such as `demo`.
    pub id: &'static str,
    /// The settings key, `experimental.` and the id. [`key`] builds the same text.
    pub key: &'static str,
}

/// The flags of this build. Empty: no feature is behind a flag yet.
pub const FLAGS: &[Flag] = &[];

const PREFIX: &str = "experimental.";

/// The settings key of flag `id`.
pub fn key(id: &str) -> String {
    format!("{PREFIX}{id}")
}

/// Whether `key` is the settings key of a flag in `flags`.
pub fn is_flag_key(flags: &[Flag], key: &str) -> bool {
    flags.iter().any(|flag| flag.key == key)
}

/// Whether flag `id` is on: the master switch and the flag's own switch.
pub fn enabled(settings: &Settings, id: &str) -> bool {
    keys::bool_or(settings, keys::EXPERIMENTAL_ENABLED, false) && keys::bool_or(settings, &key(id), false)
}

/// The label of flag `id` in the language in use. Each flag in [`FLAGS`] needs
/// an arm here, see the module docs. An id without an arm shows its id.
pub fn title(id: &str) -> String {
    id.to_string()
}

/// The rows of the flag list: one switch per flag in `flags`, or a line that
/// says there are none.
pub fn entries(flags: &[Flag], settings: &Settings) -> Vec<Entry> {
    if flags.is_empty() {
        return vec![Entry::Text(tr("No experimental features in this build."))];
    }
    flags
        .iter()
        .map(|flag| Entry::Toggle {
            key: flag.key,
            label: title(flag.id),
            on: keys::bool_or(settings, flag.key, false),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_events::Value;

    const DEMO: &[Flag] = &[Flag {
        id: "demo",
        key: "experimental.demo",
    }];

    #[test]
    fn a_flag_needs_both_switches() {
        let mut s = Settings::from_table(Default::default());
        assert!(!enabled(&s, "demo"));
        s.save("experimental.demo", Value::Bool(true));
        assert!(!enabled(&s, "demo"), "the flag alone does nothing");
        s.save(keys::EXPERIMENTAL_ENABLED, Value::Bool(true));
        assert!(enabled(&s, "demo"));
        assert!(!enabled(&s, "other"));
    }

    #[test]
    fn only_listed_flags_are_settings_keys() {
        assert!(is_flag_key(DEMO, "experimental.demo"));
        assert!(!is_flag_key(DEMO, "experimental.other"));
        assert!(!is_flag_key(FLAGS, "experimental.demo"));
        assert!(!is_flag_key(DEMO, keys::EXPERIMENTAL_ENABLED));
    }

    #[test]
    fn a_flag_key_is_its_id_under_the_prefix() {
        for flag in DEMO {
            assert_eq!(key(flag.id), flag.key);
        }
    }

    #[test]
    fn an_empty_list_says_so() {
        let s = Settings::from_table(Default::default());
        let rows = entries(FLAGS, &s);
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0], Entry::Text(_)));
    }

    #[test]
    fn each_flag_has_a_switch_in_the_list() {
        let s = Settings::from_table(Default::default());
        let rows = entries(DEMO, &s);
        assert!(matches!(
            &rows[0],
            Entry::Toggle { key: "experimental.demo", label, on: false } if label == "demo"
        ));
    }
}
