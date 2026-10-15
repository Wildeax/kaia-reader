//! The inset settings dialog and its plain index rows.

use kaia_gfx::text::TextStyle;
use kaia_gfx::{Color, Point, Rect, Size};
use kaia_input::{Gesture, Key};

use crate::components::draw_icon_at;
use crate::geometry::Constraints;
use crate::widget::{AppAction, EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};
use crate::{Theme, Tone, WidgetPod};

/// Reference pixels on the 1264 px short side, independent of text scaling.
pub(super) fn px(theme: &Theme, value: u32) -> u32 {
    ((u64::from(value) * u64::from(theme.scale.width.min(theme.scale.height)) + 632) / 1264) as u32
}

fn serif(theme: &Theme, size: u32, italic: bool) -> TextStyle {
    let mut style = TextStyle::new(px(theme, size) as f32 * theme.text_scale);
    style.family = "Noto Serif".into();
    style.italic = italic;
    style.color = theme.color(Tone::Ink);
    style
}

/// A full-screen dialog with an inset border, centered caption and large title.
/// The caller supplies translated labels and a close action for its route.
pub struct SettingsDialog {
    caption: String,
    title: String,
    content: WidgetPod,
    close: WidgetPod,
    back: Option<WidgetPod>,
    search: Option<WidgetPod>,
    panel: Rect,
    content_header: bool,
}

impl SettingsDialog {
    pub fn new(
        caption: impl Into<String>,
        title: impl Into<String>,
        content: WidgetPod,
        close: &str,
    ) -> Self {
        Self {
            caption: caption.into(),
            title: title.into(),
            content,
            close: WidgetPod::new(DialogIcon::new("close", close))
                .with_key("settings-close")
                .focusable(true),
            search: None,
            back: None,
            panel: Rect::default(),
            content_header: false,
        }
    }

    /// Reserves the title's trailing area for the settings page count.
    pub fn content_header(mut self) -> Self {
        self.content_header = true;
        self
    }

    pub fn search(mut self, action: &str) -> Self {
        self.search = Some(
            WidgetPod::new(DialogIcon::new("appbar.search", action))
                .with_key("settings-search")
                .focusable(true),
        );
        self
    }

    /// Adds a back arrow for a subpage. Close keeps its separate action.
    pub fn back(mut self, action: &str) -> Self {
        self.back = Some(
            WidgetPod::new(DialogIcon::new("settings.back", action))
                .with_key("settings-back")
                .focusable(true),
        );
        self
    }
}

impl Widget for SettingsDialog {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let inset = px(t, 25);
        self.panel = Rect::new(
            inset as i32,
            inset as i32,
            c.max.w.saturating_sub(inset * 2),
            c.max.h.saturating_sub(inset * 2),
        );
        let side = t.touch_target;
        let right = self.panel.right() - px(t, 2) as i32;
        let y = px(t, 28) as i32;
        self.close.layout(cx, Constraints::tight(Size::new(side, side)));
        self.close.set_origin(Point::new(right - side as i32, y));
        if let Some(back) = &mut self.back {
            back.layout(cx, Constraints::tight(Size::new(side, side)));
            back.set_origin(Point::new(px(t, 27) as i32, y));
        }
        if let Some(search) = &mut self.search {
            search.layout(cx, Constraints::tight(Size::new(side, side)));
            search.set_origin(Point::new(right - 2 * side as i32, y));
        }
        let x = px(t, 100);
        let top = px(t, if self.content_header { 190 } else { 280 });
        let width = c.max.w.saturating_sub(x + px(t, 60));
        let height = c.max.h.saturating_sub(top + px(t, 75));
        self.content
            .layout(cx, Constraints::tight(Size::new(width, height)));
        self.content.set_origin(Point::new(x as i32, top as i32));
        c.max
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let t = cx.theme;
        let panel = self.panel.translate(b.x, b.y);
        cx.fill_rect(b, Color::gray(0x4d));
        cx.fill_rect(panel, cx.color(Tone::Surface));
        cx.border(panel, 0, px(t, 1).max(1), cx.color(Tone::Ink));
        let caption = cx.fonts.layout(
            &self.caption,
            &serif(t, 32, false),
            b.w.saturating_sub(6 * t.touch_target).max(1),
            Some(1),
            true,
        );
        cx.text(
            &caption,
            Point::new(b.x + (b.w - caption.width) as i32 / 2, b.y + px(t, 63) as i32),
        );
        let title = cx.fonts.layout(
            &self.title,
            &serif(t, 54, false),
            b.w.saturating_sub(px(t, if self.content_header { 480 } else { 248 })),
            Some(1),
            true,
        );
        cx.text(
            &title,
            Point::new(b.x + px(t, 124) as i32, b.y + px(t, 191) as i32),
        );
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        let mut children = vec![&mut self.content, &mut self.close];
        children.extend(self.back.iter_mut());
        children.extend(self.search.iter_mut());
        children
    }
}

struct DialogIcon {
    icon: String,
    action: String,
}

impl DialogIcon {
    fn new(icon: &str, action: &str) -> Self {
        Self {
            icon: icon.into(),
            action: action.into(),
        }
    }
}

impl Widget for DialogIcon {
    crate::impl_as_any!();

    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.max
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let size = px(cx.theme, 38);
        let icon = if self.icon == "settings.back" && cx.theme.rtl {
            "settings.back.rtl"
        } else {
            &self.icon
        };
        draw_icon_at(
            cx,
            icon,
            size,
            cx.color(Tone::Ink),
            Point::new(b.x + (b.w - size) as i32 / 2, b.y + (b.h - size) as i32 / 2),
        );
        cx.focus_ring(0);
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> Handled {
        if activated(event, cx) {
            cx.action(AppAction::new(&self.action));
            Handled::Yes
        } else {
            Handled::No
        }
    }
}

fn activated(event: &UiEvent, cx: &EventCx<'_>) -> bool {
    matches!(event, UiEvent::Gesture(Gesture::Tap { .. }))
        || matches!(event, UiEvent::Key(key) if key.pressed && key.key == Key::Press && cx.is_focused())
}

/// A plain settings-index action with one-pixel rules and a full-row tap target.
pub struct SettingsIndexRow {
    label: String,
    action: String,
    first: bool,
}

impl SettingsIndexRow {
    pub fn new(label: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: action.into(),
            first: false,
        }
    }

    pub fn first(mut self, first: bool) -> Self {
        self.first = first;
        self
    }
}

impl Widget for SettingsIndexRow {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let text = cx.fonts.layout(
            &self.label,
            &serif(t, 42, true),
            c.max.w.saturating_sub(px(t, 50)),
            Some(2),
            true,
        );
        c.constrain(Size::new(
            c.max.w,
            px(t, 107).max(t.touch_target).max(text.height + px(t, 20)),
        ))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let t = cx.theme;
        let text = cx.fonts.layout(
            &self.label,
            &serif(t, 42, true),
            b.w.saturating_sub(px(t, 50)),
            Some(2),
            true,
        );
        let x = if t.rtl {
            b.right() - px(t, 25) as i32 - text.width as i32
        } else {
            b.x + px(t, 25) as i32
        };
        cx.text(&text, Point::new(x, b.y + (b.h as i32 - text.height as i32) / 2));
        let ink = cx.color(Tone::Ink);
        let rule = px(t, 1).max(1);
        cx.fill_rect(Rect::new(b.x, b.bottom() - rule as i32, b.w, rule), ink);
        if self.first {
            cx.fill_rect(Rect::new(b.x, b.y, b.w, rule), ink);
        }
        cx.focus_ring(0);
    }

    fn event(&mut self, cx: &mut EventCx, event: &UiEvent) -> Handled {
        if activated(event, cx) {
            cx.action(AppAction::new(&self.action));
            Handled::Yes
        } else {
            Handled::No
        }
    }
}
