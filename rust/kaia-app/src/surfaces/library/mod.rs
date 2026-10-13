//! Library: one page of the book list. Rows are text buttons that open the book.
//! Previous and Next turn the pages. The surface rebuilds for each page.
//! It also rebuilds when the library changes, and it opens book details.

pub mod commands;
pub mod details;

use kaia_events::Event;
use kaia_gfx::Size;
use kaia_library::{Book, Filter, Sort, SortKey};
use kaia_ui::components::{Label, Row};
use kaia_ui::controls::{Button, ButtonStyle};
use kaia_ui::{Tone, WidgetPod, Window};

use crate::services::Services;
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{body, display_title, fill, nav_bar, page, short, title, window};
use crate::surfaces::Route;
use commands::{Command, PageStep};
use details::BookDetailsSurface;

/// Books per page.
pub const PAGE_SIZE: usize = 8;

/// Builds the Library window for `page` (0-based).
pub fn build(services: &Services, screen: Size, page_index: usize) -> Window {
    let sort = Sort::new(SortKey::Title, false);
    // One extra book tells whether a next page exists.
    let fetched = services
        .library
        .books(sort, &Filter::default(), page_index * PAGE_SIZE, PAGE_SIZE + 1);

    let mut children = vec![title(services, "Library")];
    let mut has_next = false;
    match fetched {
        Ok(books) => {
            has_next = books.len() > PAGE_SIZE;
            if books.is_empty() {
                children.push(body(services, "No books in the library yet."));
            }
            children.extend(books.iter().take(PAGE_SIZE).map(row));
        }
        Err(e) => children.push(body(services, format!("Cannot read the library: {e}"))),
    }

    children.push(fill());
    children.push(footer(services, page_index, has_next));
    children.push(nav_bar());
    window(services, screen, page(services, children))
}

/// One book as a text button that opens it.
fn row(book: &Book) -> WidgetPod {
    let label = short(&display_title(book));
    WidgetPod::new(
        Button::new(ButtonStyle::Text)
            .text(label)
            .on_action(format!("open_book:{}", book.id)),
    )
    .with_key(format!("book-{}", book.id))
}

/// Previous and Next buttons, with the page number between them.
fn footer(services: &Services, page_index: usize, has_next: bool) -> WidgetPod {
    let t = &services.theme;
    let previous = WidgetPod::new(
        Button::new(ButtonStyle::Secondary)
            .text("Previous")
            .on_action("library:prev")
            .enabled(page_index > 0),
    )
    .with_key("previous");
    let next = WidgetPod::new(
        Button::new(ButtonStyle::Secondary)
            .text("Next")
            .on_action("library:next")
            .enabled(has_next),
    )
    .with_key("next");
    let number = WidgetPod::new(Label::new(
        format!("Page {}", page_index + 1),
        t.font.caption.clone(),
        Tone::InkSecondary,
    ));
    WidgetPod::new(Row::new(vec![previous, number, next]).gap(t.space.md))
}

/// The library list at `page`. The page is part of the route.
pub struct LibrarySurface {
    page: usize,
}

impl LibrarySurface {
    pub fn new(page: usize) -> Self {
        Self { page }
    }
}

impl Surface for LibrarySurface {
    fn route(&self) -> Route {
        Route::Library { page: self.page }
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(build(env.services, env.screen, self.page))
    }

    fn on_command(&mut self, cmd: &str, _args: &[kaia_events::Value], _env: &mut SurfaceEnv<'_>) -> Outcome {
        match commands::parse(cmd) {
            Some(Command::Page(step)) => {
                let next = match step {
                    PageStep::Next => self.page + 1,
                    PageStep::Previous => self.page.saturating_sub(1),
                };
                if next == self.page {
                    Outcome::Handled
                } else {
                    Outcome::Navigate(Nav::SwitchRoot(Route::Library { page: next }))
                }
            }
            Some(Command::Book(id)) => Outcome::Navigate(Nav::Push(Box::new(BookDetailsSurface::new(id)))),
            None => Outcome::NotMine,
        }
    }

    /// The list shows titles and progress, so a library change rebuilds it.
    fn on_event(&mut self, ev: &Event, _env: &mut SurfaceEnv<'_>) -> Outcome {
        if ev.name == "LibraryUpdated" {
            Outcome::Rebuild
        } else {
            Outcome::NotMine
        }
    }
}
