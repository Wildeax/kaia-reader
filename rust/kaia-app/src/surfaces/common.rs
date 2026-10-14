//! Shared builders for the surfaces: the window, the page layout, the title and
//! body text, buttons, the nav bar and the book helpers. The app shell owns this
//! file. A surface worker adds to it only when two surfaces need the same helper.

use std::path::Path;

use kaia_gfx::image::{decode_rgb, DecodeLimits};
use kaia_gfx::{Pixmap, Size};
use kaia_i18n::tr;
use kaia_library::{Book, CoverSize, Library};
use kaia_stats::{Stats, DEFAULT_MAX_READ_SEC};
use kaia_ui::components::{Column, Label, Padding, Paragraph, Spacer};
use kaia_ui::controls::{Button, ButtonStyle};
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, Tone, Widget, WidgetPod, Window};

use crate::services::Services;
use crate::surfaces::settings::facts;

/// Longest book title shown in a list row. Longer titles end with an ellipsis.
const ROW_TITLE_CHARS: usize = 60;

/// A window of `screen` pixels that shows `root`.
pub(crate) fn window(services: &Services, screen: Size, root: impl Widget + 'static) -> Window {
    Window::new(root, services.theme.clone(), services.assets.clone(), screen)
}

/// An opaque window for a screen that covers the display, including its margins.
pub(crate) fn root_window(services: &Services, screen: Size, root: impl Widget + 'static) -> Window {
    window(
        services,
        screen,
        kaia_ui::components::Frame::new(WidgetPod::new(root)).fill(Tone::Surface),
    )
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
    WidgetPod::new(Button::new(style).text(label).on_action(action))
        .focusable(true)
        .with_key(key)
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
            .unwrap_or_else(|| tr("Untitled")),
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

/// The empty library: a heading, then the steps to add books.
pub(crate) fn empty_library(t: &Theme) -> Vec<WidgetPod> {
    vec![
        WidgetPod::new(Label::new(tr("No books yet"), t.font.title.clone(), Tone::Ink)),
        WidgetPod::new(Paragraph::new(
            tr("Connect the Kobo to a computer over USB and copy book files into the Books folder. Kaia adds them after the next scan."),
            t.font.body.clone(),
            Tone::InkSecondary,
        )),
    ]
}

/// Seconds of reading per page for a book, for time-left estimates. The average
/// comes from the statistics database when that database has the book with read
/// pages. Otherwise it is the default KOReader starts with: half the longest
/// stay that counts.
pub(crate) fn seconds_per_page(data_dir: &Path, title: &str, authors: &str) -> f64 {
    let fallback = (0.5 * DEFAULT_MAX_READ_SEC as f64).floor();
    let path = facts::stats_path(data_dir);
    if !path.is_file() {
        return fallback;
    }
    let Ok(mut stats) = Stats::open(&path) else {
        return fallback;
    };
    let id = stats.books().ok().and_then(|books| {
        books
            .into_iter()
            .find(|b| b.title == title && b.authors == authors)
            .map(|b| b.id)
    });
    match id {
        Some(id) => stats
            .book_estimate(id)
            .map_or(fallback, |estimate| estimate.avg_page_seconds),
        None => fallback,
    }
}

/// Header taps outside its buttons open quick settings.
pub(crate) fn status_header(header: kaia_ui::composite::Header) -> WidgetPod {
    WidgetPod::new(
        super::library::press::Pressable::new(WidgetPod::new(header)).tap("settings:quick:open", Vec::new()),
    )
    .with_key("status-line")
}
