//! Standalone book details share the library's dialogs and command handling.
use super::{LibrarySurface, Panel};
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::Route;
use kaia_events::Value;
use kaia_ui::Window;

pub struct BookDetailsSurface {
    book: i64,
    library: LibrarySurface,
}
impl BookDetailsSurface {
    pub fn new(book: i64) -> Self {
        Self {
            book,
            library: LibrarySurface::details(book),
        }
    }
}
impl Surface for BookDetailsSurface {
    fn route(&self) -> Route {
        Route::BookDetails { book: self.book }
    }
    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        self.library.build(env)
    }
    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        if matches!(
            cmd,
            "surface:back" | "library:panel:close" | "library:popup:close"
        ) && matches!(self.library.panel, Some(Panel::Details { .. }))
        {
            return Outcome::Navigate(Nav::Pop);
        }
        self.library.on_command(cmd, args, env)
    }
}
