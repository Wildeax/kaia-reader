//! A settings section. It is pushed above the Settings root, and its controls
//! write to `settings.json` through the commands in [`super::commands`].

use std::path::PathBuf;

use kaia_events::Value;
use kaia_gfx::Size;
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_stats::Stats;
use kaia_ui::components::Expanded;
use kaia_ui::Window;

use super::commands::{self, Command};
use super::entries;
use super::facts;
use super::keys;
use super::{save, sections};
use crate::services::Services;
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{page, root_window as window};
use crate::surfaces::reader::dict;
use crate::surfaces::Route;

/// One settings section, shown above the Settings root. `typing` shows the
/// keyboard for the section's text fields (Network and sync only).
pub struct SettingsSectionSurface {
    name: String,
    typing: bool,
    kobo_plan: Option<crate::nickel::Plan>,
    kobo_report: Option<crate::nickel::Report>,
    kobo_dialog: Option<kaia_events::WindowId>,
}

impl SettingsSectionSurface {
    pub fn new(name: String) -> Self {
        Self {
            name,
            typing: false,
            kobo_plan: None,
            kobo_report: None,
            kobo_dialog: None,
        }
    }
}

impl Surface for SettingsSectionSurface {
    fn route(&self) -> Route {
        Route::SettingsSection {
            name: self.name.clone(),
        }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        if self.kobo_plan.is_some() || self.kobo_report.is_some() {
            return Some(super::kobo::build(
                env.services,
                env.screen,
                self.kobo_plan.as_ref(),
                self.kobo_report.as_ref(),
            ));
        }
        if lists_dictionaries(&self.name) {
            dict::load(env.services);
        }
        Some(build(
            env.services,
            env.screen,
            &self.name,
            self.typing,
            now_unix(),
        ))
    }

    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        match cmd {
            "settings:storage" => {
                return Outcome::Navigate(Nav::Push(Box::new(super::storage::StorageSurface::default())));
            }
            "settings:data:kobo" => {
                match crate::nickel::Plan::prepare(&env.services.paths.books_dir, &env.services.library) {
                    Ok(plan) => {
                        self.kobo_plan = Some(plan);
                        self.kobo_report = None;
                    }
                    Err(_) => env.notice(tr("Cannot read Kobo data. Close Nickel and try again.")),
                }
                return Outcome::Rebuild;
            }
            "settings:data:kobo:confirm" if self.kobo_plan.is_some() => {
                self.kobo_dialog = Some(env.dialog(kaia_ui::composite::Dialog::new(env.theme(), tr("Import from Kobo?"),
                    tr("Import the records listed in the plan? Reading positions and typography will stay unchanged."),
                    vec![kaia_ui::composite::DialogButton::new(tr("Cancel"), "settings:data:kobo:cancel"),
                         kaia_ui::composite::DialogButton::new(tr("Import"), "settings:data:kobo:run")])));
                return Outcome::Handled;
            }
            "settings:data:kobo:cancel" => {
                if let Some(window) = self.kobo_dialog.take() {
                    env.ctx.close(window, None);
                }
                return Outcome::Handled;
            }
            "settings:data:kobo:run" => {
                let Some(window) = self.kobo_dialog.take() else {
                    return Outcome::Handled;
                };
                env.ctx.close(window, None);
                if let Some(plan) = self.kobo_plan.take() {
                    match plan.import(&env.services.paths.data_dir, &mut env.services.library) {
                        Ok(report) => self.kobo_report = Some(report),
                        Err(_) => env.notice(tr("Kobo import stopped. Close any open book, wait for the library scan to finish, and try again. Records already imported will be skipped.")),
                    }
                }
                return Outcome::Rebuild;
            }
            "settings:data:kobo:close" => {
                self.kobo_plan = None;
                self.kobo_report = None;
                return Outcome::Rebuild;
            }
            _ => {}
        }
        if self.name == "network" {
            let surface = match cmd {
                "settings:catalogs" => Some(crate::surfaces::catalogs::CatalogSurface::default()),
                "settings:calibre" => Some(crate::surfaces::catalogs::CatalogSurface::calibre()),
                _ => None,
            };
            if let Some(surface) = surface {
                return Outcome::Navigate(Nav::Push(Box::new(surface)));
            }
        }
        match commands::parse(cmd) {
            Some(Command::FieldsOpen | Command::FieldsClose) => {
                let typing = commands::parse(cmd) == Some(Command::FieldsOpen);
                Outcome::Navigate(Nav::Replace(Box::new(SettingsSectionSurface {
                    name: self.name.clone(),
                    typing,
                    kobo_plan: None,
                    kobo_report: None,
                    kobo_dialog: None,
                })))
            }
            Some(Command::Set(key)) => set(&key, args, env),
            Some(Command::Pick(key)) => pick(&key, args, env),
            Some(Command::Language) => Outcome::Navigate(Nav::Push(Box::new(SettingsSectionSurface::new(
                "language".into(),
            )))),
            Some(Command::LanguagePick(tag)) => pick_language(&tag, env),
            Some(Command::TimeZone) => Outcome::Navigate(Nav::Push(Box::new(SettingsSectionSurface::new(
                "timezone".into(),
            )))),
            Some(Command::TimeZonePick(name)) => pick_time_zone(&name, env),
            Some(Command::Dictionaries) => Outcome::Navigate(Nav::Push(Box::new(
                SettingsSectionSurface::new("dictionaries".into()),
            ))),
            Some(Command::Dictionary { index, change }) => dictionary(index, change, env),
            Some(Command::Light(which)) => light(which, args, env),
            Some(Command::SyncTest) => {
                sync_test(env);
                Outcome::Handled
            }
            Some(Command::Import) => {
                import_koreader(env);
                Outcome::Rebuild
            }
            Some(Command::ExportNotes) => {
                crate::surfaces::notes::export_all(env);
                Outcome::Handled
            }
            _ => Outcome::NotMine,
        }
    }

    /// Writes the settings the text fields changed. Text fields do not flush on
    /// each key, so the file is written when the section closes.
    fn on_hide(&mut self, env: &mut SurfaceEnv<'_>) {
        save(env);
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The window of section `name` at Unix time `now`.
pub fn build(services: &Services, screen: Size, name: &str, typing: bool, now: i64) -> Window {
    window(services, screen, section_page(services, name, typing, now))
}

/// The page of section `name`: the header, the entries and, for the text
/// fields, the edit button and the keyboard while typing. Tests render it with
/// a fixed `now`.
pub fn section_page(services: &Services, name: &str, typing: bool, now: i64) -> kaia_ui::components::Padding {
    let t = &services.theme;
    let header =
        kaia_ui::WidgetPod::new(kaia_ui::composite::Header::new(t, sections::title(name)).back("back"));
    let mut children = vec![header];
    if has_text_fields(name) {
        let (label, action) = if typing {
            (tr("Done editing"), "settings:fields:close")
        } else {
            (tr("Edit fields"), "settings:fields:open")
        };
        children.push(crate::surfaces::common::button(
            kaia_ui::controls::ButtonStyle::Secondary,
            &label,
            action,
            "settings-fields",
        ));
    }
    let list = sections::entries(name, services, now);
    children.push(kaia_ui::WidgetPod::new(Expanded::new(
        kaia_ui::WidgetPod::new(kaia_ui::composite::PagedList::new(list, entries::widget))
            .with_key("settings-entries"),
        1,
    )));
    if has_text_fields(name) && typing {
        children.push(kaia_ui::WidgetPod::new(
            kaia_ui::controls::OnScreenKeyboard::new(&kaia_ui::controls::ENGLISH)
                .on_hide("settings:fields:close"),
        ));
    }
    page(services, children)
}

fn has_text_fields(name: &str) -> bool {
    name == "network"
}

/// Whether section `name` lists the dictionaries. Those sections load them before they build.
fn lists_dictionaries(name: &str) -> bool {
    matches!(name, "reading" | "dictionaries")
}

/// Turns a dictionary on or off, or moves it one place in the lookup order. The page
/// is rebuilt, so it shows the new state.
fn dictionary(index: usize, change: dict::Change, env: &mut SurfaceEnv<'_>) -> Outcome {
    if !dict::apply(env.services, index, change) {
        return Outcome::Handled;
    }
    save(env);
    Outcome::Rebuild
}

/// Writes a toggle, stepper or text value. Other keys are not stored.
fn set(key: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
    if !commands::is_known_key(key) {
        return Outcome::NotMine;
    }
    let Some(value) = args.first().cloned() else {
        return Outcome::Handled;
    };
    if key == keys::SYNC_PASSWORD {
        // Typing a password may fix a refused login, so the next request runs again.
        env.services.sync.forget_refusal();
        // The typed text never reaches settings.json. Only its user key is stored.
        crate::sync::store_password(&mut env.services.settings, value.as_str().unwrap_or(""));
        return Outcome::Handled;
    }
    env.services.settings.save(key, value);
    if !key.starts_with("sync.") {
        save(env);
    }
    if key == keys::DISPLAY_LARGE_PRINT {
        // Switching the root replaces the stack, so no screen keeps the old sizes.
        env.services.apply_large_print();
        return Outcome::Navigate(Nav::SwitchRoot(Route::Settings));
    }
    if key == keys::READING_FONT_SIZE {
        Outcome::Rebuild
    } else {
        Outcome::Handled
    }
}

/// Writes the option a radio group chose. An empty stored value removes the key.
fn pick(key: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
    let Some(index) = args.first().and_then(Value::as_i64) else {
        return Outcome::NotMine;
    };
    let Some(value) = commands::value_for_pick(key, index.max(0) as usize) else {
        return Outcome::NotMine;
    };
    if value.as_str() == Some("") {
        env.services.settings.del(key);
    } else {
        env.services.settings.save(key, value);
    }
    save(env);
    Outcome::Handled
}

/// Shows the screens in language `tag`, when it is on offer, and stores the choice.
fn pick_language(tag: &str, env: &mut SurfaceEnv<'_>) -> Outcome {
    let on_offer = crate::i18n::available(&env.services.catalog_root);
    let Some(&(known, _)) = on_offer.iter().find(|(t, _)| *t == tag) else {
        return Outcome::NotMine;
    };
    env.services.settings.save(keys::UI_LANGUAGE, Value::from(known));
    env.services.set_language(known);
    save(env);
    Outcome::Handled
}

/// Stores the zone the Time zone page chose. An empty name removes the key, which
/// means the system zone. A name that is not on offer changes nothing. Kaia sets the
/// process zone at startup only, so the new zone shows after the next start.
fn pick_time_zone(name: &str, env: &mut SurfaceEnv<'_>) -> Outcome {
    let before = keys::time_zone(&env.services.settings).map(|zone| zone.name);
    if name.is_empty() {
        env.services.settings.del(keys::DEVICE_TIME_ZONE);
    } else if kaia_platform::timezone::find(name).is_some() {
        env.services
            .settings
            .save(keys::DEVICE_TIME_ZONE, Value::from(name));
    } else {
        return Outcome::NotMine;
    }
    save(env);
    let after = keys::time_zone(&env.services.settings).map(|zone| zone.name);
    if after != before {
        env.toast(tr("The new time zone applies after a restart."), 4.0);
    }
    Outcome::Rebuild
}

/// Sets the frontlight or warmth of the platform. Only Kobo devices have them;
/// on the desktop the sliders are disabled.
fn light(which: commands::Light, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
    let Some(v) = args.first().and_then(Value::as_f64) else {
        return Outcome::NotMine;
    };
    let v = v.round().clamp(0.0, 100.0) as u8;
    let result = match which {
        commands::Light::Frontlight => env.services.platform.frontlight.set_intensity(v),
        commands::Light::Warmth => env.services.platform.frontlight.set_warmth(v),
    };
    if let Err(e) = result {
        env.notice(tr_format(&tr("cannot set the light: %1"), &[&e.to_string()]));
    }
    if which == commands::Light::Warmth {
        env.services
            .settings
            .save("display.warmth", Value::Number(f64::from(v)));
        save(env);
    }
    Outcome::Handled
}

/// Tests the server on the sync worker after the user enables sync and asks.
pub fn sync_test(env: &mut SurfaceEnv<'_>) {
    if let Err(message) = env.services.sync.health(&env.services.settings) {
        env.toast(message, 4.0);
    }
}

/// Imports what the KOReader folder holds: the reading history, the
/// collections and the statistics. KOReader's files are read and never written.
fn import_koreader(env: &mut SurfaceEnv<'_>) {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let Some(folder) = facts::find_koreader(&env.services.paths, home.as_deref()) else {
        env.toast(tr("No KOReader folder found."), 4.0);
        return;
    };
    let mut done = Vec::new();
    if folder.history {
        match env
            .services
            .library
            .import_koreader_history(&folder.history_path())
        {
            Ok(r) => done.push(tr_format(
                &tr_n("%1 history entry", "%1 history entries", r.added as i64),
                &[&r.added.to_string()],
            )),
            Err(e) => env.notice(tr_format(&tr("history import failed: %1"), &[&e.to_string()])),
        }
    }
    if folder.collections {
        match env
            .services
            .library
            .import_koreader_collections(&folder.collections_path())
        {
            Ok(r) => done.push(tr_format(
                &tr_n("%1 collection entry", "%1 collection entries", r.added as i64),
                &[&r.added.to_string()],
            )),
            Err(e) => env.notice(tr_format(&tr("collections import failed: %1"), &[&e.to_string()])),
        }
    }
    if folder.statistics {
        let stats_path = facts::stats_path(&env.services.paths.data_dir);
        match Stats::open(&stats_path).and_then(|mut stats| stats.import_koreader(folder.statistics_path())) {
            Ok(_) => done.push(tr("statistics")),
            Err(e) => env.notice(tr_format(&tr("statistics import failed: %1"), &[&e.to_string()])),
        }
    }
    let message = if done.is_empty() {
        tr("Nothing to import.")
    } else {
        let separator = tr(", ");
        tr_format(&tr("Imported %1."), &[&done.join(separator.as_str())])
    };
    env.toast(message, 5.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_network_section_has_text_fields() {
        assert!(has_text_fields("network"));
        assert!(!has_text_fields("reading"));
    }
}
