//! Settings: the root with its section buttons. Each section is a surface of its own.

pub mod commands;
pub mod section;

use kaia_gfx::Size;
use kaia_ui::controls::ButtonStyle;
use kaia_ui::Window;

use crate::services::Services;
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{button, fill, nav_bar, page, title, window};
use crate::surfaces::Route;
use commands::Command;
use section::SettingsSectionSurface;

/// The Settings root.
pub struct SettingsSurface;

impl Surface for SettingsSurface {
    fn route(&self) -> Route {
        Route::Settings
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(build(env.services, env.screen))
    }

    fn on_command(&mut self, cmd: &str, _args: &[kaia_events::Value], _env: &mut SurfaceEnv<'_>) -> Outcome {
        match commands::parse(cmd) {
            Some(Command::Section(name)) => {
                Outcome::Navigate(Nav::Push(Box::new(SettingsSectionSurface::new(name))))
            }
            None => Outcome::NotMine,
        }
    }
}

pub fn build(services: &Services, screen: Size) -> Window {
    let children = vec![
        title(services, "Settings"),
        button(
            ButtonStyle::Secondary,
            "Reading",
            "settings:reading",
            "settings-reading",
        ),
        button(
            ButtonStyle::Secondary,
            "Display and light",
            "settings:display",
            "settings-display",
        ),
        fill(),
        nav_bar(),
    ];
    window(services, screen, page(services, children))
}
