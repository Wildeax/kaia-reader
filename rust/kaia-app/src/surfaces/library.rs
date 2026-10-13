//! Library: one page of the book list. Rows are text buttons that open the book.
//! Previous and Next turn the pages. The app rebuilds this window for each page.

use kaia_gfx::Size;
use kaia_library::{Book, Filter, Sort, SortKey};
use kaia_ui::components::{Label, Row};
use kaia_ui::controls::{Button, ButtonStyle};
use kaia_ui::{Tone, WidgetPod, Window};

use super::{body, display_title, fill, nav_bar, page, short, title, window};
use crate::services::Services;

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
