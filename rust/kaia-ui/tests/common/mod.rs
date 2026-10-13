//! Shared helpers for the kaia-ui integration tests.
#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use kaia_gfx::{Point, Rect, Size};
use kaia_input::Gesture;
use kaia_ui::assets::{Assets, SharedAssets};
use kaia_ui::geometry::Constraints;
use kaia_ui::theme::{Scale, Theme};
use kaia_ui::widget::{EventCx, Handled, LayoutCx, PaintCx, UiEvent, Widget};
use kaia_ui::WidgetPod;

/// The Libra Colour panel.
pub const SCREEN: Size = Size::new(1264, 1680);

pub const FONT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/fonts/noto");
pub const ICON_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../resources/icons/mdlight");

/// Loads the bundled fonts and icons. Panics when the resources are missing.
pub fn assets() -> SharedAssets {
    Assets::load(FONT_DIR.as_ref(), ICON_DIR.as_ref())
        .unwrap_or_else(|e| panic!("cannot load assets: {e}"))
        .shared()
}

/// DESIGN.md tokens at 1264 x 1680 with a 300 DPI override.
pub fn theme(color_panel: bool) -> Theme {
    Theme::new(
        Scale {
            width: SCREEN.w,
            height: SCREEN.h,
            dpi: Some(300),
        },
        color_panel,
    )
}

/// Shared log of strings, written by test widgets.
pub type Log = Rc<RefCell<Vec<String>>>;

pub fn new_log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

/// Bounds of named probes, recorded at paint time.
pub type Bounds = Rc<RefCell<Vec<(String, Rect)>>>;

pub fn new_bounds() -> Bounds {
    Rc::new(RefCell::new(Vec::new()))
}

/// The recorded bounds of the probe called `name`. Panics when it was not painted.
pub fn bounds_of(bounds: &Bounds, name: &str) -> Rect {
    bounds
        .borrow()
        .iter()
        .rev()
        .find(|(n, _)| n == name)
        .map(|(_, r)| *r)
        .unwrap_or_else(|| panic!("probe {name} was not painted"))
}

/// A fixed-size widget that records its bounds when painted.
pub struct Probe {
    name: &'static str,
    size: Size,
    flex: u32,
    bounds: Bounds,
}

impl Probe {
    pub fn new(name: &'static str, w: u32, h: u32, bounds: &Bounds) -> Self {
        Self {
            name,
            size: Size::new(w, h),
            flex: 0,
            bounds: Rc::clone(bounds),
        }
    }

    pub fn flex(mut self, weight: u32) -> Self {
        self.flex = weight;
        self
    }
}

impl Widget for Probe {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(self.size)
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        self.bounds
            .borrow_mut()
            .push((self.name.to_string(), cx.bounds()));
    }

    fn flex(&self) -> u32 {
        self.flex
    }
}

/// A widget that records what reaches it. It returns `handle` for every
/// event and can be told to request focus on a tap.
pub struct Tapper {
    pub name: &'static str,
    pub size: Size,
    pub handle: bool,
    pub focusable: bool,
    pub focus_on_tap: bool,
    /// Asks for focus on a tap with `request_focus_if_focusable`.
    pub soft_focus_on_tap: bool,
    pub log: Log,
}

impl Tapper {
    pub fn new(name: &'static str, w: u32, h: u32, log: &Log) -> Self {
        Self {
            name,
            size: Size::new(w, h),
            handle: false,
            focusable: false,
            focus_on_tap: false,
            soft_focus_on_tap: false,
            log: Rc::clone(log),
        }
    }

    pub fn handling(mut self) -> Self {
        self.handle = true;
        self
    }
}

impl Widget for Tapper {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(self.size)
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let label = match ev {
            UiEvent::Gesture(g) => format!("{} {}", self.name, g.name()),
            UiEvent::Key(k) => format!("{} key {}", self.name, k.key.name()),
            UiEvent::Named(e) => format!("{} named {}", self.name, e.name),
            UiEvent::FocusGained => format!("{} gained", self.name),
            UiEvent::FocusLost => format!("{} lost", self.name),
        };
        self.log.borrow_mut().push(label);
        if self.focus_on_tap && matches!(ev, UiEvent::Gesture(Gesture::Tap { .. })) {
            cx.request_focus();
        }
        if self.soft_focus_on_tap && matches!(ev, UiEvent::Gesture(Gesture::Tap { .. })) {
            cx.request_focus_if_focusable();
        }
        if self.handle {
            Handled::Yes
        } else {
            Handled::No
        }
    }
}

/// A widget whose children are fixed in position: a container for Tapper tests.
pub struct Holder {
    pub children: Vec<WidgetPod>,
    pub size: Size,
    pub name: &'static str,
    pub log: Log,
    pub handle: bool,
}

impl Widget for Holder {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let mut y = 0;
        for child in &mut self.children {
            let s = child.layout(cx, c.loosen());
            child.set_origin(Point::new(0, y));
            y += s.h as i32;
        }
        c.constrain(self.size)
    }

    fn event(&mut self, _cx: &mut EventCx, ev: &UiEvent) -> Handled {
        let label = match ev {
            UiEvent::Gesture(g) => format!("{} {}", self.name, g.name()),
            UiEvent::Key(k) => format!("{} key {}", self.name, k.key.name()),
            UiEvent::Named(e) => format!("{} named {}", self.name, e.name),
            UiEvent::FocusGained | UiEvent::FocusLost => return Handled::No,
        };
        self.log.borrow_mut().push(label);
        if self.handle {
            Handled::Yes
        } else {
            Handled::No
        }
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        self.children.iter_mut().collect()
    }
}

pub fn point(x: i32, y: i32) -> Point {
    Point::new(x, y)
}

/// Paints `windows` bottom to top into one frame, as the app does for a stack.
/// Each window paints its full area once, so the windows above only change what they draw.
pub fn paint_stack(windows: &[&kaia_ui::Window], t: &kaia_ui::Theme) -> kaia_gfx::Pixmap {
    use kaia_events::DirtyQueue;
    let mut frame = kaia_gfx::Pixmap::new(SCREEN.w, SCREEN.h, t.pixel_format());
    frame.fill_rect(kaia_gfx::Rect::from_size(SCREEN), t.color(kaia_ui::Tone::Surface));
    let mut dirty = DirtyQueue::new(kaia_events::Rect::new(0, 0, SCREEN.w as i32, SCREEN.h as i32));
    for window in windows {
        window.paint(&mut frame, &mut dirty);
    }
    frame
}

/// The golden file `name` in `tests/golden`.
pub fn golden(name: &str) -> std::path::PathBuf {
    std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden")).join(name)
}

use kaia_events::menu::{SEPARATOR_ID, TABS_ID};
use kaia_events::{MenuBuild, MenuItem, MenuOrder, MenuRegistry};

/// The settings registry: two tabs. Reading has a "Text" section with a
/// submenu for margins, and a night mode leaf. Device has two leaves.
pub fn settings_build() -> MenuBuild {
    let mut reg = MenuRegistry::new();
    reg.register(MenuItem::callback("font_size", "Font size", "settings.font_size"));
    reg.register(MenuItem::callback("justify", "Justify text", "settings.justify"));
    reg.register(MenuItem::callback("hyphenate", "Hyphenate", "settings.hyphenate"));
    reg.register(MenuItem::callback("top", "Top margin", "settings.margin_top"));
    reg.register(MenuItem::callback("side", "Side margin", "settings.margin_side"));
    reg.register(MenuItem::submenu("margins", "Margins", vec![]));
    reg.register(MenuItem::submenu("text", "Text", vec![]));
    reg.register(MenuItem::callback("dark", "Night mode", "settings.night"));
    reg.register(MenuItem::callback("wifi", "Wi-Fi", "settings.wifi"));
    reg.register(MenuItem::callback("sleep", "Sleep timer", "settings.sleep"));
    reg.register(MenuItem::submenu("reading", "Reading", vec![]));
    reg.register(MenuItem::submenu("device", "Device", vec![]));

    let mut order = MenuOrder::new();
    order.set(TABS_ID, &["reading", "device"]);
    order.set("reading", &["text", SEPARATOR_ID, "dark"]);
    order.set("text", &["font_size", "justify", "hyphenate", "margins"]);
    order.set("margins", &["top", "side"]);
    order.set("device", &["wifi", "sleep"]);
    reg.build(&order)
}

use std::collections::HashMap;
use std::time::Duration;

use kaia_events::{Ctx, ManualClock, Refresh, ShowOpts, WindowId, WindowStack};
use kaia_gfx::Pixmap;
use kaia_ui::theme::Tone;
use kaia_ui::Window;

pub fn ev_rect(r: Rect) -> kaia_events::Rect {
    kaia_events::Rect::new(r.x, r.y, r.w as i32, r.h as i32)
}

/// A small copy of the app's loop: apply requests, ask the stack for the paint
/// plan, paint the visible stack with `Window::paint_stack`, take the refreshes.
pub struct MiniApp {
    pub clock: ManualClock,
    pub ctx: Ctx,
    pub stack: WindowStack,
    pub windows: HashMap<WindowId, Window>,
    pub frame: Pixmap,
    pub last: Vec<Refresh>,
    pub theme: Theme,
    pub assets: SharedAssets,
}

impl MiniApp {
    pub fn new() -> Self {
        let clock = ManualClock::new();
        let ctx = Ctx::new(Box::new(clock.clone()), ev_rect(Rect::from_size(SCREEN)));
        let theme = theme(false);
        let mut frame = Pixmap::new(SCREEN.w, SCREEN.h, theme.pixel_format());
        frame.fill_rect(Rect::from_size(SCREEN), theme.color(Tone::Surface));
        Self {
            clock,
            ctx,
            stack: WindowStack::new(),
            windows: HashMap::new(),
            frame,
            last: Vec::new(),
            theme,
            assets: assets(),
        }
    }

    pub fn show(&mut self, w: &Window, opts: ShowOpts) -> WindowId {
        let id = w.show(&mut self.ctx, opts);
        self.windows.insert(id, w.clone());
        self.stack.apply_requests(&mut self.ctx);
        id
    }

    /// The window for a root widget, sized to the screen.
    pub fn window(&self, root: impl Widget + 'static) -> Window {
        Window::new(root, self.theme.clone(), self.assets.clone(), SCREEN)
    }

    /// Paints what the stack says is dirty. Returns true when anything was painted.
    pub fn repaint(&mut self) -> bool {
        self.stack.apply_requests(&mut self.ctx);
        let plan = self.stack.paint_plan(&mut self.ctx.dirty);
        let mut painted = false;
        if !plan.is_empty() {
            let windows = Window::visible_stack(&self.stack, |id| self.windows.get(&id).cloned());
            painted = Window::paint_stack(&windows, &mut self.frame, &mut self.ctx.dirty);
        }
        self.last = self.ctx.dirty.take_frame(painted);
        painted
    }

    pub fn close(&mut self, id: WindowId) {
        self.ctx.close(id, None);
        self.stack.apply_requests(&mut self.ctx);
    }

    /// Moves the manual clock and runs the tasks that are due.
    pub fn advance(&mut self, secs: f64) {
        self.clock.advance(Duration::from_secs_f64(secs));
        let now = self.ctx.scheduler.now();
        self.ctx.run_due(now);
        self.stack.apply_requests(&mut self.ctx);
    }

    pub fn px(&self, x: i32, y: i32) -> u8 {
        self.frame.get(x, y).unwrap().r
    }

    /// The area the last frame's refreshes cover, in pixels. Overlaps count once per refresh.
    pub fn refreshed_area(&self) -> i64 {
        self.last
            .iter()
            .map(|r| {
                r.region
                    .map_or(SCREEN.w as i64 * SCREEN.h as i64, |g| g.w as i64 * g.h as i64)
            })
            .sum()
    }

    pub fn dark_pixels(&self, r: Rect) -> usize {
        let mut n = 0;
        for y in r.y..r.bottom() {
            for x in r.x..r.right() {
                if self.px(x, y) < 128 {
                    n += 1;
                }
            }
        }
        n
    }
}

/// Counts pixels below mid gray in `r`.
pub fn dark_in(pm: &Pixmap, r: Rect) -> usize {
    let mut n = 0;
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            if pm.get(x, y).is_some_and(|c| c.r < 128) {
                n += 1;
            }
        }
    }
    n
}

/// The leftmost and rightmost columns holding dark pixels in `r`.
pub fn dark_span(pm: &Pixmap, r: Rect) -> Option<(i32, i32)> {
    let mut span: Option<(i32, i32)> = None;
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            if pm.get(x, y).is_some_and(|c| c.r < 128) {
                span = Some(span.map_or((x, x), |(lo, hi)| (lo.min(x), hi.max(x))));
            }
        }
    }
    span
}
