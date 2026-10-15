//! Home shows recent reading covers, the library and a local recent shelf.
//! Optional series and author suggestions occupy the lower row. Scan progress
//! updates at most once per second. Section switches retain their saved values.

pub mod controls;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use kaia_events::{Ctx, Event, RefreshMode, Value};
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_library::{Book, CoverSize, Filter, Library, Sort, SortKey, Status};
use kaia_ui::components::{Column, CrossAlign, Expanded, Label, Padding, Paragraph, Row, SizedBox};
use kaia_ui::composite::CoverImage;
use kaia_ui::controls::ButtonStyle;
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, Tone, WidgetPod, Window};

use crate::surface::{Outcome, Surface, SurfaceEnv};
use crate::surfaces::common::{self, button, root_window as window};
use crate::surfaces::library::data;
use crate::surfaces::library::press::Pressable;
use crate::surfaces::Route;

/// The least time between two progress lines on Home.
const PROGRESS_EVERY: Duration = Duration::from_secs(1);

/// Recent books on the shelf, besides the book to continue.
const SHELF: usize = 4;

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

    fn on_command(&mut self, cmd: &str, _args: &[Value], _env: &mut SurfaceEnv<'_>) -> Outcome {
        if cmd == "home:recent" {
            return Outcome::Navigate(crate::surface::Nav::Push(Box::new(
                super::library::LibrarySurface::recent(),
            )));
        }
        Outcome::NotMine
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

    let shown = controls::Shown::read(&services.settings);
    let on_page = hero.as_ref().filter(|_| shown.current);
    let mut top = Vec::new();
    if let Some(book) = on_page {
        top.push(hero_card(env, book));
    }
    if shown.in_progress {
        if let Some(book) = recent
            .iter()
            .find(|b| b.status == Status::Reading && on_page.is_none_or(|h| h.id != b.id))
        {
            top.push(hero_card(env, book).with_key(format!("continue-{}", book.id)));
        }
    }
    let mut sections = vec![WidgetPod::new(Label::new(
        progress_text(progress),
        t.font.caption.clone(),
        Tone::InkSecondary,
    ))
    .with_key("scan-progress")];
    if hero.is_none() {
        sections.push(empty_section(&t));
    } else if !shown.any() {
        sections.push(all_hidden_section(&t));
    } else {
        sections.push(WidgetPod::new(
            SizedBox::new(Some(WidgetPod::new(Row::new(top).gap(t.scale.dp(13))))).height(t.scale.dp(314)),
        ));
    }

    let all = data::all(lib);
    let library_card = group_card(env, &tr("My books"), &all, "nav:library", "library", false);
    let shelf_books: Vec<Book> = recent
        .iter()
        .filter(|b| on_page.is_none_or(|h| h.id != b.id))
        .cloned()
        .collect();
    let shelf = if shown.recent {
        group_card(
            env,
            &tr("Recent"),
            &shelf_books,
            "home:recent",
            "home-recent",
            true,
        )
    } else {
        WidgetPod::new(SizedBox::new(None))
    };
    if hero.is_some() {
        sections.push(WidgetPod::new(
            Row::new(vec![
                WidgetPod::new(Expanded::new(library_card, 1)),
                WidgetPod::new(Expanded::new(shelf, 1)),
            ])
            .gap(t.scale.dp(12)),
        ));
    }

    let next = if shown.next_in_series {
        next_in_series(lib, &recent, hero.as_ref(), on_page)
    } else {
        None
    };
    let mut offers = Vec::new();
    if let Some(book) = &next {
        offers.push(WidgetPod::new(Expanded::new(
            offer_section(env, tr("Next in series"), book, "next-in-series"),
            1,
        )));
    }
    if shown.more_by_author {
        if let Some((name, book)) = more_by_author(lib, hero.as_ref(), on_page, next.as_ref()) {
            offers.push(WidgetPod::new(Expanded::new(
                offer_section(
                    env,
                    tr_format(&tr("More by %1"), &[&name]),
                    &book,
                    "more-by-author",
                ),
                1,
            )));
        }
    }
    if !offers.is_empty() {
        sections.push(WidgetPod::new(Row::new(offers).gap(t.scale.dp(12))));
    }
    let page = Padding::new(
        WidgetPod::new(
            Column::new(sections)
                .gap(t.scale.dp(12))
                .cross_align(CrossAlign::Stretch)
                .hide_overflow(),
        ),
        Insets {
            left: t.scale.dp(40),
            right: t.scale.dp(39),
            top: 0,
            bottom: 0,
        },
    );
    window(
        services,
        env.screen,
        common::tabbed_page(services, page, kaia_ui::composite::NavItem::Home),
    )
}

fn group_card(
    env: &SurfaceEnv<'_>,
    title: &str,
    books: &[Book],
    action: &str,
    key: &str,
    recent: bool,
) -> WidgetPod {
    let t = &env.services.theme;
    let covers = books
        .iter()
        .take(3)
        .map(|book| {
            let cover = data::cover(&env.services.library, book.id, CoverSize::Small);
            let child = WidgetPod::new(
                SizedBox::new(Some(WidgetPod::new(super::library::widgets::FittedCover::new(
                    data::title(book),
                    cover,
                ))))
                .height(t.scale.dp(132))
                .width(t.scale.dp(82)),
            );
            if recent {
                WidgetPod::new(Pressable::new(child).tap(format!("open_book:{}", book.id), Vec::new()))
                    .with_key(format!("shelf-{}", book.id))
            } else {
                child
            }
        })
        .collect();
    let count = tr_format(
        &tr_n("%1 book", "%1 books", books.len() as i64),
        &[&books.len().to_string()],
    );
    let content = Column::new(vec![
        WidgetPod::new(
            SizedBox::new(Some(WidgetPod::new(Row::new(covers).gap(t.scale.dp(4))))).height(t.scale.dp(132)),
        ),
        WidgetPod::new(Label::new(title, home_font(t), Tone::Ink)),
        WidgetPod::new(Label::new(count, t.font.caption.clone(), Tone::InkSecondary)),
        WidgetPod::new(kaia_ui::components::Divider::new(Tone::Divider)),
    ])
    .gap(t.space.sm)
    .cross_align(CrossAlign::Stretch)
    .shrink();
    WidgetPod::new(Pressable::new(WidgetPod::new(content)).tap(action, Vec::new())).with_key(key)
}

/// The next book in the series of the book read most recently. It is offered when
/// that book is finished or is the book to continue, and the next number of the
/// series is in the library and not started yet. `hero` is the book to continue,
/// whether or not its card shows. `on_page` is the card when it shows, and the
/// card's book is not offered again.
fn next_in_series(
    lib: &Library,
    recent: &[Book],
    hero: Option<&Book>,
    on_page: Option<&Book>,
) -> Option<Book> {
    let last = recent.first()?;
    let current = hero.is_some_and(|h| h.id == last.id);
    if last.status != Status::Complete && !current {
        return None;
    }
    lib.next_in_series(last.id)
        .ok()?
        .filter(|b| b.status == Status::New && b.path.exists() && on_page.is_none_or(|h| h.id != b.id))
}

/// The unread book by an author of the book to continue, for the author tile. The
/// authors are tried in the order the book lists them, and the first one with an
/// unread book in the library gives the tile. The book on the page and the book
/// offered as next in series are left out, so the two tiles never open the same
/// book. Returns the author as the book lists it, and the book.
fn more_by_author(
    lib: &Library,
    hero: Option<&Book>,
    on_page: Option<&Book>,
    next: Option<&Book>,
) -> Option<(String, Book)> {
    let hero = hero?;
    let names = Library::split_authors(hero.authors.as_deref()?);
    if names.is_empty() {
        return None;
    }
    let books = data::all(lib);
    names.into_iter().find_map(|name| {
        Library::books_by_author(&books, name)
            .into_iter()
            .find(|b| {
                b.status == Status::New
                    && on_page.is_none_or(|p| p.id != b.id)
                    && next.is_none_or(|n| n.id != b.id)
            })
            .map(|book| (name.to_string(), book))
    })
}

/// A book offered on Home: a label, then a card that opens the book. `key` names
/// the card for the tests.
fn offer_section(env: &SurfaceEnv<'_>, label: String, book: &Book, key: &str) -> WidgetPod {
    let t = &env.services.theme;
    let title = data::title(book);
    let cover = data::cover(&env.services.library, book.id, CoverSize::Small);
    let cover_box = WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(CoverImage::new(title.clone(), cover))))
            .width(t.scale.dp(44))
            .height(t.scale.dp(64)),
    );
    let mut details = vec![WidgetPod::new(Label::new(
        title,
        t.font.book_title.clone(),
        Tone::Ink,
    ))];
    if let Some(series) = book.series.as_deref() {
        let number = book.series_index.map(|n| n.to_string()).unwrap_or_default();
        details.push(WidgetPod::new(Label::new(
            tr_format(&tr("%1, book %2"), &[series, number.as_str()]),
            t.font.caption.clone(),
            Tone::InkSecondary,
        )));
    }
    let row = Row::new(vec![
        cover_box,
        WidgetPod::new(Expanded::new(
            WidgetPod::new(Column::new(details).gap(t.space.xs).shrink()),
            1,
        )),
    ])
    .gap(t.space.md);
    let card =
        WidgetPod::new(Pressable::new(WidgetPod::new(row)).tap(format!("open_book:{}", book.id), Vec::new()))
            .with_key(key);
    WidgetPod::new(
        Column::new(vec![
            WidgetPod::new(Label::new(label, t.font.caption.clone(), Tone::InkSecondary)),
            card,
        ])
        .gap(t.space.xs)
        .cross_align(CrossAlign::Stretch)
        .shrink(),
    )
}

/// The most recently added book, for a library nobody has opened yet.
fn newest_added(lib: &Library) -> Option<Book> {
    lib.books(Sort::new(SortKey::Added, true), &Filter::default(), 0, 50)
        .ok()?
        .into_iter()
        .find(|b| b.path.exists())
}

/// The empty library: the heading and the steps, then a button to the Library.
fn empty_section(t: &Theme) -> WidgetPod {
    WidgetPod::new(
        Column::new(vec![
            super::library::views::local_empty(
                t,
                &tr("No books yet"),
                &tr("Copy books into the Books folder over USB."),
                85,
            ),
            button(
                ButtonStyle::Secondary,
                &tr("My books"),
                "nav:library",
                "empty-library",
            ),
        ])
        .gap(t.scale.dp(24))
        .cross_align(CrossAlign::Center)
        .shrink(),
    )
}

/// Home with every section switched off: a note and a button to the Library.
fn all_hidden_section(t: &Theme) -> WidgetPod {
    WidgetPod::new(
        Column::new(vec![
            WidgetPod::new(Label::new(
                tr("Every Home section is off."),
                t.font.title.clone(),
                Tone::Ink,
            )),
            WidgetPod::new(Paragraph::new(
                tr("Turn sections back on in Settings under Home. Your books stay in the library."),
                t.font.body.clone(),
                Tone::InkSecondary,
            )),
            button(
                ButtonStyle::Primary,
                &tr("Open Library"),
                "nav:library",
                "home-all-hidden-library",
            ),
        ])
        .gap(t.space.md)
        .cross_align(CrossAlign::Stretch)
        .shrink(),
    )
    .with_key("home-all-hidden")
}

/// A recent cover retains its aspect ratio and shares the row's bottom edge.
fn hero_card(env: &SurfaceEnv<'_>, book: &Book) -> WidgetPod {
    let t = &env.services.theme;
    let cover = data::cover(&env.services.library, book.id, CoverSize::Large);
    let cover = WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(super::library::widgets::FittedCover::new(
            data::title(book),
            cover,
        ))))
        .width(t.scale.dp(128))
        .height(t.scale.dp(188)),
    );
    let caption = tr_format(&tr("%1% read"), &[&data::percent(book).to_string()]);
    let card = Column::new(vec![
        cover,
        WidgetPod::new(Label::new(caption, home_font(t), Tone::Ink)),
    ])
    .gap(t.scale.dp(12))
    .shrink();
    WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(
            Pressable::new(WidgetPod::new(card)).tap(format!("open_book:{}", book.id), Vec::new()),
        )))
        .width(t.scale.dp(128)),
    )
    .with_key("continue")
}

fn home_font(t: &Theme) -> kaia_ui::theme::FontSpec {
    let mut font = t.font.book_title.clone();
    font.weight = 400;
    font
}
