//! Explicit Wi-Fi requests. Credentials and scan results live only in this screen.
use std::sync::mpsc::Receiver;

use kaia_events::{Event, TaskId, Value};
use kaia_i18n::tr;
use kaia_platform::wpa::{SavedNetwork, ScanResult};
use kaia_ui::components::{Column, CrossAlign, Expanded};
use kaia_ui::composite::SettingsDialog;
use kaia_ui::controls::{OnScreenKeyboard, TextField};
use kaia_ui::{WidgetPod, Window};

use super::{
    entries::{self, Entry},
    keys,
    widgets::SettingsPages,
};
use crate::surface::{Outcome, Surface, SurfaceEnv};
use crate::surfaces::{common::root_window, Route};

use crate::surfaces::wifi_worker::{self, Completion, Reply, Request};

pub struct WifiSurface {
    saved_mode: bool,
    started: bool,
    scan: Vec<ScanResult>,
    saved: Vec<SavedNetwork>,
    selected: Option<usize>,
    password: String,
    status: String,
    pending: Option<Receiver<Completion>>,
    timer: Option<TaskId>,
    page: usize,
}
impl WifiSurface {
    pub fn new(saved_mode: bool) -> Self {
        Self {
            saved_mode,
            started: false,
            scan: vec![],
            saved: vec![],
            selected: None,
            password: String::new(),
            status: String::new(),
            pending: None,
            timer: None,
            page: 0,
        }
    }
    fn start(&mut self, request: Request, env: &mut SurfaceEnv<'_>) {
        if self.pending.is_some() {
            return;
        }
        if keys::airplane_mode(&env.services.settings) || !env.services.platform.network.is_wifi_on() {
            self.status = tr("Turn on Wi-Fi and turn off airplane mode first.");
            return;
        }
        self.status = match &request {
            Request::Scan => tr("Scanning…"),
            Request::Saved => tr("Loading…"),
            Request::Join(..) => tr("Connecting…"),
            Request::Forget(_) => tr("Removing…"),
            Request::Toggle => tr("Connecting…"),
        };
        self.pending = Some(wifi_worker::start(&mut env.services.platform.network, request));
        self.timer = Some(env.schedule_in(0.1, |_| {}));
    }
    fn rows(&self) -> Vec<Entry> {
        if self.pending.is_some() {
            return vec![Entry::Text(self.status.clone())];
        }
        if !self.status.is_empty() && self.selected.is_none() {
            return vec![
                Entry::Text(self.status.clone()),
                Entry::Button {
                    key: "wifi-retry",
                    label: tr("Refresh"),
                    action: "wifi:refresh".into(),
                },
            ];
        }
        if let Some(index) = self.selected {
            if self.saved_mode {
                return vec![
                    Entry::Text(self.saved[index].ssid.clone()),
                    Entry::Text(tr("Remove this saved connection?")),
                    Entry::Text(self.status.clone()),
                    Entry::Button {
                        key: "wifi-remove",
                        label: tr("Remove"),
                        action: "wifi:remove".into(),
                    },
                ];
            }
            return vec![
                Entry::Text(self.scan[index].ssid.clone()),
                Entry::Text(tr("Password")),
                Entry::Text(self.status.clone()),
            ];
        }
        let mut rows: Vec<Entry> = if self.saved_mode {
            self.saved
                .iter()
                .enumerate()
                .map(|(i, row)| Entry::Row {
                    primary: row.ssid.clone(),
                    secondary: None,
                    value: None,
                    action: Some(format!("wifi:select:{i}")),
                })
                .collect()
        } else {
            self.scan
                .iter()
                .enumerate()
                .map(|(i, row)| Entry::Row {
                    primary: row.ssid.clone(),
                    secondary: Some(if secured(row) {
                        tr("Password required")
                    } else {
                        tr("Open network")
                    }),
                    value: Some(if row.signal_dbm >= -60 {
                        tr("Strong")
                    } else if row.signal_dbm >= -75 {
                        tr("Fair")
                    } else {
                        tr("Weak")
                    }),
                    action: Some(format!("wifi:select:{i}")),
                })
                .collect()
        };
        rows.push(Entry::Button {
            key: "wifi-retry",
            label: tr("Refresh"),
            action: "wifi:refresh".into(),
        });
        rows
    }
}
fn secured(row: &ScanResult) -> bool {
    row.flags.contains("WPA") || row.flags.contains("WEP") || row.flags.contains("SAE")
}
impl Surface for WifiSurface {
    fn route(&self) -> Route {
        Route::SettingsSection {
            name: "wifi-connections".into(),
        }
    }
    fn busy(&self) -> bool {
        self.pending.is_some()
    }
    fn can_hide(&mut self, env: &mut SurfaceEnv<'_>) -> bool {
        if self.pending.is_some() {
            env.toast(tr("Wait for the Wi-Fi request to finish."), 2.0);
            false
        } else {
            true
        }
    }
    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        if !self.started {
            self.started = true;
            self.start(
                if self.saved_mode {
                    Request::Saved
                } else {
                    Request::Scan
                },
                env,
            );
        }
        let mut children = vec![WidgetPod::new(Expanded::new(
            WidgetPod::new(SettingsPages::new(self.rows(), self.page)),
            1,
        ))];
        if self.selected.is_some() && !self.saved_mode && self.pending.is_none() {
            children.push(
                WidgetPod::new(
                    TextField::new()
                        .with_text(&self.password)
                        .password(true)
                        .on_change("wifi:password")
                        .on_submit("wifi:join"),
                )
                .focusable(true)
                .with_key("wifi-password"),
            );
            children.push(entries::widget(
                &Entry::Button {
                    key: "wifi-join",
                    label: tr("Join"),
                    action: "wifi:join".into(),
                },
                env.theme(),
            ));
            children.push(
                WidgetPod::new(
                    OnScreenKeyboard::new(super::keyboard::layout(&env.services.language))
                        .on_hide("wifi:cancel"),
                )
                .with_key("wifi-keyboard"),
            );
        }
        Some(root_window(
            env.services,
            env.screen,
            SettingsDialog::new(
                tr("Settings"),
                if self.saved_mode {
                    tr("Saved connections")
                } else {
                    tr("Wi-Fi connections")
                },
                WidgetPod::new(Column::new(children).cross_align(CrossAlign::Stretch)),
                "settings:close",
            )
            .back("wifi:cancel")
            .content_header(),
        ))
    }
    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        if cmd == "wifi:password" {
            self.password = args.first().and_then(Value::as_str).unwrap_or_default().into();
            return Outcome::Handled;
        }
        if self.pending.is_some() && cmd.starts_with("wifi:") {
            return Outcome::Handled;
        }
        if let Some(page) = cmd.strip_prefix("settings:page:").and_then(|p| p.parse().ok()) {
            self.page = page;
            return Outcome::Rebuild;
        }
        if cmd == "wifi:cancel" || (cmd == "back" && self.selected.is_some()) {
            self.password.clear();
            if self.selected.take().is_some() {
                self.status.clear();
                return Outcome::Rebuild;
            }
            return Outcome::Navigate(crate::surface::Nav::Pop);
        }
        if cmd == "wifi:refresh" {
            self.start(
                if self.saved_mode {
                    Request::Saved
                } else {
                    Request::Scan
                },
                env,
            );
            return Outcome::Rebuild;
        }
        if let Some(index) = cmd
            .strip_prefix("wifi:select:")
            .and_then(|n| n.parse::<usize>().ok())
        {
            if self.saved_mode {
                if index < self.saved.len() {
                    self.selected = Some(index);
                }
            } else if let Some(row) = self.scan.get(index) {
                // Enterprise and WEP need a different credential flow.
                if row.flags.contains("EAP")
                    || row.flags.contains("WEP")
                    || (row.flags.contains("SAE") && !row.flags.contains("PSK"))
                {
                    env.toast(tr("This security type is not supported."), 4.0);
                    return Outcome::Handled;
                }
                if secured(row) {
                    self.selected = Some(index);
                } else {
                    self.start(Request::Join(row.ssid.clone(), None), env);
                }
            }
            return Outcome::Rebuild;
        }
        if cmd == "wifi:join" {
            if let Some(row) = self.selected.and_then(|i| self.scan.get(i)) {
                if kaia_platform::wpa::psk_value(&self.password).is_err() {
                    self.status = tr("Use 8 to 63 characters or a 64-digit hex key.");
                    return Outcome::Rebuild;
                }
                let request = Request::Join(row.ssid.clone(), Some(std::mem::take(&mut self.password)));
                self.start(request, env);
            }
            return Outcome::Rebuild;
        }
        if cmd == "wifi:remove" {
            if let Some(row) = self.selected.and_then(|i| self.saved.get(i)) {
                self.start(Request::Forget(row.id), env);
            }
            return Outcome::Rebuild;
        }
        Outcome::NotMine
    }
    fn on_event(&mut self, event: &Event, env: &mut SurfaceEnv<'_>) -> Outcome {
        if event.name != "SettingsTick" {
            return Outcome::NotMine;
        }
        let Some(receiver) = &self.pending else {
            return Outcome::NotMine;
        };
        let Ok((radio, result)) = receiver.try_recv() else {
            if let Some(timer) = self.timer.take() {
                env.ctx.scheduler.unschedule(timer);
            }
            self.timer = Some(env.schedule_in(0.1, |_| {}));
            return Outcome::Handled;
        };
        env.services.platform.network = radio;
        self.pending = None;
        if let Some(timer) = self.timer.take() {
            env.ctx.scheduler.unschedule(timer);
        }
        self.status.clear();
        match result {
            Ok(Reply::Scan(mut rows)) => {
                rows.retain(|r| !r.ssid.is_empty());
                rows.sort_by_key(|r| std::cmp::Reverse(r.signal_dbm));
                let mut seen = std::collections::HashSet::new();
                rows.retain(|r| seen.insert(r.ssid.clone()));
                self.scan = rows;
                if self.scan.is_empty() {
                    self.status = tr("No networks found.");
                }
            }
            Ok(Reply::Saved(rows)) => {
                self.saved = rows;
                if self.saved.is_empty() {
                    self.status = tr("No saved connections.");
                }
            }
            Ok(Reply::Toggled { .. }) => {}
            Ok(Reply::Joined) => {
                self.selected = None;
                self.status = tr("Connected.");
            }
            Ok(Reply::Forgotten) => {
                self.selected = None;
                self.start(Request::Saved, env);
            }
            Err(_) => {
                self.status = tr("Wi-Fi request failed. Check the connection and try again.");
            }
        }
        Outcome::Rebuild
    }
}
