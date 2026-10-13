//! The screens. Each builder returns a [`Window`] for the router.
//!
//! These are placeholders that prove the loop. They use only Column, Row, Label,
//! Paragraph, Button, ProgressBar, Image and SizedBox. The composite components
//! (Header, NavBar, ListRow, PagedList, Dialog, Menu) replace them.

pub mod home;
pub mod library;
pub mod placeholder;
pub mod reader;

use kaia_gfx::image::{decode_rgb, DecodeLimits};
use kaia_gfx::{Pixmap, Size};
use kaia_library::{Book, CoverSize, Library};
use kaia_ui::components::{Column, Expanded, Label, Padding, Paragraph, Row, Spacer};
use kaia_ui::controls::{Button, ButtonStyle};
use kaia_ui::geometry::Insets;
use kaia_ui::{Tone, Widget, WidgetPod, Window};

use crate::services::Services;

/// Longest book title shown in a list row. Longer titles end with an ellipsis.
const ROW_TITLE_CHARS: usize = 60;

/// A window of `screen` pixels that shows `root`.
pub(crate) fn window(services: &Services, screen: Size, root: impl Widget + 'static) -> Window {
    Window::new(root, services.theme.clone(), services.assets.clone(), screen)
}

/// A page: the children in a column with the standard margin.
pub(crate) fn page(services: &Services, children: Vec<WidgetPod>) -> Padding {
    let t = &services.theme;
    let column = Column::new(children).gap(t.space.md);
    Padding::new(WidgetPod::new(column), Insets::all(t.space.lg))
}

/// The page title.
pub(crate) fn title(services: &Services, text: impl Into<String>) -> WidgetPod {
    WidgetPod::new(Label::new(text, services.theme.font.title.clone(), Tone::Ink))
}

/// A paragraph of body text.
pub(crate) fn body(services: &Services, text: impl Into<String>) -> WidgetPod {
    let t = &services.theme;
    WidgetPod::new(Paragraph::new(text, t.font.body.clone(), Tone::InkSecondary))
}

/// A button that sends `action` when tapped. `key` names it for
/// [`crate::App::find_rect`] and for tests.
pub(crate) fn button(style: ButtonStyle, label: &str, action: &str, key: &str) -> WidgetPod {
    WidgetPod::new(Button::new(style).text(label).on_action(action)).with_key(key)
}

/// The temporary navigation row: Home, Library, Notes and Settings.
pub(crate) fn nav_bar() -> WidgetPod {
    let items = [
        ("Home", "nav:home"),
        ("Library", "nav:library"),
        ("Notes", "nav:notes"),
        ("Settings", "nav:settings"),
    ]
    .into_iter()
    .map(|(label, action)| {
        let button = Button::new(ButtonStyle::Text).text(label).on_action(action);
        WidgetPod::new(Expanded::new(WidgetPod::new(button), 1))
    })
    .collect();
    WidgetPod::new(Row::new(items))
}

/// Space that pushes what follows it to the bottom of the page.
pub(crate) fn fill() -> WidgetPod {
    WidgetPod::new(Spacer::new(1))
}

/// The title of a book: the metadata title, or the file name without its extension.
pub(crate) fn display_title(book: &Book) -> String {
    match &book.title {
        Some(title) if !title.trim().is_empty() => title.clone(),
        _ => book
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".to_string()),
    }
}

/// `text` cut to `ROW_TITLE_CHARS` characters, with an ellipsis when cut.
pub(crate) fn short(text: &str) -> String {
    if text.chars().count() <= ROW_TITLE_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(ROW_TITLE_CHARS - 1).collect();
    out.push('\u{2026}');
    out
}

/// The large cover of a book, decoded. None when the book has no cover or the
/// stored image does not decode.
pub(crate) fn cover(library: &Library, book_id: i64) -> Option<Pixmap> {
    let image = library.cover(book_id, CoverSize::Large).ok()??;
    decode_rgb(&image.bytes, DecodeLimits::default()).ok()
}
