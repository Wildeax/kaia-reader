//! Reads for the Home and Library surfaces: the book lists with their covers,
//! the view preferences kept in settings, and the status line text.

use kaia_events::{Settings, Value};
use kaia_gfx::image::{decode_rgb, DecodeLimits};
use kaia_gfx::Pixmap;
use kaia_i18n::{format as tr_format, tr};
use kaia_library::{Book, CoverSize, Filter, Library, Sort, SortKey, Status};
use kaia_ui::composite::BookInfo;

use crate::services::Services;
use crate::surfaces::library::commands::{Chip, SortChoice, View};

/// The most books one list reads. A library of a few thousand books fits.
pub const ALL: usize = 100_000;

/// What the Library shows, kept in settings between runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    pub view: View,
    pub sort: SortChoice,
    pub chip: Option<Chip>,
    pub collection: Option<i64>,
    pub show_finished: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            view: View::Covers,
            sort: SortChoice::Title,
            chip: None,
            collection: None,
            show_finished: true,
        }
    }
}

impl Prefs {
    pub fn load(settings: &Settings) -> Prefs {
        let text = |key: &str| settings.read(key).as_str().map(str::to_owned);
        let view = match text("library.view").as_deref() {
            Some("list") => View::List,
            Some("folders") => View::Folders,
            Some("collections") => View::Collections,
            _ => View::Covers,
        };
        let sort = match text("library.sort").as_deref() {
            Some("author") => SortChoice::Author,
            Some("read") => SortChoice::Read,
            Some("added") => SortChoice::Added,
            Some("progress") => SortChoice::Progress,
            Some("series") => SortChoice::Series,
            _ => SortChoice::Title,
        };
        let chip = match text("library.chip").as_deref() {
            Some("reading") => Some(Chip::Reading),
            Some("not-started") => Some(Chip::NotStarted),
            Some("finished") => Some(Chip::Finished),
            _ => None,
        };
        Prefs {
            view,
            sort,
            chip,
            collection: settings.read("library.collection").as_i64(),
            show_finished: !settings.is_false("library.show_finished"),
        }
    }

    pub fn save(&self, settings: &mut Settings) {
        let view = match self.view {
            View::Covers => "covers",
            View::List => "list",
            View::Folders => "folders",
            View::Collections => "collections",
        };
        let sort = match self.sort {
            SortChoice::Title => "title",
            SortChoice::Author => "author",
            SortChoice::Read => "read",
            SortChoice::Added => "added",
            SortChoice::Progress => "progress",
            SortChoice::Series => "series",
        };
        let chip = match self.chip {
            None => "",
            Some(Chip::Reading) => "reading",
            Some(Chip::NotStarted) => "not-started",
            Some(Chip::Finished) => "finished",
        };
        settings.save("library.view", Value::Str(view.into()));
        settings.save("library.sort", Value::Str(sort.into()));
        settings.save("library.chip", Value::Str(chip.into()));
        settings.save(
            "library.collection",
            self.collection.map_or(Value::Nil, |id| Value::Number(id as f64)),
        );
    }
}

/// The index sort for a sort choice. Each key has the direction that reads best:
/// titles and authors ascend, the rest put the newest or the furthest first.
pub fn sort_for(choice: SortChoice) -> Sort {
    match choice {
        SortChoice::Title => Sort::new(SortKey::Title, false),
        SortChoice::Series => Sort::new(SortKey::Series, false),
        SortChoice::Author => Sort::new(SortKey::Author, false),
        SortChoice::Read => Sort::new(SortKey::LastRead, true),
        SortChoice::Added => Sort::new(SortKey::Added, true),
        SortChoice::Progress => Sort::new(SortKey::Progress, true),
    }
}

/// The index filter for the chips and the collection.
pub fn filter_for(prefs: &Prefs) -> Filter {
    Filter {
        status: prefs.chip.map(|c| match c {
            Chip::Reading => Status::Reading,
            Chip::NotStarted => Status::New,
            Chip::Finished => Status::Complete,
        }),
        collection: prefs.collection,
        ..Filter::default()
    }
}

/// Every book whose file is still there, by title. The folder tree reads these.
pub fn all(library: &Library) -> Vec<Book> {
    library
        .books(Sort::new(SortKey::Title, false), &Filter::default(), 0, ALL)
        .unwrap_or_default()
        .into_iter()
        .filter(|b| b.path.exists())
        .collect()
}

/// Books in the order and filter the prefs give. A book whose file is gone is
/// left out, even when the index still has its row.
pub fn list(library: &Library, prefs: &Prefs) -> Vec<Book> {
    library
        .books(sort_for(prefs.sort), &filter_for(prefs), 0, ALL)
        .unwrap_or_default()
        .into_iter()
        .filter(|b| b.path.exists() && (prefs.show_finished || b.status != Status::Complete))
        .collect()
}

/// Decodes the cover of a book at `size`. None when the book has none or the
/// image does not decode.
pub fn cover(library: &Library, book_id: i64, size: CoverSize) -> Option<Pixmap> {
    let image = library.cover(book_id, size).ok()??;
    decode_rgb(&image.bytes, DecodeLimits::default()).ok()
}

/// A book as a grid tile or a list row: the small cover, the title and the progress.
pub fn info(library: &Library, book: &Book) -> BookInfo {
    let mut info = info_without_cover(book);
    info.cover = cover(library, book.id, CoverSize::Small);
    info
}

pub fn info_without_cover(book: &Book) -> BookInfo {
    BookInfo {
        id: book.id.to_string(),
        title: crate::surfaces::common::display_title(book),
        author: book.authors.clone().filter(|a| !a.trim().is_empty()),
        cover: None,
        progress: book.percent as f32,
    }
}

/// The title of a book, as the list shows it.
pub fn title(book: &Book) -> String {
    crate::surfaces::common::display_title(book)
}

/// The status line: battery level and Wi-Fi, in that order. Parts the device
/// cannot read are left out.
pub fn status_line(services: &Services) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(level) = services.platform.battery.capacity() {
        parts.push(format!("{level}%"));
    }
    let wifi = &services.platform.network;
    parts.push(if wifi.is_connected() {
        tr("Wi-Fi")
    } else {
        tr("Wi-Fi off")
    });
    Some(parts.join("  "))
}

/// Where the book sits in the folder tree: the path of its parent folder.
pub fn folder_of(book: &Book) -> Option<&std::path::Path> {
    book.path.parent()
}

/// The longest folder that holds every book, or None for an empty list.
pub fn common_root(books: &[Book]) -> Option<std::path::PathBuf> {
    let mut root = folder_of(books.first()?)?.to_path_buf();
    for book in books {
        let Some(parent) = folder_of(book) else {
            continue;
        };
        while !parent.starts_with(&root) {
            if !root.pop() {
                break;
            }
        }
    }
    Some(root)
}

/// The label of a book's status for the details page.
pub fn status_label(book: &Book) -> String {
    match book.status {
        Status::New => tr("Not started"),
        Status::Reading => tr("Reading"),
        Status::Abandoned => tr("Abandoned"),
        Status::Complete => tr("Finished"),
    }
}

/// Progress as a percent number, rounded.
pub fn percent(book: &Book) -> u32 {
    (book.percent * 100.0).round().clamp(0.0, 100.0) as u32
}

/// The file size in megabytes with one decimal place.
pub fn size_text(bytes: u64) -> String {
    if bytes >= 1_000_000 {
        tr_format(&tr("%1 MB"), &[&format!("{:.1}", bytes as f64 / 1_000_000.0)])
    } else {
        tr_format(&tr("%1 KB"), &[&(bytes / 1000).to_string()])
    }
}
