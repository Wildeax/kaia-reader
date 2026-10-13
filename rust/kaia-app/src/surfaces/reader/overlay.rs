//! The reader overlay: a transparent layer with a Back button and the page
//! number. The app closes it on a tap that the overlay does not use.

use kaia_events::{Refresh, RefreshMode, ShowOpts};
use kaia_gfx::Size;
use kaia_ui::components::{Expanded, Row};
use kaia_ui::controls::ButtonStyle;
use kaia_ui::{Tone, WidgetPod, Window};

use crate::services::Services;
use crate::surface::{Surface, SurfaceEnv};
use crate::surfaces::common::{button, fill, page, window};
use crate::surfaces::Route;

/// The overlay over the reader. It holds the title and the page it was opened at.
pub struct ReaderOverlaySurface {
    title: String,
    page: u32,
    count: u32,
}

impl ReaderOverlaySurface {
    pub fn new(title: String, page: u32, count: u32) -> Self {
        Self { title, page, count }
    }
}

impl Surface for ReaderOverlaySurface {
    fn route(&self) -> Route {
        Route::Overlay
    }

    /// The overlay is not full screen, so the reader stays visible under it.
    fn show_opts(&self) -> ShowOpts {
        ShowOpts {
            covers_fullscreen: false,
            refresh: Some(Refresh::new(RefreshMode::Ui)),
            ..ShowOpts::default()
        }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(build(
            env.services,
            env.screen,
            &self.title,
            self.page,
            self.count,
        ))
    }

    fn closes_on_unused_tap(&self) -> bool {
        true
    }
}

/// The overlay window: a Back button, then the title and the page number.
pub fn build(services: &Services, screen: Size, title: &str, page_index: u32, count: u32) -> Window {
    let t = &services.theme;
    let back = button(ButtonStyle::Secondary, "Back", "back", "back");
    let label = WidgetPod::new(kaia_ui::components::Label::new(
        format!("{title}, page {} of {count}", page_index + 1),
        t.font.body.clone(),
        Tone::Ink,
    ));
    let bar = WidgetPod::new(Row::new(vec![back, WidgetPod::new(Expanded::new(label, 1))]).gap(t.space.md));
    window(services, screen, page(services, vec![bar, fill()]))
}
