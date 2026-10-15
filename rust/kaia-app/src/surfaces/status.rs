//! Shared application status row and the battery callout.

use chrono::Local;
use kaia_events::{ShowOpts, Value};
use kaia_gfx::{Point, Rect, Size};
use kaia_i18n::{format as tr_format, tr};
use kaia_input::Gesture;
use kaia_ui::components::{Column, CrossAlign, Divider, Icon, Label, Padding, SizedBox};
use kaia_ui::geometry::{Constraints, Insets};
use kaia_ui::{AppAction, EventCx, Handled, LayoutCx, PaintCx, Tone, UiEvent, Widget, WidgetPod};

use super::library::press::Pressable;
use super::{common, library, settings};
use crate::services::Services;
use crate::surface::{Nav, Outcome, SurfaceEnv};

/// The clock keeps its own text so ticks repaint only when the minute changes.
pub struct Clock {
    text: String,
    fixed: bool,
}

impl Clock {
    /// Pins a clock for a reproducible headless capture.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.fixed = true;
    }
}

impl Widget for Clock {
    kaia_ui::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(cx.theme.scale.dp(100), cx.theme.scale.dp(35)))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        cx.clear_background();
        let mut font = cx.theme.font.caption.clone();
        font.size = cx.theme.scale.dp(14) as f32 * cx.theme.text_scale;
        let text = cx.fonts.layout(
            &self.text,
            &cx.theme.style(&font, Tone::Ink),
            cx.bounds().w,
            Some(1),
            true,
        );
        let b = cx.bounds();
        cx.text(
            &text,
            Point::new(b.x, b.y + (b.h as i32 - text.height as i32) / 2),
        );
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        if matches!(ev, UiEvent::Named(ev) if ev.name == "ReaderTick") && !self.fixed {
            let text = Local::now().format("%H:%M").to_string();
            if text != self.text {
                self.text = text;
                cx.request_paint();
            }
        }
        Handled::No
    }
}

/// Full-width row with separate touch targets and no sync action.
pub struct StatusRow {
    clock: WidgetPod,
    actions: Vec<WidgetPod>,
}

pub fn row(services: &Services) -> WidgetPod {
    // Adjacent Nickel icons are closer than Kaia's minimum touch target.
    // Offset their ink inside separate targets to preserve both positions.
    let inset = |amount| {
        if services.theme.rtl {
            Insets {
                right: services.theme.scale.dp(amount),
                ..Insets::default()
            }
        } else {
            Insets {
                left: services.theme.scale.dp(amount),
                ..Insets::default()
            }
        }
    };
    let icons = vec![
        WidgetPod::new(Padding::new(
            WidgetPod::new(DeviceIcon::Light(Tone::Ink)),
            inset(19),
        )),
        WidgetPod::new(Padding::new(
            WidgetPod::new(Icon::new(
                "wifi",
                services.theme.scale.dp(25),
                if services.platform.network.is_wifi_on() {
                    Tone::Ink
                } else {
                    Tone::InkMuted
                },
            )),
            inset(10),
        )),
        WidgetPod::new(DeviceIcon::Battery(services.platform.battery.capacity())),
        WidgetPod::new(Icon::new("appbar.search", services.theme.scale.dp(25), Tone::Ink)),
    ];
    let actions = icons
        .into_iter()
        .zip(["light", "wifi", "battery", "search"])
        .map(|(icon, name)| {
            WidgetPod::new(Pressable::new(icon).tap(format!("status:{name}"), Vec::new()))
                .with_key(format!("status-{name}"))
                .focusable(true)
        })
        .collect();
    WidgetPod::new(StatusRow {
        clock: WidgetPod::new(Clock {
            text: Local::now().format("%H:%M").to_string(),
            fixed: false,
        })
        .with_key("status-clock"),
        actions,
    })
    .with_key("status-row")
}

impl Widget for StatusRow {
    kaia_ui::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let size = c.constrain(Size::new(c.max.w, t.scale.dp(69)));
        let touch = t.touch_target.min(size.h);
        let y = t.scale.dp(42) as i32 - touch as i32 / 2;
        self.clock
            .layout(cx, Constraints::loose(Size::new(size.w / 3, touch)));
        let clock_w = self.clock.rect().w;
        self.clock.set_origin(Point::new(
            if t.rtl {
                size.w as i32 - t.scale.dp(34) as i32 - clock_w as i32
            } else {
                t.scale.dp(34) as i32
            },
            t.scale.dp(25) as i32,
        ));
        // Reference centers: light 765, Wi-Fi 863, battery 960, search 1157.
        // Keep at least one touch target between centers on smaller screens.
        let pitch = t.scale.dp(49).max(touch);
        let right = size.w.saturating_sub(t.scale.dp(54));
        for (i, action) in self.actions.iter_mut().enumerate() {
            let offset = [
                t.scale.dp(98) + 2 * pitch,
                t.scale.dp(98) + pitch,
                t.scale.dp(98),
                0,
            ][i];
            let center = right.saturating_sub(offset);
            let x = center as i32 - touch as i32 / 2;
            let x = if t.rtl {
                size.w as i32 - x - touch as i32
            } else {
                x
            };
            action.layout(cx, Constraints::tight(Size::new(touch, touch)));
            action.set_origin(Point::new(x, y));
        }
        size
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        let mut children = vec![&mut self.clock];
        children.extend(self.actions.iter_mut());
        children
    }
}

enum DeviceIcon {
    Light(Tone),
    Battery(Option<u8>),
}

impl Widget for DeviceIcon {
    kaia_ui::impl_as_any!();

    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.max
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let center = Point::new(b.x + b.w as i32 / 2, b.y + b.h as i32 / 2);
        let ink = cx.color(Tone::Ink);
        let scale = cx.theme.scale;
        let dp = |v| scale.dp(v);
        let stroke = dp(1);
        match self {
            Self::Light(tone) => {
                let ink = cx.color(*tone);
                let radius = dp(6);
                let ray = dp(2);
                let distance = dp(10);
                cx.stroke_circle(center, radius as f32, stroke as f32, ink);
                for i in 0..8 {
                    let angle = i as f32 * std::f32::consts::FRAC_PI_4;
                    for d in distance..=dp(13) {
                        let p = Point::new(
                            center.x + (angle.cos() * d as f32) as i32,
                            center.y + (angle.sin() * d as f32) as i32,
                        );
                        cx.fill_circle(p, ray as f32 / 2.0, ink);
                    }
                }
            }
            Self::Battery(level) => {
                let w = dp(24);
                let h = dp(12);
                let body = Rect::new(center.x - w as i32 / 2, center.y - h as i32 / 2, w, h);
                let cap = Rect::new(body.right(), center.y - dp(3) as i32, dp(2), dp(6));
                let pad = dp(2);
                cx.border(body, 0, stroke, ink);
                cx.fill_rect(cap, ink);
                if let Some(level) = level {
                    let fill = (w - 2 * pad) * u32::from((*level).min(100)) / 100;
                    cx.fill_rect(
                        Rect::new(body.x + pad as i32, body.y + pad as i32, fill, h - 2 * pad),
                        ink,
                    );
                }
            }
        }
    }
}

pub(crate) fn command(id: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
    let section = match id {
        "status:light" | "status:wifi" => {
            let light = id == "status:light";
            let panel = if light {
                light_panel(env.services)
            } else {
                wifi_panel(env.services)
            };
            let rect = if light {
                Rect::new(182, 69, 401, 435)
            } else {
                Rect::new(162, 69, 465, 200)
            };
            let popup = library::widgets::Overlay::new(None, panel, rect, "dialog:dismiss");
            env.show(
                common::window(
                    env.services,
                    env.screen,
                    StatusCallout::new(popup, light, env.services),
                ),
                ShowOpts {
                    modal: true,
                    ..ShowOpts::default()
                },
            );
            return Outcome::Handled;
        }
        "status:light-settings" => "display",
        "status:network-settings" => "network",
        "status:auto-light" => {
            if let Some(on) = args.first().and_then(Value::as_bool) {
                env.services
                    .settings
                    .save(settings::keys::DISPLAY_NATURAL_LIGHT_ENABLED, Value::Bool(on));
                settings::save(env);
            }
            return Outcome::Handled;
        }
        "status:power" => "device",
        "status:search" => return Outcome::Navigate(Nav::Push(Box::new(library::LibrarySurface::search()))),
        "status:battery" => {
            let popup = BatteryPopup::new(env.services);
            let window = common::window(env.services, env.screen, popup);
            env.show(
                window,
                ShowOpts {
                    modal: true,
                    ..ShowOpts::default()
                },
            );
            return Outcome::Handled;
        }
        _ => return Outcome::NotMine,
    };
    Outcome::Navigate(Nav::Push(Box::new(
        settings::section::SettingsSectionSurface::new(section.into()),
    )))
}

struct BatteryPopup {
    panel: WidgetPod,
    bounds: Rect,
    anchor: Point,
    charge: Option<u8>,
}

impl BatteryPopup {
    fn new(services: &Services) -> Self {
        let t = &services.theme;
        let text = services.platform.battery.capacity().map_or_else(
            || tr("Battery unavailable"),
            |level| tr_format(&tr("Battery: %1%"), &[&level.to_string()]),
        );
        let mut font = t.font.body.clone();
        font.size = t.scale.dp(18) as f32 * t.text_scale;
        let line = |label: String, key: &str| {
            Padding::new(
                WidgetPod::new(Label::new(label, font.clone(), Tone::Ink)).with_key(key),
                Insets {
                    left: t.scale.dp(27),
                    top: t.scale.dp(18),
                    ..Insets::default()
                },
            )
        };
        let value = WidgetPod::new(
            SizedBox::new(Some(WidgetPod::new(line(text, "battery-charge-text"))))
                .height(t.scale.dp(59).saturating_sub(3)),
        )
        .with_key("battery-charge");
        let action = WidgetPod::new(
            Pressable::new(WidgetPod::new(
                SizedBox::new(Some(WidgetPod::new(line(
                    tr("Power settings"),
                    "battery-settings-text",
                ))))
                .height(t.scale.dp(59).saturating_sub(3)),
            ))
            .tap("status:power", Vec::new()),
        )
        .with_key("battery-settings")
        .focusable(true);
        let rule = WidgetPod::new(SizedBox::new(Some(WidgetPod::new(Divider::new(Tone::Divider)))).height(1));
        let panel = Column::new(vec![value, rule, action]).cross_align(CrossAlign::Stretch);
        Self {
            panel: WidgetPod::new(panel).with_key("battery-popup"),
            bounds: Rect::default(),
            anchor: Point::default(),
            charge: services.platform.battery.capacity(),
        }
    }
}

impl Widget for BatteryPopup {
    kaia_ui::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let w = t.scale.dp(317).min(c.max.w.saturating_sub(t.scale.dp(10)));
        let h = t.scale.dp(119);
        let x = c.max.w.saturating_sub(t.scale.dp(23) + w) as i32;
        let x = if t.rtl { c.max.w as i32 - x - w as i32 } else { x };
        self.bounds = Rect::new(x, t.scale.dp(69) as i32, w, h);
        let anchor_x = c.max.w.saturating_sub(t.scale.dp(54) + t.scale.dp(98));
        self.anchor = Point::new(
            if t.rtl {
                c.max.w as i32 - anchor_x as i32
            } else {
                anchor_x as i32
            },
            t.scale.dp(42) as i32,
        );
        self.panel.layout(
            cx,
            Constraints::tight(Size::new(w.saturating_sub(6), h.saturating_sub(6))),
        );
        self.panel.set_origin(Point::new(x + 3, self.bounds.y + 3));
        c.max
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let ink = cx.color(Tone::Ink);
        let paper = cx.color(Tone::Surface);
        cx.fill_circle(self.anchor, cx.theme.scale.dp(20) as f32, ink);
        let w = cx.theme.scale.dp(24);
        let h = cx.theme.scale.dp(12);
        let body = Rect::new(self.anchor.x - w as i32 / 2, self.anchor.y - h as i32 / 2, w, h);
        cx.border(body, 0, cx.theme.scale.dp(1), paper);
        let pad = cx.theme.scale.dp(2);
        cx.fill_rect(
            Rect::new(body.right(), self.anchor.y - (h / 4) as i32, pad, h / 2),
            paper,
        );
        if let Some(charge) = self.charge {
            cx.fill_rect(
                Rect::new(
                    body.x + pad as i32,
                    body.y + pad as i32,
                    (w - 2 * pad) * u32::from(charge.min(100)) / 100,
                    h - 2 * pad,
                ),
                paper,
            );
        }
        cx.fill_rect(self.bounds, paper);
        cx.border(self.bounds, 0, 3, ink);
        for dy in 0..10 {
            cx.fill_rect(
                Rect::new(
                    self.anchor.x - dy,
                    self.bounds.y - 10 + dy,
                    (2 * dy + 1) as u32,
                    1,
                ),
                ink,
            );
        }
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        if let UiEvent::Gesture(Gesture::Tap { pos, .. }) = ev {
            if !self.bounds.contains(*pos) {
                cx.action(AppAction::new("dialog:dismiss"));
            }
            return Handled::Yes;
        }
        Handled::No
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.panel]
    }
}

fn framed_panel(services: &Services, rows: Vec<WidgetPod>, key: &str) -> WidgetPod {
    use kaia_ui::components::Frame;
    let t = &services.theme;
    WidgetPod::new(
        Frame::new(WidgetPod::new(Padding::new(
            WidgetPod::new(Column::new(rows).gap(t.space.sm).cross_align(CrossAlign::Stretch)),
            Insets::all(t.scale.dp(25)),
        )))
        .fill(Tone::Surface)
        .border(Tone::Ink, 3),
    )
    .with_key(key)
}

fn light_panel(services: &Services) -> WidgetPod {
    use kaia_ui::composite::ListRow;
    let t = &services.theme;
    let enabled = settings::sections::has_frontlight(services);
    let slider = |value, action: &str, key: &str| {
        WidgetPod::new(
            crate::slider::Slider::new(value, 0.0, 100.0, 1.0)
                .format(|v| tr_format(&tr("%1%"), &[&v.to_string()]))
                .enabled(enabled)
                .on_action(action),
        )
        .with_key(key)
    };
    let rows = vec![
        WidgetPod::new(Label::new(tr("Brightness"), t.font.book_title.clone(), Tone::Ink)),
        slider(
            f64::from(services.platform.frontlight.intensity()),
            "settings:quick:frontlight",
            "quick-frontlight",
        ),
        WidgetPod::new(Divider::new(Tone::Divider)),
        WidgetPod::new(
            ListRow::new(tr("Natural light"))
                .toggle(settings::keys::bool_or(
                    &services.settings,
                    settings::keys::DISPLAY_NATURAL_LIGHT_ENABLED,
                    false,
                ))
                .on_action("status:auto-light"),
        )
        .with_key("light-auto"),
        slider(
            f64::from(services.platform.frontlight.warmth()),
            "settings:quick:warmth",
            "quick-warmth",
        ),
        WidgetPod::new(
            ListRow::new(tr("Bedtime"))
                .value(settings::keys::text_or(
                    &services.settings,
                    settings::keys::DISPLAY_NATURAL_LIGHT_BEDTIME,
                    "23:00",
                ))
                .on_action("status:light-settings"),
        )
        .with_key("light-bedtime"),
        common::button(
            kaia_ui::controls::ButtonStyle::Text,
            &tr("Light settings"),
            "status:light-settings",
            "light-settings",
        ),
    ];
    framed_panel(services, rows, "light-popup")
}

fn wifi_panel(services: &Services) -> WidgetPod {
    use kaia_ui::composite::ListRow;
    let blocked = settings::keys::airplane_mode(&services.settings);
    framed_panel(
        services,
        vec![
            WidgetPod::new(
                ListRow::new(tr("Wi-Fi"))
                    .toggle(services.platform.network.is_wifi_on())
                    .on_action(if blocked {
                        "settings:quick:wifi-blocked"
                    } else {
                        "settings:quick:wifi"
                    }),
            )
            .with_key("quick-wifi"),
            common::button(
                kaia_ui::controls::ButtonStyle::Text,
                &tr("Network settings"),
                "status:network-settings",
                "wifi-settings",
            ),
        ],
        "wifi-popup",
    )
}

struct StatusCallout {
    popup: WidgetPod,
    icon: WidgetPod,
    light: bool,
    center: Point,
}
impl StatusCallout {
    fn new(popup: library::widgets::Overlay, light: bool, services: &Services) -> Self {
        Self {
            popup: WidgetPod::new(popup),
            light,
            center: Point::default(),
            icon: if light {
                WidgetPod::new(DeviceIcon::Light(Tone::Surface))
            } else {
                WidgetPod::new(
                    kaia_ui::components::Frame::new(WidgetPod::new(Icon::new(
                        "wifi",
                        services.theme.scale.dp(25),
                        Tone::Surface,
                    )))
                    .fill(Tone::Ink),
                )
            },
        }
    }
}
impl Widget for StatusCallout {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        self.popup.layout(cx, Constraints::tight(c.max));
        let dp = |v| cx.theme.scale.dp(v);
        let x = c.max.w.saturating_sub(dp(if self.light { 249 } else { 200 })) as i32;
        self.center = Point::new(if cx.theme.rtl { c.max.w as i32 - x } else { x }, dp(42) as i32);
        let side = dp(28);
        self.icon.layout(cx, Constraints::tight(Size::new(side, side)));
        self.icon.set_origin(Point::new(
            self.center.x - side as i32 / 2,
            self.center.y - side as i32 / 2,
        ));
        c.max
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let ink = cx.color(Tone::Ink);
        cx.fill_circle(self.center, cx.theme.scale.dp(20) as f32, ink);
        let y = cx.theme.scale.dp(69) as i32;
        for dy in 0..10 {
            cx.fill_rect(
                Rect::new(self.center.x - dy, y - 10 + dy, (2 * dy + 1) as u32, 1),
                ink,
            );
        }
    }
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.popup, &mut self.icon]
    }
}
