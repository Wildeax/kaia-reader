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
    pub ink: bool,
    pub(super) overview: Option<super::zoom::Overview>,
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

    fn on_show(&mut self, env: &mut SurfaceEnv<'_>) {
        clear_external_tools(&self.window, env.ctx);
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
    window(services, screen, chrome(services, screen, model, None))
}

pub(super) fn chrome(
    services: &Services,
    screen: Size,
    model: &OverlayModel,
    active: Option<&str>,
) -> super::chrome::Chrome {
    use super::chrome::{rect, Chrome, Tool};
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
        1170,
        28,
        56,
        32,
        WidgetPod::new(super::compact::Battery {
            percent: model.battery.trim_end_matches('%').parse().ok(),
        })
        .with_key("overlay-battery"),
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
            "reader:tool:orientation",
            "overlay-orientation",
        ),
        (888, "sun", "reader:tool:light", "overlay-light"),
        (974, "appbar.typeset", "reader:panel:layout", "overlay-layout"),
        (1060, "appbar.settings", "settings:reading", "overlay-settings"),
        (1155, "more", "reader:panel:more", "overlay-more"),
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
                "overlay-pen" => ("move", "reader:panel:layout"),
                "overlay-orientation" => ("edit", "reader:ink:toggle"),
                "overlay-light" => ("appbar.rotation", "reader:tool:orientation"),
                "overlay-layout" => ("sun", "reader:tool:light"),
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
        let mut tool = Tool::new(icon, action, px);
        tool.active = active == Some(key) || (key == "overlay-pen" && model.ink);
        if key == "overlay-layout" {
            tool.icon.clear();
            tool.label = "Aa".into();
            tool.centered = true;
            tool.face = Some("Noto Serif".into());
            tool.bold = true;
        }
        put(x, y, 88, 88, WidgetPod::new(tool).focusable(true).with_key(key));
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
    let compact = matches!(active, Some("overlay-layout" | "overlay-zoom"));
    if compact {
        children.retain(|(r, _)| r.y < (screen.h * 179 / 1680) as i32);
    }
    if !compact {
        children.push((
            rect(screen, 42, 294, 303, 575),
            WidgetPod::new(super::zoom::Map {
                model: model.overview.clone(),
            })
            .with_key("zoom-map"),
        ));
    }
    Chrome {
        panel_border: 3,
        size: screen,
        children,
        panels: if compact {
            vec![rect(screen, 0, 0, 1264, 179)]
        } else {
            vec![rect(screen, 0, 0, 1264, 179), rect(screen, 0, 1344, 1264, 336)]
        },
    }
}
/// The label of the bookmark button. It names the state of the current page.
pub fn bookmark_label(bookmarked: bool) -> String {
    if bookmarked {
        tr("Bookmarked")
    } else {
        tr("Bookmark")
    }
}

pub(super) fn clear_external_tools(window: &Window, ctx: &mut kaia_events::Ctx) {
    for key in ["overlay-light", "overlay-orientation", "overlay-settings"] {
        window.update::<super::chrome::Tool, _>(ctx, key, |tool, cx| {
            if tool.active {
                tool.active = false;
                cx.request_paint();
            }
        });
    }
}
