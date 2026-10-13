//! The reader: one page of a book, full screen, with the overlay that the
//! center tap opens.
//!
//! This is a stand-in until `kaia-book` lands. It renders pages with
//! `kaia-doc` and keeps only a page index. Tap zones: the left third goes back a
//! page, the rest goes forward, and the middle square opens the overlay. The
//! page-turn keys and the left and right swipes turn pages too.
//!
//! The screen saves its progress when it leaves the stack (`on_hide`).

pub mod commands;
pub mod overlay;
mod page;

use kaia_doc::Document;
use kaia_events::{Refresh, RefreshMode, ShowOpts, Value};
use kaia_gfx::Size;
use kaia_ui::components::Stack;
use kaia_ui::{WidgetPod, Window};

use crate::services::Services;
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{display_title, window};
use crate::surfaces::Route;
use commands::Command;
use overlay::ReaderOverlaySurface;

pub use page::{fit_scale, page_for_progress, progress_for_page, ReaderPage, READER_KEY};

/// The reader screen for one book.
pub struct ReaderSurface {
    book: i64,
    /// The window built for the book. Progress and the overlay read the page from it.
    window: Option<Window>,
}

impl ReaderSurface {
    pub fn new(book: i64) -> Self {
        Self { book, window: None }
    }

    /// Stores the page as the library's progress. The library API has no
    /// progress-only call, so this uses `record_open`, which also sets the
    /// last-opened time.
    fn save_progress(&self, env: &mut SurfaceEnv<'_>) {
        let Some(window) = self.window.as_ref() else {
            return;
        };
        let mut progress = None;
        window.update::<ReaderPage, _>(env.ctx, READER_KEY, |reader, _| {
            progress = Some(reader.progress());
        });
        if let Some(percent) = progress {
            if let Err(e) = env.services.library.record_open(self.book, percent) {
                env.notice(format!("cannot save progress: {e}"));
            }
        }
    }

    /// Pushes the overlay with the title and page of the current view.
    fn show_overlay(&self, env: &mut SurfaceEnv<'_>) -> Outcome {
        let Some(window) = self.window.as_ref() else {
            return Outcome::Handled;
        };
        let mut state = None;
        window.update::<ReaderPage, _>(env.ctx, READER_KEY, |reader, _| {
            state = Some((reader.title().to_string(), reader.page(), reader.page_count()));
        });
        match state {
            Some((title, page, count)) => {
                Outcome::Navigate(Nav::Push(Box::new(ReaderOverlaySurface::new(title, page, count))))
            }
            None => Outcome::Handled,
        }
    }
}

impl Surface for ReaderSurface {
    fn route(&self) -> Route {
        Route::Reader { book: self.book }
    }

    /// Full screen, with a full refresh: the page is a picture.
    fn show_opts(&self) -> ShowOpts {
        ShowOpts {
            covers_fullscreen: true,
            refresh: Some(Refresh::new(RefreshMode::Full)),
            ..ShowOpts::default()
        }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        let row = match env.services.library.book(self.book) {
            Ok(Some(row)) => row,
            _ => {
                env.notice(format!("no book {}", self.book));
                return None;
            }
        };
        let doc = kaia_doc::open(&row.path).map_err(|e| e.to_string());
        let page = match &doc {
            Ok(doc) => page_for_progress(row.percent, doc.page_count() as u32),
            Err(_) => 0,
        };
        // Opening a book is a reading event, so the library records it now.
        if let Err(e) = env.services.library.record_open(row.id, row.percent) {
            env.notice(format!("cannot record the open: {e}"));
        }
        let title = display_title(&row);
        let window = build(env.services, env.screen, title, doc, page);
        self.window = Some(window.clone());
        Some(window)
    }

    fn on_command(&mut self, cmd: &str, _args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        match commands::parse(cmd) {
            Some(Command::Overlay) => self.show_overlay(env),
            None => Outcome::NotMine,
        }
    }

    fn on_hide(&mut self, env: &mut SurfaceEnv<'_>) {
        self.save_progress(env);
    }

    /// A picture is dithered on a full refresh only (see `kaia_display`).
    fn content_dither(&self, mode: RefreshMode) -> bool {
        mode == RefreshMode::Full
    }
}

/// The reader window for a book. `doc` is the opened document, or the error text.
pub fn build(
    services: &Services,
    screen: Size,
    title: String,
    doc: Result<Box<dyn Document>, String>,
    page_index: u32,
) -> Window {
    let dpi = services.theme.scale.dpi.unwrap_or(160);
    let reader = ReaderPage::new(title, doc, page_index, screen, dpi);
    window(
        services,
        screen,
        Stack::new(vec![WidgetPod::new(reader).with_key(READER_KEY)]),
    )
}
