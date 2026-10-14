//! The page widget: the rendered page, its highlights, the selection and the
//! search hits. It turns touches and page-turn keys into reader commands
//! (`reader:tap`, `reader:next`, ...). The reader surface decides what they do.

use std::sync::Arc;

use kaia_events::Value;
use kaia_gfx::{Pixmap, Point, Rect};
use kaia_input::{Direction, Gesture};
use kaia_ui::geometry::Constraints;
use kaia_ui::widget::{AppAction, EventCx, Handled, LayoutCx, PaintCx, UiEvent};
use kaia_ui::{Tone, Widget};

/// The key of the page widget. The reader surface finds it with `Window::update`.
pub const PAGE_KEY: &str = "reader-page";

/// Edge width, in device-independent units, where a swipe belongs to the app (Back).
const EDGE_DP: u32 = 40;

/// How a highlight is drawn. Grayscale panels get no color; the shape carries the style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkStyle {
    /// A dimmed box behind the text (KOReader's lighten).
    Box,
    /// A line under the text (underscore).
    Underline,
    /// A line through the middle of the text (strikeout).
    Strike,
}

/// A highlight box on the page, in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub rect: Rect,
    pub style: MarkStyle,
}

/// The reader's page. The reader sets the image, the marks and the message;
/// the widget only paints them and reports input.
pub struct PageView {
    #[cfg(feature = "lua")]
    pub plugins: Option<std::rc::Weak<std::cell::RefCell<kaia_lua::LuaUi>>>,
    /// The rendered page, or None when the page cannot be drawn.
    pub page: Option<Arc<Pixmap>>,
    /// Highlights on the page.
    pub marks: Vec<Mark>,
    pub stickers: Vec<super::stickers::Drawing>,
    /// The selected word, one box per line.
    pub selection: Vec<Rect>,
    /// Search hits on the page.
    pub hits: Vec<Rect>,
    /// Text shown in place of the page, such as "Cannot open this book."
    pub message: Option<String>,
    /// The page shown (0-based) and the page count. Tests read the position here.
    pub position: Option<(u32, usize)>,
}

impl PageView {
    pub fn new() -> Self {
        Self {
            page: None,
            #[cfg(feature = "lua")]
            plugins: None,
            marks: Vec::new(),
            stickers: Vec::new(),
            selection: Vec::new(),
            hits: Vec::new(),
            message: None,
            position: None,
        }
    }
}

impl Default for PageView {
    fn default() -> Self {
        Self::new()
    }
}

fn action(id: &str, args: Vec<Value>) -> AppAction {
    AppAction {
        id: id.to_string(),
        args,
    }
}

fn point_args(pos: kaia_gfx::Point, bounds: Rect) -> Vec<Value> {
    vec![
        Value::Number(f64::from(pos.x - bounds.x)),
        Value::Number(f64::from(pos.y - bounds.y)),
    ]
}

impl Widget for PageView {
    kaia_ui::impl_as_any!();

    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> kaia_gfx::Size {
        c.max
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let bounds = cx.bounds();
        let surface = cx.color(Tone::Surface);
        cx.fill_rect(bounds, surface);

        match &self.page {
            Some(page) => {
                let x = bounds.x + (bounds.w as i32 - page.width() as i32) / 2;
                let y = bounds.y + (bounds.h as i32 - page.height() as i32) / 2;
                cx.blit_pixmap(page, Point::new(x, y));
            }
            None => {
                if let Some(message) = &self.message {
                    let style = cx.theme.style(&cx.theme.font.body, Tone::InkSecondary);
                    let margin = cx.theme.space.lg;
                    let text =
                        cx.fonts
                            .layout(message, &style, bounds.w.saturating_sub(2 * margin), None, true);
                    cx.text(
                        &text,
                        Point::new(bounds.x + margin as i32, bounds.y + margin as i32),
                    );
                }
            }
        }

        let ink = cx.color(Tone::Ink);
        for mark in &self.marks {
            let r = mark.rect;
            match mark.style {
                MarkStyle::Box => cx.dim(r, 40),
                MarkStyle::Underline => cx.fill_rect(
                    Rect {
                        x: r.x,
                        y: r.y + r.h as i32 - 3,
                        w: r.w,
                        h: 3,
                    },
                    ink,
                ),
                MarkStyle::Strike => cx.fill_rect(
                    Rect {
                        x: r.x,
                        y: r.y + r.h as i32 / 2,
                        w: r.w,
                        h: 2,
                    },
                    ink,
                ),
            }
        }
        for rect in &self.hits {
            cx.border(*rect, 0, 3, ink);
        }
        for rect in &self.selection {
            cx.dim(*rect, 80);
            cx.border(*rect, 0, 2, ink);
        }
        for sticker in &self.stickers {
            sticker.paint(cx);
        }
        #[cfg(feature = "lua")]
        if let Some(ui) = self.plugins.as_ref().and_then(|ui| ui.upgrade()) {
            // Lua paints into a copy of the page, which costs a page conversion
            // each way. Skip it when no plugin registered a view module.
            let overlays = ui
                .borrow()
                .host()
                .eval(
                    "reader-overlays",
                    "return KAIA_PLUGIN ~= nil and next(KAIA_PLUGIN.view_modules) ~= nil",
                )
                .map_or(true, |v| v.as_bool() != Some(false));
            if overlays {
                cx.draw_region(bounds, |page, _| {
                    if let Err(e) = ui.borrow().paint_reader_overlays(page) {
                        eprintln!("kaia: plugin overlay: {e}");
                    }
                });
            }
        }
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let bounds = cx.bounds();
        match ev {
            UiEvent::Named(e) if e.name == "PageForward" => {
                cx.action(action("reader:next", Vec::new()));
                Handled::Yes
            }
            UiEvent::Named(e) if e.name == "PageBack" => {
                cx.action(action("reader:prev", Vec::new()));
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                cx.action(action("reader:tap", point_args(*pos, bounds)));
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Swipe { pos, direction, .. }) => {
                let edge = cx.theme().scale.dp(EDGE_DP) as i32;
                if pos.x - bounds.x < edge {
                    // A swipe from the left edge is Back. The app handles it.
                    return Handled::No;
                }
                match direction {
                    Some(Direction::West) | Some(Direction::NorthWest) | Some(Direction::SouthWest) => {
                        cx.action(action("reader:next", Vec::new()));
                        Handled::Yes
                    }
                    Some(Direction::East) | Some(Direction::NorthEast) | Some(Direction::SouthEast) => {
                        cx.action(action("reader:prev", Vec::new()));
                        Handled::Yes
                    }
                    _ => Handled::No,
                }
            }
            UiEvent::Gesture(Gesture::Hold { pos, .. }) => {
                cx.action(action("reader:hold", point_args(*pos, bounds)));
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::HoldPan { pos, .. }) => {
                cx.action(action("reader:drag", point_args(*pos, bounds)));
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::HoldRelease { .. }) => {
                cx.action(action("reader:release", Vec::new()));
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}
