//! Compact controls and whole-page scrolling used only by Settings.

use kaia_events::Value;
use kaia_gfx::text::TextStyle;
use kaia_gfx::{Color, Point, Rect, Size};
use kaia_i18n::{format as tr_format, tr};
use kaia_input::{Gesture, Key};
use kaia_ui::geometry::Constraints;
use kaia_ui::widget::{AppAction, EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};
use kaia_ui::{Theme, Tone, WidgetPod};

use super::entries::{self, Entry};

fn draw_icon_at(cx: &mut PaintCx, name: &str, size: u32, color: Color, at: Point) {
    if let Ok(mask) = cx.icons.get(name, Size::new(size, size), color) {
        cx.blit_alpha(&mask, at);
    }
}

pub fn px(t: &Theme, n: u32) -> u32 {
    ((u64::from(n) * u64::from(t.scale.width.min(t.scale.height)) + 632) / 1264) as u32
}

pub fn style(t: &Theme, n: u32) -> TextStyle {
    let mut s = TextStyle::new(px(t, n) as f32 * t.text_scale);
    s.family = "Noto Sans".into();
    s.color = t.color(Tone::Ink);
    s
}

pub struct CompactRow {
    label: String,
    secondary: Option<String>,
    value: Option<String>,
    checked: Option<bool>,
    action: Option<String>,
    enabled: bool,
    chevron: bool,
    dropdown: bool,
}

impl CompactRow {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            secondary: None,
            value: None,
            checked: None,
            action: None,
            enabled: true,
            chevron: false,
            dropdown: false,
        }
    }
    pub fn is_on(&self) -> bool {
        self.checked == Some(true)
    }
    pub fn secondary(mut self, text: impl Into<String>) -> Self {
        self.secondary = Some(text.into());
        self
    }
    pub fn value(mut self, text: impl Into<String>) -> Self {
        self.value = Some(text.into());
        self
    }
    pub fn toggle(mut self, on: bool) -> Self {
        self.checked = Some(on);
        self
    }
    pub fn checkbox(self, on: bool) -> Self {
        self.toggle(on)
    }
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
    pub fn chevron(mut self) -> Self {
        self.chevron = true;
        self
    }
    pub fn dropdown(mut self) -> Self {
        self.chevron = true;
        self.dropdown = true;
        self
    }
    pub fn on_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }
    fn label_width(&self, t: &Theme, width: u32) -> u32 {
        let reserved = if self.checked.is_some() {
            px(t, 278)
        } else if self.value.is_some() {
            width * 45 / 100 + if self.chevron { px(t, 50) } else { 0 }
        } else if self.chevron {
            px(t, 80)
        } else {
            0
        };
        width.saturating_sub(reserved + px(t, 50)).max(1)
    }
}

impl Widget for CompactRow {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let label = cx.fonts.layout(
            &self.label,
            &style(t, 34),
            self.label_width(t, c.max.w),
            None,
            false,
        );
        let secondary = self.secondary.as_ref().map_or(0, |text| {
            cx.fonts
                .layout(
                    text,
                    &style(t, 30),
                    c.max.w.saturating_sub(px(t, 50)),
                    None,
                    false,
                )
                .height
                + px(t, 12)
        });
        let value_h = self.value.as_ref().map_or(0, |value| {
            let mut s = style(t, 34);
            s.italic = true;
            cx.fonts
                .layout(value, &s, c.max.w * 43 / 100, Some(2), true)
                .height
        });
        c.constrain(Size::new(
            c.max.w,
            px(t, 106).max((label.height + secondary).max(value_h) + px(t, 28)),
        ))
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let t = cx.theme;
        let color = if self.enabled {
            cx.color(Tone::Ink)
        } else {
            cx.color(Tone::InkMuted)
        };
        let mut s = style(t, 34);
        s.color = color;
        let label = cx
            .fonts
            .layout(&self.label, &s, self.label_width(t, b.w), None, false);
        let secondary = self.secondary.as_ref().map(|text| {
            cx.fonts
                .layout(text, &style(t, 30), b.w.saturating_sub(px(t, 50)), None, false)
        });
        let block = label.height + secondary.as_ref().map_or(0, |v| v.height + px(t, 12));
        let y = b.y + (b.h.saturating_sub(block) / 2) as i32;
        cx.text(&label, Point::new(b.x + px(t, 25) as i32, y));
        if let Some(text) = secondary {
            cx.text(
                &text,
                Point::new(b.x + px(t, 25) as i32, y + label.height as i32 + px(t, 12) as i32),
            );
        }
        let mid = if self.secondary.is_some() {
            y + label.height as i32 / 2
        } else {
            b.y + b.h as i32 / 2
        };
        if let Some(on) = self.checked {
            let side = px(t, 38);
            let rect = Rect::new(b.right() - px(t, 228) as i32, mid - side as i32 / 2, side, side);
            cx.border(rect, 0, px(t, 2).max(1), color);
            if on {
                draw_icon_at(cx, "check", side, color, Point::new(rect.x, rect.y));
            }
            let state = cx.fonts.layout(
                &if on { tr("On") } else { tr("Off") },
                &s,
                px(t, 155),
                Some(1),
                true,
            );
            cx.text(
                &state,
                Point::new(rect.x + px(t, 63) as i32, mid - state.height as i32 / 2),
            );
        } else if let Some(value) = &self.value {
            s.italic = true;
            let value = cx.fonts.layout(value, &s, b.w * 43 / 100, Some(2), true);
            let right = b.right() - px(t, if self.chevron { 76 } else { 25 }) as i32;
            cx.text(
                &value,
                Point::new(right - value.width as i32, mid - value.height as i32 / 2),
            );
        }
        if self.chevron {
            draw_icon_at(
                cx,
                if self.dropdown {
                    "settings.down"
                } else if t.rtl {
                    "chevron.left"
                } else {
                    "chevron.right"
                },
                px(t, 30),
                color,
                Point::new(b.right() - px(t, 53) as i32, mid - px(t, 15) as i32),
            );
        }
        cx.fill_rect(Rect::new(b.x, b.bottom() - 1, b.w, 1), cx.color(Tone::Ink));
        if self.action.is_some() {
            cx.focus_ring(0);
        }
    }
    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        if matches!(ev, UiEvent::Gesture(Gesture::Tap { .. }))
            || matches!(ev, UiEvent::Key(k) if k.pressed && k.key == Key::Press && cx.is_focused())
        {
            if self.enabled {
                if let Some(action) = &self.action {
                    if let Some(on) = &mut self.checked {
                        *on = !*on;
                    }
                    let args = self.checked.map(|on| vec![Value::Bool(on)]).unwrap_or_default();
                    cx.request_paint();
                    cx.action(AppAction {
                        id: action.clone(),
                        args,
                    });
                }
            }
            return Handled::Yes;
        }
        Handled::No
    }
}

pub struct SectionBar(pub String);
impl Widget for SectionBar {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(
            c.max.w,
            px(cx.theme, 49).max((px(cx.theme, 40) as f32 * cx.theme.text_scale) as u32),
        ))
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        cx.fill_rect(b, Color::gray(0xe5));
        let text = cx.fonts.layout(
            &self.0,
            &style(cx.theme, 28),
            b.w.saturating_sub(px(cx.theme, 50)),
            Some(1),
            true,
        );
        cx.text(
            &text,
            Point::new(
                b.x + px(cx.theme, 25) as i32,
                b.y + (b.h.saturating_sub(text.height) / 2) as i32,
            ),
        );
    }
}

/// Measures every row, keeps headings with their next row and never draws a disabled pager.
pub struct SettingsPages {
    entries: Vec<Entry>,
    rows: Vec<WidgetPod>,
    page: usize,
    pages: usize,
}
impl SettingsPages {
    pub fn page_count(&self) -> usize {
        self.pages
    }
    pub fn new(entries: Vec<Entry>, page: usize) -> Self {
        Self {
            entries,
            rows: vec![],
            page,
            pages: 1,
        }
    }
}
impl Widget for SettingsPages {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let top = px(cx.theme, 90);
        let width = c.max.w.saturating_sub(px(cx.theme, 48));
        let avail = c.max.h.saturating_sub(top);
        let row_c = Constraints {
            min: Size::new(width, 0),
            max: Size::new(width, avail),
        };
        let mut rows: Vec<_> = self
            .entries
            .iter()
            .map(|e| entries::widget(e, cx.theme))
            .collect();
        let heights: Vec<_> = rows.iter_mut().map(|r| r.layout(cx, row_c).h).collect();
        let mut starts = vec![0];
        let mut used = 0;
        for (i, h) in heights.iter().copied().enumerate() {
            let keep = if matches!(self.entries[i], Entry::Heading(_)) {
                heights.get(i + 1).copied().unwrap_or(0)
            } else {
                0
            };
            if (used + h + keep > avail || matches!(self.entries[i], Entry::PageBreak))
                && i > *starts.last().unwrap()
            {
                starts.push(i);
                used = 0;
            }
            used += h;
        }
        self.pages = starts.len();
        self.page = self.page.min(self.pages - 1);
        let start = starts[self.page];
        let end = starts.get(self.page + 1).copied().unwrap_or(rows.len());
        self.rows = rows.drain(start..end).collect();
        let mut y = top as i32;
        for (row, height) in self.rows.iter_mut().zip(&heights[start..end]) {
            row.set_origin(Point::new(0, y));
            y += *height as i32;
        }
        c.max
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        if self.pages <= 1 {
            return;
        }
        let b = cx.bounds();
        let t = cx.theme;
        let mut s = style(t, 31);
        s.italic = true;
        let text = tr_format(
            &tr("Page %1 of %2"),
            &[&(self.page + 1).to_string(), &self.pages.to_string()],
        );
        let text = cx.fonts.layout(&text, &s, px(t, 240), Some(1), true);
        cx.text(
            &text,
            Point::new(b.right() - px(t, 244) as i32, b.y + px(t, 20) as i32),
        );
        let top = px(t, 108);
        let h = b.h.saturating_sub(top);
        let thumb = h / self.pages as u32;
        let x = b.right() - px(t, 12) as i32;
        cx.fill_rect(
            Rect::new(x + px(t, 5) as i32, b.y + top as i32, px(t, 1).max(1), h),
            cx.color(Tone::Ink),
        );
        let thumb = Rect::new(
            x,
            b.y + top as i32 + (thumb * self.page as u32) as i32,
            px(t, 12).max(1),
            thumb,
        );
        cx.fill_rect(thumb, cx.color(Tone::Surface));
        cx.border(thumb, px(t, 5), px(t, 2).max(1), cx.color(Tone::Ink));
    }
    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let next = match ev {
            UiEvent::Gesture(Gesture::Swipe { pos, end_pos, .. }) => {
                Some(if (end_pos.y - pos.y).abs() > (end_pos.x - pos.x).abs() {
                    end_pos.y < pos.y
                } else {
                    end_pos.x < pos.x
                })
            }
            UiEvent::Key(k) if k.pressed => match k.key {
                Key::LPgFwd | Key::RPgFwd | Key::Down => Some(true),
                Key::LPgBack | Key::RPgBack | Key::Up => Some(false),
                _ => None,
            },
            UiEvent::Gesture(Gesture::Tap { pos, .. })
                if pos.x >= cx.bounds().right() - px(cx.theme(), 48) as i32 =>
            {
                Some(pos.y >= cx.bounds().y + cx.bounds().h as i32 / 2)
            }
            _ => None,
        };
        let Some(next) = next else {
            return Handled::No;
        };
        let page = if next {
            (self.page + 1).min(self.pages - 1)
        } else {
            self.page.saturating_sub(1)
        };
        if page != self.page {
            cx.action(AppAction::new(format!("settings:page:{page}")));
        }
        Handled::Yes
    }
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.rows.iter_mut().collect()
    }
}

/// Original line drawing of a reader and its two page keys.
pub struct DeviceDrawing {
    inverted: bool,
}
impl DeviceDrawing {
    pub fn new(inverted: bool) -> Self {
        Self { inverted }
    }
}
impl Widget for DeviceDrawing {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(c.max.w, px(cx.theme, 650)))
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let t = cx.theme;
        let ink = cx.color(Tone::Ink);
        let x = b.x + px(t, 246) as i32;
        let y = b.y + px(t, 79) as i32;
        cx.border(
            Rect::new(x, y, px(t, 345), px(t, 388)),
            px(t, 14),
            px(t, 4).max(1),
            ink,
        );
        cx.border(
            Rect::new(x + px(t, 23) as i32, y + px(t, 24) as i32, px(t, 238), px(t, 340)),
            0,
            px(t, 3).max(1),
            ink,
        );
        for i in 0..2 {
            let by = y + px(t, 115 + 113 * i) as i32;
            cx.border(
                Rect::new(x + px(t, 292) as i32, by, px(t, 20), px(t, 63)),
                px(t, 8),
                px(t, 3).max(1),
                ink,
            );
            let forward = (i == 0) == self.inverted;
            let text = cx.fonts.layout(
                &if forward {
                    tr("Next page")
                } else {
                    tr("Previous page")
                },
                &style(t, 34),
                px(t, 365),
                Some(2),
                true,
            );
            cx.text(&text, Point::new(b.x + px(t, 665) as i32, by + px(t, 10) as i32));
        }
        let rect = Rect::new(
            b.x + px(t, 36) as i32,
            b.y + px(t, 536) as i32,
            b.w.saturating_sub(px(t, 72)),
            px(t, 90),
        );
        cx.border(rect, px(t, 8), px(t, 3).max(1), ink);
        for (i, label) in [tr("Default"), tr("Reversed")].iter().enumerate() {
            let cell = Rect::new(
                rect.x + (rect.w / 2 * i as u32) as i32,
                rect.y,
                rect.w / 2,
                rect.h,
            );
            let selected = (i == 1) == self.inverted;
            if selected {
                cx.fill_rect(cell, ink);
            }
            let mut s = style(t, 34);
            if selected {
                s.color = cx.color(Tone::Surface);
            }
            let text = cx.fonts.layout(label, &s, cell.w, Some(1), true);
            cx.text(
                &text,
                Point::new(
                    cell.x + (cell.w - text.width) as i32 / 2,
                    cell.y + (cell.h - text.height) as i32 / 2,
                ),
            );
        }
    }
    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        if let UiEvent::Gesture(Gesture::Tap { pos, .. }) = ev {
            let b = cx.bounds();
            if pos.y >= b.y + px(cx.theme(), 536) as i32 {
                self.inverted = pos.x >= b.x + b.w as i32 / 2;
                cx.request_paint();
                cx.action(AppAction {
                    id: format!("settings:set:{}", super::keys::READING_INVERT_PAGE_BUTTONS),
                    args: vec![Value::Bool(self.inverted)],
                });
                return Handled::Yes;
            }
        }
        Handled::No
    }
}

pub struct CompactStepper {
    pub key: &'static str,
    pub label: String,
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub unit: entries::StepUnit,
}
impl Widget for CompactStepper {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let text = cx.fonts.layout(
            &self.label,
            &style(t, 34),
            c.max.w.saturating_sub(px(t, 420)).max(1),
            None,
            false,
        );
        c.constrain(Size::new(c.max.w, px(t, 106).max(text.height + px(t, 28))))
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let t = cx.theme;
        let ink = cx.color(Tone::Ink);
        let text = cx.fonts.layout(
            &self.label,
            &style(t, 34),
            b.w.saturating_sub(px(t, 420)).max(1),
            None,
            false,
        );
        cx.text(
            &text,
            Point::new(
                b.x + px(t, 25) as i32,
                b.y + (b.h.saturating_sub(text.height) / 2) as i32,
            ),
        );
        let width = px(t, 360);
        let left = b.right() - width as i32;
        for (offset, label, enabled) in [(0, "−", self.value > self.min), (280, "+", self.value < self.max)]
        {
            let rect = Rect::new(
                left + px(t, offset) as i32,
                b.y + (b.h.saturating_sub(px(t, 70)) / 2) as i32,
                px(t, 70),
                px(t, 70),
            );
            let color = cx.color(if enabled { Tone::Ink } else { Tone::InkMuted });
            cx.border(rect, 0, px(t, 1).max(1), color);
            let mut s = style(t, 34);
            s.color = color;
            let text = cx.fonts.layout(label, &s, rect.w, Some(1), false);
            cx.text(
                &text,
                Point::new(
                    rect.x + (rect.w.saturating_sub(text.width) / 2) as i32,
                    rect.y + (rect.h.saturating_sub(text.height) / 2) as i32,
                ),
            );
        }
        let text = cx.fonts.layout(
            &entries::unit_text(self.unit, self.value),
            &style(t, 30),
            px(t, 200),
            Some(2),
            true,
        );
        cx.text(
            &text,
            Point::new(
                left + (width.saturating_sub(text.width) / 2) as i32,
                b.y + (b.h.saturating_sub(text.height) / 2) as i32,
            ),
        );
        cx.fill_rect(Rect::new(b.x, b.bottom() - 1, b.w, 1), ink);
    }
    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let UiEvent::Gesture(Gesture::Tap { pos, .. }) = ev else {
            return Handled::No;
        };
        let right = cx.bounds().right();
        let touch = cx.theme().touch_target as i32;
        let delta = if pos.x > right - touch {
            self.step
        } else if pos.x >= right - px(cx.theme(), 360) as i32
            && pos.x < right - px(cx.theme(), 360) as i32 + touch
        {
            -self.step
        } else {
            return Handled::No;
        };
        self.value = (self.value + delta).clamp(self.min, self.max);
        cx.request_paint();
        cx.action(AppAction {
            id: format!("settings:set:{}", self.key),
            args: vec![Value::Number(self.value)],
        });
        Handled::Yes
    }
}

/// Original diagrams of the four touch layouts. These proportions describe the
/// illustration only; measured reader hit areas belong to the input code.
pub struct TapZones {
    selected: Option<usize>,
}
impl TapZones {
    pub fn new(preset: &str) -> Self {
        Self {
            selected: super::keys::ILLUSTRATED_TAP_ZONES
                .iter()
                .position(|p| *p == preset),
        }
    }
    fn cell(b: Rect, i: usize, t: &Theme) -> Rect {
        let width = b.w / 4;
        Rect::new(
            b.x + (width * i as u32 + width.saturating_sub(px(t, 150)) / 2) as i32,
            b.y + px(t, 153) as i32,
            px(t, 150),
            px(t, 200),
        )
    }
}
impl Widget for TapZones {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(c.max.w, px(cx.theme, 369)))
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let t = cx.theme;
        let ink = cx.color(Tone::Ink);
        let text = cx.fonts.layout(
            &tr("Choose areas for previous page, menu and next page."),
            &style(t, 34),
            b.w.saturating_sub(px(t, 50)),
            Some(2),
            true,
        );
        cx.text(&text, Point::new(b.x + px(t, 25) as i32, b.y + px(t, 28) as i32));
        for i in 0..4 {
            let r = Self::cell(b, i, t);
            cx.border(
                r,
                0,
                px(t, 10).max(1),
                if self.selected == Some(i) {
                    cx.color(Tone::Accent)
                } else {
                    Color::gray(0xd9)
                },
            );
            let r = Rect::new(
                r.x + px(t, 10) as i32,
                r.y + px(t, 10) as i32,
                r.w.saturating_sub(px(t, 20)),
                r.h.saturating_sub(px(t, 20)),
            );
            cx.border(r, 0, px(t, 2).max(1), ink);
            let mut glyph = |name: &str, x: u32, y: u32| {
                let at = Point::new(
                    r.x + (r.w * x / 100) as i32 - px(t, 13) as i32,
                    r.y + (r.h * y / 100) as i32 - px(t, 13) as i32,
                );
                if name == "menu" {
                    for offset in [5, 12, 19] {
                        cx.fill_rect(
                            Rect::new(at.x, at.y + px(t, offset) as i32, px(t, 26), px(t, 2).max(1)),
                            ink,
                        );
                    }
                } else {
                    draw_icon_at(cx, name, px(t, 26), ink, at);
                }
            };
            if i == 0 {
                glyph("chevron.left", 17, 50);
                glyph("menu", 50, 50);
                glyph("chevron.right", 83, 50);
                for x in [33, 67] {
                    cx.fill_rect(Rect::new(r.x + (r.w * x / 100) as i32, r.y, 1, r.h), ink);
                }
            } else if i == 3 {
                glyph("chevron.left", 50, 20);
                glyph("menu", 50, 50);
                glyph("chevron.right", 50, 80);
                for y in [38, 62] {
                    cx.fill_rect(Rect::new(r.x, r.y + (r.h * y / 100) as i32, r.w, 1), ink);
                }
            } else {
                glyph(
                    if i == 1 { "chevron.left" } else { "chevron.right" },
                    if i == 1 { 15 } else { 35 },
                    50,
                );
                glyph(
                    if i == 1 { "chevron.right" } else { "chevron.left" },
                    if i == 1 { 65 } else { 85 },
                    50,
                );
                glyph("menu", 50, 88);
                let x = if i == 1 { 30 } else { 70 };
                cx.fill_rect(Rect::new(r.x + (r.w * x / 100) as i32, r.y, 1, r.h), ink);
                cx.border(
                    Rect::new(
                        r.x + (r.w * 30 / 100) as i32,
                        r.y + (r.h * 76 / 100) as i32,
                        r.w * 40 / 100,
                        r.h * 24 / 100,
                    ),
                    0,
                    1,
                    ink,
                );
            }
        }
        cx.fill_rect(Rect::new(b.x, b.bottom() - 1, b.w, 1), ink);
    }
    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> Handled {
        let UiEvent::Gesture(Gesture::Tap { pos, .. }) = event else {
            return Handled::No;
        };
        for i in 0..4 {
            if Self::cell(cx.bounds(), i, cx.theme()).contains(*pos) {
                self.selected = Some(i);
                cx.request_paint();
                cx.action(AppAction {
                    id: format!("settings:set:{}", super::keys::READING_TAP_ZONES),
                    args: vec![Value::from(super::keys::ILLUSTRATED_TAP_ZONES[i])],
                });
                return Handled::Yes;
            }
        }
        Handled::No
    }
}
