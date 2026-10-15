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
use crate::surfaces::common::root_window as window;
use crate::surfaces::reader::dict;
use crate::surfaces::Route;

/// One settings section, shown above the Settings root. `typing` shows the
/// keyboard for the section's text fields (Network and sync only).
pub struct SettingsSectionSurface {
    name: String,
    typing: bool,
    page: usize,
    choosing: Option<String>,
    choice_page: usize,
    lua_plan: Option<crate::lua_import::Plan>,
    lua_report: Option<crate::lua_import::Report>,
    kobo_plan: Option<crate::nickel::Plan>,
    kobo_report: Option<crate::nickel::Report>,
    kobo_dialog: Option<kaia_events::WindowId>,
    /// A screen lock flow (set, change or turn off). It replaces the rows while it runs.
    pin: Option<crate::surfaces::pin::session::Session>,
}

impl SettingsSectionSurface {
    pub fn new(name: String) -> Self {
        Self {
            name,
            typing: false,
            page: 0,
            choosing: None,
            choice_page: 0,
            lua_plan: None,
            lua_report: None,
            kobo_plan: None,
            kobo_report: None,
            kobo_dialog: None,
            pin: None,
        }
    }
}

/// The screen lock actions of the device section. `settings:pin:*` opens a flow.
/// The pad's own actions go to the flow while it runs.
fn pin_flow_start(cmd: &str) -> Option<crate::surfaces::pin::session::Purpose> {
    use crate::surfaces::pin::session::Purpose;
    match cmd {
        "settings:pin:set" => Some(Purpose::Set),
        "settings:pin:change" => Some(Purpose::Change),
        "settings:pin:off" => Some(Purpose::Off),
        _ => None,
    }
}

fn pin_done_message(done: crate::surfaces::pin::session::Done) -> String {
    use crate::surfaces::pin::session::Done;
    match done {
        Done::Set => tr("Screen lock is on."),
        Done::Changed => tr("PIN changed."),
        Done::Cleared => tr("Screen lock is off."),
        Done::Unlocked => String::new(),
    }
}

impl Surface for SettingsSectionSurface {
    fn route(&self) -> Route {
        Route::SettingsSection {
            name: self.name.clone(),
        }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        if let Some(session) = &self.pin {
            return Some(crate::surfaces::pin::view(
                env.services,
                env.screen,
                session,
                true,
            ));
        }
        if self.lua_plan.is_some() || self.lua_report.is_some() {
            return Some(super::lua_import::build(
                env.services,
                env.screen,
                self.lua_plan.as_ref(),
                self.lua_report.as_ref(),
            ));
        }
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
        Some(window(
            env.services,
            env.screen,
            section_page_at(
                env.services,
                &self.name,
                self.typing,
                now_unix(),
                if self.choosing.is_some() {
                    self.choice_page
                } else {
                    self.page
                },
                self.choosing.as_deref(),
            ),
        ))
    }

    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        if let Some(page) = cmd.strip_prefix("settings:page:").and_then(|s| s.parse().ok()) {
            if self.choosing.is_some() {
                self.choice_page = page;
            } else {
                self.page = page;
            }
            return Outcome::Rebuild;
        }
        if let Some(key) = cmd.strip_prefix("settings:choose:") {
            self.choosing = Some(key.into());
            self.choice_page = 0;
            return Outcome::Rebuild;
        }
        if cmd == "back" && self.choosing.is_some() {
            self.choosing = None;
            return Outcome::Rebuild;
        }
        if let Some((key, index)) = cmd
            .strip_prefix("settings:pick:option:")
            .and_then(|s| s.rsplit_once(':'))
        {
            if let Ok(index) = index.parse::<usize>() {
                self.choosing = None;
                if key == keys::HEADER_CHOICE || key == keys::FOOTER_CHOICE {
                    let choices = if key == keys::HEADER_CHOICE {
                        &keys::HEADER_CHOICES[..]
                    } else {
                        &keys::FOOTER_CHOICES[..]
                    };
                    if index < choices.len() {
                        for (i, (flag, _)) in choices.iter().enumerate().skip(1) {
                            env.services.settings.save(*flag, Value::Bool(i == index));
                        }
                        save(env);
                    }
                    return Outcome::Rebuild;
                }
                pick(key, &[Value::Number(index as f64)], env);
                return Outcome::Rebuild;
            }
        }
        if self.name == "device" {
            if let Some(purpose) = pin_flow_start(cmd) {
                let now = crate::surfaces::pin::session::unix_now();
                self.pin = Some(crate::surfaces::pin::session::Session::new(
                    &env.services.paths.data_dir,
                    purpose,
                    now,
                ));
                return Outcome::Rebuild;
            }
        }
        if self.pin.is_some() {
            if cmd == "back" {
                self.pin = None;
                return Outcome::Rebuild;
            }
            let result = self.pin.as_mut().and_then(|session| {
                crate::surfaces::pin::act(session, cmd, crate::surfaces::pin::session::unix_now())
            });
            match result {
                Some(Ok(None)) => return Outcome::Rebuild,
                Some(Ok(Some(done))) => {
                    self.pin = None;
                    env.toast(pin_done_message(done), 4.0);
                    return Outcome::Rebuild;
                }
                Some(Err(error)) => {
                    env.notice(tr_format(&tr("Cannot save the PIN: %1"), &[&error.to_string()]));
                    return Outcome::Rebuild;
                }
                None => {}
            }
        }
        match cmd {
            "settings:data:lua" => {
                if let Some(source) = crate::lua_import::find(&env.services.paths) {
                    match crate::lua_import::Plan::prepare(
                        &source,
                        &env.services.paths.data_dir,
                        &env.services.library,
                    ) {
                        Ok(mut plan) => {
                            plan.viewport = env.screen;
                            self.lua_plan = Some(plan);
                            self.lua_report = None;
                        }
                        Err(_) => {
                            env.notice(tr("Cannot read Lua app data. Close the Lua app and try again."))
                        }
                    }
                }
                return Outcome::Rebuild;
            }
            "settings:data:lua:confirm" => {
                if let Some(plan) = self.lua_plan.take() {
                    match plan.import(&env.services.paths.data_dir, &mut env.services.library, &mut env.services.settings) {
                        Ok(report) => {
                            if report.imported.get("Settings") > 0 {
                                if let Some(n) = env.services.settings.read("display.frontlight").as_i64() { let _ = env.services.platform.frontlight.set_intensity(n.clamp(0,100) as u8); }
                                if let Some(n) = env.services.settings.read("display.warmth").as_i64() { let _ = env.services.platform.frontlight.set_warmth(n.clamp(0,100) as u8); }
                            }
                            self.lua_report = Some(report);
                        }
                        Err(_) => env.notice(tr("Import stopped. Close any open book, wait for the library scan, and try again. Existing data will be skipped.")),
                    }
                }
                return Outcome::Rebuild;
            }
            "settings:data:lua:close" => {
                self.lua_plan = None;
                self.lua_report = None;
                return Outcome::Rebuild;
            }
            "settings:storage" => {
                return Outcome::Navigate(Nav::Push(Box::new(super::storage::StorageSurface::default())));
            }
            "settings:reset:settings" => {
                return Outcome::Navigate(Nav::Push(Box::new(super::reset::ResetSurface::new(
                    super::reset::Scope::Settings,
                ))));
            }
            "settings:reset:erase" => {
                return Outcome::Navigate(Nav::Push(Box::new(super::reset::ResetSurface::new(
                    super::reset::Scope::Everything,
                ))));
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
            if cmd == "settings:webdav:test" {
                let connected = env.services.platform.network.is_connected();
                if let Err(message) = env
                    .services
                    .webdav
                    .test_connection(&env.services.settings, connected)
                {
                    env.toast(message, 5.0);
                }
                return Outcome::Handled;
            }
            if cmd == "settings:webdav:export" {
                if !env.services.settings.is_true(crate::webdav::ENABLED) {
                    env.toast(tr("Turn on Export notes to WebDAV first."), 4.0);
                } else {
                    crate::webdav::enqueue_saved(env, None, true);
                }
                return Outcome::Handled;
            }
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
            Some(Command::Section(name)) => {
                Outcome::Navigate(Nav::Push(Box::new(SettingsSectionSurface::new(name))))
            }
            Some(Command::FieldsOpen | Command::FieldsClose) => {
                let typing = commands::parse(cmd) == Some(Command::FieldsOpen);
                Outcome::Navigate(Nav::Replace(Box::new(SettingsSectionSurface {
                    name: self.name.clone(),
                    typing,
                    page: self.page,
                    choosing: None,
                    choice_page: 0,
                    lua_plan: None,
                    lua_report: None,
                    kobo_plan: None,
                    kobo_report: None,
                    kobo_dialog: None,
                    pin: self.pin.take(),
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
            Some(Command::DictionaryDownloads) => {
                if !crate::dictionary_downloads::allowed(&env.services.settings) {
                    return Outcome::Handled;
                }
                Outcome::Navigate(Nav::Push(Box::new(SettingsSectionSurface::new(
                    "dictionary-downloads".into(),
                ))))
            }
            Some(Command::DictionaryInstall(id)) => {
                if let Some(entry) = kaia_dict::download::catalog().iter().find(|entry| entry.id == id) {
                    match env.services.dictionary_downloads.start(
                        entry.clone(),
                        env.services.paths.data_dir.join("dict"),
                        &env.services.settings,
                    ) {
                        Ok(()) => env.notice(tr("Downloading dictionary. Files come from the internet.")),
                        Err(error) => env.notice(error),
                    }
                }
                Outcome::Rebuild
            }
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

    fn on_event(&mut self, event: &kaia_events::Event, _env: &mut SurfaceEnv<'_>) -> Outcome {
        if event.name == "DictionariesChanged"
            && matches!(
                self.name.as_str(),
                "reading" | "dictionaries" | "dictionary-downloads" | "language-dictionaries"
            )
        {
            Outcome::Rebuild
        } else {
            Outcome::NotMine
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
pub fn section_page(
    services: &Services,
    name: &str,
    typing: bool,
    now: i64,
) -> kaia_ui::composite::SettingsDialog {
    section_page_at(services, name, typing, now, 0, None)
}

fn section_page_at(
    services: &Services,
    name: &str,
    typing: bool,
    now: i64,
    page: usize,
    choosing: Option<&str>,
) -> kaia_ui::composite::SettingsDialog {
    let mut children = Vec::new();
    let mut list = sections::entries(name, services, now);
    if has_text_fields(name) {
        list.insert(
            0,
            entries::Entry::Button {
                key: "settings-fields",
                label: if typing {
                    tr("Done editing")
                } else {
                    tr("Edit fields")
                },
                action: if typing {
                    "settings:fields:close"
                } else {
                    "settings:fields:open"
                }
                .into(),
            },
        );
    }
    if let Some(key) = choosing {
        list = list
            .iter()
            .find_map(|entry| match entry {
                entries::Entry::Select {
                    key: k,
                    options,
                    selected,
                    ..
                }
                | entries::Entry::Radio {
                    key: k,
                    options,
                    selected,
                } if *k == key => Some(
                    options
                        .iter()
                        .enumerate()
                        .map(|(i, label)| entries::Entry::Choice {
                            label: label.clone(),
                            selected: i == *selected,
                            action: format!("settings:pick:option:{key}:{i}"),
                        })
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default();
    }
    children.push(kaia_ui::WidgetPod::new(Expanded::new(
        kaia_ui::WidgetPod::new(super::widgets::SettingsPages::new(list, page)).with_key("settings-entries"),
        1,
    )));
    if has_text_fields(name) && typing {
        children.push(kaia_ui::WidgetPod::new(
            kaia_ui::controls::OnScreenKeyboard::new(&kaia_ui::controls::ENGLISH)
                .on_hide("settings:fields:close"),
        ));
    }
    let content = kaia_ui::WidgetPod::new(
        kaia_ui::components::Column::new(children).cross_align(kaia_ui::components::CrossAlign::Stretch),
    );
    kaia_ui::composite::SettingsDialog::new(tr("Settings"), sections::title(name), content, "settings:close")
        .back("back")
        .content_header()
}

fn has_text_fields(name: &str) -> bool {
    name == "network"
}

/// Whether section `name` lists the dictionaries. Those sections load them before they build.
fn lists_dictionaries(name: &str) -> bool {
    matches!(
        name,
        "reading" | "fonts" | "dictionaries" | "language-dictionaries"
    )
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
    if key == crate::webdav::PASSWORD {
        match crate::webdav::store_password(&env.services.paths.data_dir, value.as_str().unwrap_or("")) {
            Ok(()) => {
                if let Err(message) = env.services.webdav.account_changed() {
                    env.toast(message, 5.0);
                }
            }
            Err(_) => {
                env.toast(tr("Could not save the WebDAV password."), 5.0);
            }
        }
        return Outcome::Handled;
    }
    if matches!(
        key,
        crate::webdav::SERVER | crate::webdav::USERNAME | crate::webdav::FOLDER
    ) {
        if let Err(message) = env.services.webdav.account_changed() {
            env.toast(message, 5.0);
        }
    }
    if key == keys::SYNC_PASSWORD {
        // Typing a password may fix a refused login, so the next request runs again.
        env.services.sync.forget_refusal();
        // The typed text never reaches settings.json. Only its user key is stored.
        crate::sync::store_password(&mut env.services.settings, value.as_str().unwrap_or(""));
        return Outcome::Handled;
    }
    env.services.settings.save(key, value);
    if key == crate::webdav::ENABLED {
        env.services.webdav.update_gate(&env.services.settings, false);
    }
    if !key.starts_with("sync.") {
        save(env);
    }
    if key == keys::DISPLAY_LARGE_PRINT {
        // Returning to the index rebuilds it and marks covered screens for rebuild.
        env.services.apply_large_print();
        return Outcome::Navigate(Nav::SwitchRoot(Route::Settings));
    }
    if key == keys::READING_FONT_SIZE || key == crate::dictionary_downloads::ALLOW_KEY {
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
    let key = if which == commands::Light::Warmth {
        "display.warmth"
    } else {
        "display.frontlight"
    };
    env.services.settings.save(key, Value::Number(f64::from(v)));
    save(env);
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
