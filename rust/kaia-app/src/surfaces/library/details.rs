//! Details of one book: its path, format, size and progress, with Open and Back.

use kaia_gfx::Size;
use kaia_library::Book;
use kaia_ui::controls::ButtonStyle;
use kaia_ui::{Tone, WidgetPod, Window};

use crate::services::Services;
use crate::surface::{Surface, SurfaceEnv};
use crate::surfaces::common::{body, button, display_title, fill, page, window};
use crate::surfaces::Route;

/// The details of the book with row id `book`. The row is read on build, so a
/// book that is gone shows a notice and no screen.
pub struct BookDetailsSurface {
    book: i64,
}

impl BookDetailsSurface {
    pub fn new(book: i64) -> Self {
        Self { book }
    }
}

impl Surface for BookDetailsSurface {
    fn route(&self) -> Route {
        Route::BookDetails { book: self.book }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        match env.services.library.book(self.book) {
            Ok(Some(row)) => Some(book_details(env.services, env.screen, &row)),
            _ => {
                env.notice(format!("no book {}", self.book));
                None
            }
        }
    }
}

/// Details of one book: its path, format, size and progress, with Open and Back.
pub fn book_details(services: &Services, screen: Size, book: &Book) -> Window {
    let t = &services.theme;
    let mut lines = Vec::new();
    if let Some(authors) = book.authors.as_deref().filter(|a| !a.trim().is_empty()) {
        lines.push(authors.to_string());
    }
    lines.push(format!("Format: {}", book.format));
    if let Some(pages) = book.pages {
        lines.push(format!("Pages: {pages}"));
    }
    lines.push(format!(
        "Progress: {} percent",
        (book.percent * 100.0).round() as u32
    ));
    lines.push(format!("File: {}", book.path.display()));

    let mut children = vec![WidgetPod::new(kaia_ui::components::Label::new(
        display_title(book),
        t.font.title.clone(),
        Tone::Ink,
    ))];
    children.extend(lines.into_iter().map(|line| body(services, line)));
    children.push(fill());
    children.push(button(
        ButtonStyle::Primary,
        "Open",
        &format!("open_book:{}", book.id),
        "open-book",
    ));
    children.push(button(ButtonStyle::Secondary, "Back", "back", "back"));
    window(services, screen, page(services, children))
}
