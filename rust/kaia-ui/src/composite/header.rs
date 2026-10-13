//! [`Header`]: the title bar at the top of a surface. It has an optional back
//! chevron on the leading edge, up to two icon actions on the trailing edge,
//! and status text (such as the battery level) before the actions.
//!
//! The title and the icon boxes sit on the page edges. Each touch target hangs
//! over its edge by the space around the icon, so the icon lines up with the
//! content below the header.

use kaia_gfx::text::TextLayout;
use kaia_gfx::{Point, Rect, Size};

use crate::controls::{IconButton, Reaction};
use crate::geometry::Constraints;
use crate::pod::WidgetPod;
use crate::theme::{Theme, Tone};
use crate::widget::{LayoutCx, PaintCx, Widget};

const MAX_ACTIONS: usize = 2;

/// A title bar. In RTL the back chevron and the actions move to the other edge.
pub struct Header {
    title: String,
    status: Option<String>,
    back_icon: &'static str,
    back: Option<WidgetPod>,
    actions: Vec<WidgetPod>,
    /// Layout results for the current title and status. `None` when the text
    /// changed and paint must lay it out again.
    title_tl: Option<TextLayout>,
    status_tl: Option<TextLayout>,
    title_box: Rect,
    status_box: Rect,
}

/// A logical x (counted from the start edge, negative past it) as a physical x
/// in a row of `w`.
fn physical(rtl: bool, w: u32, lx: i32, item_w: u32) -> i32 {
    if rtl {
        w as i32 - lx - item_w as i32
    } else {
        lx
    }
}

impl Header {
    /// A header with `title`. The theme decides the back chevron's direction.
    pub fn new(theme: &Theme, title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            status: None,
            back_icon: if theme.rtl {
                "chevron.right"
            } else {
                "chevron.left"
            },
            back: None,
            actions: Vec::new(),
            title_tl: None,
            status_tl: None,
            title_box: Rect::default(),
            status_box: Rect::default(),
        }
    }

    /// Adds a back chevron that queues an action with this id.
    pub fn back(mut self, action: impl Into<String>) -> Self {
        self.back = Some(WidgetPod::new(
            IconButton::new(self.back_icon).reaction(Reaction::Action(action.into())),
        ));
        self
    }

    /// Adds a back chevron that runs `f` in the widget tree. Menus use it to
    /// return to the page above.
    pub fn back_hook(mut self, f: impl Fn(&mut crate::EventCx<'_>) + 'static) -> Self {
        self.back = Some(WidgetPod::new(
            IconButton::new(self.back_icon).reaction(Reaction::hook(f)),
        ));
        self
    }

    /// Adds an icon action on the trailing edge. The first one added is the
    /// leftmost in LTR. A header holds at most two.
    pub fn action(mut self, icon: impl Into<String>, action: impl Into<String>) -> Self {
        assert!(
            self.actions.len() < MAX_ACTIONS,
            "a header holds two actions at most"
        );
        self.actions.push(WidgetPod::new(
            IconButton::new(icon).reaction(Reaction::Action(action.into())),
        ));
        self
    }

    /// Status text, such as the battery level, drawn before the actions.
    pub fn status(mut self, text: impl Into<String>) -> Self {
        self.status = Some(text.into());
        self
    }

    /// Changes the title. Call `request_layout` in the update when the height may change.
    pub fn set_title(&mut self, text: impl Into<String>) {
        self.title = text.into();
        self.title_tl = None;
    }

    /// Changes or clears the status text. Call `request_layout` in the update when the
    /// status width changes.
    pub fn set_status(&mut self, text: Option<String>) {
        self.status = text;
        self.status_tl = None;
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

impl Widget for Header {
    crate::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> Size {
        let t = cx.theme;
        let w = c.max.w as i32;
        let touch = t.touch_target;
        let icon = t.icon_size as i32;
        // The icon is centred in its touch target, so the target hangs over the
        // edge by `hang`. The icon box then sits exactly on the edge.
        let hang = (touch as i32 - icon) / 2;
        let side = t.space.sm;
        let gap = t.space.md as i32;
        let n = self.actions.len() as i32;

        let status_tl = self.status.as_ref().map(|s| {
            let style = t.style(&t.font.caption, Tone::InkSecondary);
            cx.fonts.layout(s, &style, (w / 4).max(1) as u32, Some(1), true)
        });

        // The first action's icon box starts here. The last one ends at the page edge.
        let icons_left = w - n * touch as i32 + 2 * hang;
        let status_right = if n > 0 { icons_left - gap } else { w };
        let status_w = status_tl.as_ref().map_or(0, |tl| tl.width as i32);
        let status_x = status_right - status_w;
        let title_start = if self.back.is_some() { icon + gap } else { 0 };
        let title_end = match (status_tl.is_some(), n > 0) {
            (true, _) => status_x - gap,
            (false, true) => icons_left - gap,
            (false, false) => w,
        };
        let title_w = (title_end - title_start).max(1) as u32;
        let title_tl = cx.fonts.layout(
            &self.title,
            &t.style(&t.font.title, Tone::Ink),
            title_w,
            Some(2),
            true,
        );

        let inner_h = touch.max(title_tl.height);
        let height = inner_h + 2 * side;
        let band_y = side as i32;
        let btn_y = band_y + (inner_h as i32 - touch as i32) / 2;

        self.title_box = Rect::new(
            physical(t.rtl, c.max.w, title_start, title_w),
            band_y + (inner_h as i32 - title_tl.height as i32) / 2,
            title_w,
            title_tl.height,
        );
        if let Some(back) = &mut self.back {
            back.layout(cx, Constraints::tight(Size::new(touch, touch)));
            back.set_origin(Point::new(physical(t.rtl, c.max.w, -hang, touch), btn_y));
        }
        for (i, pod) in self.actions.iter_mut().enumerate() {
            // Action i is the (n - i)th target from the page edge.
            let lx = w - (n - i as i32) * touch as i32 + hang;
            pod.layout(cx, Constraints::tight(Size::new(touch, touch)));
            pod.set_origin(Point::new(physical(t.rtl, c.max.w, lx, touch), btn_y));
        }
        self.status_box = match &status_tl {
            Some(tl) => Rect::new(
                physical(t.rtl, c.max.w, status_x, tl.width),
                band_y + (inner_h as i32 - tl.height as i32) / 2,
                tl.width,
                tl.height,
            ),
            None => Rect::default(),
        };
        self.title_tl = Some(title_tl);
        self.status_tl = status_tl;
        c.constrain(Size::new(c.max.w, height))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let background = cx.color(cx.background());
        cx.fill_rect(b, background);
        let t = cx.theme;
        if self.title_tl.is_none() {
            let style = t.style(&t.font.title, Tone::Ink);
            self.title_tl =
                Some(
                    cx.fonts
                        .layout(&self.title, &style, self.title_box.w.max(1), Some(2), true),
                );
        }
        if let Some(tl) = &self.title_tl {
            // In RTL a short title sits at the right edge of its box.
            let dx = if t.rtl {
                self.title_box.w as i32 - tl.width as i32
            } else {
                0
            };
            cx.text(
                tl,
                Point::new(b.x + self.title_box.x + dx, b.y + self.title_box.y),
            );
        }
        if let Some(text) = &self.status {
            if self.status_tl.is_none() {
                let style = t.style(&t.font.caption, Tone::InkSecondary);
                let max_w = (b.w / 4).max(1);
                self.status_tl = Some(cx.fonts.layout(text, &style, max_w, Some(1), true));
            }
            if let Some(tl) = &self.status_tl {
                cx.text(tl, Point::new(b.x + self.status_box.x, b.y + self.status_box.y));
            }
        }
    }

    fn children_mut(&mut self) -> Vec<&mut WidgetPod> {
        let mut out: Vec<&mut WidgetPod> = Vec::new();
        if let Some(back) = &mut self.back {
            out.push(back);
        }
        out.extend(self.actions.iter_mut());
        out
    }
}
