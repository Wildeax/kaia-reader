//! Anchored light controls. General device actions stay in Quick settings.
use kaia_events::Value;
use kaia_gfx::{Point, Rect, Size};
use kaia_i18n::{format as tr_format, tr};
use kaia_input::Gesture;
use kaia_ui::geometry::Constraints;
use kaia_ui::widget::{AppAction, EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};
use kaia_ui::{Tone, WidgetPod};

use super::widgets::{px, style};
use super::{keys, sections};
use crate::services::Services;
use crate::slider::Slider;

pub struct LightPanel {
    brightness: WidgetPod,
    warmth: WidgetPod,
    auto: bool,
    bedtime: u32,
    choosing: bool,
    panel: Rect,
    reader: bool,
    anchor_x: Option<i32>,
}

impl LightPanel {
    pub fn new(services: &Services, reader: bool) -> Self {
        let enabled = sections::has_frontlight(services);
        Self {
            brightness: WidgetPod::new(
                Slider::new(
                    f64::from(services.platform.frontlight.intensity()),
                    0.0,
                    100.0,
                    1.0,
                )
                .format(|v| tr_format(&tr("%1%"), &[&v.to_string()]))
                .light_style(false)
                .enabled(enabled)
                .on_action("settings:quick:frontlight"),
            )
            .with_key("light-brightness")
            .focusable(true),
            warmth: WidgetPod::new(
                Slider::new(f64::from(services.platform.frontlight.warmth()), 0.0, 100.0, 1.0)
                    .light_style(true)
                    .enabled(enabled)
                    .on_action("settings:quick:warmth"),
            )
            .with_key("light-warmth")
            .focusable(true),
            auto: keys::bool_or(&services.settings, keys::DISPLAY_NATURAL_LIGHT_ENABLED, false),
            bedtime: keys::number_or(&services.settings, keys::DISPLAY_NATURAL_LIGHT_BEDTIME, 1380.0)
                .round()
                .clamp(0.0, 1439.0) as u32,
            choosing: false,
            panel: Rect::default(),
            reader,
            anchor_x: None,
        }
    }
    pub fn anchor(mut self, x: Option<i32>) -> Self {
        self.anchor_x = x;
        self
    }

    fn text(cx: &mut PaintCx, text: &str, x: i32, y: i32, size: u32, serif: bool, width: u32) {
        let mut font = style(cx.theme, size);
        if serif {
            font.family = "Noto Serif".into();
        }
        let layout = cx.fonts.layout(text, &font, px(cx.theme, width), Some(1), true);
        cx.text(&layout, Point::new(x, y));
    }
}

impl Widget for LightPanel {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let w = px(t, 803).min(c.max.w);
        let h = px(t, 869).min(c.max.h);
        let x = if self.reader {
            c.max.w.saturating_sub(w + px(t, 10))
        } else {
            px(t, 363).min(c.max.w.saturating_sub(w))
        };
        let y = px(t, if self.reader { 182 } else { 139 }).min(c.max.h.saturating_sub(h));
        self.panel = Rect::new(x as i32, y as i32, w, h);
        for (child, top) in [(&mut self.brightness, 111), (&mut self.warmth, 453)] {
            child.set_visible(!self.choosing);
            child.layout(
                cx,
                Constraints::tight(Size::new(w.saturating_sub(px(t, 104)), px(t, 180))),
            );
            child.set_origin(Point::new((x + px(t, 52)) as i32, (y + px(t, top)) as i32));
        }
        c.max
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = self.panel.translate(cx.bounds().x, cx.bounds().y);
        let t = cx.theme;
        let ink = cx.color(Tone::Ink);
        cx.fill_rect(b, cx.color(Tone::Surface));
        cx.border(b, 0, px(t, 3).max(1), ink);
        // A short callout points at the toolbar's light control.
        let tip_x = self
            .anchor_x
            .unwrap_or(b.x + px(t, 480) as i32)
            .clamp(b.x + px(t, 20) as i32, b.right() - px(t, 20) as i32);
        for n in 0..px(t, 10) {
            let span = 2 * n + 1;
            cx.fill_rect(
                Rect::new(tip_x - n as i32, b.y - px(t, 10) as i32 + n as i32, span, 1),
                ink,
            );
        }
        let x = b.x + px(t, 52) as i32;
        if self.choosing {
            Self::text(cx, &tr("Bedtime"), x, b.y + px(t, 45) as i32, 42, true, 700);
            // Half-hour choices cover the schedule's supported evening/night range.
            for (i, minutes) in bedtime_choices().enumerate() {
                let col = i % 3;
                let row = i / 3;
                let text = format!("{:02}:{:02}", minutes / 60, minutes % 60);
                let cell = Rect::new(
                    x + px(t, col as u32 * 230) as i32,
                    b.y + px(t, 135 + row as u32 * 92) as i32,
                    px(t, 205),
                    px(t, 82),
                );
                cx.border(
                    cell,
                    0,
                    px(t, if minutes == self.bedtime { 3 } else { 1 }).max(1),
                    if minutes == self.bedtime {
                        cx.color(Tone::Accent)
                    } else {
                        ink
                    },
                );
                Self::text(
                    cx,
                    &text,
                    cell.x + px(t, 30) as i32,
                    cell.y + px(t, 18) as i32,
                    34,
                    false,
                    700,
                );
            }
            return;
        }
        Self::text(cx, &tr("Brightness"), x, b.y + px(t, 56) as i32, 42, true, 700);
        Self::text(
            cx,
            &tr("Natural light"),
            x,
            b.y + px(t, 398) as i32,
            42,
            true,
            480,
        );
        Self::text(
            cx,
            &tr("Auto"),
            b.x + px(t, 553) as i32,
            b.y + px(t, 404) as i32,
            30,
            false,
            90,
        );
        let switch = Rect::new(
            b.x + px(t, 659) as i32,
            b.y + px(t, 397) as i32,
            px(t, 88),
            px(t, 49),
        );
        cx.fill_rrect(
            switch,
            px(t, 24),
            if self.auto {
                cx.color(Tone::Accent)
            } else {
                cx.color(Tone::Surface)
            },
        );
        cx.border(switch, px(t, 24), px(t, 2).max(1), ink);
        let knob = Point::new(
            switch.x + px(t, if self.auto { 63 } else { 25 }) as i32,
            switch.y + px(t, 24) as i32,
        );
        cx.fill_circle(knob, px(t, 21) as f32, cx.color(Tone::Surface));
        cx.stroke_circle(knob, px(t, 16) as f32, px(t, 1).max(1) as f32, ink);
        Self::text(cx, &tr("Bedtime"), x, b.y + px(t, 674) as i32, 36, true, 225);
        Self::text(
            cx,
            &format!("{:02}:{:02}", self.bedtime / 60, self.bedtime % 60),
            b.x + px(t, 297) as i32,
            b.y + px(t, 674) as i32,
            36,
            true,
            110,
        );
        if let Ok(icon) = cx
            .icons
            .get("settings.down", Size::new(px(t, 30), px(t, 30)), ink)
        {
            cx.blit_alpha(
                &icon,
                Point::new(b.x + px(t, 408) as i32, b.y + px(t, 687) as i32),
            );
        }
        for y in [342, 767] {
            cx.fill_rect(
                Rect::new(b.x + 1, b.y + px(t, y) as i32, b.w.saturating_sub(2), 1),
                cx.color(Tone::InkMuted),
            );
        }
        Self::text(
            cx,
            &tr("Quick settings"),
            b.x + px(t, 36) as i32,
            b.y + px(t, 795) as i32,
            36,
            true,
            700,
        );
    }
    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        if let UiEvent::Gesture(Gesture::Tap { pos, .. }) = ev {
            let b = self.panel.translate(cx.bounds().x, cx.bounds().y);
            if !b.contains(*pos) {
                if let Some(id) = cx.window_id() {
                    cx.ctx().close(id, None);
                }
                return Handled::Yes;
            }
            let unit = |value: i32| {
                (i64::from(value) * 1264
                    / i64::from(cx.theme().scale.width.min(cx.theme().scale.height).max(1)))
                    as u32
            };
            let x = unit(pos.x - b.x);
            let y = unit(pos.y - b.y);
            if self.choosing {
                if (135..779).contains(&y) && (52..742).contains(&x) {
                    let index = ((y - 135) / 92 * 3 + (x - 52) / 230) as usize;
                    if let Some(minutes) = bedtime_choices().nth(index) {
                        self.bedtime = minutes;
                        self.choosing = false;
                        cx.action(AppAction {
                            id: "settings:quick:bedtime".into(),
                            args: vec![Value::Number(f64::from(minutes))],
                        });
                        cx.request_layout();
                    }
                }
            } else if (365..475).contains(&y) && x >= 530 {
                self.auto = !self.auto;
                cx.action(AppAction {
                    id: "settings:quick:auto".into(),
                    args: vec![Value::Bool(self.auto)],
                });
                cx.request_paint();
            } else if (650..756).contains(&y) {
                self.choosing = true;
                cx.request_layout();
            } else if y >= 767 {
                if let Some(id) = cx.window_id() {
                    cx.ctx().close(id, None);
                }
                cx.action(AppAction {
                    id: "settings:quick:open".into(),
                    args: vec![],
                });
            }
        }
        if matches!(ev, UiEvent::Gesture(_)) {
            Handled::Yes
        } else {
            Handled::No
        }
    }
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        vec![&mut self.brightness, &mut self.warmth]
    }
}

fn bedtime_choices() -> impl Iterator<Item = u32> {
    (0..20).map(|i| (19 * 60 + i * 30) % 1440)
}
