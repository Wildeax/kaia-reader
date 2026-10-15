//! [`NavBar`]: the bottom bar with Home, My books and More.

use kaia_gfx::{Point, Rect, Size};
use kaia_input::Gesture;

use crate::components::draw_icon_at;
use crate::geometry::Constraints;
use crate::theme::Tone;
use crate::widget::{AppAction, EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};

/// One item of the bar. Tapping it queues the action from [`NavItem::action`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavItem {
    Home,
    Library,
    /// Notes belongs to More. Kept for callers that identify their current page.
    Notes,
    Settings,
    More,
}

impl NavItem {
    pub const ALL: [NavItem; 3] = [NavItem::Home, NavItem::Library, NavItem::More];

    /// The action id a tap queues.
    pub fn action(self) -> &'static str {
        match self {
            NavItem::Home => "nav.home",
            NavItem::Library => "nav.library",
            NavItem::Notes => "nav.notes",
            NavItem::Settings => "nav.settings",
            NavItem::More => "nav.more",
        }
    }

    fn icon(self) -> &'static str {
        match self {
            NavItem::Home => "home",
            NavItem::Library => "book.opened",
            NavItem::Notes => "bookmark",
            NavItem::Settings => "appbar.settings",
            NavItem::More => "appbar.menu",
        }
    }

    fn tab(self) -> Self {
        match self {
            Self::Notes | Self::Settings => Self::More,
            item => item,
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|item| *item == self).unwrap_or(0)
    }
}

/// The bar at the bottom of the Home, Library, Notes and Settings surfaces.
/// The active item uses the theme accent for its label, icon and top mark.
/// In RTL the items run from right to left.
pub struct NavBar {
    active: NavItem,
    labels: [String; 3],
}

impl NavBar {
    /// A bar with the English labels.
    pub fn new(active: NavItem) -> Self {
        Self::with_labels(active, ["Home", "My books", "More"].map(String::from))
    }

    /// A bar with labels in the order of [`NavItem::ALL`]. Use it for translated labels.
    pub fn with_labels(active: NavItem, labels: [String; 3]) -> Self {
        Self { active, labels }
    }

    pub fn active(&self) -> NavItem {
        self.active
    }

    /// Changes the active item. Call `request_paint` in the update.
    pub fn set_active(&mut self, active: NavItem) {
        self.active = active;
    }

    /// The item at display position `slot` (0 is the leading edge).
    fn slot_item(rtl: bool, slot: usize) -> NavItem {
        if rtl {
            NavItem::ALL[2 - slot]
        } else {
            NavItem::ALL[slot]
        }
    }
}

impl Widget for NavBar {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(c.max.w, cx.theme.nav_bar))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let background = cx.color(cx.background());
        cx.fill_rect(b, background);
        let t = cx.theme;
        let divider = cx.color(Tone::Divider);
        cx.fill_rect(Rect::new(b.x, b.y, b.w, 1), divider);

        let cell_w = b.w / 3;
        let icon = t.scale.dp(24);
        for slot in 0..3 {
            let item = Self::slot_item(t.rtl, slot);
            let cell = Rect::new(b.x + (slot as u32 * cell_w) as i32, b.y, cell_w, b.h);
            let active = item == self.active.tab();
            let tone = if active { Tone::Accent } else { Tone::Ink };
            let mut label_font = t.font.caption.clone();
            if active {
                label_font.weight = 700;
            }
            let color = cx.color(tone);
            let mut style = t.style(&label_font, tone);
            style.color = color;
            let label = cx.fonts.layout(
                &self.labels[item.index()],
                &style,
                cell_w.saturating_sub(2 * t.space.xs).max(1),
                Some(1),
                true,
            );
            let stack_h = icon as i32 + t.space.xs as i32 + label.height as i32;
            let top = cell.y + (cell.h as i32 - stack_h - t.line.focus as i32) / 2;
            draw_icon_at(
                cx,
                item.icon(),
                icon,
                color,
                Point::new(cell.x + (cell_w as i32 - icon as i32) / 2, top),
            );
            let label_at = Point::new(
                cell.x + (cell_w as i32 - label.width as i32) / 2,
                top + icon as i32 + t.space.xs as i32,
            );
            cx.text(&label, label_at);
            if active {
                let accent = color;
                let line = Rect::new(cell.x, b.y, cell_w, t.line.focus);
                cx.fill_rect(line, accent);
            }
        }
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let UiEvent::Gesture(Gesture::Tap { pos, .. }) = ev else {
            return Handled::No;
        };
        let b = cx.bounds();
        if !b.contains(*pos) {
            return Handled::No;
        }
        let cell_w = (b.w / 3).max(1);
        let slot = (((pos.x - b.x) as u32) / cell_w).min(2) as usize;
        let item = Self::slot_item(cx.theme().rtl, slot);
        cx.action(AppAction::new(item.action()));
        Handled::Yes
    }
}
