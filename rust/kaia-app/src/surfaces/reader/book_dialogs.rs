//! Contents and annotations share the book dialog and sixteen-row pager.
use super::{
    chrome::{Chrome, Tool},
    compact::*,
    panels::ContentItem,
    state::Core,
};
use crate::{
    surface::{Outcome, SurfaceEnv},
    surfaces::common::window,
};
use kaia_gfx::{Point, Rect, Size};
use kaia_i18n::tr;
use kaia_ui::{Constraints, EventCx, Handled, LayoutCx, PaintCx, Tone, UiEvent, Widget, WidgetPod};
use std::{cell::RefCell, rc::Rc};

pub struct ContentsList {
    items: Vec<ContentItem>,
    page: usize,
    screen: Size,
    children: Vec<WidgetPod>,
}
impl ContentsList {
    fn new(items: Vec<ContentItem>, screen: Size) -> Self {
        Self {
            items,
            page: 0,
            screen,
            children: vec![],
        }
    }
    pub fn items(&self) -> &[ContentItem] {
        &self.items
    }
    pub fn page(&self) -> usize {
        self.page
    }
    pub(super) fn step(&mut self, next: bool, cx: &mut kaia_ui::widget::UpdateCx) {
        self.page = if next {
            (self.page + 1).min(self.pages() - 1)
        } else {
            self.page.saturating_sub(1)
        };
        self.children.clear();
        cx.request_layout();
        cx.request_paint();
    }
    fn pages(&self) -> usize {
        self.items.len().div_ceil(16).max(1)
    }
    fn turn(&mut self, cx: &mut EventCx, next: bool) {
        let page = if next {
            (self.page + 1).min(self.pages() - 1)
        } else {
            self.page.saturating_sub(1)
        };
        if page != self.page {
            self.page = page;
            self.children.clear();
            cx.request_layout();
            cx.request_paint();
        }
    }
}
impl Widget for ContentsList {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let row_h = c.max.h / 16;
        if self.children.is_empty() {
            for item in self.items.iter().skip(self.page * 16).take(16) {
                let mut tool = Tool::new("", &item.action, 48 * self.screen.w.min(self.screen.h) / 1264);
                tool.label = item.label.clone();
                tool.clear = false;
                tool.bold = item.current;
                tool.face = Some("Noto Serif".into());
                self.children.push(
                    WidgetPod::new(tool)
                        .focusable(true)
                        .with_key(super::panels::key_of(&item.action)),
                );
            }
        }
        for (i, child) in self.children.iter_mut().enumerate() {
            let level = self.items[self.page * 16 + i].level;
            let indent = (82 + 71 * level.min(8)) * self.screen.w / 1264;
            child.layout(
                cx,
                Constraints::tight(Size::new(
                    c.max.w.saturating_sub(indent + 72 * self.screen.w / 1264),
                    row_h,
                )),
            );
            child.set_origin(Point::new(indent as i32, (i as u32 * row_h) as i32));
        }
        c.max
    }
    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.children.iter_mut().collect()
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        cx.clear_background();
        let b = cx.bounds();
        let row_h = b.h / 16;
        let right = b.right() - (38 * self.screen.w / 1264) as i32;
        for i in 1..=self.children.len() {
            cx.fill_rect(
                Rect::new(
                    b.x,
                    b.y + (i as u32 * row_h) as i32,
                    b.w.saturating_sub(72 * self.screen.w / 1264),
                    1,
                ),
                cx.color(Tone::Divider),
            );
        }
        let px = 36 * self.screen.w / 1264;
        for (y, name, enabled) in [
            (b.y, "chevron.up", self.page > 0),
            (
                b.bottom() - px as i32,
                "chevron.down",
                self.page + 1 < self.pages(),
            ),
        ] {
            let color = cx.color(if enabled { Tone::Ink } else { Tone::InkMuted });
            if let Ok(icon) = cx.icons.get(name, Size::new(px, px), color) {
                cx.blit_alpha(&icon, Point::new(right - px as i32 / 2, y));
            }
        }
        let top = b.y + 2 * px as i32;
        let height = b.h.saturating_sub(4 * px);
        cx.fill_rect(Rect::new(right, top, 1, height), cx.color(Tone::Ink));
        let h = height / self.pages() as u32;
        let thumb = Rect::new(right - 5, top + (self.page as u32 * h) as i32, 11, h.max(10));
        cx.fill_rect(thumb, cx.color(Tone::Surface));
        cx.border(thumb, 4, 2, cx.color(Tone::Ink));
    }
    fn event(&mut self, cx: &mut EventCx, e: &UiEvent) -> Handled {
        let next = match e {
            UiEvent::Named(event) if matches!(event.name.as_str(), "PageForward" | "PageBack") => {
                Some(event.name == "PageForward")
            }
            UiEvent::Gesture(kaia_input::Gesture::Tap { pos, .. })
                if pos.x > cx.bounds().right() - (72 * self.screen.w / 1264) as i32 =>
            {
                Some(pos.y > cx.bounds().y + cx.bounds().h as i32 / 2)
            }
            UiEvent::Gesture(kaia_input::Gesture::Swipe { direction, .. }) => Some(matches!(
                direction,
                Some(kaia_input::Direction::West | kaia_input::Direction::North)
            )),
            UiEvent::Key(k) if k.pressed => match k.key {
                kaia_input::Key::RPgFwd
                | kaia_input::Key::LPgFwd
                | kaia_input::Key::Right
                | kaia_input::Key::Down => Some(true),
                kaia_input::Key::RPgBack
                | kaia_input::Key::LPgBack
                | kaia_input::Key::Left
                | kaia_input::Key::Up => Some(false),
                _ => None,
            },
            _ => None,
        };
        if let Some(next) = next {
            self.turn(cx, next);
            Handled::Yes
        } else {
            Handled::No
        }
    }
}
pub(super) fn contents(
    screen: Size,
    title: &str,
    tab: usize,
    items: Vec<ContentItem>,
    empty: &str,
    filter: usize,
    reverse: bool,
) -> Chrome {
    let mut chrome = dialog(screen);
    heading(&mut chrome, screen, title);
    for (i, x, label, w) in [(0, 72, tr("Contents"), 210), (1, 302, tr("Annotations"), 290)] {
        put(
            &mut chrome,
            screen,
            [x, 134, w, 80],
            action(
                screen,
                &label,
                &format!("reader:contents:tab:{i}"),
                &format!("contents-tab-{i}"),
            ),
        );
        if (tab == 0) == (i == 0) {
            rule(&mut chrome, screen, [x, 213, w, 6], Tone::Accent);
        }
    }
    rule(&mut chrome, screen, [72, 219, 1120, 1], Tone::Ink);
    let annotations = tab != 0;
    if annotations {
        let label = [tr("All"), tr("Bookmarks"), tr("Highlights"), tr("Notes")][filter.min(3)].clone();
        put(
            &mut chrome,
            screen,
            [72, 240, 280, 80],
            action(
                screen,
                &format!("{label} ▾"),
                "reader:annotations:filter",
                "annotations-filter",
            ),
        );
        put(
            &mut chrome,
            screen,
            [354, 246, 461, 64],
            dropdown(
                screen,
                &if reverse {
                    tr("Position, descending")
                } else {
                    tr("Position, ascending")
                },
                "reader:annotations:sort",
                "annotations-sort",
            ),
        );
        put(
            &mut chrome,
            screen,
            [1110, 236, 90, 80],
            icon(screen, "more", "reader:annotations:more", "annotations-more"),
        );
    }
    if items.is_empty() {
        put(
            &mut chrome,
            screen,
            [180, 920, 904, 110],
            WidgetPod::new(kaia_ui::components::Align::new(
                text(screen, empty),
                kaia_ui::geometry::Alignment::Center,
                kaia_ui::geometry::Alignment::Center,
            ))
            .with_key("contents-empty"),
        );
    } else {
        let y = if annotations { 330 } else { 220 };
        put(
            &mut chrome,
            screen,
            [93, y, 1112, 1312],
            WidgetPod::new(ContentsList::new(items, screen))
                .focusable(true)
                .with_key("contents-list"),
        );
    }
    chrome
}
pub(super) fn command(rc: &Rc<RefCell<Core>>, cmd: &str, env: &mut SurfaceEnv<'_>) -> Option<Outcome> {
    if matches!(cmd, "reader:contents:page:next" | "reader:contents:page:previous") {
        if let Some(layer) = &rc.borrow().panel {
            layer
                .window
                .update::<ContentsList, _>(env.ctx, "contents-list", |list, cx| {
                    list.step(cmd.ends_with(":next"), cx);
                });
        }
        return Some(Outcome::Handled);
    }
    let mut c = rc.borrow_mut();
    match cmd {
        "reader:annotations:filter" => {
            let mut chrome = Chrome {
                panel_border: 3,
                size: env.screen,
                children: vec![],
                panels: vec![super::chrome::rect(env.screen, 72, 320, 400, 364)],
            };
            for (i, label) in [tr("All"), tr("Bookmarks"), tr("Highlights"), tr("Notes")]
                .into_iter()
                .enumerate()
            {
                put(
                    &mut chrome,
                    env.screen,
                    [76, 324 + i as u32 * 91, 392, 83],
                    action(
                        env.screen,
                        &label,
                        &format!("reader:annotations:filter:{i}"),
                        &format!("annotations-filter-{i}"),
                    ),
                );
            }
            let window = window(env.services, env.screen, chrome);
            let serial = super::next_serial(&mut c);
            return Some(Outcome::Navigate(crate::surface::Nav::Push(
                super::panels::panel_surface(window, serial, rc.clone()),
            )));
        }
        "reader:annotations:sort" => c.annotation_reverse = !c.annotation_reverse,
        "reader:annotations:more" => {
            let window = super::panels::annotation_tools(env.services, env.screen);
            return Some(super::show_panel(rc, &mut c, env.ctx, window, true));
        }
        _ => {
            let filter = cmd
                .strip_prefix("reader:annotations:filter:")?
                .parse::<usize>()
                .ok()?;
            c.annotation_filter = filter.min(3);
            // Close the dropdown, then rebuild the dialog beneath it in place.
            let window = super::contents_chrome(&mut c, env, 1);
            if let Some(layer) = &c.panel {
                layer
                    .window
                    .update::<Chrome, _>(env.ctx, "contents-root", |chrome, cx| {
                        *chrome = window;
                        cx.request_layout();
                        cx.request_paint();
                    });
            }
            return Some(Outcome::Navigate(crate::surface::Nav::Pop));
        }
    }
    let window = super::contents_window(&mut c, env, 1);
    Some(super::show_panel(rc, &mut c, env.ctx, window, true))
}
