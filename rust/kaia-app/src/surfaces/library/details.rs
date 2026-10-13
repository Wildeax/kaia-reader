//! The book details route. The Library shows the same details inline when the
//! user holds a book, so this surface only serves the route: a full page with
//! Back. Its actions go to the Library below, which owns the book commands.

use kaia_library::CoverSize;
use kaia_ui::components::{Column, Padding};
use kaia_ui::geometry::Insets;
use kaia_ui::{WidgetPod, Window};

use crate::surface::{Surface, SurfaceEnv};
use crate::surfaces::common::root_window as window;
use crate::surfaces::library::{data, views};
use crate::surfaces::Route;

/// The details of the book with row id `book`. A book that is gone shows a
/// notice and no screen.
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
        let t = env.services.theme.clone();
        match env.services.library.book(self.book) {
            Ok(Some(book)) => {
                let cover = data::cover(&env.services.library, book.id, CoverSize::Large);
                let details = views::details_panel(&t, &book, cover, "back");
                let page = Padding::new(
                    WidgetPod::new(Column::new(vec![details]).gap(t.space.md)),
                    Insets::all(t.space.md),
                );
                Some(window(env.services, env.screen, page))
            }
            _ => {
                env.notice(format!("no book {}", self.book));
                None
            }
        }
    }
}
