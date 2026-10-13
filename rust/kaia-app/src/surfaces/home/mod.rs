//! Home: the featured book with its cover and progress, and a Library button.
//! Home has no commands of its own. Its buttons send global and library actions.

use kaia_events::{Event, RefreshMode};
use kaia_gfx::image::FitMode;
use kaia_gfx::Size;
use kaia_library::{Book, Filter, Library, Sort, SortKey};
use kaia_ui::components::{Column, Expanded, Image, Label, ProgressBar, Row, SizedBox};
use kaia_ui::controls::ButtonStyle;
use kaia_ui::{Tone, WidgetPod, Window};

use crate::services::Services;
use crate::surface::{Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{body, button, cover, display_title, fill, nav_bar, page, title, window};
use crate::surfaces::Route;

/// The Home root.
pub struct HomeSurface;

impl Surface for HomeSurface {
    fn route(&self) -> Route {
        Route::Home
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(build(env.services, env.screen))
    }

    /// Home shows covers, so every refresh dithers its pictures.
    fn content_dither(&self, _mode: RefreshMode) -> bool {
        true
    }

    /// Home features a book from the library, so a library change rebuilds it.
    fn on_event(&mut self, ev: &Event, _env: &mut SurfaceEnv<'_>) -> Outcome {
        if ev.name == "LibraryUpdated" {
            Outcome::Rebuild
        } else {
            Outcome::NotMine
        }
    }
}

/// The book Home features: the last one opened, or else the newest one added.
pub fn featured_book(library: &Library) -> Option<Book> {
    if let Some(book) = library.recent(1).ok().and_then(|mut v| v.pop()) {
        return Some(book);
    }
    library
        .books(Sort::new(SortKey::Added, true), &Filter::default(), 0, 1)
        .ok()
        .and_then(|mut v| v.pop())
}

/// Builds the Home window.
pub fn build(services: &Services, screen: Size) -> Window {
    let mut children = vec![title(services, "Kaia")];
    match featured_book(&services.library) {
        Some(book) => {
            children.push(WidgetPod::new(card(services, &book)));
            let label = if book.last_opened_at.is_some() {
                "Continue reading"
            } else {
                "Start reading"
            };
            let action = format!("open_book:{}", book.id);
            children.push(button(ButtonStyle::Primary, label, &action, "continue"));
        }
        None => children.push(body(
            services,
            "No books yet. Copy book files into the library folder. Kaia adds them after a scan.",
        )),
    }
    children.push(button(
        ButtonStyle::Secondary,
        "Library",
        "nav:library",
        "library",
    ));
    children.push(fill());
    children.push(nav_bar());
    window(services, screen, page(services, children))
}

/// The cover, the title, the author and the progress of one book.
fn card(services: &Services, book: &Book) -> Row {
    let t = &services.theme;
    let cover_box = match cover(&services.library, book.id) {
        Some(pixmap) => SizedBox::new(Some(WidgetPod::new(Image::new(pixmap, FitMode::Contain)))),
        None => SizedBox::new(None),
    }
    .width(t.scale.dp(200))
    .height(t.scale.dp(300));

    let mut details = vec![WidgetPod::new(Label::new(
        display_title(book),
        t.font.book_title.clone(),
        Tone::Ink,
    ))];
    if let Some(authors) = book.authors.as_deref().filter(|a| !a.trim().is_empty()) {
        details.push(WidgetPod::new(Label::new(
            authors,
            t.font.body.clone(),
            Tone::InkSecondary,
        )));
    }
    details.push(WidgetPod::new(ProgressBar::new(book.percent as f32)));
    let status = if book.last_opened_at.is_some() {
        "Reading"
    } else {
        "Not started"
    };
    details.push(WidgetPod::new(Label::new(
        status,
        t.font.caption.clone(),
        Tone::InkSecondary,
    )));

    Row::new(vec![
        WidgetPod::new(cover_box),
        WidgetPod::new(Expanded::new(
            // The details column shrinks to its text. Filling the height would let
            // the card take the rest of the page.
            WidgetPod::new(Column::new(details).gap(t.space.sm).shrink()),
            1,
        )),
    ])
    .gap(t.space.md)
}
