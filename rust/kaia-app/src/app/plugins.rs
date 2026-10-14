//! Plugin calls run between native input, session work and painting.
use kaia_i18n::{format as tr_format, tr};
use std::cell::RefCell;
use std::rc::Rc;

use kaia_events::{Key, SystemClock, Value, WindowId};
use kaia_lua::{LuaHost, LuaUi, PluginOptions, UiConfig};
use kaia_ui::Window;

use super::App;
use crate::backend::Backend;
use crate::plugins::{Request, ENABLED, SAFE_MODE};

impl<B: Backend> App<B> {
    pub(super) fn plugin_window(&self, id: WindowId) -> Option<Window> {
        self.services
            .plugins
            .ui
            .as_ref()
            .and_then(|ui| ui.borrow().window(id))
    }

    pub(super) fn pump_plugins(&mut self) {
        let busy = self.router.busy();
        let enabled = self.services.settings.is_true(ENABLED);
        let p = &mut self.services.plugins;
        p.active = enabled;
        p.discover(&self.services.settings);
        if enabled && !p.initialized {
            p.initialized = true;
            let result = (|| {
                std::fs::create_dir_all(p.library.borrow().data.join("plugins"))
                    .map_err(|e| e.to_string())?;
                let host = LuaHost::new(p.paths.clone()).map_err(|e| e.to_string())?;
                let device = p.device.clone();
                host.register("KAIA_DEVICE", move |args| {
                    let key = args
                        .first()
                        .and_then(Value::as_str)
                        .ok_or("Missing device operation")?;
                    let mut d = device.borrow_mut();
                    if let Some(value) = args.get(1) {
                        d.requests.push((key.into(), value.clone()));
                        if matches!(key, "intensity" | "warmth") {
                            if let Value::Table(values) = &mut d.values {
                                values.insert(Key::from(key), value.clone());
                            }
                        }
                        Ok(Value::Nil)
                    } else {
                        Ok(d.values.get(key).clone())
                    }
                })
                .map_err(|e| e.to_string())?;
                let mut ui = LuaUi::new(
                    host,
                    UiConfig {
                        size: self.screen,
                        dpi: Some(self.backend.dpi()),
                        color: self.services.theme.color_panel,
                    },
                    self.services.assets.clone(),
                    Box::new(SystemClock::new()),
                )
                .map_err(|e| e.to_string())?;
                ui.set_library_host(p.library.clone())
                    .map_err(|e| e.to_string())?;
                Ok::<_, String>(ui)
            })();
            match result {
                Ok(ui) => p.ui = Some(Rc::new(RefCell::new(ui))),
                Err(e) => {
                    eprintln!("kaia: plugins: {e}");
                    p.error = Some(e);
                }
            }
        }
        let platform = &self.services.platform;
        let was_busy = p
            .device
            .borrow()
            .values
            .get("reader_busy")
            .as_bool()
            .unwrap_or(false);
        if enabled && was_busy && !busy && p.reader.is_some() {
            self.stack.set_dirty_all(None, &mut self.ctx);
        }
        let was_charging = p.device.borrow().values.get("charging").as_bool();
        let charging = platform.battery.is_charging().unwrap_or(false);
        if was_charging.is_some_and(|old| old != charging) {
            p.event(if charging { "Charging" } else { "NotCharging" }, vec![]);
        }
        p.device.borrow_mut().values = Value::Table(
            [
                (Key::from("reader_busy"), Value::from(busy)),
                (
                    Key::from("battery"),
                    Value::from(i64::from(platform.battery.capacity().unwrap_or(100))),
                ),
                (Key::from("charging"), Value::from(charging)),
                (
                    Key::from("charged"),
                    Value::from(platform.battery.is_charged().unwrap_or(false)),
                ),
                (
                    Key::from("intensity"),
                    Value::from(i64::from(platform.frontlight.intensity())),
                ),
                (
                    Key::from("warmth"),
                    Value::from(i64::from(platform.frontlight.warmth())),
                ),
                (
                    Key::from("kobo"),
                    Value::from(matches!(platform.kind, kaia_platform::platform::Kind::Kobo(_))),
                ),
            ]
            .into(),
        );
        let Some(ui) = p.ui.clone() else {
            return;
        };
        // Reader callbacks wait until the worker returns the session.
        if busy {
            return;
        }
        let pending = std::mem::take(&mut p.requests);
        let result = ui.borrow_mut().in_app(&mut self.stack, &mut self.ctx, |lua| -> Result<(), String> {
            for request in pending {
                match request {
                    Request::Reader(host) => {
                        if p.reader.is_some() {
                            lua.send_reader_event("CloseDocument", &[]);
                            lua.flush_plugin_settings().map_err(|e| e.to_string())?;
                        }
                        p.reader = Some(host.clone());
                        lua.set_reader_host(host).map_err(|e| e.to_string())?;
                        p.reload = true;
                    }
                    Request::Library(close) => {
                        lua.close_plugin_windows();
                        lua.clear_reader_host().map_err(|e| e.to_string())?;
                        p.reader = None;
                        close();
                        p.reload = true;
                    }
                    Request::Dictionary(word, text) => { lua.dictionary_lookup(&word, &text).map_err(|e| e.to_string())?; }
                    Request::DictionaryActions => { lua.dictionary_actions().map_err(|e| e.to_string())?; }
                    Request::Reload => p.reload = true,
                    Request::Event(event) => {
                        if event.name == "FlushSettings" { lua.flush_plugin_settings().map_err(|e| e.to_string())?; }
                        if !p.reload { lua.send_reader_event(&event.name, &event.args); }
                    }
                    Request::Menu(id) => { lua.open_plugin_menu(&id).map_err(|e| e.to_string())?; }
                }
            }
            if p.reload {
                for status in lua.plugin_status() {
                    if let Some(error) = status.error { p.failed.insert(status.name, error); }
                }
                p.reload = false;
                lua.close_plugin_windows();
                let mut names: Vec<_> = p.entries.iter().filter(|e| enabled && e.enabled && !p.failed.contains_key(&e.name)).map(|e| e.name.clone()).collect();
                // The layer's empty list means all plugins.
                if names.is_empty() { names.push("__kaia_none__".into()); }
                lua.load_plugins(PluginOptions {
                    force_hello: names.iter().any(|n| n == "hello"), names,
                    safe_mode: p.startup_safe_mode || self.services.settings.is_true(SAFE_MODE),
                    extra_paths: vec![p.library.borrow().data.join("plugins")],
                }).map_err(|e| e.to_string())?;
                if let Some(reader) = &p.reader {
                    lua.send_reader_event("DocSettingsLoad", &[]);
                    lua.send_reader_event("ReaderReady", &[]);
                    let page = reader.borrow().current_page();
                    lua.send_reader_event("PageUpdate", &[Value::from(page as i64)]);
                }
            }
            p.paused = lua.eval("idle-policy", "local u = require('ui/uimanager'); return require('pluginshare').pause_auto_suspend == true or (u._standby_holds or 0) > 0")
                .map_err(|e| e.to_string())?.as_bool().unwrap_or(false);
            for status in lua.plugin_status() {
                if let Some(error) = status.error {
                    p.failed.insert(status.name.clone(), error);
                }
                if let Some(entry) = p.entries.iter_mut().find(|e| e.name == status.name) {
                    entry.state = if status.loaded { tr("Works") } else { tr("Disabled") };
                }
            }
            for entry in &mut p.entries {
                if let Some(error) = p.failed.get(&entry.name) { entry.state = tr_format(&tr("Failed: %1"), &[error.as_str()]); }
            }
            p.menus = lua.menu_entries();
            p.owners = p.menus.iter().filter_map(|m| lua.menu_owner(&m.id).map(|o| (m.id.clone(), o))).collect();
            for error in lua.take_errors() { eprintln!("kaia: plugin: {error}"); }
            Ok(())
        });
        if let Err(e) = result {
            eprintln!("kaia: plugins: {e}");
            self.services.plugins.error = Some(e);
        }
        let requests = std::mem::take(&mut self.services.plugins.device.borrow_mut().requests);
        for (key, value) in requests {
            let v = value.as_f64().unwrap_or(0.0).clamp(0.0, 100.0) as u8;
            let result = match key.as_str() {
                "intensity" => self.services.platform.frontlight.set_intensity(v),
                "warmth" => self.services.platform.frontlight.set_warmth(v),
                "suspend" => {
                    self.sleep();
                    Ok(())
                }
                "lookup" => {
                    self.command("reader:dictionary-word", &[value]);
                    Ok(())
                }
                "dictionary_close" => {
                    self.back();
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(e) = result {
                self.notices.push(format!("Plugin device request failed: {e}"));
            }
        }
        let folder = self.services.plugins.library.borrow_mut().show.take();
        if let Some(folder) = folder {
            self.command("nav:library", &[]);
            self.command("library:plugin-folder", &[Value::from(folder)]);
        }
        let open = self.services.plugins.library.borrow_mut().open.take();
        if let Some(path) = open {
            self.command(&format!("open_book:{path}"), &[]);
        }
    }

    pub(super) fn plugin_command(&mut self, id: &str, args: &[Value]) -> bool {
        if let Some(name) = id.strip_prefix("plugins:detail:") {
            self.push(Box::new(
                crate::surfaces::settings::section::SettingsSectionSurface::new(format!("plugin:{name}")),
            ));
            return true;
        }
        if id == "reader:plugins" {
            self.push(Box::new(
                crate::surfaces::settings::section::SettingsSectionSurface::new("plugin-menus".into()),
            ));
            return true;
        }
        if id == "plugins:dictionary" {
            self.services.plugins.requests.push(Request::DictionaryActions);
        } else if let Some(menu) = id.strip_prefix("plugins:menu:") {
            self.services.plugins.requests.push(Request::Menu(menu.into()));
        } else if let Some(key) = id
            .strip_prefix("settings:set:")
            .filter(|k| k.starts_with("plugins."))
        {
            let Some(on) = args.first().and_then(Value::as_bool) else {
                return true;
            };
            self.services.settings.save(key, Value::from(on));
            if let Err(e) = self.services.settings.flush(&self.services.settings_path()) {
                self.notices.push(e.to_string());
            }
            self.services.plugins.requests.push(Request::Reload);
        } else {
            return false;
        }
        self.pump_plugins();
        if id.starts_with("settings:set:") && self.router.depth() > 0 {
            self.rebuild(self.router.depth() - 1);
        }
        true
    }
}
