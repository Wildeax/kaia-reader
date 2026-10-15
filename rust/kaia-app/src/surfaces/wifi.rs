//! Wi-Fi status and joining. Names and credentials stay in memory.
use super::wifi_worker::{self, Completion, Reply, Request};
use super::{
    common,
    library::{
        views,
        widgets::{Overlay, Scroll},
    },
    settings, Route,
};
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use kaia_events::{Event, TaskId, Value};
use kaia_gfx::Rect;
use kaia_i18n::tr;
use kaia_platform::wpa::ScanResult;
use kaia_ui::components::{Column, CrossAlign, Expanded, Frame, Icon, Label, Padding, Row, SizedBox};
use kaia_ui::controls::{Checkbox, TextField};
use kaia_ui::geometry::Insets;
use kaia_ui::{Tone, WidgetPod, Window};
use std::sync::mpsc::Receiver;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Scanning,
    Password,
    Joining,
    Connected,
    Failed,
}

pub struct WifiSurface {
    state: State,
    networks: Vec<ScanResult>,
    selected: Option<usize>,
    password: String,
    error: String,
    pending: bool,
    worker: Option<Receiver<Completion>>,
    timer: Option<TaskId>,
    ready_at: std::time::Instant,
    initialized: bool,
}
impl Default for WifiSurface {
    fn default() -> Self {
        Self {
            state: State::Idle,
            networks: vec![],
            selected: None,
            password: String::new(),
            error: String::new(),
            pending: false,
            worker: None,
            timer: None,
            ready_at: std::time::Instant::now(),
            initialized: false,
        }
    }
}
impl WifiSurface {
    fn scan(&mut self) {
        self.state = State::Scanning;
        self.pending = true;
        self.ready_at = std::time::Instant::now() + std::time::Duration::from_millis(250);
        self.selected = None;
        self.password.clear();
        self.error.clear();
    }
    fn fail(&mut self, message: String) {
        self.state = State::Failed;
        self.error = message;
        self.password.clear();
    }
    fn work(&mut self, env: &mut SurfaceEnv<'_>) {
        self.pending = false;
        if settings::keys::airplane_mode(&env.services.settings) {
            self.fail(tr("Turn off airplane mode in Network settings."));
            return;
        }
        let request = if self.state == State::Scanning {
            Request::Scan
        } else if self.state == State::Joining {
            let Some(network) = self.selected.and_then(|n| self.networks.get(n)) else {
                return;
            };
            let password = std::mem::take(&mut self.password);
            Request::Join(
                network.ssid.clone(),
                if password.is_empty() { None } else { Some(password) },
            )
        } else {
            return;
        };
        self.start(request, env);
    }
    fn start(&mut self, request: Request, env: &mut SurfaceEnv<'_>) {
        self.pending = true;
        self.worker = Some(wifi_worker::start(&mut env.services.platform.network, request));
        self.timer = Some(env.schedule_in(0.1, |_| {}));
    }
    fn poll(&mut self, env: &mut SurfaceEnv<'_>) -> Outcome {
        let Some(receiver) = &self.worker else {
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
        self.worker = None;
        self.pending = false;
        if let Some(timer) = self.timer.take() {
            env.ctx.scheduler.unschedule(timer);
        }
        match result {
            Ok(Reply::Scan(mut networks)) => {
                networks.sort_by_key(|n| std::cmp::Reverse(n.signal_dbm));
                let mut seen = std::collections::HashSet::new();
                networks.retain(|n| seen.insert(n.ssid.clone()));
                self.networks = networks;
                self.state = State::Idle;
            }
            Ok(Reply::Joined) => self.state = State::Connected,
            Ok(Reply::Toggled { requested_on }) => {
                self.networks.clear();
                self.state = State::Idle;
                if env.services.platform.network.is_wifi_on() {
                    self.scan();
                } else if requested_on {
                    self.fail(tr("Wi-Fi is unavailable on this device."));
                }
            }
            Err(_) if self.state == State::Scanning => self.fail(tr("Cannot scan for networks. Try again.")),
            Err(_) if self.state == State::Joining => {
                self.fail(tr("Cannot join this network. Check the password and try again."))
            }
            Err(_) => self.fail(tr("Cannot change Wi-Fi power. Try again.")),
            _ => {}
        }
        Outcome::Rebuild
    }
}
impl Surface for WifiSurface {
    fn route(&self) -> Route {
        Route::Wifi
    }
    fn busy(&self) -> bool {
        self.pending
    }
    fn can_hide(&mut self, env: &mut SurfaceEnv<'_>) -> bool {
        if self.worker.is_some() {
            env.toast(tr("Wait for the Wi-Fi request to finish."), 2.0);
            false
        } else {
            true
        }
    }
    fn show_opts(&self) -> kaia_events::ShowOpts {
        kaia_events::ShowOpts {
            modal: true,
            ..Default::default()
        }
    }
    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        if !self.initialized {
            self.initialized = true;
            if env.services.platform.network.is_wifi_on() {
                self.scan();
            }
        }
        let t = &env.services.theme;
        let on = env.services.platform.network.is_wifi_on();
        let typing = self.state == State::Password;
        let mut rows = vec![WidgetPod::new(
            Row::new(vec![
                WidgetPod::new(Expanded::new(
                    WidgetPod::new(Label::new(tr("Wi-Fi"), t.font.book_title.clone(), Tone::Ink)),
                    1,
                )),
                WidgetPod::new(Checkbox::new(on).on_action("wifi:toggle")).with_key("quick-wifi"),
                views::compact(t, "↻", "wifi:scan", "wifi-scan"),
                views::compact(t, "×", "back", "wifi-close"),
            ])
            .cross_align(CrossAlign::Center),
        )];
        let message = match self.state {
            State::Scanning => tr("Scanning…"),
            State::Joining => tr("Joining…"),
            State::Connected => tr("Connected"),
            State::Failed => self.error.clone(),
            _ if !on => tr("Wi-Fi is off"),
            _ if self.networks.is_empty() => tr("No networks found"),
            _ => String::new(),
        };
        if !message.is_empty() {
            rows.push(views::body_text(t, &message).with_key("wifi-message"));
        }
        if typing {
            rows.push(views::body_text(t, &tr("Network password")));
            rows.push(
                WidgetPod::new(
                    TextField::new()
                        .password(true)
                        .with_text(&self.password)
                        .placeholder(tr("Password"))
                        .on_change("wifi:password")
                        .on_submit("wifi:join"),
                )
                .focusable(true)
                .with_key("wifi-password"),
            );
            rows.push(super::search::keyboard());
            rows.push(views::compact(t, &tr("Join"), "wifi:join", "wifi-join"));
            rows.push(views::compact(t, &tr("Cancel"), "wifi:cancel", "wifi-cancel"));
        } else if !self.pending && on {
            let items: Vec<_> = self.networks.iter().cloned().enumerate().collect();
            rows.push(WidgetPod::new(Expanded::new(
                WidgetPod::new(Scroll::new(
                    items,
                    1,
                    68,
                    |(index, network): &(usize, ScanResult), t| {
                        let secure = network.flags.contains("WPA") || network.flags.contains("WEP");
                        let strength = if network.signal_dbm >= -55 {
                            tr("Strong")
                        } else if network.signal_dbm >= -75 {
                            tr("Good")
                        } else {
                            tr("Weak")
                        };
                        let label = if network.ssid.trim().is_empty() {
                            tr("Hidden network")
                        } else {
                            network.ssid.clone()
                        };
                        let line = Row::new(vec![
                            WidgetPod::new(Icon::new(
                                match (secure, network.signal_dbm) {
                                    (true, -55..) => "wifi.secure.100",
                                    (true, -75..) => "wifi.secure.50",
                                    (true, _) => "wifi.secure.25",
                                    (false, -55..) => "wifi.open.100",
                                    (false, -75..) => "wifi.open.50",
                                    (false, _) => "wifi.open.25",
                                },
                                t.scale.dp(24),
                                Tone::Ink,
                            )),
                            WidgetPod::new(Expanded::new(
                                views::compact(
                                    t,
                                    &label,
                                    &format!("wifi:network:{index}"),
                                    &format!("wifi-network-{index}"),
                                ),
                                1,
                            )),
                            WidgetPod::new(Label::new(strength, t.font.caption.clone(), Tone::Ink)),
                        ])
                        .gap(t.space.sm)
                        .cross_align(CrossAlign::Center);
                        WidgetPod::new(
                            Column::new(vec![
                                WidgetPod::new(line),
                                WidgetPod::new(kaia_ui::components::Divider::new(Tone::Divider)),
                            ])
                            .shrink(),
                        )
                    },
                ))
                .with_key("wifi-networks"),
                1,
            )));
        }
        if self.state == State::Failed && self.selected.is_some() {
            rows.push(views::compact(t, &tr("Try again"), "wifi:retry", "wifi-retry"));
        }
        rows.push(views::compact(
            t,
            &tr("Network settings"),
            "wifi:settings",
            "wifi-settings",
        ));
        let panel = WidgetPod::new(
            Frame::new(WidgetPod::new(Padding::new(
                WidgetPod::new(Column::new(rows).gap(t.space.sm).cross_align(CrossAlign::Stretch)),
                Insets::all(t.scale.dp(12)),
            )))
            .fill(Tone::Surface)
            .border(Tone::Ink, 2),
        )
        .with_key("wifi-popup");
        let overlay = Overlay::new(
            None,
            panel,
            if typing {
                Rect::new(12, 12, 608, 816)
            } else {
                Rect::new(162, 69, 465, 470)
            },
            "back",
        );
        Some(common::window(
            env.services,
            env.screen,
            SizedBox::new(Some(WidgetPod::new(if typing {
                overlay.dialog()
            } else {
                overlay
            }))),
        ))
    }
    fn on_event(&mut self, ev: &Event, env: &mut SurfaceEnv<'_>) -> Outcome {
        if ev.name == "ReaderTick" && self.worker.is_some() {
            return self.poll(env);
        }
        if ev.name == "ReaderTick" && self.pending && std::time::Instant::now() >= self.ready_at {
            self.work(env);
            return Outcome::Rebuild;
        }
        Outcome::NotMine
    }
    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        if self.worker.is_some() && cmd.starts_with("wifi:") {
            return Outcome::Handled;
        }
        match cmd {
            "wifi:toggle" => {
                self.password.clear();
                self.selected = None;
                if settings::keys::airplane_mode(&env.services.settings) {
                    self.fail(tr("Turn off airplane mode in Network settings."));
                } else {
                    self.state = State::Idle;
                    self.start(Request::Toggle, env);
                }
            }
            "wifi:scan" => {
                if env.services.platform.network.is_wifi_on() {
                    self.scan();
                }
            }
            "wifi:password" => {
                self.password = args.first().and_then(Value::as_str).unwrap_or("").into();
                return Outcome::Handled;
            }
            "wifi:join" => {
                if self.selected.is_some() {
                    self.state = State::Joining;
                    self.ready_at = std::time::Instant::now() + std::time::Duration::from_millis(250);
                    self.pending = true;
                }
            }
            "wifi:retry" => {
                self.password.clear();
                self.state = State::Password;
            }
            "wifi:cancel" => {
                self.password.clear();
                self.state = State::Idle;
            }
            "surface:back" if self.state == State::Password => {
                self.password.clear();
                self.state = State::Idle;
            }
            "wifi:settings" => {
                return Outcome::Navigate(Nav::Replace(Box::new(
                    settings::section::SettingsSectionSurface::new("network".into()),
                )))
            }
            _ => {
                if let Some(index) = cmd
                    .strip_prefix("wifi:network:")
                    .and_then(|v| v.parse::<usize>().ok())
                {
                    if let Some(network) = self.networks.get(index) {
                        self.selected = Some(index);
                        self.password.clear();
                        if network.flags.contains("WPA") {
                            self.state = State::Password;
                        } else if network.flags.contains("WEP") {
                            self.fail(tr("This network uses unsupported security."));
                        } else {
                            self.state = State::Joining;
                            self.ready_at = std::time::Instant::now() + std::time::Duration::from_millis(250);
                            self.pending = true;
                        }
                    }
                } else {
                    return Outcome::NotMine;
                }
            }
        }
        Outcome::Rebuild
    }
}
