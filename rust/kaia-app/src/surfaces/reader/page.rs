//! The page widget: the rendered page, its highlights, the selection and the
//! search hits. It turns touches and page-turn keys into reader commands
//! (`reader:tap`, `reader:next`, ...). The reader surface decides what they do.

use std::sync::Arc;

use kaia_events::Value;
use kaia_gfx::{Color, Pixmap, Point, Rect};
use kaia_i18n::{format as tr_format, tr};
use kaia_input::{Axis, Direction, Gesture, Key};
use kaia_ui::geometry::Constraints;
use kaia_ui::widget::{AppAction, EventCx, Handled, LayoutCx, PaintCx, UiEvent};
use kaia_ui::{Tone, Widget};

/// The key of the page widget. The reader surface finds it with `Window::update`.
pub const PAGE_KEY: &str = "reader-page";

/// How a highlight is drawn. On a color panel, a mark with a color takes that color.
/// Grayscale panels and marks without a color draw the shape alone. A box is dimmed
/// and a line is ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkStyle {
    /// A box behind the text (KOReader's lighten). Tinted by its color on a color
    /// panel, dimmed otherwise.
    Box,
    /// A line under the text (underscore).
    Underline,
    /// A line through the middle of the text (strikeout).
    Strike,
    /// The box with its pixels inverted (KOReader's invert). It ignores the color.
    Invert,
}

/// A highlight box on the page, in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    pub rect: Rect,
    pub style: MarkStyle,
    /// The highlight color. None when the annotation has no color or an unknown one.
    pub color: Option<Color>,
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
    pub brightness: Option<u8>,
    pub flipping: bool,
    light_drag: Option<bool>,
    light_start: Point,
    touch_flip: bool,
    held_key: Option<bool>,
    repeats: u32,
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
            brightness: None,
            flipping: false,
            light_drag: None,
            light_start: Point::new(0, 0),
            touch_flip: false,
            held_key: None,
            repeats: 0,
        }
    }

    pub(super) fn cancel_gestures(&mut self) {
        self.light_drag = None;
        self.touch_flip = false;
        self.held_key = None;
        self.repeats = 0;
    }

    fn brightness_action(&self, cx: &mut EventCx, start: Point, end: Point, release: bool) {
        let delta = f64::from(start.y - end.y) * 100.0 / f64::from(cx.bounds().h.max(1));
        cx.action(action(
            "reader:brightness",
            vec![Value::Number(delta), Value::Bool(release)],
        ));
    }

    fn flip_action(&self, cx: &mut EventCx, pos: Point) {
        let bounds = cx.bounds();
        let zone = ((i64::from(pos.x - bounds.x) * 5) / i64::from(bounds.w.max(1))).clamp(0, 4);
        cx.action(action("reader:flip", vec![Value::Number(zone as f64)]));
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

/// A pinch or spread: its distance, the sum of both fingers' travel, and its axis.
/// The reader turns them into a size step, as KOReader's `gesToFontSize` does.
fn size_action(id: &str, distance: f64, axis: Option<Axis>) -> AppAction {
    action(
        id,
        vec![
            Value::Number(distance),
            Value::Str(axis.map_or("", Axis::name).to_string()),
        ],
    )
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
            // Only a color panel shows the highlight color. A grayscale panel draws the shape alone.
            let color = if cx.theme.color_panel { mark.color } else { None };
            match mark.style {
                MarkStyle::Box => match color {
                    Some(tint) => cx.multiply_rect(r, tint),
                    None => cx.dim(r, 40),
                },
                MarkStyle::Invert => cx.invert_rect(r),
                MarkStyle::Underline => cx.fill_rect(
                    Rect {
                        x: r.x,
                        y: r.y + r.h as i32 - 3,
                        w: r.w,
                        h: 3,
                    },
                    color.unwrap_or(ink),
                ),
                MarkStyle::Strike => cx.fill_rect(
                    Rect {
                        x: r.x,
                        y: r.y + r.h as i32 / 2,
                        w: r.w,
                        h: 2,
                    },
                    color.unwrap_or(ink),
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
        let header = self
            .brightness
            .map(|level| tr_format(&tr("Brightness: %1%"), &[&level.to_string()]));
        let indicator = if self.flipping {
            self.position.map(|(page, count)| {
                tr_format(
                    &tr("Page %1 of %2"),
                    &[&(page + 1).to_string(), &count.to_string()],
                )
            })
        } else {
            None
        };
        for (text, bottom) in [(header, false), (indicator, true)] {
            if let Some(text) = text {
                let pad = cx.theme.space.sm;
                let style = cx.theme.style(&cx.theme.font.body, Tone::Ink);
                let text = cx
                    .fonts
                    .layout(&text, &style, bounds.w.saturating_sub(4 * pad), Some(1), true);
                let h = cx.theme.touch_target.min(bounds.h);
                let rect = Rect {
                    x: bounds.x,
                    y: if bottom {
                        bounds.y + bounds.h as i32 - h as i32
                    } else {
                        bounds.y
                    },
                    w: bounds.w,
                    h,
                };
                cx.fill_rect(rect, surface);
                cx.text(
                    &text,
                    Point::new(
                        bounds.x + pad as i32,
                        rect.y + h.saturating_sub(text.height) as i32 / 2,
                    ),
                );
            }
        }
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let bounds = cx.bounds();
        match ev {
            UiEvent::Named(e) if matches!(e.name.as_str(), "PageForward" | "PageBack") => {
                let forward = e.name == "PageForward";
                cx.action(action(
                    if forward { "reader:next" } else { "reader:prev" },
                    Vec::new(),
                ));
                Handled::Yes
            }
            UiEvent::Named(e) if e.name == "ReaderPageKey" => {
                let (Some(forward), Some(pressed)) = (
                    e.args.first().and_then(Value::as_bool),
                    e.args.get(1).and_then(Value::as_bool),
                ) else {
                    return Handled::No;
                };
                self.event(
                    cx,
                    &UiEvent::Key(kaia_input::KeyEvent {
                        key: if forward { Key::RPgFwd } else { Key::RPgBack },
                        pressed,
                        time_us: 0,
                    }),
                )
            }
            UiEvent::Key(key)
                if matches!(key.key, Key::LPgFwd | Key::RPgFwd | Key::LPgBack | Key::RPgBack) =>
            {
                let forward = matches!(key.key, Key::LPgFwd | Key::RPgFwd);
                if !key.pressed {
                    if self.held_key == Some(forward) {
                        self.held_key = None;
                        self.repeats = 0;
                        cx.action(action("reader:flip:stop", Vec::new()));
                    }
                } else if self.held_key == Some(forward) {
                    self.repeats = self.repeats.saturating_add(1);
                    if self.repeats == 1 {
                        cx.action(action(
                            if forward { "reader:next" } else { "reader:prev" },
                            Vec::new(),
                        ));
                    } else if self.repeats == 5 {
                        cx.action(action(
                            "reader:flip",
                            vec![Value::Number(if forward { 4.0 } else { 0.0 })],
                        ));
                    }
                } else {
                    self.held_key = Some(forward);
                    self.repeats = 0;
                    cx.action(action("reader:flip:stop", Vec::new()));
                    cx.action(action(
                        if forward { "reader:next" } else { "reader:prev" },
                        Vec::new(),
                    ));
                }
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Touch { .. }) => {
                self.light_drag = None;
                self.touch_flip = false;
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Pan { pos, start_pos, .. }) => {
                let vertical = (pos.y - start_pos.y).abs() >= (pos.x - start_pos.x).abs();
                let edge = start_pos.x >= bounds.x
                    && i64::from(start_pos.x - bounds.x) * 100 < i64::from(bounds.w) * 13;
                if *self.light_drag.get_or_insert(edge && vertical) {
                    self.light_start = *start_pos;
                    self.brightness_action(cx, *start_pos, *pos, false);
                    Handled::Yes
                } else {
                    Handled::No
                }
            }
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                cx.action(action("reader:tap", point_args(*pos, bounds)));
                Handled::Yes
            }
            // Pinch and spread change the text size of a reflowable book. The reader
            // ignores them on a fixed-layout book.
            UiEvent::Gesture(Gesture::Pinch { distance, axis, .. }) => {
                cx.action(size_action("reader:size:smaller", *distance, *axis));
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Spread { distance, axis, .. }) => {
                cx.action(size_action("reader:size:larger", *distance, *axis));
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Swipe {
                pos,
                end_pos,
                direction,
                ..
            }) => {
                if self.light_drag == Some(true) {
                    self.brightness_action(cx, *pos, *end_pos, true);
                    self.light_drag = None;
                    return Handled::Yes;
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
                if i64::from(pos.y - bounds.y) * 10 >= i64::from(bounds.h) * 9 {
                    self.touch_flip = true;
                    self.flip_action(cx, *pos);
                    return Handled::Yes;
                }
                cx.action(action("reader:hold", point_args(*pos, bounds)));
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::HoldPan { pos, .. }) => {
                if self.touch_flip {
                    self.flip_action(cx, *pos);
                    return Handled::Yes;
                }
                cx.action(action("reader:drag", point_args(*pos, bounds)));
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::HoldRelease { .. }) => {
                if self.touch_flip {
                    self.touch_flip = false;
                    cx.action(action("reader:flip:stop", Vec::new()));
                    return Handled::Yes;
                }
                cx.action(action("reader:release", Vec::new()));
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::PanRelease { pos, .. }) if self.light_drag == Some(true) => {
                self.brightness_action(cx, self.light_start, *pos, true);
                self.light_drag = None;
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Multiswipe { pos, end_pos, .. }) if self.light_drag == Some(true) => {
                self.brightness_action(cx, *pos, *end_pos, true);
                self.light_drag = None;
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}
