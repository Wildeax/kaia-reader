//! The book details route. The Library shows the same details inline when the
//! user holds a book, so this surface only serves the route: a full page with
//! Back. Its actions go to the Library below, which owns the book commands.

use kaia_events::Value;
use kaia_i18n::{format as tr_format, tr};
use kaia_library::CoverSize;
use kaia_ui::components::{Column, Padding};
use kaia_ui::geometry::Insets;
use kaia_ui::{WidgetPod, Window};

use crate::surface::{Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::root_window as window;
use crate::surfaces::library::commands::{parse, Command};
use crate::surfaces::library::{data, views};
use crate::surfaces::Route;

/// The details of the book with row id `book`. A book that is gone shows a
/// notice and no screen. The rating sheet of the page is this surface's own, so
/// its rating commands stop here.
pub struct BookDetailsSurface {
    book: i64,
    rating_open: bool,
}

impl BookDetailsSurface {
    pub fn new(book: i64) -> Self {
        Self {
            book,
            rating_open: false,
        }
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
                let rating = data::rating(env.services, &book);
                let content = if self.rating_open {
                    views::rating_panel(&t, book.id, rating)
                } else {
                    views::details_panel(&t, &book, cover, "back", None, rating)
                };
                let page = Padding::new(
                    WidgetPod::new(Column::new(vec![content]).gap(t.space.md)),
                    Insets::all(t.space.md),
                );
                Some(window(env.services, env.screen, page))
            }
            _ => {
                env.notice(tr_format(&tr("no book %1"), &[&self.book.to_string()]));
                None
            }
        }
    }

    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        match parse(cmd, args) {
            Some(Command::OpenRating(id)) if id == self.book => {
                self.rating_open = true;
                Outcome::Rebuild
            }
            Some(Command::BackRating(id)) if id == self.book => {
                self.rating_open = false;
                Outcome::Rebuild
            }
            Some(Command::SetRating { book, rating }) if book == self.book => {
                match data::set_rating(env.services, book, rating) {
                    Ok(true) => self.rating_open = false,
                    Ok(false) => env.notice(tr("Open the book once before rating it.")),
                    Err(e) => env.notice(tr_format(&tr("cannot save the rating: %1"), &[&e])),
                }
                Outcome::Rebuild
            }
            _ => Outcome::NotMine,
        }
    }
}
