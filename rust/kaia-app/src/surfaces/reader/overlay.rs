//! Reader status, tools, seek strip and chapter navigation.

use std::cell::RefCell;
use std::rc::Rc;

use crate::slider::Slider;
use kaia_events::ShowOpts;
use kaia_gfx::Size;
use kaia_i18n::tr;
use kaia_ui::components::Label;
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
    pub chapter: String,
    pub fixed_layout: bool,
    pub clock: String,
    pub battery: String,
    /// The page shown, 0-based.
    pub page: u32,
    pub page_count: usize,
    /// The single position label, using logical EPUB or fixed-layout pages.
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

/// The menu overlays the existing page without changing its viewport.
pub fn build(services: &Services, screen: Size, model: &OverlayModel) -> Window {
    use super::chrome::{rect, Chrome, Tool};
    use kaia_ui::controls::IconButton;
    let t = &services.theme;
    let px = 48 * screen.w.min(screen.h) / 1264;
    let mut font = t.font.caption.clone();
    font.size = 30.0 * screen.w.min(screen.h) as f32 / 1264.0;
    let mut chapter_font = font.clone();
    chapter_font.size *= 1.13;
    chapter_font.weight = 700;
    let mut children = Vec::new();
    let mut put = |x, y, w, h, pod| children.push((rect(screen, x, y, w, h), pod));
    put(
        50,
        20,
        300,
        60,
        WidgetPod::new(Label::new(model.clock.clone(), font.clone(), Tone::Ink)).with_key("overlay-clock"),
    );
    put(
        1100,
        20,
        130,
        60,
        WidgetPod::new(Label::new(model.battery.clone(), font.clone(), Tone::Ink)),
    );
    let mut home = Tool::new("chevron.left", "nav:home", px);
    home.label = tr("Back to Home");
    put(
        34,
        85,
        570,
        88,
        WidgetPod::new(home).focusable(true).with_key("overlay-home"),
    );
    for (x, icon, action, key) in [
        (706, "edit", "reader:ink:toggle", "overlay-pen"),
        (
            798,
            "appbar.rotation",
            "settings:quick:orientation",
            "overlay-orientation",
        ),
        (888, "appbar.contrast", "settings:quick:open", "overlay-light"),
        (974, "appbar.typeset", "reader:panel:layout", "overlay-layout"),
        (1060, "appbar.settings", "settings:reading", "overlay-settings"),
        (1155, "appbar.menu", "reader:panel:more", "overlay-more"),
        (
            40,
            "chevron.first",
            "reader:jump:previous",
            "overlay-jump-previous",
        ),
        (1136, "chevron.last", "reader:jump:next", "overlay-jump-next"),
        (
            48,
            "appbar.navigation",
            "reader:panel:contents",
            "overlay-contents",
        ),
        (1048, "book.opened", "reader:contents:tab:2", "overlay-notes"),
        (1128, "appbar.search", "reader:search:open", "overlay-search"),
    ] {
        let original_key = key;
        let (icon, action) = if model.fixed_layout {
            match key {
                "overlay-pen" => ("appbar.pagefit", "reader:panel:layout"),
                "overlay-orientation" => ("edit", "reader:ink:toggle"),
                "overlay-light" => ("appbar.rotation", "settings:quick:orientation"),
                "overlay-layout" => ("appbar.contrast", "settings:quick:open"),
                _ => (icon, action),
            }
        } else {
            (icon, action)
        };
        let key = if model.fixed_layout {
            match original_key {
                "overlay-pen" => "overlay-zoom",
                "overlay-orientation" => "overlay-pen",
                "overlay-light" => "overlay-orientation",
                "overlay-layout" => "overlay-light",
                _ => original_key,
            }
        } else {
            original_key
        };
        let y = if key.contains("jump") {
            1464
        } else if matches!(key, "overlay-contents" | "overlay-notes" | "overlay-search") {
            1560
        } else {
            85
        };
        put(
            x,
            y,
            88,
            88,
            WidgetPod::new(Tool::new(icon, action, px))
                .focusable(true)
                .with_key(key),
        );
    }
    put(
        200,
        1390,
        864,
        64,
        WidgetPod::new(kaia_ui::components::Align::new(
            WidgetPod::new(Label::new(model.footer.clone(), font.clone(), Tone::Ink))
                .with_key("overlay-footer"),
            kaia_ui::geometry::Alignment::Center,
            kaia_ui::geometry::Alignment::Center,
        )),
    );
    put(
        610,
        85,
        88,
        88,
        WidgetPod::new(kaia_ui::components::Row::new(vec![WidgetPod::new(
            IconButton::new("chevron.left").on_action("reader:back"),
        )
        .with_key("overlay-back")
        .visible(model.can_go_back)]))
        .with_key("overlay-history"),
    );

    put(
        106,
        1464,
        1052,
        96,
        WidgetPod::new(
            Slider::new(
                f64::from(model.page + 1),
                1.0,
                model.page_count.max(1) as f64,
                1.0,
            )
            .reader()
            .on_action("reader:slider"),
        )
        .with_key("overlay-slider"),
    );
    put(
        140,
        1572,
        860,
        72,
        WidgetPod::new(Label::new(model.chapter.clone(), chapter_font, Tone::Ink))
            .with_key("overlay-chapter"),
    );
    window(
        services,
        screen,
        Chrome {
            size: screen,
            children,
            panels: vec![rect(screen, 0, 0, 1264, 179), rect(screen, 0, 1344, 1264, 336)],
        },
    )
}

/// The label of the bookmark button. It names the state of the current page.
pub fn bookmark_label(bookmarked: bool) -> String {
    if bookmarked {
        tr("Bookmarked")
    } else {
        tr("Bookmark")
    }
}
