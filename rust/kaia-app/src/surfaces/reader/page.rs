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

use crate::surfaces::settings::keys::DEFAULT_HIGHLIGHT_SHADE;

/// The key of the page widget. The reader surface finds it with `Window::update`.
pub const PAGE_KEY: &str = "reader-page";

/// How a highlight is drawn. On a color panel, a mark with a color takes that color.
/// Grayscale panels and marks without a color draw the shape alone. A box is dimmed
/// and a line is ink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkStyle {
    /// A short bar in the right margin, beside the first line of a highlight that has
    /// a note. The rect is that first line. KOReader draws the same mark.
    Note,
    /// A box behind the text (KOReader's lighten). Tinted by its color on a color
    /// panel, dimmed otherwise. The highlight shade sets the strength of both.
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
    pub ink: Option<super::ink::PageInk>,
    pub ink_on: bool,
    pub ink_pencil: bool,
    pub ink_width: f32,
    pub ink_color: String,
    pub ink_eraser: bool,
    #[cfg(feature = "lua")]
    pub plugins: Option<std::rc::Weak<std::cell::RefCell<kaia_lua::LuaUi>>>,
    /// The rendered page, or None when the page cannot be drawn.
    pub page: Option<Arc<Pixmap>>,
    /// Highlights on the page.
    pub marks: Vec<Mark>,
    /// True when the page is bookmarked. A bookmarked page shows a dog-ear in its top right corner.
    pub bookmarked: bool,
    pub stickers: Vec<super::stickers::Drawing>,
    /// The selected word, one box per line.
    pub selection: Vec<Rect>,
    /// Search hits on the page.
    pub hits: Vec<Rect>,
    /// Text shown in place of the page, such as "Cannot open this book."
    pub message: Option<String>,
    /// The page shown (0-based) and the page count. Tests read the position here.
    pub position: Option<(u32, usize)>,
    /// The persistent footer, rebuilt from the current page on every frame.
    pub footer: String,
    /// The title at the left and the chapter or device status at the right.
    pub header: (String, String),
    header_content: Option<(super::header::Header, String, kaia_book::PageInfo)>,
    pub header_height: u32,
    footer_content: Option<(super::footer::Footer, kaia_book::PageInfo, f64)>,
    pub(crate) footer_battery: Option<u8>,
    /// The strip reserved by the book layout, including its vertical space.
    pub footer_height: u32,
    /// The shade of box highlights, from 0 to 1. See [`shade_tint`] and [`dim_amount`].
    pub highlight_shade: f64,
    pub brightness: Option<u8>,
    pub flipping: bool,
    pub fixed_layout: bool,
    pub zoomed: bool,
    pub zoom_area: bool,
    zoom_drag: Option<(Point, Point)>,
    zoom_pinch: Option<(f32, Point, Point)>,
    light_drag: Option<bool>,
    pub brightness_swipe: bool,
    light_start: Point,
    touch_flip: bool,
    held_key: Option<bool>,
    repeats: u32,
}

impl PageView {
    pub fn new() -> Self {
        Self {
            ink: None,
            ink_on: false,
            ink_pencil: false,
            ink_width: 3.0,
            ink_color: "black".into(),
            ink_eraser: false,
            page: None,
            #[cfg(feature = "lua")]
            plugins: None,
            marks: Vec::new(),
            bookmarked: false,
            stickers: Vec::new(),
            selection: Vec::new(),
            hits: Vec::new(),
            message: None,
            position: None,
            footer: String::new(),
            header: (String::new(), String::new()),
            header_content: None,
            header_height: 0,
            footer_content: None,
            footer_battery: None,
            footer_height: 0,
            highlight_shade: DEFAULT_HIGHLIGHT_SHADE,
            brightness: None,
            flipping: false,
            fixed_layout: false,
            zoomed: false,
            zoom_area: false,
            zoom_drag: None,
            zoom_pinch: None,
            light_drag: None,
            brightness_swipe: true,
            light_start: Point::new(0, 0),
            touch_flip: false,
            held_key: None,
            repeats: 0,
        }
    }

    fn finish_zoom_drag(&mut self, cx: &mut EventCx, start: Point, end: Point, bounds: Rect) {
        let mut args = point_args(start, bounds);
        args.extend(point_args(end, bounds));
        args.push(Value::Bool(self.zoom_area));
        self.zoom_area = false;
        self.zoom_drag = None;
        cx.action(action("reader:zoom:drag", args));
    }

    fn finish_zoom_pinch(&mut self, cx: &mut EventCx, bounds: Rect) {
        self.zoom_drag = None;
        self.light_drag = None;
        if let Some((ratio, start, end)) = self.zoom_pinch.take() {
            let mut args = vec![Value::Number(f64::from(ratio))];
            args.extend(point_args(start, bounds));
            args.extend(point_args(end, bounds));
            cx.action(action("reader:zoom:pinch", args));
        }
    }

    pub(super) fn set_header(
        &mut self,
        header: super::header::Header,
        title: String,
        info: kaia_book::PageInfo,
    ) {
        self.header = header.text(&title, &info, &super::footer::Status::default());
        self.header_content = Some((header, title, info));
    }

    pub(super) fn set_footer(&mut self, footer: super::footer::Footer, info: kaia_book::PageInfo, pace: f64) {
        if !footer.is_visible() {
            self.footer.clear();
        }
        self.footer_content = Some((footer, info, pace));
    }

    pub(super) fn cancel_gestures(&mut self) {
        self.zoom_drag = None;
        self.zoom_pinch = None;
        self.zoom_area = false;
        self.light_drag = None;
        self.touch_flip = false;
        self.held_key = None;
        self.repeats = 0;
    }

    /// The page image centred between the header and footer, as `paint` draws it.
    /// The book area when no page is shown.
    /// Where the page image is drawn inside `bounds`, below the header strip and
    /// above the footer strip. Tests compare it with the ink area.
    pub fn page_area(&self, bounds: Rect) -> Rect {
        self.placed(bounds)
    }

    fn placed(&self, bounds: Rect) -> Rect {
        let bounds = self.content_bounds(bounds);
        match &self.page {
            Some(page) => Rect {
                x: bounds.x + (bounds.w as i32 - page.width() as i32) / 2,
                y: bounds.y + (bounds.h as i32 - page.height() as i32) / 2,
                w: page.width(),
                h: page.height(),
            },
            None => bounds,
        }
    }

    fn content_bounds(&self, bounds: Rect) -> Rect {
        Rect {
            y: bounds.y + self.header_height as i32,
            h: bounds.h.saturating_sub(self.header_height + self.footer_height),
            ..bounds
        }
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

/// The darkening of a box without a color, the `amount` for `PaintCx::dim`.
/// The amount is 200 times the shade, so the default 0.2 gives 40, the amount the page
/// used before the shade setting. Each step of 0.1 adds 20. A shade of 1 gives 200.
pub fn dim_amount(shade: f64) -> u8 {
    (shade * 200.0).round().clamp(0.0, 255.0) as u8
}

/// The factor a box with a color multiplies the page by. Each channel of the factor is
/// 255 minus the channel's distance from white, times `shade / 0.2`. At the default shade
/// the factor is the color itself, so the default draws the page as it did before the
/// setting. A shade of 0.1 moves the factor halfway to white and pales the color. A shade
/// above 0.2 moves it past the color toward black, and a channel stops at 0.
pub fn shade_tint(tint: Color, shade: f64) -> Color {
    let scale = shade / DEFAULT_HIGHLIGHT_SHADE;
    let channel = |c: u8| (255.0 - (255.0 - f64::from(c)) * scale).round().clamp(0.0, 255.0) as u8;
    Color::rgb(channel(tint.r), channel(tint.g), channel(tint.b))
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

        let content = self.content_bounds(bounds);
        match &self.page {
            Some(page) => {
                let placed = self.placed(bounds);
                cx.clipped(content, |cx| cx.blit_pixmap(page, Point::new(placed.x, placed.y)));
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
        let page_rect = self.placed(bounds);
        for mark in &self.marks {
            let r = mark.rect;
            // Only a color panel shows the highlight color. A grayscale panel draws the shape alone.
            let color = if cx.theme.color_panel { mark.color } else { None };
            match mark.style {
                MarkStyle::Note => {
                    // The bar sits a gap inside the page edge, so it stays in the margin.
                    let bar = cx.theme.line.focus.max(1);
                    let gap = cx.theme.space.xs;
                    let x = page_rect.x + page_rect.w as i32 - (gap + bar) as i32;
                    cx.fill_rect(
                        Rect {
                            x,
                            y: r.y,
                            w: bar,
                            h: r.h,
                        },
                        ink,
                    );
                }
                MarkStyle::Box => match color {
                    Some(tint) => cx.multiply_rect(r, shade_tint(tint, self.highlight_shade)),
                    None => cx.dim(r, dim_amount(self.highlight_shade)),
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
        if self.bookmarked {
            // KOReader's dog-ear is 1/40 of the shorter screen side, rounded up.
            let size = bounds.w.min(bounds.h).div_ceil(40);
            // A right triangle in the top right corner of the page: each row is one pixel high,
            // and the rows get shorter going down.
            for row in 0..size {
                let width = size - row;
                let x = page_rect.x + page_rect.w as i32 - width as i32;
                cx.fill_rect(
                    Rect {
                        x,
                        y: page_rect.y + row as i32,
                        w: width,
                        h: 1,
                    },
                    ink,
                );
            }
        }
        if self.zoom_area {
            if let Some((start, end)) = self.zoom_drag {
                cx.border(
                    Rect::new(
                        start.x.min(end.x),
                        start.y.min(end.y),
                        start.x.abs_diff(end.x),
                        start.y.abs_diff(end.y),
                    ),
                    0,
                    3,
                    ink,
                );
            }
        }
        for rect in &self.hits {
            cx.border(*rect, 0, 3, ink);
        }
        for rect in &self.selection {
            cx.dim(*rect, 80);
            cx.border(*rect, 0, 2, ink);
        }
        // Kaia's header and footer go under stickers and plugin overlays, so a plugin can draw
        // in the footer strip as it does in KOReader.
        if self.footer_height > 0 {
            if let Some((footer, info, pace)) = &self.footer_content {
                self.footer = super::footer::page_line(
                    footer,
                    info,
                    *pace,
                    &super::footer::Status::now(self.footer_battery),
                );
            }
            let rect = Rect::new(
                bounds.x,
                content.y + content.h as i32,
                bounds.w,
                self.footer_height,
            );
            cx.fill_rect(rect, surface);
            let style = cx.theme.style(&cx.theme.font.caption, Tone::InkSecondary);
            let margin = cx.theme.space.lg;
            let text = cx.fonts.layout(
                &self.footer,
                &style,
                bounds.w.saturating_sub(2 * margin),
                Some(1),
                true,
            );
            cx.clipped(rect, |cx| {
                cx.text(
                    &text,
                    Point::new(
                        bounds.x + bounds.w.saturating_sub(text.width) as i32 / 2,
                        rect.y + rect.h.saturating_sub(text.height) as i32 / 2,
                    ),
                );
            });
        }
        if self.header_height > 0 {
            if let Some((header, title, info)) = &self.header_content {
                self.header = header.text(title, info, &super::footer::Status::now(self.footer_battery));
            }
            let rect = Rect::new(bounds.x, bounds.y, bounds.w, self.header_height);
            cx.fill_rect(rect, surface);
            let style = cx.theme.style(&cx.theme.font.caption, Tone::InkSecondary);
            let margin = cx.theme.space.lg;
            let available = bounds.w.saturating_sub(2 * margin);
            let width = if !self.header.0.is_empty() && !self.header.1.is_empty() {
                available.saturating_sub(cx.theme.space.md) / 2
            } else {
                available
            };
            for (value, right) in [(&self.header.0, false), (&self.header.1, true)] {
                let text = cx.fonts.layout(value, &style, width, Some(1), true);
                let x = if right {
                    bounds.x + bounds.w.saturating_sub(margin + text.width) as i32
                } else {
                    bounds.x + margin as i32
                };
                cx.clipped(rect, |cx| {
                    cx.text(
                        &text,
                        Point::new(x, rect.y + rect.h.saturating_sub(text.height) as i32 / 2),
                    );
                });
            }
        }
        if let Some(ink) = &self.ink {
            ink.paint(cx);
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
            UiEvent::Pen(pen) => {
                if self.ink_on
                    && (pen.phase != kaia_input::Phase::Down || pen.pos.y >= cx.theme().touch_target as i32)
                {
                    if let Some(ink) = &mut self.ink {
                        match ink.pen(*pen, self.ink_width, &self.ink_color, self.ink_eraser) {
                            Ok(Some((rect, hint))) => {
                                cx.request_paint_rect(rect, hint);
                                if self.ink_pencil && pen.phase == kaia_input::Phase::Up {
                                    cx.action(action(
                                        "reader:ink:pencil-bookmark",
                                        vec![Value::Bool(!ink.strokes.is_empty())],
                                    ));
                                }
                            }
                            Err(e) => {
                                cx.request_paint();
                                cx.action(action("reader:ink:error", vec![Value::Str(e)]));
                            }
                            _ => {}
                        }
                    }
                }
                Handled::Yes
            }
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
                self.zoom_drag = None;
                self.zoom_pinch = None;
                self.light_drag = None;
                self.touch_flip = false;
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Pan { pos, start_pos, .. })
                if self.fixed_layout && (self.zoomed || self.zoom_area) =>
            {
                cx.action(action("reader:zoom:begin", Vec::new()));
                self.zoom_drag = Some((*start_pos, *pos));
                if self.zoom_area {
                    cx.request_paint_with(kaia_ui::widget::PaintHint::Fast);
                }
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::PanRelease { pos, .. }) if self.zoom_drag.is_some() => {
                let (start, _) = self.zoom_drag.unwrap();
                self.finish_zoom_drag(cx, start, *pos, bounds);
                Handled::Yes
            }
            UiEvent::Gesture(
                Gesture::Swipe { pos, end_pos, .. } | Gesture::Multiswipe { pos, end_pos, .. },
            ) if self.fixed_layout && (self.zoomed || self.zoom_area) => {
                self.finish_zoom_drag(cx, *pos, *end_pos, bounds);
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::InwardPan {
                pos,
                start_pos,
                span,
                start_span,
                ..
            }) if self.fixed_layout => {
                cx.action(action("reader:zoom:begin", Vec::new()));
                self.zoom_pinch =
                    (*start_span > 0.0).then_some((*span as f32 / *start_span as f32, *start_pos, *pos));
                self.zoom_drag = None;
                self.light_drag = None;
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::OutwardPan {
                pos,
                end_pos,
                span,
                start_span,
                ..
            }) if self.fixed_layout => {
                cx.action(action("reader:zoom:begin", Vec::new()));
                self.zoom_pinch =
                    (*start_span > 0.0).then_some((*span as f32 / *start_span as f32, *pos, *end_pos));
                self.zoom_drag = None;
                self.light_drag = None;
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Pinch {
                pos,
                start_pos,
                span,
                start_span,
                ..
            }) if self.fixed_layout => {
                cx.action(action("reader:zoom:begin", Vec::new()));
                self.zoom_pinch =
                    (*start_span > 0.0).then_some((*span as f32 / *start_span as f32, *start_pos, *pos));
                self.finish_zoom_pinch(cx, bounds);
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Spread {
                pos,
                end_pos,
                span,
                start_span,
                ..
            }) if self.fixed_layout => {
                cx.action(action("reader:zoom:begin", Vec::new()));
                self.zoom_pinch =
                    (*start_span > 0.0).then_some((*span as f32 / *start_span as f32, *pos, *end_pos));
                self.finish_zoom_pinch(cx, bounds);
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::HoldPan {
                pos,
                start_pos,
                end_pos,
                span: Some(span),
                start_span: Some(start_span),
                ..
            }) if self.fixed_layout => {
                cx.action(action("reader:zoom:begin", Vec::new()));
                self.zoom_pinch = (*start_span > 0.0).then_some((
                    *span as f32 / *start_span as f32,
                    start_pos.unwrap_or(*pos),
                    end_pos.unwrap_or(*pos),
                ));
                self.zoom_drag = None;
                self.light_drag = None;
                Handled::Yes
            }
            UiEvent::Gesture(
                Gesture::TwoFingerPanRelease { .. } | Gesture::TwoFingerHoldPanRelease { .. },
            ) if self.fixed_layout => {
                self.finish_zoom_pinch(cx, bounds);
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::Pan { pos, start_pos, .. }) => {
                let vertical = (pos.y - start_pos.y).abs() >= (pos.x - start_pos.x).abs();
                let edge = start_pos.x >= bounds.x
                    && i64::from(start_pos.x - bounds.x) * 100 < i64::from(bounds.w) * 13;
                if *self
                    .light_drag
                    .get_or_insert(edge && vertical && self.brightness_swipe)
                {
                    self.light_start = *start_pos;
                    self.brightness_action(cx, *start_pos, *pos, false);
                    Handled::Yes
                } else {
                    Handled::No
                }
            }
            UiEvent::Gesture(Gesture::Tap { .. }) if self.zoom_area => Handled::Yes,
            UiEvent::Gesture(Gesture::Tap { pos, .. }) => {
                cx.action(action("reader:tap", point_args(*pos, bounds)));
                Handled::Yes
            }
            // Reflowable books keep the font-size gesture.
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
            UiEvent::Gesture(Gesture::Hold { pos, .. }) if self.zoom_area => {
                self.zoom_drag = Some((*pos, *pos));
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::HoldPan { pos, start_pos, .. }) if self.zoom_area => {
                let start = self
                    .zoom_drag
                    .map(|(start, _)| start)
                    .or(*start_pos)
                    .unwrap_or(*pos);
                self.zoom_drag = Some((start, *pos));
                cx.request_paint_with(kaia_ui::widget::PaintHint::Fast);
                Handled::Yes
            }
            UiEvent::Gesture(Gesture::HoldRelease { pos, .. }) if self.zoom_area => {
                if let Some((start, _)) = self.zoom_drag {
                    self.finish_zoom_drag(cx, start, *pos, bounds);
                }
                Handled::Yes
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
