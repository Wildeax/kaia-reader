//! Confirmation pages for the two resets in Advanced. Each page has a text field
//! and a destructive button that stays disabled until the typed word matches.
//! The file work is in `kaia_platform::reset`, which only touches Kaia's data
//! folder. Both actions leave the screens in place and ask for a restart, so the
//! services that read settings at start (language, clock zone, sync) take the new
//! state on the next start.

use kaia_events::{Settings, Value};
use kaia_i18n::{format as tr_format, tr};
use kaia_library::Library;
use kaia_platform::reset;
use kaia_ui::components::{CrossAlign, MainAlign, Row};
use kaia_ui::controls::{Button, ButtonStyle, OnScreenKeyboard, TextField, ENGLISH};
use kaia_ui::{WidgetPod, Window};

use crate::services::Services;
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{body, button, page, window};
use crate::surfaces::Route;

/// Which reset a page confirms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Settings only. The word is RESET.
    Settings,
    /// Everything in the data folder. The word is ERASE.
    Everything,
}

impl Scope {
    /// The word the user types to confirm, in the language in use.
    pub fn word(self) -> String {
        match self {
            Scope::Settings => tr("RESET"),
            Scope::Everything => tr("ERASE"),
        }
    }

    fn route_name(self) -> &'static str {
        match self {
            Scope::Settings => "reset-settings",
            Scope::Everything => "erase-data",
        }
    }

    fn title(self) -> String {
        match self {
            Scope::Settings => tr("Reset settings"),
            Scope::Everything => tr("Erase all Kaia data"),
        }
    }

    fn body(self) -> Vec<String> {
        match self {
            Scope::Settings => vec![
                tr("This sets every setting back to its default. Books, reading progress, notes, highlights, statistics, fonts and dictionaries stay as they are."),
                tr("Kaia keeps a copy of the old settings as settings.json.before-reset. Copy the .kaia folder to a computer first."),
            ],
            Scope::Everything => vec![
                tr("This deletes everything Kaia keeps in the .kaia folder: settings, reading progress, notes and highlights, the library index, statistics, fonts, dictionaries and the cache."),
                tr("Book files, the Kobo and KOReader folders and the Lua app are not touched. Copy the .kaia folder to a computer first."),
            ],
        }
    }

    fn confirm_label(self) -> String {
        self.title()
    }
}

/// True when `typed` is the confirmation word. Case and spaces at either end do
/// not count. An empty word never matches.
pub fn matches(word: &str, typed: &str) -> bool {
    !word.trim().is_empty() && typed.trim().to_uppercase() == word.trim().to_uppercase()
}

/// A confirmation page. `typed` is the text field's text and `matched` says whether
/// the destructive button is enabled.
pub struct ResetSurface {
    scope: Scope,
    typed: String,
    matched: bool,
}

impl ResetSurface {
    pub fn new(scope: Scope) -> Self {
        Self {
            scope,
            typed: String::new(),
            matched: false,
        }
    }
}

impl Surface for ResetSurface {
    fn route(&self) -> Route {
        Route::SettingsSection {
            name: self.scope.route_name().into(),
        }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        let root = build_page(env.services, self.scope, &self.typed, self.matched);
        Some(window(env.services, env.screen, root))
    }

    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        match cmd {
            "settings:reset:type" => {
                let typed = args.first().and_then(Value::as_str).unwrap_or("").to_string();
                let now = matches(&self.scope.word(), &typed);
                self.typed = typed;
                if now == self.matched {
                    // The field already shows the text. Only the button needs a rebuild.
                    return Outcome::Handled;
                }
                self.matched = now;
                Outcome::Rebuild
            }
            "settings:reset:run" => self.run(env),
            "settings:reset:cancel" => Outcome::Navigate(Nav::Pop),
            _ => Outcome::NotMine,
        }
    }
}

impl ResetSurface {
    fn run(&mut self, env: &mut SurfaceEnv<'_>) -> Outcome {
        // The button is disabled until the word matches. The check runs again here,
        // so no other route to this action skips it.
        if !matches(&self.scope.word(), &self.typed) {
            return Outcome::Handled;
        }
        match self.scope {
            Scope::Settings => reset_settings(env),
            Scope::Everything => erase_everything(env),
        }
    }
}

fn reset_settings(env: &mut SurfaceEnv<'_>) -> Outcome {
    match reset::reset_settings(&env.services.paths.data_dir) {
        Ok(copy) => {
            // The settings in memory are the old ones. Replace them with the file as it
            // is now, so a later save does not write the old values back.
            env.services.settings = Settings::open(&env.services.settings_path());
            env.services.apply_large_print();
            let message = match copy {
                Some(_) => tr("Settings reset. Restart Kaia to use the default settings. The old settings are in settings.json.before-reset."),
                None => tr("Settings reset. Restart Kaia to use the default settings."),
            };
            env.toast(message, 8.0);
            Outcome::Navigate(Nav::SwitchRoot(Route::Settings))
        }
        Err(error) => {
            env.notice(tr_format(&tr("Cannot reset settings: %1"), &[&error.to_string()]));
            Outcome::Handled
        }
    }
}

fn erase_everything(env: &mut SurfaceEnv<'_>) -> Outcome {
    let data = env.services.paths.data_dir.clone();
    match reset::erase(&data) {
        Ok(()) => {
            forget_erased_state(env);
            env.toast(
                tr("Kaia data erased. Restart Kaia to start as on a new install."),
                8.0,
            );
            Outcome::Navigate(Nav::SwitchRoot(Route::Home))
        }
        Err(error) if error.moved => {
            // The folder is already aside. The old handles point into it, so they go.
            forget_erased_state(env);
            env.notice(tr_format(
                &tr("Erase stopped part way: %1. Restart Kaia to finish it."),
                &[&error.source.to_string()],
            ));
            Outcome::Navigate(Nav::SwitchRoot(Route::Home))
        }
        Err(error) => {
            env.notice(tr_format(
                &tr("Cannot erase Kaia data. Nothing was deleted: %1"),
                &[&error.source.to_string()],
            ));
            Outcome::Handled
        }
    }
}

/// After the data folder moved aside, the open handles and the settings in memory
/// point into the erased folder. This replaces them with empty state for the new
/// folder. Anything the session writes after this lands in the new folder, so
/// the user restarts next. The sync worker stops here, so pending progress is not
/// written into the new folder.
fn forget_erased_state(env: &mut SurfaceEnv<'_>) {
    env.services.sync.close();
    env.services.settings = Settings::open(&env.services.settings_path());
    env.services.dictionary = None;
    match Library::open(&env.services.library_path()) {
        Ok(library) => env.services.library = library,
        Err(error) => env.notice(tr_format(
            &tr("Cannot open the library index: %1"),
            &[&error.to_string()],
        )),
    }
}

/// The page: the explanation, the typed word, the field, the buttons and the keyboard.
fn build_page(services: &Services, scope: Scope, typed: &str, matched: bool) -> kaia_ui::components::Padding {
    let t = &services.theme;
    let word = scope.word();
    let mut children = vec![WidgetPod::new(
        kaia_ui::composite::Header::new(t, scope.title()).back("back"),
    )];
    for text in scope.body() {
        children.push(body(services, text));
    }
    children.push(body(services, tr_format(&tr("Type %1 to confirm."), &[&word])));
    children.push(
        WidgetPod::new(
            TextField::new()
                .with_text(typed.to_string())
                .placeholder(word.clone())
                .on_change("settings:reset:type"),
        )
        .focusable(true)
        .with_key("reset-confirm-field"),
    );
    let confirm = WidgetPod::new(
        Button::new(ButtonStyle::Primary)
            .text(scope.confirm_label())
            .enabled(matched)
            .on_action("settings:reset:run"),
    )
    .focusable(matched)
    .with_key("reset-confirm-run");
    let cancel = button(
        ButtonStyle::Secondary,
        &tr("Cancel"),
        "settings:reset:cancel",
        "reset-cancel",
    );
    children.push(WidgetPod::new(
        Row::new(vec![cancel, confirm])
            .gap(t.space.md)
            .main_align(MainAlign::End)
            .cross_align(CrossAlign::Center),
    ));
    children.push(WidgetPod::new(OnScreenKeyboard::new(&ENGLISH)));
    page(services, children)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_matches_in_any_case_with_spaces_around_it() {
        assert!(matches("RESET", "RESET"));
        assert!(matches("RESET", "reset"));
        assert!(matches("RESET", "  Reset \n"));
        assert!(matches("ERASE", "erase"));
    }

    #[test]
    fn partial_or_other_text_does_not_match() {
        assert!(!matches("RESET", ""));
        assert!(!matches("RESET", "RES"));
        assert!(!matches("RESET", "RESETS"));
        assert!(!matches("RESET", "RE SET"));
        assert!(!matches("ERASE", "RESET"));
        assert!(!matches("", ""));
    }
}
