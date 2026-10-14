//! Home: the book to continue with its progress and time left, the recent books,
//! the other books in progress, the library counts and the status line. While the
//! first scan runs, Home shows how many books it has indexed, and updates that
//! line at most once a second so a large scan does not flood the screen.
//!
//! Each section is one child of the content column. A section that does not fit
//! on the page is left out whole, so no heading is ever cut in half.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use kaia_events::{Ctx, Event, RefreshMode, Value};
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_library::{Book, CoverSize, Filter, Library, Sort, SortKey, Status};
use kaia_ui::components::{Column, CrossAlign, Expanded, Label, Padding, ProgressBar, Row, SizedBox};
use kaia_ui::composite::Header;
use kaia_ui::composite::{BookInfo, BookTile, CoverImage, ListRow};
use kaia_ui::controls::ButtonStyle;
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, Tone, WidgetPod, Window};

use crate::surface::{Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{self, button, root_window as window};
use crate::surfaces::library::data;
use crate::surfaces::library::press::Pressable;
use crate::surfaces::settings::facts;
use crate::surfaces::Route;

/// The least time between two progress lines on Home.
const PROGRESS_EVERY: Duration = Duration::from_secs(1);

/// Recent books on the shelf, besides the book to continue.
const SHELF: usize = 4;

/// Other books in progress that get their own row.
const CONTINUE_ROWS: usize = 2;

#[derive(Default)]
struct ScanLine {
    progress: Option<(usize, usize)>,
    last: Option<Duration>,
    trailing: bool,
    window: Option<Window>,
}

#[derive(Default)]
pub struct HomeSurface {
    scan: Rc<RefCell<ScanLine>>,
}

fn progress_text(progress: Option<(usize, usize)>) -> String {
    progress
        .map(|(done, total)| {
            tr_format(
                &tr("Indexing books: %1 of %2"),
                &[&done.to_string(), &total.to_string()],
            )
        })
        .unwrap_or_default()
}

fn set_progress_line(line: &ScanLine, ctx: &mut Ctx) {
    if let Some(window) = &line.window {
        window.update::<Label, _>(ctx, "scan-progress", |label, cx| {
            label.set_text(progress_text(line.progress));
            cx.request_paint();
        });
    }
}

impl Surface for HomeSurface {
    fn route(&self) -> Route {
        Route::Home
    }

    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        let win = build_window(env, self.scan.borrow().progress);
        self.scan.borrow_mut().window = Some(win.clone());
        Some(win)
    }

    fn content_dither(&self, mode: RefreshMode) -> bool {
        mode == RefreshMode::Full
    }

    fn on_event(&mut self, ev: &Event, env: &mut SurfaceEnv<'_>) -> Outcome {
        match ev.name.as_str() {
            "LibraryUpdated" | "LibraryScanFailed" => {
                self.scan.borrow_mut().progress = None;
                Outcome::Rebuild
            }
            "LibraryScanProgress" => {
                let (Some(done), Some(total)) = (
                    ev.args.first().and_then(Value::as_f64),
                    ev.args.get(1).and_then(Value::as_f64),
                ) else {
                    return Outcome::NotMine;
                };
                let now = env.ctx.scheduler.now();
                let mut line = self.scan.borrow_mut();
                line.progress = Some((done as usize, total as usize));
                if let Some(last) = line
                    .last
                    .filter(|last| now.saturating_sub(*last) < PROGRESS_EVERY)
                {
                    if !line.trailing {
                        line.trailing = true;
                        let scan = self.scan.clone();
                        env.schedule_in(
                            (last + PROGRESS_EVERY).saturating_sub(now).as_secs_f64(),
                            move |ctx| {
                                let mut line = scan.borrow_mut();
                                line.trailing = false;
                                line.last = Some(ctx.scheduler.now());
                                set_progress_line(&line, ctx);
                            },
                        );
                    }
                } else {
                    line.last = Some(now);
                    set_progress_line(&line, env.ctx);
                }
                Outcome::Handled
            }
            _ => Outcome::NotMine,
        }
    }
}

/// Builds the Home window from the library and the scan progress.
fn build_window(env: &mut SurfaceEnv<'_>, progress: Option<(usize, usize)>) -> Window {
    let services = &*env.services;
    let t = services.theme.clone();
    let lib = &services.library;

    let recent = lib.recent(SHELF + 2).unwrap_or_default();
    // The hero is the last book opened that is not finished. A library with
    // only finished books features the newest one added.
    let hero = recent
        .iter()
        .find(|b| b.status != Status::Complete)
        .cloned()
        .or_else(|| newest_added(lib));

    let header = crate::surfaces::common::status_header(
        Header::new(&t, "Kaia")
            .status(data::status_line(services).unwrap_or_default())
            .action("appbar.settings", "nav:settings"),
    );

    // The scan line is empty, and takes no room, when no scan runs.
    let mut sections = vec![WidgetPod::new(Label::new(
        progress_text(progress),
        t.font.caption.clone(),
        Tone::InkSecondary,
    ))
    .with_key("scan-progress")];
    match &hero {
        Some(book) => sections.push(hero_section(env, book)),
        None => sections.push(empty_section(&t)),
    }
    if let Some(shelf) = shelf_section(env, &recent, hero.as_ref()) {
        sections.push(shelf);
    }
    if let Some(others) = continue_section(&t, lib, hero.as_ref()) {
        sections.push(others);
    }

    let total = data::all(lib).len();
    let reading = count(lib, Status::Reading);
    let finished = count(lib, Status::Complete);
    let counts = tr_format(
        &tr_n(
            "Library: %1 book. Reading: %2. Finished: %3.",
            "Library: %1 books. Reading: %2. Finished: %3.",
            total as i64,
        ),
        &[&total.to_string(), &reading.to_string(), &finished.to_string()],
    );
    let counts = button(ButtonStyle::Text, &counts, "nav:library", "library");

    // The sections scroll out of view rather than pushing the counts and the nav
    // bar off the page. Whole sections are hidden, so nothing is cut in half.
    let content = WidgetPod::new(Expanded::new(
        WidgetPod::new(
            Column::new(sections)
                .gap(t.space.sm)
                .cross_align(CrossAlign::Stretch)
                .hide_overflow(),
        ),
        1,
    ));
    let page = Padding::new(
        WidgetPod::new(
            Column::new(vec![
                header,
                content,
                counts,
                WidgetPod::new(kaia_ui::composite::NavBar::new(kaia_ui::composite::NavItem::Home))
                    .with_key("nav-bar"),
            ])
            .gap(t.space.md)
            .cross_align(CrossAlign::Stretch),
        ),
        // Home is the tallest page, so its vertical margin is the medium space.
        Insets::symmetric(t.space.lg, t.space.md),
    );
    window(services, env.screen, page)
}

/// The most recently added book, for a library nobody has opened yet.
fn newest_added(lib: &Library) -> Option<Book> {
    lib.books(Sort::new(SortKey::Added, true), &Filter::default(), 0, 50)
        .ok()?
        .into_iter()
        .find(|b| b.path.exists())
}

/// How many books have the status.
fn count(lib: &Library, status: Status) -> usize {
    lib.books(
        Sort::new(SortKey::Title, false),
        &Filter {
            status: Some(status),
            ..Filter::default()
        },
        0,
        data::ALL,
    )
    .map_or(0, |v| v.len())
}

/// The empty library: the heading and the steps, then a button to the Library.
fn empty_section(t: &Theme) -> WidgetPod {
    let mut children = common::empty_library(t);
    children.push(button(
        ButtonStyle::Primary,
        &tr("Open Library"),
        "nav:library",
        "empty-library",
    ));
    WidgetPod::new(
        Column::new(children)
            .gap(t.space.md)
            .cross_align(CrossAlign::Stretch)
            .shrink(),
    )
}

/// The book to continue: its label and the card.
fn hero_section(env: &SurfaceEnv<'_>, book: &Book) -> WidgetPod {
    let t = &env.services.theme;
    let label = if book.last_opened_at.is_some() {
        tr("Continue reading")
    } else {
        tr("Start reading")
    };
    WidgetPod::new(
        Column::new(vec![
            WidgetPod::new(Label::new(label, t.font.label.clone(), Tone::InkSecondary)),
            hero_card(env, book),
        ])
        .gap(t.space.md)
        .cross_align(CrossAlign::Stretch)
        .shrink(),
    )
}

/// The recent books as four equal columns. None when there is no other book.
fn shelf_section(env: &SurfaceEnv<'_>, recent: &[Book], hero: Option<&Book>) -> Option<WidgetPod> {
    let t = &env.services.theme;
    let lib = &env.services.library;
    let shelf: Vec<BookInfo> = recent
        .iter()
        .filter(|b| hero.is_none_or(|h| h.id != b.id))
        .take(SHELF)
        .map(|b| data::info(lib, b))
        .collect();
    if shelf.is_empty() {
        return None;
    }
    // Four equal columns across the page. Each tile keeps its cover at 2:3.
    let tile_w = env.screen.w.saturating_sub(2 * t.space.lg + 3 * t.space.sm) / 4;
    let tiles = shelf
        .iter()
        .map(|info| {
            let tile = WidgetPod::new(
                Pressable::new(WidgetPod::new(BookTile::new(t, info)))
                    .tap(format!("open_book:{}", info.id), Vec::new()),
            );
            WidgetPod::new(SizedBox::new(Some(tile.with_key(format!("shelf-{}", info.id)))).width(tile_w))
        })
        .collect();
    Some(WidgetPod::new(
        Column::new(vec![
            WidgetPod::new(Label::new(tr("Recent"), t.font.label.clone(), Tone::InkSecondary)),
            WidgetPod::new(Row::new(tiles).gap(t.space.sm)),
        ])
        .gap(t.space.md)
        .cross_align(CrossAlign::Stretch)
        .shrink(),
    ))
}

/// The other books in progress, one row each. None when there are none.
fn continue_section(t: &Theme, lib: &Library, hero: Option<&Book>) -> Option<WidgetPod> {
    let in_progress = lib
        .books(
            Sort::new(SortKey::LastRead, true),
            &Filter {
                status: Some(Status::Reading),
                ..Filter::default()
            },
            0,
            CONTINUE_ROWS + 1,
        )
        .unwrap_or_default();
    let others: Vec<&Book> = in_progress
        .iter()
        .filter(|b| hero.is_none_or(|h| h.id != b.id))
        .filter(|b| b.path.exists())
        .take(CONTINUE_ROWS)
        .collect();
    if others.is_empty() {
        return None;
    }
    let mut children = vec![WidgetPod::new(Label::new(
        tr("Continue"),
        t.font.label.clone(),
        Tone::InkSecondary,
    ))];
    for book in others {
        let row = ListRow::new(data::title(book))
            .secondary(book.authors.clone().unwrap_or_default())
            .value(format!("{}%", data::percent(book)))
            .on_action(format!("open_book:{}", book.id));
        children.push(WidgetPod::new(row).with_key(format!("continue-{}", book.id)));
    }
    Some(WidgetPod::new(
        Column::new(children)
            .gap(t.space.md)
            .cross_align(CrossAlign::Stretch)
            .shrink(),
    ))
}

/// How long the rest of a book takes at the reading pace, as `3 h 12 min`.
/// None without a page count, or when the book is finished.
fn time_left(env: &SurfaceEnv<'_>, book: &Book) -> Option<String> {
    let pages = f64::from(book.pages?);
    let left = ((1.0 - book.percent.clamp(0.0, 1.0)) * pages).round();
    if left < 1.0 {
        return None;
    }
    let authors = book.authors.as_deref().unwrap_or("N/A");
    let pace = common::seconds_per_page(
        &env.services.paths.data_dir,
        &common::display_title(book),
        authors,
    );
    Some(facts::duration((left * pace) as i64))
}

/// The book to continue: cover, title, author, progress, and the status with the
/// time left. A tap opens it.
fn hero_card(env: &SurfaceEnv<'_>, book: &Book) -> WidgetPod {
    let t = &env.services.theme;
    let title = data::title(book);
    let cover = data::cover(&env.services.library, book.id, CoverSize::Large);
    let cover_box = WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(CoverImage::new(title.clone(), cover))))
            .width(t.scale.dp(100))
            .height(t.scale.dp(150)),
    );
    let mut details = vec![WidgetPod::new(Label::new(
        title,
        t.font.book_title.clone(),
        Tone::Ink,
    ))];
    if let Some(author) = book.authors.as_deref().filter(|a| !a.trim().is_empty()) {
        details.push(WidgetPod::new(Label::new(
            author,
            t.font.body.clone(),
            Tone::InkSecondary,
        )));
    }
    details.push(WidgetPod::new(ProgressBar::new(book.percent as f32)));
    let status = match (book.status, time_left(env, book)) {
        (Status::Reading, Some(left)) => tr_format(&tr("%1 · %2 left"), &[&data::status_label(book), &left]),
        _ => data::status_label(book),
    };
    details.push(WidgetPod::new(Label::new(
        status,
        t.font.caption.clone(),
        Tone::InkSecondary,
    )));
    let row = Row::new(vec![
        cover_box,
        WidgetPod::new(Expanded::new(
            WidgetPod::new(Column::new(details).gap(t.space.sm).shrink()),
            1,
        )),
    ])
    .gap(t.space.md);
    WidgetPod::new(Pressable::new(WidgetPod::new(row)).tap(format!("open_book:{}", book.id), Vec::new()))
        .with_key("continue")
}
