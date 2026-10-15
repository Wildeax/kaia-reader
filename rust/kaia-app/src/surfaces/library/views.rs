//! Widget trees for the Library: the toolbar, the four views, the search page,
//! the panels (sort, collection picker, name, delete confirm) and the book
//! details. Each builder takes the theme and the data it shows, and returns
//! widgets. The surface decides which one is on screen.

use kaia_gfx::{Pixmap, Size};
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_library::storage::Source;
use kaia_library::{Book, Collection, Library, Tag, FAVORITES, MAX_TAG_CHARS};
use kaia_ui::components::{Column, CrossAlign, Expanded, Frame, Label, Paragraph, Row, SizedBox, Spacer};
use kaia_ui::composite::Header;
use kaia_ui::composite::{BookInfo, CoverImage, ListRow, PagedList};
use kaia_ui::controls::{Button, ButtonStyle, IconButton, OnScreenKeyboard, TextField, ENGLISH};
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, Tone, WidgetPod};

use crate::surfaces::common::short;
use crate::surfaces::library::commands::{Chip, SortChoice, View};
use crate::surfaces::library::data::{self, AuthorGroup, Prefs, Rating, SeriesGroup};
use crate::surfaces::library::press::Pressable;

/// One row of a list: a title, a line of detail, an optional cover, and the
/// actions of a tap and a hold.
#[derive(Clone)]
pub struct Line {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub cover: Option<Pixmap>,
    pub tap: String,
    pub hold: Option<String>,
    pub enabled: bool,
    /// In select mode a book row shows a check box with this state, in place of the chevron.
    pub checked: Option<bool>,
}

impl Line {
    /// A book row: the author and the progress as detail, tap opens, hold shows the details.
    pub fn book(info: &BookInfo) -> Line {
        let mut parts = Vec::new();
        if let Some(author) = &info.author {
            parts.push(author.clone());
        }
        parts.push(format!("{}%", (info.progress * 100.0).round() as u32));
        Line {
            key: format!("book-{}", info.id),
            title: short(&info.title),
            detail: parts.join(" · "),
            cover: info.cover.clone(),
            tap: format!("library:open:{}", info.id),
            hold: Some(format!("library:details:{}", info.id)),
            enabled: true,
            checked: None,
        }
    }

    /// A book row in select mode: a check box, and a tap that checks the book.
    pub fn checkable(info: &BookInfo, checked: bool) -> Line {
        let mut line = Line::book(info);
        line.tap = format!("library:select:toggle:{}", info.id);
        line.hold = None;
        line.checked = Some(checked);
        line
    }
}

fn line_row(line: &Line) -> WidgetPod {
    let mut row = ListRow::new(line.title.clone())
        .enabled(line.enabled)
        .on_action(line.tap.clone());
    if !line.detail.is_empty() {
        row = row.secondary(line.detail.clone());
    }
    if line.key.starts_with("book-") {
        row = row.cover(CoverImage::new(line.title.clone(), line.cover.clone()));
    }
    row = match line.checked {
        Some(on) => row.checkbox(on),
        None => row.chevron(),
    };
    let pod = WidgetPod::new(row);
    let pod = match &line.hold {
        Some(id) => WidgetPod::new(Pressable::new(pod).hold(id.clone(), Vec::new())),
        None => pod,
    };
    pod.with_key(line.key.clone())
}

/// A button with a key for the tests and the golden checks.
pub fn button(style: ButtonStyle, label: &str, action: &str) -> WidgetPod {
    WidgetPod::new(Button::new(style).text(label).on_action(action)).with_key(action.replace(':', "-"))
}

/// The page header: the title, the status line and the icon actions. Select mode
/// has none: its bar holds the way out.
pub fn header(theme: &Theme, title: &str, status: Option<String>, selecting: bool, back: bool) -> WidgetPod {
    let mut h = Header::new(theme, title);
    if back {
        h = h.back("back");
    }
    if !selecting {
        h = h
            .action("check", "library:select:on")
            .action("appbar.search", "library:search:open");
    }
    if let Some(text) = status {
        h = h.status(text);
    }
    crate::surfaces::common::status_header(h).with_key("library-header")
}

/// Four primary tabs. Folders and display modes live in the library menu.
pub fn view_tabs(t: &Theme, active: View) -> WidgetPod {
    let books = matches!(active, View::Covers | View::List);
    let tab = |weight: u32, selected: bool, label: &str, action: &str| {
        let mut font = t.font.body.clone();
        font.size = t.scale.dp(16) as f32 * t.text_scale;
        font.weight = if selected { 700 } else { 400 };
        let button = WidgetPod::new(
            Pressable::new(WidgetPod::new(kaia_ui::components::Padding::new(
                WidgetPod::new(Label::new(
                    label,
                    font,
                    if selected { Tone::Accent } else { Tone::Ink },
                )),
                Insets::symmetric(t.scale.dp(6), t.scale.dp(8)),
            )))
            .tap(action, Vec::new()),
        )
        .with_key(action.replace(':', "-"));
        let mark = if selected {
            WidgetPod::new(
                Frame::new(WidgetPod::new(SizedBox::new(None).height(t.line.focus))).fill(Tone::Accent),
            )
        } else {
            WidgetPod::new(SizedBox::new(None).height(t.line.focus))
        };
        WidgetPod::new(Expanded::new(
            WidgetPod::new(
                Column::new(vec![button, mark])
                    .cross_align(CrossAlign::Stretch)
                    .shrink(),
            ),
            weight,
        ))
    };
    let mut tabs = vec![
        tab(7, books, &tr("Books"), "library:view:books"),
        tab(8, active == View::Authors, &tr("Authors"), "library:view:authors"),
        tab(7, active == View::Series, &tr("Series"), "library:view:series"),
        tab(
            12,
            active == View::Collections,
            &tr("Collections"),
            "library:view:collections",
        ),
        WidgetPod::new(Spacer::new(22)),
    ];
    if t.text_scale > 1.0 {
        tabs.pop();
    }
    WidgetPod::new(Row::new(tabs))
}

pub fn books_controls(t: &Theme, filter: &str, sort: &str, active: Option<&str>) -> WidgetPod {
    WidgetPod::new(
        Row::new(vec![
            compact_state(
                t,
                &tr_format(&tr("Filter: %1"), &[filter]),
                "library:filter",
                "library-filter",
                active == Some("filter"),
            ),
            compact_state(
                t,
                &tr_format(&tr("Sort: %1"), &[sort]),
                "library:sort",
                "library-sort",
                active == Some("sort"),
            ),
            WidgetPod::new(Spacer::new(1)),
            compact_state(
                t,
                "...",
                "library:options",
                "library-options",
                active == Some("options"),
            ),
        ])
        .gap(t.scale.dp(18)),
    )
}

/// Compact text controls keep a separate touch target around the label.
pub fn compact(t: &Theme, label: &str, action: &str, key: &str) -> WidgetPod {
    compact_state(t, label, action, key, false)
}

fn compact_state(t: &Theme, label: &str, action: &str, key: &str, active: bool) -> WidgetPod {
    let mut font = t.font.body.clone();
    font.size = t.scale.dp(18) as f32 * t.text_scale;
    let label = WidgetPod::new(kaia_ui::components::Padding::new(
        WidgetPod::new(Label::new(
            label,
            font,
            if active { Tone::Surface } else { Tone::Ink },
        )),
        Insets::symmetric(t.scale.dp(8), t.scale.dp(12)),
    ));
    let label = WidgetPod::new(Frame::new(label).fill(if active { Tone::Ink } else { Tone::Surface }));
    WidgetPod::new(Pressable::new(label).tap(action, Vec::new()))
        .with_key(key)
        .focusable(true)
}

/// The name of the filter in force: each active filter, joined with a comma.
/// "All books" when none is set.
pub fn filter_name(prefs: &Prefs, collection: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(chip) = prefs.chip {
        parts.push(chip_name(chip));
    }
    if prefs.favorite {
        parts.push(tr("Favorites"));
    }
    if let Some(tag) = prefs.tag.as_deref() {
        parts.push(tag.to_string());
    }
    if let Some(name) = collection {
        parts.push(name.to_string());
    }
    if let Some(source) = prefs.source {
        parts.push(source_name(source));
    }
    if let Some(format) = prefs.format.as_deref() {
        parts.push(format_name(format));
    }
    if parts.is_empty() {
        tr("All books")
    } else {
        parts.join(", ")
    }
}

/// The name of a status filter.
fn chip_name(chip: Chip) -> String {
    match chip {
        Chip::Reading => tr("Reading"),
        Chip::NotStarted => tr("Not started"),
        Chip::Finished => tr("Finished"),
    }
}

/// The name of a source filter, as the filter sheet and the filter button show it.
pub fn source_name(source: Source) -> String {
    match source {
        Source::Calibre => tr("Calibre"),
        Source::Opds => tr("OPDS downloads"),
        Source::Other => tr("Other"),
    }
}

/// The name of a file type filter. The text format is `txt`, which reads as "Text".
pub fn format_name(format: &str) -> String {
    if format == "txt" {
        tr("Text")
    } else {
        format.to_uppercase()
    }
}

/// Four independent covers per row, with status and an overflow action.
pub fn covers(
    _t: &Theme,
    _screen: Size,
    infos: Vec<BookInfo>,
    loader: crate::covers::Shared,
    position: std::rc::Rc<std::cell::Cell<i32>>,
) -> WidgetPod {
    WidgetPod::new(
        super::widgets::Scroll::new(infos, 4, 286, move |info: &BookInfo, t: &Theme| {
            let id = info.id.parse().unwrap_or_default();
            let info = info.clone();
            let key = format!("book-{}", info.id);
            let theme = t.clone();
            WidgetPod::new(crate::covers::Cell::new(id, loader.clone(), move |cover| {
                let t = &theme;
                let cover = WidgetPod::new(
                    SizedBox::new(Some(WidgetPod::new(super::widgets::FittedCover::new(
                        info.title.clone(),
                        cover,
                    ))))
                    .height(t.scale.dp(190)),
                );
                let cover = WidgetPod::new(
                    Pressable::new(cover)
                        .tap(format!("library:open:{}", info.id), Vec::new())
                        .hold(format!("library:details:{}", info.id), Vec::new()),
                );
                let status = if info.progress >= 1.0 {
                    tr("Finished")
                } else if info.progress == 0.0 {
                    tr("Unread")
                } else {
                    tr_format(
                        &tr("%1% read"),
                        &[&((info.progress * 100.0).round() as u32).to_string()],
                    )
                };
                WidgetPod::new(
                    Column::new(vec![
                        cover,
                        WidgetPod::new(Label::new(status, t.font.body.clone(), Tone::Ink)),
                        compact(
                            t,
                            "...",
                            &format!("library:menu:{}", info.id),
                            &format!("book-menu-{}", info.id),
                        ),
                    ])
                    .gap(t.space.xs)
                    .cross_align(CrossAlign::Stretch)
                    .shrink(),
                )
            }))
            .with_key(key)
        })
        .position(position),
    )
    .with_key("book-scroll")
}

pub fn book_list(
    books: Vec<Book>,
    loader: crate::covers::Shared,
    position: std::rc::Rc<std::cell::Cell<i32>>,
) -> WidgetPod {
    WidgetPod::new(
        super::widgets::Scroll::new(books, 1, 122, move |book: &Book, t: &Theme| {
            let book = book.clone();
            let key = format!("book-{}", book.id);
            let theme = t.clone();
            WidgetPod::new(crate::covers::Cell::new(book.id, loader.clone(), move |cover| {
                let t = &theme;
                let image = WidgetPod::new(
                    SizedBox::new(Some(WidgetPod::new(super::widgets::FittedCover::new(
                        data::title(&book),
                        cover,
                    ))))
                    .width(t.scale.dp(62))
                    .height(t.scale.dp(82)),
                );
                let title = WidgetPod::new(
                    Column::new(vec![
                        WidgetPod::new(Label::new(
                            data::title(&book),
                            t.font.book_title.clone(),
                            Tone::Ink,
                        )),
                        WidgetPod::new(Label::new(
                            Library::display_authors(book.authors.as_deref().unwrap_or("")),
                            t.font.caption.clone(),
                            Tone::InkSecondary,
                        )),
                    ])
                    .gap(t.space.xs)
                    .shrink(),
                );
                let open = WidgetPod::new(
                    Pressable::new(WidgetPod::new(
                        Row::new(vec![image, WidgetPod::new(Expanded::new(title, 1))]).gap(t.scale.dp(16)),
                    ))
                    .tap(format!("library:open:{}", book.id), Vec::new())
                    .hold(format!("library:details:{}", book.id), Vec::new()),
                );
                WidgetPod::new(
                    Column::new(vec![
                        WidgetPod::new(
                            Row::new(vec![
                                WidgetPod::new(Expanded::new(open, 1)),
                                WidgetPod::new(Label::new(
                                    data::status_label(&book),
                                    t.font.caption.clone(),
                                    Tone::Ink,
                                )),
                                compact(
                                    t,
                                    "...",
                                    &format!("library:menu:{}", book.id),
                                    &format!("book-menu-{}", book.id),
                                ),
                            ])
                            .gap(t.space.sm),
                        ),
                        WidgetPod::new(kaia_ui::components::Divider::new(Tone::Divider)),
                    ])
                    .gap(t.scale.dp(16))
                    .shrink(),
                )
            }))
            .with_key(key)
        })
        .position(position),
    )
    .with_key("list")
}
/// The list view and the search results: rows of cover, title and detail, paged.
pub fn list(lines: Vec<Line>) -> PagedList<Line> {
    PagedList::new(lines, |line: &Line, _t: &Theme| line_row(line))
}

/// Like [`list`], with covers. Only book rows load a cover. Other rows, such as
/// the recent searches, are plain rows.
pub fn list_covers(lines: Vec<Line>, loader: crate::covers::Shared) -> PagedList<Line> {
    PagedList::new(lines, move |line: &Line, _t: &Theme| {
        if !line.key.starts_with("book-") {
            return line_row(line);
        }
        let line = line.clone();
        let id = line
            .key
            .strip_prefix("book-")
            .and_then(|v| v.parse().ok())
            .unwrap_or_default();
        WidgetPod::new(crate::covers::Cell::new(id, loader.clone(), move |cover| {
            let mut line = line.clone();
            line.cover = cover;
            line_row(&line)
        }))
    })
}

/// The folder view: a heading with the place, then the rows.
pub fn folder_rows(t: &Theme, heading: String, rows: Vec<Line>) -> WidgetPod {
    WidgetPod::new(Column::new(vec![
        WidgetPod::new(Label::new(heading, t.font.caption.clone(), Tone::InkSecondary)),
        WidgetPod::new(Expanded::new(
            WidgetPod::new(list(rows)).with_key("folder-list"),
            1,
        )),
    ]))
}

/// The Series view: one row per series, with its number of books. A tap opens the
/// series. The index of a row is the index `library:series:<n>` names.
pub fn series_list(groups: &[SeriesGroup]) -> WidgetPod {
    let items: Vec<(usize, SeriesGroup)> = groups.iter().cloned().enumerate().collect();
    let list = super::widgets::Scroll::new(items, 1, 90, |(n, group): &(usize, SeriesGroup), _t: &Theme| {
        let row = ListRow::new(group.name.clone())
            .secondary(tr_format(
                &tr_n("%1 book", "%1 books", group.books as i64),
                &[&group.books.to_string()],
            ))
            .chevron()
            .on_action(format!("library:series:{n}"));
        WidgetPod::new(row).with_key(format!("series-{n}"))
    });
    WidgetPod::new(Expanded::new(WidgetPod::new(list).with_key("series-list"), 1))
}

/// The books of an open series, in series order: the name, a row to go back to
/// the list of series, then the books with their covers.
pub fn series_books(t: &Theme, name: &str, books: Vec<Book>, loader: crate::covers::Shared) -> WidgetPod {
    WidgetPod::new(
        Column::new(vec![
            WidgetPod::new(Label::new(
                name.to_string(),
                t.font.caption.clone(),
                Tone::InkSecondary,
            )),
            WidgetPod::new(ListRow::new(tr("Up")).on_action("library:series:up")).with_key("series-up"),
            WidgetPod::new(Expanded::new(
                book_list(books, loader, std::rc::Rc::new(std::cell::Cell::new(0))).with_key("series-books"),
                1,
            )),
        ])
        .gap(t.space.sm)
        .cross_align(CrossAlign::Stretch),
    )
}

/// The Authors view: one row per author, with the number of books. A tap opens the
/// author. The index of a row is the index `library:author:<n>` names.
pub fn author_list(groups: &[AuthorGroup], position: std::rc::Rc<std::cell::Cell<i32>>) -> WidgetPod {
    let items: Vec<_> = groups.iter().cloned().enumerate().collect();
    WidgetPod::new(Expanded::new(
        WidgetPod::new(
            super::widgets::Scroll::new(items, 1, 60, |(n, group): &(usize, AuthorGroup), t: &Theme| {
                let row = Row::new(vec![
                    WidgetPod::new(Expanded::new(
                        compact(
                            t,
                            &group.name,
                            &format!("library:author:{n}"),
                            &format!("author-{n}"),
                        ),
                        1,
                    )),
                    WidgetPod::new(Label::new(
                        tr_format(
                            &tr_n("%1 book", "%1 books", group.books as i64),
                            &[&group.books.to_string()],
                        ),
                        t.font.caption.clone(),
                        Tone::Ink,
                    )),
                ])
                .cross_align(CrossAlign::Center);
                WidgetPod::new(
                    Column::new(vec![
                        WidgetPod::new(row),
                        WidgetPod::new(kaia_ui::components::Divider::new(Tone::Divider)),
                    ])
                    .shrink(),
                )
            })
            .position(position),
        )
        .with_key("author-list"),
        1,
    ))
}

pub fn empty_group(t: &Theme, collections: bool) -> WidgetPod {
    let circle = WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(EmptyBooks)))
            .width(t.scale.dp(252))
            .height(t.scale.dp(252)),
    );
    let illustration = WidgetPod::new(Row::new(vec![
        WidgetPod::new(Spacer::new(1)),
        circle,
        WidgetPod::new(Spacer::new(1)),
    ]));
    let mut rows = vec![
        WidgetPod::new(SizedBox::new(None).height(t.scale.dp(if collections { 48 } else { 82 }))),
        illustration,
        WidgetPod::new(Label::new(
            if collections {
                tr("Your collections")
            } else {
                tr("No series yet")
            },
            t.font.title.clone(),
            Tone::Ink,
        )),
        body_text(
            t,
            &if collections {
                tr("Group your books into collections.")
            } else {
                tr("Books with series information appear here.")
            },
        ),
    ];
    if collections {
        rows.push(button(
            ButtonStyle::Secondary,
            &tr("Create collection"),
            "library:collections:new",
        ));
    }
    WidgetPod::new(
        Column::new(rows)
            .gap(t.scale.dp(22))
            .cross_align(CrossAlign::Center),
    )
}

struct EmptyBooks;
impl kaia_ui::Widget for EmptyBooks {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _cx: &mut kaia_ui::LayoutCx, c: kaia_ui::geometry::Constraints) -> Size {
        c.max
    }
    fn paint(&mut self, cx: &mut kaia_ui::PaintCx) {
        let b = cx.bounds();
        let center = kaia_gfx::Point::new(b.x + b.w as i32 / 2, b.y + b.h as i32 / 2);
        cx.fill_circle(center, b.w.min(b.h) as f32 / 2.0, cx.color(Tone::Accent));
        let ink = cx.color(Tone::Surface);
        let dp = |v| cx.theme.scale.dp(v);
        for n in 0..3 {
            let r = kaia_gfx::Rect::new(
                center.x - dp(62) as i32 + n * dp(42) as i32,
                center.y - dp(66) as i32,
                dp(36),
                dp(132),
            );
            cx.border(r, dp(2), dp(3), ink);
            cx.fill_rect(
                kaia_gfx::Rect::new(r.x + dp(6) as i32, r.y + dp(18) as i32, dp(24), dp(3)),
                ink,
            );
        }
    }
}

/// The books of an open author: the name, a row to go back to the list of
/// authors, then `books`, which the caller lays out as covers or as a list.
pub fn author_books(t: &Theme, name: &str, books: WidgetPod) -> WidgetPod {
    WidgetPod::new(
        Column::new(vec![
            WidgetPod::new(Label::new(
                name.to_string(),
                t.font.caption.clone(),
                Tone::InkSecondary,
            )),
            WidgetPod::new(ListRow::new(tr("Up")).on_action("library:author:up")).with_key("author-up"),
            WidgetPod::new(Expanded::new(books.with_key("author-books"), 1)),
        ])
        .gap(t.space.sm)
        .cross_align(CrossAlign::Stretch),
    )
}

/// The collections tab: one row per collection, which filters the covers by it.
pub fn collections(t: &Theme, collections: &[Collection]) -> WidgetPod {
    if collections.is_empty() {
        return empty_group(t, true);
    }
    let items: Vec<_> = collections
        .iter()
        .enumerate()
        .map(|(position, c)| (c.clone(), position))
        .collect();
    let list =
        super::widgets::Scroll::new(items, 1, 150, |(c, position): &(Collection, usize), t: &Theme| {
            let id = c.id;
            // The favorites collection keeps its name and its members: no rename or delete.
            let mut actions = Vec::new();
            if !c.name.eq_ignore_ascii_case(FAVORITES) {
                actions.push(button(
                    ButtonStyle::Secondary,
                    &tr("Rename"),
                    &format!("library:collections:rename:{id}"),
                ));
                actions.push(button(
                    ButtonStyle::Secondary,
                    &tr("Delete"),
                    &format!("library:collections:delete:{id}"),
                ));
            }
            actions.push(button(
                ButtonStyle::Secondary,
                &tr("Move up"),
                &format!("library:collections:move:{id}:{}", position.saturating_sub(1)),
            ));
            actions.push(button(
                ButtonStyle::Secondary,
                &tr("Move down"),
                &format!("library:collections:move:{id}:{}", position + 1),
            ));
            WidgetPod::new(
                Column::new(vec![
                    WidgetPod::new(
                        ListRow::new(c.name.clone())
                            .secondary(tr_format(
                                &tr_n("%1 book", "%1 books", c.books as i64),
                                &[&c.books.to_string()],
                            ))
                            .on_action(format!("library:collection:{id}")),
                    )
                    .with_key(format!("collection-{id}")),
                    WidgetPod::new(Row::new(actions).gap(t.space.sm)),
                ])
                .shrink(),
            )
        });
    WidgetPod::new(Expanded::new(
        WidgetPod::new(
            Column::new(vec![
                button(
                    ButtonStyle::Primary,
                    &tr("New collection"),
                    "library:collections:new",
                ),
                WidgetPod::new(Expanded::new(WidgetPod::new(list).with_key("collection-list"), 1)),
                button(
                    ButtonStyle::Secondary,
                    &tr("Clear reading history"),
                    "library:history:clear",
                ),
            ])
            .gap(t.space.md)
            .cross_align(CrossAlign::Stretch),
        ),
        1,
    ))
}

/// A paragraph in the secondary tone.
pub fn body_text(t: &Theme, text: &str) -> WidgetPod {
    WidgetPod::new(Paragraph::new(
        text.to_string(),
        t.font.body.clone(),
        Tone::InkSecondary,
    ))
}

/// The search page: a field with a close button, the results, and the keyboard.
/// With no text the list holds the recent searches. `has_recent` enables the
/// button that clears them.
pub fn search(
    t: &Theme,
    query: &str,
    results: Vec<Line>,
    loader: crate::covers::Shared,
    has_recent: bool,
) -> Vec<WidgetPod> {
    let field = WidgetPod::new(
        TextField::new()
            .placeholder(tr("Title, author, series"))
            .with_text(query)
            .on_change("library:search")
            .on_submit("library:search:submit"),
    )
    .focusable(true)
    .with_key("search-field");
    let top = WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(field, 1)),
            button(ButtonStyle::Secondary, &tr("Close"), "library:search:close"),
        ])
        .gap(t.space.sm)
        .cross_align(CrossAlign::Center),
    );
    // A fixed hint: the results change without a rebuild, so the hint cannot depend on them.
    // The clear button sits beside it, out of the list, so the list can page.
    let clear = WidgetPod::new(
        Button::new(ButtonStyle::Secondary)
            .text(tr("Clear"))
            .enabled(has_recent)
            .on_action("library:search:recent:clear"),
    )
    .with_key("library-search-recent-clear");
    let hint = WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(
                body_text(t, &tr("Type a title, an author or a word from the description.")),
                1,
            )),
            clear,
        ])
        .gap(t.space.sm)
        .cross_align(CrossAlign::Center),
    );
    let list = WidgetPod::new(Expanded::new(
        WidgetPod::new(list_covers(results, loader)).with_key("results"),
        1,
    ));
    let keyboard = WidgetPod::new(OnScreenKeyboard::new(&ENGLISH).background(Tone::Surface));
    vec![top, hint, list, keyboard]
}

/// A sheet: a bordered panel with a title and the content, used for every panel.
pub fn sheet(t: &Theme, title: &str, children: Vec<WidgetPod>) -> WidgetPod {
    let head = WidgetPod::new(Label::new(title, t.font.title.clone(), Tone::Ink));
    sheet_with(t, head, children, t.space.md)
}

/// A sheet whose head is a widget of its own, with `gap` between its parts.
pub fn sheet_with(t: &Theme, head: WidgetPod, children: Vec<WidgetPod>, gap: u32) -> WidgetPod {
    let mut all = vec![head];
    all.extend(children);
    let column = Column::new(all)
        .gap(gap)
        .cross_align(CrossAlign::Stretch)
        .shrink();
    framed(t, WidgetPod::new(column))
}

/// A sheet with a list that takes the height the other parts leave. The parts
/// before and after it keep their natural height, so a long list pages and the
/// buttons below it stay on the page.
pub fn sheet_list(
    t: &Theme,
    title: &str,
    before: Vec<WidgetPod>,
    list: WidgetPod,
    after: Vec<WidgetPod>,
) -> WidgetPod {
    let mut all = vec![WidgetPod::new(Label::new(title, t.font.title.clone(), Tone::Ink))];
    all.extend(before);
    all.push(WidgetPod::new(Expanded::new(list, 1)));
    all.extend(after);
    let column = Column::new(all).gap(t.space.md).cross_align(CrossAlign::Stretch);
    framed(t, WidgetPod::new(column))
}

/// The border, the fill and the padding of a sheet around its column.
fn framed(t: &Theme, column: WidgetPod) -> WidgetPod {
    let padded = WidgetPod::new(kaia_ui::components::Padding::new(column, Insets::all(t.space.md)));
    WidgetPod::new(
        Frame::new(padded)
            .fill(Tone::Surface)
            .border(Tone::Ink, t.line.hairline)
            .radius(t.radius.card),
    )
}

/// The name of a sort choice, as the sort panel and the sort button show it.
pub fn sort_name(choice: SortChoice) -> String {
    match choice {
        SortChoice::Title => tr("Title"),
        SortChoice::Author => tr("Author"),
        SortChoice::Read => tr("Recent"),
        SortChoice::Added => tr("Added"),
        SortChoice::Progress => tr("Progress"),
        SortChoice::Series => tr("Series"),
        SortChoice::Size => tr("File size"),
        SortChoice::Type => tr("File type"),
    }
}

/// The sort panel: one row per choice. The current one shows a value.
pub fn sort_panel(t: &Theme, current: SortChoice) -> WidgetPod {
    let options = [
        (SortChoice::Read, "read"),
        (SortChoice::Added, "added"),
        (SortChoice::Title, "title"),
        (SortChoice::Author, "author"),
        (SortChoice::Series, "series"),
        (SortChoice::Size, "size"),
        (SortChoice::Type, "type"),
    ];
    menu(
        t,
        options
            .into_iter()
            .map(|(choice, key)| {
                menu_row(
                    t,
                    &sort_name(choice),
                    &format!("library:sort:{key}"),
                    &format!("sort-{key}"),
                    current == choice,
                    None,
                )
            })
            .collect(),
    )
}

pub fn menu_row(
    t: &Theme,
    label: &str,
    action: &str,
    key: &str,
    checked: bool,
    count: Option<usize>,
) -> WidgetPod {
    let text = if checked {
        format!("✓ {label}")
    } else {
        label.into()
    };
    let mut row = vec![WidgetPod::new(Expanded::new(compact(t, &text, action, key), 1))];
    if let Some(count) = count {
        row.push(WidgetPod::new(Label::new(
            count.to_string(),
            t.font.caption.clone(),
            Tone::Ink,
        )));
    }
    WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(
            Column::new(vec![
                WidgetPod::new(Row::new(row).cross_align(CrossAlign::Center)),
                WidgetPod::new(kaia_ui::components::Divider::new(Tone::Divider)),
            ])
            .cross_align(CrossAlign::Stretch)
            .shrink(),
        )))
        .height(t.scale.dp(46)),
    )
}

pub fn menu(t: &Theme, rows: Vec<WidgetPod>) -> WidgetPod {
    WidgetPod::new(
        Frame::new(WidgetPod::new(kaia_ui::components::Padding::new(
            WidgetPod::new(Column::new(rows).cross_align(CrossAlign::Stretch).shrink()),
            Insets::all(t.scale.dp(5)),
        )))
        .fill(Tone::Surface)
        .border(Tone::Ink, 2),
    )
}

pub fn options_panel(t: &Theme, layout: View) -> WidgetPod {
    let (label, action) = if layout == View::List {
        (tr("Show covers"), "library:view:covers")
    } else {
        (tr("Show list"), "library:view:list")
    };
    menu(
        t,
        vec![
            menu_row(t, &label, action, &action.replace(':', "-"), false, None),
            menu_row(
                t,
                &tr("Select books"),
                "library:select:on",
                "library-select-on",
                false,
                None,
            ),
            menu_row(
                t,
                &tr("Folders"),
                "library:view:folders",
                "library-view-folders",
                false,
                None,
            ),
            menu_row(
                t,
                &tr("More filters"),
                "library:filters:more",
                "library-filters-more",
                false,
                None,
            ),
            menu_row(
                t,
                &tr("Sort by progress"),
                "library:sort:progress",
                "sort-progress",
                false,
                None,
            ),
        ],
    )
}

pub fn book_menu(t: &Theme, book: i64, details: bool) -> WidgetPod {
    let mut rows = Vec::new();
    if !details {
        for (label, action) in [(tr("Read"), "open"), (tr("Details"), "details")] {
            let id = format!("library:{action}:{book}");
            rows.push(menu_row(t, &label, &id, &id.replace(':', "-"), false, None));
        }
    }
    for (label, action) in [
        (tr("Add to collection"), "collection"),
        (tr("Mark unread"), "unread"),
        (tr("Mark finished"), "finished"),
        (tr("Remove"), "delete"),
    ] {
        let id = format!("library:book:{action}:{book}");
        rows.push(menu_row(t, &label, &id, &id.replace(':', "-"), false, None));
    }
    menu(t, rows)
}

pub fn primary_filter(t: &Theme, services: &crate::services::Services, prefs: &Prefs) -> WidgetPod {
    let choices = [
        (tr("All books"), "all", None),
        (tr("Unread"), "not-started", Some(Chip::NotStarted)),
        (tr("Reading"), "reading", Some(Chip::Reading)),
        (tr("Finished"), "finished", Some(Chip::Finished)),
    ];
    let mut rows: Vec<_> = choices
        .into_iter()
        .map(|(label, action, chip)| {
            let mut counted = prefs.clone();
            counted.chip = chip;
            menu_row(
                t,
                &label,
                &format!("library:filter:{action}"),
                &format!("library-filter-{action}"),
                prefs.chip == chip,
                Some(data::list_in(services, &counted).len()),
            )
        })
        .collect();
    for (source, label, key) in [
        (Source::Other, tr("Imported"), "other"),
        (Source::Opds, tr("Downloaded"), "opds"),
    ] {
        let mut counted = prefs.clone();
        counted.source = Some(source);
        rows.push(menu_row(
            t,
            &label,
            &format!("library:filter:source:{key}"),
            &format!("library-filter-source-{key}"),
            prefs.source == Some(source),
            Some(data::list_in(services, &counted).len()),
        ));
    }
    menu(t, rows).with_key("primary-filter")
}

/// The filter panel: the statuses and the collection as buttons, the choice in
/// force filled. A button fits the page where a row list would not, so the panel
/// keeps its Cancel on screen.
pub fn filter_panel(t: &Theme, prefs: &Prefs, collection: Option<&str>) -> WidgetPod {
    let button = |style: ButtonStyle, label: &str, action: &str| {
        WidgetPod::new(
            Button::new(style)
                .accent(style == ButtonStyle::Primary)
                .text(label)
                .on_action(action),
        )
        .with_key(action.replace(':', "-"))
    };
    let chip = prefs.chip;
    let source = prefs.source;
    let format = prefs.format.as_deref();
    let choice = |active: bool| {
        if active {
            ButtonStyle::Primary
        } else {
            ButtonStyle::Secondary
        }
    };
    let pair = |left: WidgetPod, right: WidgetPod| {
        WidgetPod::new(
            Row::new(vec![
                WidgetPod::new(Expanded::new(left, 1)),
                WidgetPod::new(Expanded::new(right, 1)),
            ])
            .gap(t.space.sm),
        )
    };
    let all = button(choice(chip.is_none()), &tr("All books"), "library:filter:all");
    let reading = button(
        choice(chip == Some(Chip::Reading)),
        &chip_name(Chip::Reading),
        "library:filter:reading",
    );
    let not_started = button(
        choice(chip == Some(Chip::NotStarted)),
        &chip_name(Chip::NotStarted),
        "library:filter:not-started",
    );
    let finished = button(
        choice(chip == Some(Chip::Finished)),
        &chip_name(Chip::Finished),
        "library:filter:finished",
    );
    let collection_label = collection.map_or_else(|| tr("Collection"), str::to_string);
    let collection_button = button(
        choice(collection.is_some()),
        &collection_label,
        "library:filter:collection",
    );
    let any_source = button(
        choice(source.is_none()),
        &tr("All sources"),
        "library:filter:source:all",
    );
    let source_button =
        |which: Source, action: &str| button(choice(source == Some(which)), &source_name(which), action);
    let type_label = format.map_or_else(|| tr("File type"), format_name);
    let type_button = button(choice(format.is_some()), &type_label, "library:filter:type");
    let favorites = button(
        choice(prefs.favorite),
        &tr("Favorites"),
        "library:filter:favorite",
    );
    let tags_label = prefs.tag.clone().unwrap_or_else(|| tr("Tags"));
    let tags = button(choice(prefs.tag.is_some()), &tags_label, "library:filter:tags");
    sheet(
        t,
        &tr("Show books"),
        vec![
            WidgetPod::new(
                Column::new(vec![
                    pair(all, reading),
                    pair(not_started, finished),
                    pair(collection_button, type_button),
                    pair(
                        any_source,
                        source_button(Source::Calibre, "library:filter:source:calibre"),
                    ),
                    pair(
                        source_button(Source::Opds, "library:filter:source:opds"),
                        source_button(Source::Other, "library:filter:source:other"),
                    ),
                    pair(favorites, tags),
                ])
                .gap(t.space.sm)
                .cross_align(CrossAlign::Stretch)
                .shrink(),
            ),
            button(ButtonStyle::Secondary, &tr("Cancel"), "library:panel:close"),
        ],
    )
}

/// A row of a picker. The chosen row shows a value, so the choice does not depend on colour.
fn picker_row(name: String, action: String, chosen: bool) -> ListRow {
    let row = ListRow::new(name).on_action(action);
    if chosen {
        row.value(tr("Showing"))
    } else {
        row
    }
}

/// The file type picker: every type in the library, and All types. The current
/// one shows a value.
pub fn file_type_panel(t: &Theme, formats: &[String], current: Option<&str>) -> WidgetPod {
    let mut rows = vec![WidgetPod::new(picker_row(
        tr("All types"),
        "library:format:all".into(),
        current.is_none(),
    ))
    .with_key("format-all")];
    for format in formats {
        let row = picker_row(
            format_name(format),
            format!("library:format:{format}"),
            current == Some(format.as_str()),
        );
        rows.push(WidgetPod::new(row).with_key(format!("format-{format}")));
    }
    if formats.is_empty() {
        rows.push(body_text(t, &tr("No books yet.")));
    }
    let children = vec![
        WidgetPod::new(Column::new(rows).cross_align(CrossAlign::Stretch).shrink()),
        button(ButtonStyle::Secondary, &tr("Cancel"), "library:panel:close"),
    ];
    sheet(t, &tr("File type"), children)
}

pub fn collection_panel(
    t: &Theme,
    collections: &[Collection],
    book: Option<i64>,
    filter: Option<i64>,
) -> WidgetPod {
    let mut rows = Vec::new();
    for c in collections {
        let action = match book {
            Some(b) => format!("library:add:{b}:{}", c.id),
            None => format!("library:collection:{}", c.id),
        };
        let mut row = ListRow::new(c.name.clone())
            .secondary(tr_format(
                &tr_n("%1 book", "%1 books", c.books as i64),
                &[&c.books.to_string()],
            ))
            .on_action(action.clone());
        if book.is_none() && filter == Some(c.id) {
            row = row.value(tr("Showing"));
        }
        rows.push(WidgetPod::new(row).with_key(format!("collection-{}", c.id)));
    }
    if rows.is_empty() {
        rows.push(body_text(t, &tr("No collections yet.")));
    }
    let mut footer = Vec::new();
    match book {
        Some(b) => footer.push(button(
            ButtonStyle::Primary,
            &tr("New collection"),
            &format!("library:add:{b}:new"),
        )),
        None if filter.is_some() => footer.push(button(
            ButtonStyle::Secondary,
            &tr("Show all books"),
            "library:collection:none",
        )),
        None => {}
    }
    footer.push(button(
        ButtonStyle::Secondary,
        &tr("Cancel"),
        "library:panel:close",
    ));
    let title = if book.is_some() {
        tr("Add to collection")
    } else {
        tr("Filter by collection")
    };
    let mut children = vec![WidgetPod::new(
        Column::new(rows).cross_align(CrossAlign::Stretch).shrink(),
    )];
    children.push(WidgetPod::new(Row::new(footer).gap(t.space.md)));
    sheet(t, &title, children)
}

/// The tag picker of the filter: every tag the books carry, with its book count.
/// The tag in force shows a value. The list pages when there are many tags.
pub fn tag_filter_panel(t: &Theme, tags: &[Tag], current: Option<&str>) -> WidgetPod {
    let list = if tags.is_empty() {
        body_text(t, &tr("No tags yet."))
    } else {
        let current = current.map(str::to_lowercase);
        WidgetPod::new(PagedList::new(tags.to_vec(), move |tag: &Tag, _t: &Theme| {
            let action = format!("library:filter:tag:{}", tag.id);
            let mut row = ListRow::new(tag.name.clone())
                .secondary(tr_format(
                    &tr_n("%1 book", "%1 books", tag.books as i64),
                    &[&tag.books.to_string()],
                ))
                .on_action(action.clone());
            if current.as_deref() == Some(tag.name.to_lowercase().as_str()) {
                row = row.value(tr("Showing"));
            }
            WidgetPod::new(row).with_key(action.replace(':', "-"))
        }))
        .with_key("tag-list")
    };
    let mut after = Vec::new();
    if current.is_some() {
        after.push(button(
            ButtonStyle::Secondary,
            &tr("Show all tags"),
            "library:filter:tag:none",
        ));
    }
    after.push(button(
        ButtonStyle::Secondary,
        &tr("Cancel"),
        "library:panel:close",
    ));
    sheet_list(t, &tr("Filter by tag"), Vec::new(), list, after)
}

/// The tag sheet of a book. The list shows its tags, each with a Remove action,
/// and an Add tag button. Typing shows the field and the keyboard for the new
/// tag, with Back to the list and Add. The two do not share the page: the keyboard
/// takes most of its height. `text` is the tag being typed.
pub fn tags_panel(t: &Theme, book: i64, tags: &[Tag], text: &str, typing: bool) -> WidgetPod {
    if typing {
        let field = WidgetPod::new(
            TextField::new()
                .placeholder(tr("New tag"))
                .with_text(text)
                .max_length(MAX_TAG_CHARS)
                .on_change("library:tag")
                .on_submit(format!("library:tag:add:{book}")),
        )
        .focusable(true)
        .with_key("tag-field");
        let keyboard = WidgetPod::new(OnScreenKeyboard::new(&ENGLISH).background(Tone::Surface));
        return sheet(
            t,
            &tr("New tag"),
            vec![
                field,
                keyboard,
                pair(
                    t,
                    button(
                        ButtonStyle::Secondary,
                        &tr("Back"),
                        &format!("library:tags:list:{book}"),
                    ),
                    button(
                        ButtonStyle::Primary,
                        &tr("Add"),
                        &format!("library:tag:add:{book}"),
                    ),
                ),
            ],
        );
    }
    let list = if tags.is_empty() {
        body_text(t, &tr("No tags yet."))
    } else {
        WidgetPod::new(PagedList::new(tags.to_vec(), move |tag: &Tag, _t: &Theme| {
            let action = format!("library:tag:remove:{book}:{}", tag.id);
            WidgetPod::new(
                ListRow::new(tag.name.clone())
                    .value(tr("Remove"))
                    .on_action(action.clone()),
            )
            .with_key(action.replace(':', "-"))
        }))
        .with_key("tag-list")
    };
    let after = vec![
        button(
            ButtonStyle::Primary,
            &tr("Add tag"),
            &format!("library:tags:new:{book}"),
        ),
        button(
            ButtonStyle::Secondary,
            &tr("Back"),
            &format!("library:tags:back:{book}"),
        ),
    ];
    sheet_list(t, &tr("Tags"), Vec::new(), list, after)
}

/// The name panel for a new collection: a field, the keyboard and two buttons.
pub fn name_panel(t: &Theme, name: &str) -> WidgetPod {
    let field = WidgetPod::new(
        TextField::new()
            .placeholder(tr("Collection name"))
            .with_text(name)
            .on_change("library:name")
            .on_submit("library:name:save"),
    )
    .focusable(true)
    .with_key("name-field");
    let keyboard = WidgetPod::new(OnScreenKeyboard::new(&ENGLISH).background(Tone::Surface));
    sheet(
        t,
        &tr("New collection"),
        vec![
            field,
            keyboard,
            WidgetPod::new(
                Row::new(vec![
                    button(ButtonStyle::Secondary, &tr("Cancel"), "library:panel:close"),
                    button(ButtonStyle::Primary, &tr("Save"), "library:name:save"),
                ])
                .gap(t.space.md),
            ),
        ],
    )
}

/// The delete confirm: the book name, one sentence, Cancel and Delete.
pub fn confirm_panel(t: &Theme, book: &Book) -> WidgetPod {
    let title = crate::surfaces::common::display_title(book);
    sheet(
        t,
        &tr("Delete this file?"),
        vec![
            body_text(
                t,
                &tr_format(
                    &tr("\"%1\" is removed from the device. The book leaves the library. This cannot be undone."),
                    &[&title],
                ),
            ),
            WidgetPod::new(
                Row::new(vec![
                    button(ButtonStyle::Secondary, &tr("Cancel"), "library:panel:close"),
                    button(
                        ButtonStyle::Primary,
                        &tr("Delete"),
                        &format!("library:book:delete-confirm:{}", book.id),
                    ),
                ])
                .gap(t.space.md),
            ),
        ],
    )
}

/// What the details sheet shows about a book, beside its row.
pub struct DetailsInfo<'a> {
    pub rating: Rating,
    pub metadata: bool,
    /// True when the book has a row in the statistics database, which adds the
    /// reading statistics button after the rating.
    pub statistics: bool,
    pub favorite: bool,
    pub tags: &'a [Tag],
}

/// The details of one book: cover and facts, the star, the description, and the
/// actions. `remove` is the action that takes the book out of the collection being
/// viewed. None when the book is not in that collection, and then the sheet offers
/// to add the book to a collection.
pub fn details_panel(
    t: &Theme,
    book: &Book,
    cover: Option<Pixmap>,
    close: &str,
    remove: Option<String>,
    info: DetailsInfo<'_>,
) -> WidgetPod {
    let DetailsInfo {
        rating,
        statistics,
        favorite,
        tags,
        metadata,
    } = info;
    let id = book.id;
    let title = data::title(book);
    let next = if favorite { "off" } else { "on" };
    let star = WidgetPod::new(
        IconButton::new(if favorite { "star.full" } else { "star.empty" })
            .on_action(format!("library:favorite:{id}:{next}")),
    )
    .with_key(format!("library-favorite-{id}-{next}"));
    let header = WidgetPod::new(Row::new(vec![
        WidgetPod::new(Expanded::new(
            WidgetPod::new(Label::new(tr("My books"), t.font.caption.clone(), Tone::Ink)),
            1,
        )),
        star,
        WidgetPod::new(IconButton::new("close").on_action(close)).with_key(close.replace(':', "-")),
    ]));
    let mut facts = vec![
        WidgetPod::new(Paragraph::new(title.clone(), t.font.book_title.clone(), Tone::Ink).max_lines(3)),
        WidgetPod::new(Label::new(
            Library::display_authors(book.authors.as_deref().unwrap_or("")),
            t.font.body.clone(),
            Tone::Ink,
        )),
        WidgetPod::new(
            Row::new(vec![
                WidgetPod::new(
                    Pressable::new(WidgetPod::new(
                        Frame::new(WidgetPod::new(
                            SizedBox::new(Some(WidgetPod::new(kaia_ui::components::Align::new(
                                WidgetPod::new(Label::new(tr("Read"), t.font.body.clone(), Tone::Ink)),
                                kaia_ui::geometry::Alignment::Center,
                                kaia_ui::geometry::Alignment::Center,
                            ))))
                            .width(t.scale.dp(125))
                            .height(t.scale.dp(40)),
                        ))
                        .fill(Tone::SurfaceAlt),
                    ))
                    .tap(format!("library:open:{id}"), Vec::new()),
                )
                .with_key(format!("library-open-{id}")),
                compact(
                    t,
                    "...",
                    &format!("library:menu:{id}"),
                    &format!("details-menu-{id}"),
                ),
            ])
            .gap(t.space.sm),
        ),
    ];
    if let Some(series) = book.series.as_deref() {
        facts.push(
            WidgetPod::new(Label::new(series, t.font.caption.clone(), Tone::InkSecondary))
                .with_key("details-series"),
        );
    }
    if !tags.is_empty() {
        facts.push(
            WidgetPod::new(Label::new(
                tags.iter()
                    .map(|tag| tag.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                t.font.caption.clone(),
                Tone::InkSecondary,
            ))
            .with_key("details-tags"),
        );
    }
    let upper = WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(
            Row::new(vec![
                WidgetPod::new(
                    SizedBox::new(Some(WidgetPod::new(super::widgets::FittedCover::new(
                        title, cover,
                    ))))
                    .width(t.scale.dp(143))
                    .height(t.scale.dp(189)),
                ),
                WidgetPod::new(Expanded::new(
                    WidgetPod::new(Column::new(facts).gap(t.space.sm).shrink()),
                    1,
                )),
            ])
            .gap(t.scale.dp(13)),
        )))
        .height(t.scale.dp(320)),
    );
    let tab = |label: &str, action: &str, active| {
        let mut font = t.font.body.clone();
        font.size = t.scale.dp(16) as f32 * t.text_scale;
        font.weight = if active { 700 } else { 400 };
        WidgetPod::new(
            SizedBox::new(Some(WidgetPod::new(
                Column::new(vec![
                    WidgetPod::new(
                        Pressable::new(WidgetPod::new(kaia_ui::components::Padding::new(
                            WidgetPod::new(Label::new(
                                label,
                                font,
                                if active { Tone::Accent } else { Tone::Ink },
                            )),
                            Insets::symmetric(t.scale.dp(8), t.scale.dp(4)),
                        )))
                        .tap(action, Vec::new()),
                    )
                    .with_key(action.replace(':', "-")),
                    WidgetPod::new(
                        Frame::new(WidgetPod::new(SizedBox::new(None).height(t.scale.dp(3))))
                            .fill(if active { Tone::Accent } else { Tone::Surface }),
                    ),
                ])
                .cross_align(CrossAlign::Stretch)
                .shrink(),
            )))
            .width((t.scale.dp(110) as f32 * t.text_scale) as u32),
        )
    };
    let tabs = WidgetPod::new(
        Row::new(vec![
            tab(&tr("Synopsis"), "library:details-tab:synopsis", !metadata),
            tab(&tr("Details"), "library:details-tab:metadata", metadata),
        ])
        .gap(t.space.md),
    );
    let text = if metadata {
        [
            tr_format(&tr("Identifier: %1"), &[&book.partial_md5]),
            tr_format(&tr("Language: %1"), &[book.language.as_deref().unwrap_or("-")]),
            tr_format(
                &tr("File: %1"),
                &[&book.path.file_name().unwrap_or_default().to_string_lossy()],
            ),
            tr_format(
                &tr("Folder: %1"),
                &[&book
                    .path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(""))
                    .to_string_lossy()],
            ),
            tr_format(&tr("Size: %1"), &[&data::size_text(book.size)]),
            tr_format(&tr("Format: %1"), &[&book.format.to_uppercase()]),
        ]
        .join("\n")
    } else {
        book.description
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| tr("No synopsis available."))
    };
    let content = WidgetPod::new(
        super::widgets::Scroll::new(vec![text], 1, 1, |text: &String, t: &Theme| {
            WidgetPod::new(Paragraph::new(text.clone(), t.font.body.clone(), Tone::Ink))
        })
        .measure_single_row(),
    )
    .with_key("details-text");
    let mut footer = vec![
        rating_button(rating, id),
        button(ButtonStyle::Text, &tr("Tags"), &format!("library:tags:open:{id}")),
    ];
    if statistics {
        footer.push(button(
            ButtonStyle::Text,
            &tr("Statistics"),
            &format!("library:stats:open:{id}"),
        ));
    }
    if let Some(action) = remove {
        footer.push(button(ButtonStyle::Text, &tr("Remove from collection"), &action));
    }
    let body = Column::new(vec![
        WidgetPod::new(SizedBox::new(Some(header)).height(t.scale.dp(36))),
        upper,
        tabs,
        WidgetPod::new(kaia_ui::components::Divider::new(Tone::Divider)),
        WidgetPod::new(Expanded::new(content, 1)),
        WidgetPod::new(Row::new(footer).gap(t.space.sm)),
    ])
    .gap(t.space.sm)
    .cross_align(CrossAlign::Stretch);
    WidgetPod::new(
        Frame::new(WidgetPod::new(kaia_ui::components::Padding::new(
            WidgetPod::new(body),
            Insets::symmetric(t.scale.dp(28), t.scale.dp(12)),
        )))
        .fill(Tone::Surface)
        .border(Tone::Ink, 2),
    )
    .with_key("book-details-dialog")
}

/// The rating button of the details: the current value. It opens the rating sheet.
/// A book with no state file shows the same button, and the sheet explains why.
fn rating_button(rating: Rating, book: i64) -> WidgetPod {
    let label = match rating {
        Rating::Stars(Some(n)) => tr_format(&tr("Rating: %1 of 5"), &[&n.to_string()]),
        Rating::Stars(None) => tr("Not rated"),
        Rating::Unopened | Rating::Unreadable => tr("Rating"),
    };
    button(
        ButtonStyle::Secondary,
        &label,
        &format!("library:rating:open:{book}"),
    )
}

/// The rating sheet: the current value, one button for each star count and
/// Clear, then Back to the details. A book with no state file gets no stars,
/// because its first open must run the import before a rating is kept.
pub fn rating_panel(t: &Theme, book: i64, rating: Rating) -> WidgetPod {
    let mut children = Vec::new();
    match rating {
        Rating::Unopened => children.push(body_text(t, &tr("Open the book once to rate it."))),
        Rating::Unreadable => children.push(body_text(t, &tr("Cannot read this book's rating."))),
        Rating::Stars(stars) => {
            let value = match stars {
                Some(n) => tr_format(&tr("Rating: %1 of 5"), &[&n.to_string()]),
                None => tr("Not rated"),
            };
            children.push(body_text(t, &value));
            let mut buttons: Vec<WidgetPod> = (1..=5u8)
                .map(|n| {
                    let style = if stars == Some(n) {
                        ButtonStyle::Primary
                    } else {
                        ButtonStyle::Secondary
                    };
                    let action = format!("library:rating:{book}:{n}");
                    WidgetPod::new(Expanded::new(button(style, &n.to_string(), &action), 1))
                })
                .collect();
            buttons.push(WidgetPod::new(Expanded::new(
                button(
                    ButtonStyle::Secondary,
                    &tr("Clear"),
                    &format!("library:rating:{book}:0"),
                ),
                2,
            )));
            children.push(WidgetPod::new(Row::new(buttons).gap(t.space.sm)));
        }
    }
    children.push(button(
        ButtonStyle::Secondary,
        &tr("Back"),
        &format!("library:rating:back:{book}"),
    ));
    sheet(t, &tr("Rating"), children)
}

/// The bar of select mode: the count of checked books, select all and none, the
/// two marks, and Done. The buttons that need a checked book are off without one.
/// A batch in progress does not change them: the screen takes the tap and says so.
pub fn select_bar(t: &Theme, label: String, checked: usize) -> WidgetPod {
    let mark = |style: ButtonStyle, text: &str, action: &str, enabled: bool| {
        WidgetPod::new(Button::new(style).text(text).enabled(enabled).on_action(action))
            .with_key(action.replace(':', "-"))
    };
    let any = checked > 0;
    WidgetPod::new(
        Column::new(vec![
            WidgetPod::new(Label::new(label, t.font.caption.clone(), Tone::InkSecondary))
                .with_key("select-count"),
            pair(
                t,
                mark(
                    ButtonStyle::Secondary,
                    &tr("Select all"),
                    "library:select:all",
                    true,
                ),
                mark(
                    ButtonStyle::Secondary,
                    &tr("Deselect all"),
                    "library:select:none",
                    any,
                ),
            ),
            pair(
                t,
                mark(
                    ButtonStyle::Primary,
                    &tr("Mark as finished"),
                    "library:select:finished",
                    any,
                ),
                mark(
                    ButtonStyle::Secondary,
                    &tr("Mark as unread"),
                    "library:select:unread",
                    any,
                ),
            ),
            mark(ButtonStyle::Secondary, &tr("Done"), "library:select:off", true),
        ])
        .gap(t.space.sm)
        .cross_align(CrossAlign::Stretch)
        .shrink(),
    )
}

/// Two buttons side by side, each taking half the row.
fn pair(t: &Theme, left: WidgetPod, right: WidgetPod) -> WidgetPod {
    WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(left, 1)),
            WidgetPod::new(Expanded::new(right, 1)),
        ])
        .gap(t.space.sm),
    )
}
