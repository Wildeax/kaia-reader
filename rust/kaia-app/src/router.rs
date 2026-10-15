//! The route stack. The bottom screen is a root (Home, Library or Notes).
//! Pushed screens (the reader, its overlay, book details, settings
//! sections) sit above it.
//!
//! The router owns one [`Surface`] and one `kaia_ui::Window` per screen, and
//! keeps the window stack in step with the stack. It does not call the surface
//! hooks. The app does, because the hooks need the services.
//!
//! Switching the root replaces every screen with a `ui` refresh. Pushing and
//! popping show and close one screen.

use kaia_events::{Ctx, Refresh, RefreshMode, ShowOpts, WindowId, WindowStack};
use kaia_ui::{Out, Window};

use crate::surface::Surface;
pub use crate::surfaces::Route;

struct Screen {
    /// Cached from the surface when the screen is built.
    route: Route,
    surface: Box<dyn Surface>,
    window: Window,
    id: WindowId,
    /// The options the window was shown with. A rebuild shows it with the same options.
    opts: ShowOpts,
    /// Set when the surface asked for a rebuild while another screen was on top.
    stale: bool,
}

/// The route stack. Index 0 is the root.
#[derive(Default)]
pub struct Router {
    screens: Vec<Screen>,
    transient: Vec<Window>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of screens, root included.
    pub fn busy(&self) -> bool {
        self.screens.iter().any(|s| s.surface.busy())
    }

    pub fn depth(&self) -> usize {
        self.screens.len()
    }

    pub fn root_route(&self) -> Option<&Route> {
        self.screens.first().map(|s| &s.route)
    }

    pub fn top_route(&self) -> Option<&Route> {
        self.screens.last().map(|s| &s.route)
    }

    /// The routes from the root up.
    pub fn routes(&self) -> Vec<&Route> {
        self.screens.iter().map(|s| &s.route).collect()
    }

    /// The first settings screen, including a section opened without the index.
    /// Closing settings removes this screen and its descendants.
    pub fn settings_start(&self) -> Option<usize> {
        self.screens
            .iter()
            .position(|s| matches!(s.route, Route::Settings | Route::SettingsSection { .. }))
    }

    pub fn top_window(&self) -> Option<Window> {
        self.screens.last().map(|s| s.window.clone())
    }

    /// The window for a window id on the stack.
    pub fn window(&self, id: WindowId) -> Option<Window> {
        self.screens
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.window.clone())
            .or_else(|| self.transient.iter().find(|w| w.id() == Some(id)).cloned())
    }

    pub fn top_surface(&self) -> Option<&dyn Surface> {
        self.screens.last().map(|s| &*s.surface)
    }

    pub fn register(&mut self, windows: &mut Vec<Window>) {
        self.transient.append(windows);
    }

    pub fn closed_screens(&self, stack: &WindowStack) -> Vec<usize> {
        self.screens
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(i, s)| (!stack.contains(s.id)).then_some(i))
            .collect()
    }

    pub fn remove_closed(&mut self, stack: &WindowStack) {
        self.screens.retain(|s| stack.contains(s.id));
        self.transient
            .retain(|w| w.id().is_some_and(|id| stack.contains(id)));
    }

    /// The surface at `index`, root at 0.
    pub fn surface_mut(&mut self, index: usize) -> Option<&mut dyn Surface> {
        let screen = self.screens.get_mut(index)?;
        Some(&mut *screen.surface)
    }

    /// Replaces every screen with `surface` shown in `window`. `mode` is the
    /// refresh the new root asks for when it is shown.
    pub fn switch_root(
        &mut self,
        surface: Box<dyn Surface>,
        window: Window,
        mode: RefreshMode,
        stack: &mut WindowStack,
        ctx: &mut Ctx,
    ) {
        let opts = ShowOpts {
            covers_fullscreen: true,
            refresh: Some(Refresh::new(mode)),
            ..ShowOpts::default()
        };
        let screen = Self::show_screen(surface, window, opts, stack, ctx);
        let old = std::mem::replace(&mut self.screens, vec![screen]);
        for screen in old.into_iter().rev() {
            stack.close(screen.id, None, ctx);
        }
    }

    /// Shows `window` above the top screen.
    pub fn push(
        &mut self,
        surface: Box<dyn Surface>,
        window: Window,
        opts: ShowOpts,
        stack: &mut WindowStack,
        ctx: &mut Ctx,
    ) {
        let screen = Self::show_screen(surface, window, opts, stack, ctx);
        self.screens.push(screen);
    }

    /// Closes the top screen and returns its route. Does nothing on the root.
    pub fn pop(&mut self, mode: RefreshMode, stack: &mut WindowStack, ctx: &mut Ctx) -> Option<Route> {
        if self.screens.len() < 2 {
            return None;
        }
        let screen = self.screens.pop()?;
        let refresh = screen.opts.covers_fullscreen.then(|| Refresh::new(mode));
        stack.close(screen.id, refresh, ctx);
        Some(screen.route)
    }

    /// Shows `window` in place of the top screen. The surface stays the same, and
    /// the window gets the `mode` refresh. The old window closes.
    pub fn swap_top_window(
        &mut self,
        window: Window,
        mode: RefreshMode,
        stack: &mut WindowStack,
        ctx: &mut Ctx,
    ) {
        let Some(top) = self.screens.last_mut() else {
            return;
        };
        let opts = ShowOpts {
            refresh: Some(Refresh::new(mode)),
            ..top.opts
        };
        let id = window.show(ctx, opts);
        stack.apply_requests(ctx);
        let old_id = std::mem::replace(&mut top.id, id);
        top.window = window;
        top.route = top.surface.route();
        top.stale = false;
        stack.close(old_id, None, ctx);
    }

    /// Marks the screen at `index` for a rebuild when it becomes the top screen.
    pub fn mark_stale(&mut self, index: usize) {
        if let Some(screen) = self.screens.get_mut(index) {
            screen.stale = true;
        }
    }

    /// Clears the stale flag of the screen at `index` and returns whether it was set.
    pub fn take_stale(&mut self, index: usize) -> bool {
        match self.screens.get_mut(index) {
            Some(screen) => std::mem::take(&mut screen.stale),
            None => false,
        }
    }

    /// Takes the actions and events that every screen queued since the last call.
    pub fn drain_outboxes(&self) -> Vec<Out> {
        self.screens
            .iter()
            .flat_map(|s| s.window.drain_outbox())
            .chain(self.transient.iter().flat_map(Window::drain_outbox))
            .collect()
    }

    fn show_screen(
        surface: Box<dyn Surface>,
        window: Window,
        opts: ShowOpts,
        stack: &mut WindowStack,
        ctx: &mut Ctx,
    ) -> Screen {
        let route = surface.route();
        let id = window.show(ctx, opts);
        stack.apply_requests(ctx);
        Screen {
            route,
            surface,
            window,
            id,
            opts,
            stale: false,
        }
    }
}
