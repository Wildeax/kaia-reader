//! A settings section. It is a placeholder until the sections are built.

use kaia_gfx::Size;
use kaia_ui::controls::ButtonStyle;
use kaia_ui::Window;

use crate::services::Services;
use crate::surface::{Surface, SurfaceEnv};
use crate::surfaces::common::{body, button, fill, page, title, window};
use crate::surfaces::Route;

/// One settings section, shown above the Settings root.
pub struct SettingsSectionSurface {
    name: String,
}

impl SettingsSectionSurface {
    pub fn new(name: String) -> Self {
        Self { name }
    }
}

impl Surface for SettingsSectionSurface {
    fn route(&self) -> Route {
        Route::SettingsSection {
            name: self.name.clone(),
        }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(build(env.services, env.screen, &self.name))
    }
}

/// A section that is not built yet. It has a Back button only.
pub fn build(services: &Services, screen: Size, name: &str) -> Window {
    let children = vec![
        title(services, name.to_string()),
        body(services, "This section is not built yet."),
        fill(),
        button(ButtonStyle::Secondary, "Back", "back", "back"),
    ];
    window(services, screen, page(services, children))
}
