//! Reader controls placed in capture-relative rectangles, scaled to the viewport.
use kaia_gfx::{Point, Rect, Size};
use kaia_ui::{Constraints, LayoutCx, PaintCx, Tone, Widget, WidgetPod};

pub(super) struct Chrome {
    pub size: Size,
    pub children: Vec<(Rect, WidgetPod)>,
    pub panels: Vec<Rect>,
    pub panel_border: u32,
}
impl Widget for Chrome {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        for (rect, child) in &mut self.children {
            child.layout(cx, Constraints::tight(rect.size()));
            child.set_origin(Point::new(rect.x, rect.y));
        }
        c.constrain(self.size)
    }
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.children.iter_mut().map(|(_, child)| child).collect()
    }
    fn event(&mut self, cx: &mut kaia_ui::EventCx, event: &kaia_ui::UiEvent) -> kaia_ui::Handled {
        if !self
            .children
            .iter()
            .any(|(_, child)| child.key() == Some("contents-list"))
        {
            return kaia_ui::Handled::No;
        }
        match event {
            kaia_ui::UiEvent::Named(e) if e.name == "ReaderPanelFocus" => {
                cx.request_focus();
                kaia_ui::Handled::Yes
            }
            kaia_ui::UiEvent::Key(k) if k.pressed => {
                let next = match k.key {
                    kaia_input::Key::RPgFwd | kaia_input::Key::LPgFwd => true,
                    kaia_input::Key::RPgBack | kaia_input::Key::LPgBack => false,
                    _ => return kaia_ui::Handled::No,
                };
                cx.action(kaia_ui::AppAction::new(if next {
                    "reader:contents:page:next"
                } else {
                    "reader:contents:page:previous"
                }));
                kaia_ui::Handled::Yes
            }
            _ => kaia_ui::Handled::No,
        }
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let origin = cx.bounds();
        for panel in &self.panels {
            let r = Rect::new(origin.x + panel.x, origin.y + panel.y, panel.w, panel.h);
            cx.fill_rect(r, cx.color(Tone::Surface));
            if panel.x == 0 && panel.w == self.size.w {
                if panel.y > 0 {
                    cx.fill_rect(Rect::new(r.x, r.y, r.w, 1), cx.color(Tone::Ink));
                }
                if panel.bottom() < self.size.h as i32 {
                    cx.fill_rect(Rect::new(r.x, r.bottom() - 1, r.w, 1), cx.color(Tone::Ink));
                }
            } else {
                cx.border(r, 0, self.panel_border, cx.color(Tone::Ink));
            }
        }
    }
}
pub(super) fn rect(screen: Size, x: u32, y: u32, w: u32, h: u32) -> Rect {
    Rect::new(
        (x * screen.w / 1264) as i32,
        (y * screen.h / 1680) as i32,
        w * screen.w / 1264,
        h * screen.h / 1680,
    )
}

/// Icon-only reader tool with a circular pressed state and an optional short label.
pub(super) struct Tool {
    pub icon: String,
    pub label: String,
    pub action: String,
    pub active: bool,
    pub pressed: bool,
    pub clear: bool,
    pub trailing: Option<String>,
    pub centered: bool,
    pub face: Option<String>,
    pub bold: bool,
    pub fill: bool,
    pub border: bool,
    pub args: Vec<kaia_events::Value>,
    pub px: u32,
}
impl Tool {
    pub fn new(icon: &str, action: &str, px: u32) -> Self {
        Self {
            icon: icon.into(),
            label: String::new(),
            action: action.into(),
            active: false,
            pressed: false,
            clear: true,
            trailing: None,
            centered: false,
            face: None,
            bold: false,
            fill: false,
            border: false,
            args: Vec::new(),
            px,
        }
    }
}
impl Widget for Tool {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(c.max)
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        if self.clear {
            cx.clear_background();
        }
        let center = Point::new(b.x + b.w as i32 / 2, b.y + b.h as i32 / 2);
        if self.fill {
            cx.fill_rect(b, cx.color(Tone::SurfaceAlt));
        }
        if self.border {
            cx.border(b, 0, 2, cx.color(Tone::Ink));
        }
        if self.active || self.pressed {
            if self.icon.is_empty() && !self.centered {
                cx.fill_rect(b, cx.color(Tone::Ink));
            } else {
                cx.fill_circle(center, self.px as f32 * 0.73, cx.color(Tone::Ink));
            }
        }
        let tone = if self.active || self.pressed {
            Tone::Surface
        } else {
            Tone::Ink
        };
        let at = if self.label.is_empty() {
            Point::new(center.x - self.px as i32 / 2, center.y - self.px as i32 / 2)
        } else {
            Point::new(b.x, center.y - self.px as i32 / 2)
        };
        if !self.icon.is_empty() {
            if let Ok(mask) = cx
                .icons
                .get(&self.icon, Size::new(self.px, self.px), cx.color(tone))
            {
                cx.blit_alpha(&mask, at);
            }
        }
        if !self.label.is_empty() {
            let mut font = cx.theme.font.body_bold.clone();
            font.size = self.px as f32 * 0.72;
            if self.icon.is_empty() {
                font.weight = 400;
            }
            let indent = if self.centered {
                0
            } else if self.icon.is_empty() {
                self.px / 2
            } else {
                self.px * 3 / 2
            };
            if self.bold {
                font.weight = 700;
            }
            let mut style = cx.theme.style(&font, tone);
            if let Some(face) = &self.face {
                style.family = face.clone();
            }
            let text = cx.fonts.layout(
                &self.label,
                &style,
                b.w.saturating_sub(indent + 12 + if self.trailing.is_some() { self.px } else { 0 }),
                Some(1),
                true,
            );
            cx.text(
                &text,
                Point::new(
                    if self.centered {
                        center.x - text.width as i32 / 2
                    } else {
                        b.x + indent as i32
                    },
                    center.y - text.height as i32 / 2,
                ),
            );
        }
        if let Some(name) = &self.trailing {
            let px = self.px * 2 / 3;
            if let Ok(mask) = cx.icons.get(name, Size::new(px, px), cx.color(tone)) {
                cx.blit_alpha(
                    &mask,
                    Point::new(b.right() - self.px as i32, center.y - px as i32 / 2),
                );
            }
        }
    }
    fn event(&mut self, cx: &mut kaia_ui::EventCx, event: &kaia_ui::UiEvent) -> kaia_ui::Handled {
        let activate = match event {
            kaia_ui::UiEvent::Gesture(kaia_input::Gesture::Tap { .. }) => true,
            kaia_ui::UiEvent::Key(k) => k.pressed && k.key == kaia_input::Key::Press && cx.is_focused(),
            _ => false,
        };
        if !activate {
            return kaia_ui::Handled::No;
        }
        cx.request_focus_if_focusable();
        if self.action.starts_with("reader:tool:") || self.action == "settings:reading" {
            self.active = true;
        }
        self.pressed = true;
        cx.request_paint();
        cx.schedule_release::<Tool>(|tool, cx| {
            tool.pressed = false;
            cx.request_paint();
        });
        cx.action(kaia_ui::AppAction {
            id: self.action.clone(),
            args: self.args.clone(),
        });
        kaia_ui::Handled::Yes
    }
}
