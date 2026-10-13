//! Notes: a placeholder until the Notes surface is built.

use kaia_gfx::Size;
use kaia_ui::Window;

use crate::services::Services;
use crate::surface::{Surface, SurfaceEnv};
use crate::surfaces::common::{body, fill, nav_bar, page, title, window};
use crate::surfaces::Route;

/// The Notes root.
pub struct NotesSurface;

impl Surface for NotesSurface {
    fn route(&self) -> Route {
        Route::Notes
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(build(env.services, env.screen))
    }
}

pub fn build(services: &Services, screen: Size) -> Window {
    let children = vec![
        title(services, "Notes"),
        body(
            services,
            "Notes are not built yet. They return with the composite components.",
        ),
        fill(),
        nav_bar(),
    ];
    window(services, screen, page(services, children))
}
