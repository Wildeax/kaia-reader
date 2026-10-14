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
use crate::surfaces::Route;

/// One settings section, shown above the Settings root. `typing` shows the
/// keyboard for the section's text fields (Network and sync only).
pub struct SettingsSectionSurface {
    name: String,
    typing: bool,
}

impl SettingsSectionSurface {
    pub fn new(name: String) -> Self {
        Self { name, typing: false }
    }
}

impl Surface for SettingsSectionSurface {
    fn route(&self) -> Route {
        Route::SettingsSection {
            name: self.name.clone(),
        }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(build(
            env.services,
            env.screen,
            &self.name,
            self.typing,
            now_unix(),
        ))
    }

    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
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
                })))
            }
            Some(Command::Set(key)) => set(&key, args, env),
            Some(Command::Pick(key)) => pick(&key, args, env),
            Some(Command::Language) => Outcome::Navigate(Nav::Push(Box::new(SettingsSectionSurface::new(
                "language".into(),
            )))),
            Some(Command::LanguagePick(tag)) => pick_language(&tag, env),
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

/// Writes a toggle, stepper or text value. Other keys are not stored.
fn set(key: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
    if !commands::is_known_key(key) {
        return Outcome::NotMine;
    }
    let Some(value) = args.first().cloned() else {
        return Outcome::Handled;
    };
    if key == keys::DISPLAY_ROTATION_LOCK {
        env.toast(tr("Automatic rotation is not available yet."), 3.0);
        return Outcome::Handled;
    }
    env.services.settings.save(key, value);
    if !key.starts_with("sync.") {
        save(env);
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
        if key == keys::DISPLAY_ROTATION_LOCK {
            env.toast(tr("Automatic rotation is not available yet."), 3.0);
            return Outcome::Handled;
        }
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
