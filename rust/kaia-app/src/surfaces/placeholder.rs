//! Placeholder surfaces: Notes, Settings, one settings section and book details.
//! They hold their place in the navigation until the composite components arrive.

use kaia_gfx::Size;
use kaia_library::Book;
use kaia_ui::controls::ButtonStyle;
use kaia_ui::{Tone, WidgetPod, Window};

use super::{body, button, display_title, fill, nav_bar, page, title, window};
use crate::services::Services;

pub fn notes(services: &Services, screen: Size) -> Window {
    let children = vec![
        title(services, "Notes"),
        body(
            services,
            "Notes are not built yet. They return with the composite components.",
        ),
        fill(),
        nav_bar(),
    ];
    window(services, screen, page(services, children))
}

pub fn settings(services: &Services, screen: Size) -> Window {
    let children = vec![
        title(services, "Settings"),
        button(
            ButtonStyle::Secondary,
            "Reading",
            "settings:reading",
            "settings-reading",
        ),
        button(
            ButtonStyle::Secondary,
            "Display and light",
            "settings:display",
            "settings-display",
        ),
        fill(),
        nav_bar(),
    ];
    window(services, screen, page(services, children))
}

/// A settings section that is not built yet. It has a Back button only.
pub fn settings_section(services: &Services, screen: Size, name: &str) -> Window {
    let children = vec![
        title(services, name.to_string()),
        body(services, "This section is not built yet."),
        fill(),
        button(ButtonStyle::Secondary, "Back", "back", "back"),
    ];
    window(services, screen, page(services, children))
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
