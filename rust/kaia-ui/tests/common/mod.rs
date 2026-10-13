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
            log: Rc::clone(log),
        }
    }

    pub fn handling(mut self) -> Self {
        self.handle = true;
        self
    }
}

impl Widget for Tapper {
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
