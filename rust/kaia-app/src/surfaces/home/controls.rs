//! The Home sections the reader can switch off. Each section has a switch on the
//! Home page of Settings, stored as a `home.show_*` key in `settings.json`. A key
//! that is not set is on. Home reads the switches each time it builds, so a
//! switch that changes takes effect the next time Home shows.

use kaia_events::Settings;

use crate::surfaces::settings::keys;

/// Which Home sections are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    /// The card of the book to continue, or the book to start.
    pub current: bool,
    /// The next book in the series of the book read last.
    pub next_in_series: bool,
    /// Another unread book by an author of the book on Home.
    pub more_by_author: bool,
    /// The shelf of recent books.
    pub recent: bool,
    /// The other books in progress, one row each.
    pub in_progress: bool,
}

impl Shown {
    /// The switches as the settings hold them. Unset switches are on.
    pub fn read(settings: &Settings) -> Self {
        Self {
            current: keys::bool_or(settings, keys::HOME_SHOW_CURRENT, true),
            next_in_series: keys::bool_or(settings, keys::HOME_SHOW_NEXT_IN_SERIES, true),
            more_by_author: keys::bool_or(settings, keys::HOME_SHOW_MORE_BY_AUTHOR, true),
            recent: keys::bool_or(settings, keys::HOME_SHOW_RECENT, true),
            in_progress: keys::bool_or(settings, keys::HOME_SHOW_IN_PROGRESS, true),
        }
    }

    /// Whether at least one section is on.
    pub fn any(self) -> bool {
        self.current || self.next_in_series || self.more_by_author || self.recent || self.in_progress
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_events::Value;

    fn settings() -> Settings {
        // The file does not exist, so nothing is read from disk and nothing is written.
        Settings::open(std::path::Path::new(
            "/nonexistent/kaia-home-controls/settings.json",
        ))
    }

    #[test]
    fn every_section_is_on_when_no_switch_is_saved() {
        let shown = Shown::read(&settings());
        assert!(shown.current && shown.next_in_series && shown.more_by_author);
        assert!(shown.recent && shown.in_progress);
        assert!(shown.any());
    }

    #[test]
    fn a_saved_off_switch_hides_only_its_section() {
        let mut s = settings();
        s.save(keys::HOME_SHOW_RECENT, Value::Bool(false));
        let shown = Shown::read(&s);
        assert!(!shown.recent);
        assert!(shown.current && shown.next_in_series && shown.more_by_author && shown.in_progress);
        assert!(shown.any());
    }

    #[test]
    fn no_section_on_means_nothing_to_show() {
        let mut s = settings();
        for key in [
            keys::HOME_SHOW_CURRENT,
            keys::HOME_SHOW_NEXT_IN_SERIES,
            keys::HOME_SHOW_MORE_BY_AUTHOR,
            keys::HOME_SHOW_RECENT,
            keys::HOME_SHOW_IN_PROGRESS,
        ] {
            s.save(key, Value::Bool(false));
        }
        assert!(!Shown::read(&s).any());
    }
}
