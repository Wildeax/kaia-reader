//! The route stack. The bottom screen is a top-level surface (Home, Library,
//! Notes or Settings). Pushed screens (the reader, its overlay, book details and
//! settings sections) sit above it.
//!
//! The router owns one `kaia_ui::Window` per screen and keeps the window stack in
//! step with the route stack. Switching the root replaces every screen with a
//! `ui` refresh. Pushing and popping show and close one screen.

use kaia_events::{Ctx, Refresh, RefreshMode, ShowOpts, WindowId, WindowStack};
use kaia_ui::{Out, Window};

/// A screen the app can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Home,
    Library {
        page: usize,
    },
    Notes,
    Settings,
    Reader {
        book: i64,
    },
    /// The reader's overlay: a transparent layer with a Back button.
    Overlay,
    BookDetails {
        book: i64,
    },
    SettingsSection {
        name: String,
    },
}

impl Route {
    /// Home, Library, Notes and Settings.
    pub fn is_top_level(&self) -> bool {
        matches!(
            self,
            Route::Home | Route::Library { .. } | Route::Notes | Route::Settings
        )
    }
}

struct Screen {
    route: Route,
    window: Window,
    id: WindowId,
}

/// The route stack. Index 0 is the root.
#[derive(Default)]
pub struct Router {
    screens: Vec<Screen>,
}

impl Router {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of screens, root included.
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

    pub fn top_window(&self) -> Option<Window> {
        self.screens.last().map(|s| s.window.clone())
    }

    /// The window for a window id on the stack.
    pub fn window(&self, id: WindowId) -> Option<Window> {
        self.screens.iter().find(|s| s.id == id).map(|s| s.window.clone())
    }

    /// Replaces every screen with `window` as the new root. `mode` is the refresh
    /// the new root asks for when it is shown.
    pub fn switch_root(
        &mut self,
        route: Route,
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
        let id = window.show(ctx, opts);
        stack.apply_requests(ctx);
        let old = std::mem::replace(&mut self.screens, vec![Screen { route, window, id }]);
        for screen in old.into_iter().rev() {
            stack.close(screen.id, None, ctx);
        }
    }

    /// Shows `window` above the top screen.
    pub fn push(
        &mut self,
        route: Route,
        window: Window,
        opts: ShowOpts,
        stack: &mut WindowStack,
        ctx: &mut Ctx,
    ) {
        let id = window.show(ctx, opts);
        stack.apply_requests(ctx);
        self.screens.push(Screen { route, window, id });
    }

    /// Closes the top screen and returns its route. Does nothing and returns None
    /// on an empty stack. The root can be popped too, so callers check the depth.
    pub fn pop(&mut self, mode: RefreshMode, stack: &mut WindowStack, ctx: &mut Ctx) -> Option<Route> {
        let screen = self.screens.pop()?;
        stack.close(screen.id, Some(Refresh::new(mode)), ctx);
        Some(screen.route)
    }

    /// Takes the actions and events that every screen queued since the last call.
    pub fn drain_outboxes(&self) -> Vec<Out> {
        self.screens
            .iter()
            .flat_map(|s| s.window.drain_outbox())
            .collect()
    }
}
