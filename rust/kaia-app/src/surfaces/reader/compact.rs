//! Compact reader controls, measured at 1264 by 1680 and scaled with the screen.
use super::chrome::{rect, Chrome, Tool};
use kaia_gfx::{Color, Point, Rect, Size};
use kaia_ui::{Constraints, LayoutCx, PaintCx, Tone, Widget, WidgetPod};

pub(super) fn text(screen: Size, label: &str) -> WidgetPod {
    WidgetPod::new(kaia_ui::components::Label::new(
        label,
        kaia_ui::theme::FontSpec {
            family: "Noto Sans",
            size: 34.0 * screen.w.min(screen.h) as f32 / 1264.0,
            weight: 400,
        },
        Tone::Ink,
    ))
}
pub(super) fn action(screen: Size, label: &str, id: &str, key: &str) -> WidgetPod {
    let mut tool = Tool::new("", id, 48 * screen.w.min(screen.h) / 1264);
    tool.label = label.into();
    WidgetPod::new(tool).focusable(true).with_key(key)
}
pub(super) fn icon(screen: Size, name: &str, id: &str, key: &str) -> WidgetPod {
    WidgetPod::new(Tool::new(name, id, 48 * screen.w.min(screen.h) / 1264))
        .focusable(true)
        .with_key(key)
}
pub(super) fn put(chrome: &mut Chrome, screen: Size, bounds: [u32; 4], pod: WidgetPod) {
    let [x, y, w, h] = bounds;
    chrome.children.push((rect(screen, x, y, w, h), pod));
}
pub(super) fn rule(chrome: &mut Chrome, screen: Size, bounds: [u32; 4], tone: Tone) {
    put(
        chrome,
        screen,
        bounds,
        WidgetPod::new(
            kaia_ui::components::Frame::new(WidgetPod::new(kaia_ui::components::SizedBox::new(None)))
                .fill(tone),
        ),
    );
}
pub(super) fn dialog(screen: Size) -> Chrome {
    let mut chrome = Chrome {
        panel_border: 1,
        size: screen,
        children: vec![],
        panels: vec![],
    };
    put(&mut chrome, screen, [0, 0, 1264, 1680], WidgetPod::new(Surround));
    put(
        &mut chrome,
        screen,
        [25, 25, 1214, 1630],
        WidgetPod::new(
            kaia_ui::components::Frame::new(WidgetPod::new(kaia_ui::components::SizedBox::new(None)))
                .fill(Tone::Surface)
                .border(Tone::Ink, 1),
        ),
    );
    chrome
}
pub(super) fn heading(chrome: &mut Chrome, screen: Size, title: &str) {
    use kaia_ui::geometry::Alignment;
    put(
        chrome,
        screen,
        [140, 45, 984, 75],
        WidgetPod::new(kaia_ui::components::Align::new(
            text(screen, title),
            Alignment::Center,
            Alignment::Center,
        )),
    );
    put(
        chrome,
        screen,
        [1135, 40, 96, 85],
        icon(screen, "close", "reader:panel:close", "panel-close"),
    );
}
struct Surround;
impl Widget for Surround {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _: &mut LayoutCx, c: Constraints) -> Size {
        c.max
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        cx.fill_rect(cx.bounds(), Color::rgb(77, 77, 77));
    }
}

pub(super) struct Battery {
    pub percent: Option<u8>,
}
impl Widget for Battery {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _: &mut LayoutCx, c: Constraints) -> Size {
        c.max
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let line = (b.h / 14).max(1);
        let ink = cx.color(Tone::Ink);
        let body = Rect::new(b.x + (b.w / 10) as i32, b.y, b.w * 9 / 10, b.h);
        cx.border(body, 0, line, ink);
        cx.fill_rect(Rect::new(b.x, b.y + (b.h / 3) as i32, b.w / 10, b.h / 3), ink);
        if let Some(percent) = self.percent {
            let pad = line * 3;
            let width = body.w.saturating_sub(2 * pad) * u32::from(percent.min(100)) / 100;
            cx.fill_rect(
                Rect::new(
                    body.right() - pad as i32 - width as i32,
                    body.y + pad as i32,
                    width,
                    body.h.saturating_sub(2 * pad),
                ),
                ink,
            );
        } else {
            let mut font = cx.theme.font.caption.clone();
            font.size = b.h as f32 * 0.8;
            let style = cx.theme.style(&font, Tone::Ink);
            let text = cx.fonts.layout("?", &style, body.w, Some(1), false);
            cx.text(
                &text,
                Point::new(body.x + (body.w as i32 - text.width as i32) / 2, body.y),
            );
        }
    }
}

/// A slider and its two endpoint buttons share the same numeric range.
pub(super) fn slider(
    chrome: &mut Chrome,
    screen: Size,
    y: u32,
    value: f64,
    range: [f64; 3],
    id: &str,
    key: &str,
) {
    let [min, max, step] = range;
    put(
        chrome,
        screen,
        [495, y - 48, 642, 96],
        WidgetPod::new(
            crate::slider::Slider::new(value, min, max, step)
                .reader()
                .on_action(id),
        )
        .with_key(key),
    );
    for (x, label, delta, suffix) in [(412, "−", -step, "minus"), (1131, "+", step, "plus")] {
        let mut tool = Tool::new("", id, 48 * screen.w.min(screen.h) / 1264);
        tool.label = label.into();
        tool.centered = true;
        tool.args = vec![kaia_events::Value::Number((value + delta).clamp(min, max))];
        put(
            chrome,
            screen,
            [x, y - 44, 88, 88],
            WidgetPod::new(tool)
                .focusable(true)
                .with_key(format!("{key}-{suffix}")),
        );
    }
}

pub(super) fn dropdown(screen: Size, label: &str, id: &str, key: &str) -> WidgetPod {
    let mut tool = Tool::new("", id, 48 * screen.w.min(screen.h) / 1264);
    tool.label = label.into();
    tool.trailing = Some("chevron.down".into());
    tool.border = true;
    WidgetPod::new(tool).focusable(true).with_key(key)
}

pub(super) fn flat(screen: Size, label: &str, id: &str, key: &str) -> WidgetPod {
    let mut tool = Tool::new("", id, 48 * screen.w.min(screen.h) / 1264);
    tool.label = label.into();
    tool.fill = true;
    tool.centered = true;
    WidgetPod::new(tool).focusable(true).with_key(key)
}
