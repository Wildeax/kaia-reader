//! Surfaces: the screens of the app. A surface owns its state and builds its
//! widget tree. The app shows it, routes commands and events to it, and applies
//! the [`Outcome`] it returns. A surface never reaches the router or the window
//! stack. It asks for a change by returning an outcome.
//!
//! The app calls a surface like this:
//! - [`Surface::build`] when it is shown, and again after [`Outcome::Rebuild`].
//! - [`Surface::on_show`] when it becomes the top screen, and [`Surface::on_hide`]
//!   when it leaves the stack (popped, replaced or switched away from). A screen
//!   that is only covered gets neither.
//! - [`Surface::on_command`] for an action id that a widget sent, and
//!   [`Surface::on_event`] for an app event such as `LibraryUpdated`.

use kaia_events::{Ctx, Event, Refresh, RefreshMode, ShowOpts, TaskId, Value, WindowId};
use kaia_gfx::Size;
use kaia_ui::assets::SharedAssets;
use kaia_ui::composite::{Dialog, SheetPanel, Toast};
use kaia_ui::{Theme, Window};

use crate::services::Services;
use crate::surfaces::Route;

/// A screen. See the module docs for when the app calls each method.
pub trait Surface {
    /// The route this surface is shown under. It can change with the state, as the
    /// library's page does.
    fn route(&self) -> Route;

    /// True while a requested page or layout is still being prepared.
    fn busy(&self) -> bool {
        false
    }

    /// Builds the widget tree from the current state. None when there is nothing
    /// to show, such as a book that is gone. The app then keeps the current screen.
    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window>;

    /// How the screen appears when it is pushed. Roots take their refresh from the
    /// app. The default is a full-screen `ui` refresh.
    fn show_opts(&self) -> ShowOpts {
        ShowOpts {
            covers_fullscreen: true,
            refresh: Some(Refresh::new(RefreshMode::Ui)),
            ..ShowOpts::default()
        }
    }

    /// Handles an action id, such as `reader:overlay`. `args` are the arguments the
    /// widget sent. Return [`Outcome::NotMine`] for ids this surface does not own.
    fn on_command(&mut self, _cmd: &str, _args: &[Value], _env: &mut SurfaceEnv<'_>) -> Outcome {
        Outcome::NotMine
    }

    /// Handles an app event. Return [`Outcome::Rebuild`] to refresh the screen.
    fn on_event(&mut self, _ev: &Event, _env: &mut SurfaceEnv<'_>) -> Outcome {
        Outcome::NotMine
    }

    /// The screen became the top of the stack.
    fn on_show(&mut self, _env: &mut SurfaceEnv<'_>) {}

    /// Save before leaving. False keeps the screen and its unsaved state alive.
    fn can_hide(&mut self, _env: &mut SurfaceEnv<'_>) -> bool {
        true
    }

    /// The screen is leaving the stack after `can_hide` succeeds.
    fn on_hide(&mut self, _env: &mut SurfaceEnv<'_>) {}

    /// True when a tap the screen does not use closes it (the reader's overlay).
    fn closes_on_unused_tap(&self) -> bool {
        false
    }

    /// True when a refresh in `mode` should dither the picture on the screen.
    fn content_dither(&self, _mode: RefreshMode) -> bool {
        false
    }
}

/// What a surface asks the app to do after it saw a command or an event.
pub enum Outcome {
    /// The surface used the command. Routing stops here.
    Handled,
    /// Not this surface's command. The app tries the next surface, then its own
    /// global commands. An unknown id is recorded.
    NotMine,
    /// Build the surface again and show the new window in place of the old one.
    /// A surface that is covered is rebuilt when it becomes the top screen.
    Rebuild,
    /// Change the stack. The change acts on the stack, not on the surface that asked.
    Navigate(Nav),
}

/// A change to the stack.
pub enum Nav {
    OpenBook {
        book: i64,
        location: Option<Value>,
    },
    /// Show `surface` above the top screen.
    Push(Box<dyn Surface>),
    /// Close the top screen. Does nothing on the root.
    Pop,
    /// Replace the whole stack with the surface for `route`. Choosing the root
    /// that is already shown does nothing.
    SwitchRoot(Route),
    /// Close the top screen and show `surface` in its place. On the root, this is
    /// a root switch.
    Replace(Box<dyn Surface>),
}

/// What a surface can reach while it works: the services, the window context,
/// notices, tasks, toasts, dialogs and refresh hints.
pub struct SurfaceEnv<'a> {
    pub services: &'a mut Services,
    /// The scheduler, the refresh queue and the window requests.
    pub ctx: &'a mut Ctx,
    /// Screen size in pixels.
    pub screen: Size,
    notices: &'a mut Vec<String>,
    transient: &'a mut Vec<Window>,
}

impl<'a> SurfaceEnv<'a> {
    pub(crate) fn new(
        services: &'a mut Services,
        ctx: &'a mut Ctx,
        screen: Size,
        notices: &'a mut Vec<String>,
        transient: &'a mut Vec<Window>,
    ) -> Self {
        Self {
            services,
            ctx,
            screen,
            notices,
            transient,
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.services.theme
    }

    pub fn assets(&self) -> SharedAssets {
        self.services.assets.clone()
    }

    /// Keeps a note about a request that could not be done. [`crate::App::notices`] lists them.
    pub fn notice(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.notices.push(text.clone());
        self.toast(text, 6.0);
    }

    /// Runs `task` after `seconds` on the app clock.
    pub fn schedule_in(&mut self, seconds: f64, task: impl FnOnce(&mut Ctx) + 'static) -> TaskId {
        self.ctx.scheduler.schedule_in(seconds, Box::new(task))
    }

    /// Asks for a refresh of the whole screen in `mode`.
    pub fn refresh(&mut self, mode: RefreshMode) {
        self.ctx.dirty.refresh(mode, None, false);
    }

    /// Shows `window` above the stack. Its actions reach the command routing like
    /// the actions of a screen. The app drops the handle once the window closes.
    pub fn show(&mut self, window: Window, opts: ShowOpts) -> WindowId {
        let id = window.show(&mut *self.ctx, opts);
        self.transient.push(window);
        id
    }

    /// Shows a modal dialog. Its buttons send their actions.
    pub fn dialog(&mut self, dialog: Dialog) -> WindowId {
        let window = Window::new(
            dialog,
            self.services.theme.clone(),
            self.services.assets.clone(),
            self.screen,
        );
        self.show(window, Dialog::show_opts())
    }

    /// Shows a sheet over the screen below it, which stays visible.
    pub fn sheet(&mut self, sheet: SheetPanel) -> WindowId {
        let window = Window::new(
            sheet,
            self.services.theme.clone(),
            self.services.assets.clone(),
            self.screen,
        );
        self.show(
            window,
            ShowOpts {
                covers_fullscreen: false,
                ..ShowOpts::default()
            },
        )
    }

    /// Shows a short message that closes itself after `seconds`.
    pub fn toast(&mut self, message: impl Into<String>, seconds: f64) -> WindowId {
        let window = Window::new(
            Toast::new(&self.services.theme, message),
            self.services.theme.clone(),
            self.services.assets.clone(),
            self.screen,
        );
        let id = self.show(
            window,
            ShowOpts {
                toast: true,
                ..ShowOpts::default()
            },
        );
        self.schedule_in(seconds, move |ctx| ctx.close(id, None));
        id
    }
}
