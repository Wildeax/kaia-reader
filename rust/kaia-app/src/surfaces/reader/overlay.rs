//! The reader overlay: a top bar with Home, the title, search, the bookmark and
//! More, and a bottom bar with the page slider, the footer line and the Contents,
//! Notes, Text and layout and Light buttons. Every bar item is an
//! icon with its label, the same style as the nav bar. The page stays visible
//! between the bars. A tap the bars do not use closes the overlay.

use std::cell::RefCell;
use std::rc::Rc;

use crate::slider::Slider;
use kaia_events::ShowOpts;
use kaia_gfx::Size;
use kaia_i18n::{format as tr_format, tr};
use kaia_ui::components::{Column, CrossAlign, Expanded, Frame, Label, Padding, Row, Spacer};
use kaia_ui::controls::ToolButton;
use kaia_ui::geometry::Insets;
use kaia_ui::{Tone, WidgetPod, Window};

use super::state::Core;
use crate::services::Services;
use crate::surface::{Surface, SurfaceEnv};
use crate::surfaces::common::window;
use crate::surfaces::Route;

/// What the bars show. The reader fills it from the session.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayModel {
    pub title: String,
    /// The page shown, 0-based.
    pub page: u32,
    pub page_count: usize,
    /// The footer line: the items the Footer sheet turned on, such as
    /// `Page 3 of 200 · 12 min left in chapter`. Empty when none is on.
    pub footer: String,
    pub bookmarked: bool,
    pub can_go_back: bool,
}

/// The overlay surface. The reader keeps a handle to its window, so it can update the bars.
pub struct Overlay {
    window: Window,
    serial: u64,
    core: Rc<RefCell<Core>>,
}

impl Overlay {
    pub fn new(window: Window, serial: u64, core: Rc<RefCell<Core>>) -> Self {
        Self { window, serial, core }
    }
}

impl Surface for Overlay {
    fn route(&self) -> Route {
        Route::Overlay
    }

    /// The overlay is not full screen, so the page stays visible under the bars.
    fn show_opts(&self) -> ShowOpts {
        ShowOpts {
            covers_fullscreen: false,
            ..ShowOpts::default()
        }
    }

    fn build(&mut self, _env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(self.window.clone())
    }

    fn closes_on_unused_tap(&self) -> bool {
        true
    }

    fn on_hide(&mut self, _env: &mut SurfaceEnv<'_>) {
        let mut core = self.core.borrow_mut();
        if core.overlay.as_ref().is_some_and(|l| l.serial == self.serial) {
            core.overlay = None;
        }
    }
}

/// One bar item: an icon over its label. Tapping it queues `action`. The item is
/// focusable, so a page-turn key moves focus along the bar before it turns a page.
fn tool(icon: &str, label: &str, action: &str, key: &str) -> WidgetPod {
    WidgetPod::new(ToolButton::new(icon, label).on_action(action))
        .focusable(true)
        .with_key(key)
}

/// The window of the overlay for `model`.
pub fn build(services: &Services, screen: Size, model: &OverlayModel) -> Window {
    let t = &services.theme;
    let sp = t.space.md;
    let bar_pad = Insets::symmetric(t.space.lg, t.space.sm);

    let home = tool("home", &tr("Home"), "nav:home", "overlay-home");
    // A zero-gap group keeps the original bar spacing when Back is hidden.
    let back = WidgetPod::new(Padding::new(
        tool("chevron.left", &tr("Back"), "reader:back", "overlay-back"),
        Insets {
            left: sp,
            ..Insets::all(0)
        },
    ))
    .visible(model.can_go_back);
    let history = WidgetPod::new(
        Row::new(vec![home, back])
            .shrink()
            .cross_align(CrossAlign::Center),
    )
    .with_key("overlay-history");
    let title = WidgetPod::new(Label::new(
        model.title.clone(),
        t.font.body_bold.clone(),
        Tone::Ink,
    ));
    let search = tool(
        "appbar.search",
        &tr("Search"),
        "reader:search:open",
        "overlay-search",
    );
    let bookmark = WidgetPod::new(
        ToolButton::new("bookmark", bookmark_label(model.bookmarked)).on_action("reader:bookmark"),
    )
    .focusable(true)
    .with_key("overlay-bookmark");
    let more = tool("appbar.menu", &tr("More"), "reader:panel:more", "overlay-more");
    let top = WidgetPod::new(
        Row::new(vec![
            history,
            WidgetPod::new(Expanded::new(title, 1)),
            search,
            bookmark,
            tool("light.sun", &tr("Light"), "settings:light:open", "overlay-light"),
            tool(
                "appbar.settings",
                &tr("Reading"),
                "settings:reading:open",
                "overlay-reading-settings",
            ),
            more,
        ])
        .gap(sp)
        .cross_align(CrossAlign::Center),
    );

    let count = model.page_count.max(1);
    let page = model.page;
    let slider = WidgetPod::new(
        Slider::new(f64::from(page + 1), 1.0, count as f64, 1.0)
            .format(move |v| {
                tr_format(
                    &tr("page %1 of %2"),
                    &[&(v as i64).to_string(), &count.to_string()],
                )
            })
            .on_action("reader:slider"),
    )
    .with_key("overlay-slider");
    let footer = WidgetPod::new(Label::new(
        model.footer.clone(),
        t.font.caption.clone(),
        Tone::InkSecondary,
    ))
    .with_key("overlay-footer");
    let tools = WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(
                tool(
                    "appbar.navigation",
                    &tr("Contents"),
                    "reader:panel:contents",
                    "overlay-contents",
                ),
                1,
            )),
            WidgetPod::new(Expanded::new(
                tool("edit", &tr("Notes"), "nav:notes", "overlay-notes"),
                1,
            )),
            WidgetPod::new(Expanded::new(
                tool(
                    "appbar.typeset",
                    &tr("Text and layout"),
                    "reader:panel:layout",
                    "overlay-layout",
                ),
                1,
            )),
        ])
        .gap(t.space.sm),
    );
    let bottom = WidgetPod::new(
        Column::new(vec![slider, footer, tools])
            .gap(sp)
            .cross_align(CrossAlign::Stretch)
            .shrink(),
    );

    let top_bar = WidgetPod::new(
        Frame::new(top)
            .fill(Tone::Surface)
            .border(Tone::Divider, t.line.divider)
            .padding(bar_pad),
    );
    let bottom_bar = WidgetPod::new(
        Frame::new(bottom)
            .fill(Tone::Surface)
            .border(Tone::Divider, t.line.divider)
            .padding(bar_pad),
    );
    let middle = WidgetPod::new(Expanded::new(WidgetPod::new(Spacer::new(1)), 1));
    let root = Column::new(vec![top_bar, middle, bottom_bar]).cross_align(CrossAlign::Stretch);
    window(services, screen, root)
}

/// The label of the bookmark button. It names the state of the current page.
pub fn bookmark_label(bookmarked: bool) -> String {
    if bookmarked {
        tr("Bookmarked")
    } else {
        tr("Bookmark")
    }
}
