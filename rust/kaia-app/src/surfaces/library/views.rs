//! Widget trees for the Library: the toolbar, the four views, the search page,
//! the panels (sort, collection picker, name, delete confirm) and the book
//! details. Each builder takes the theme and the data it shows, and returns
//! widgets. The surface decides which one is on screen.

use kaia_events::Value;
use kaia_gfx::{Pixmap, Size};
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_library::{Book, Collection};
use kaia_ui::components::{
    Column, CrossAlign, Expanded, Frame, Label, Paragraph, ProgressBar, Row, SizedBox, Spacer,
};
use kaia_ui::composite::Header;
use kaia_ui::composite::{BookInfo, BookTile, CoverImage, Grid, ListRow, PagedList};
use kaia_ui::controls::{Button, ButtonStyle, OnScreenKeyboard, TextField, ENGLISH};
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, Tone, WidgetPod};

use crate::surfaces::common::short;
use crate::surfaces::library::commands::{Chip, SortChoice, View};
use crate::surfaces::library::data::{self, Prefs, SeriesGroup};
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
pub fn header(theme: &Theme, title: &str, status: Option<String>, selecting: bool) -> WidgetPod {
    let mut h = Header::new(theme, title);
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

/// The view tabs and the sort button. The active tab is the primary button.
/// A button keeps 16 dp of padding on each side, so each tab takes a share of the
/// row that fits its label. A label that is too wide ends with an ellipsis.
pub fn view_tabs(t: &Theme, active: View) -> WidgetPod {
    let tab = |weight: u32, view: Option<View>, label: &str, action: &str| {
        let style = if view == Some(active) {
            ButtonStyle::Primary
        } else {
            ButtonStyle::Secondary
        };
        WidgetPod::new(Expanded::new(button(style, label, action), weight))
    };
    WidgetPod::new(
        Row::new(vec![
            tab(7, Some(View::Covers), &tr("Covers"), "library:view:covers"),
            tab(5, Some(View::List), &tr("List"), "library:view:list"),
            tab(8, Some(View::Folders), &tr("Folders"), "library:view:folders"),
            tab(
                12,
                Some(View::Collections),
                &tr("Collections"),
                "library:view:collections",
            ),
            tab(7, Some(View::Series), &tr("Series"), "library:view:series"),
            tab(6, None, &tr("Sort"), "library:sort"),
        ])
        .gap(t.space.xs),
    )
}

/// The filter chips. The active chip is the primary button. The weights fit the
/// labels, as for the view tabs.
pub fn filter_row(t: &Theme, prefs: &Prefs) -> WidgetPod {
    let chip = |weight: u32, active: bool, label: &str, action: &str| {
        let style = if active {
            ButtonStyle::Primary
        } else {
            ButtonStyle::Secondary
        };
        WidgetPod::new(Expanded::new(button(style, label, action), weight))
    };
    WidgetPod::new(
        Row::new(vec![
            chip(
                9,
                prefs.chip == Some(Chip::Reading),
                &tr("Reading"),
                "library:filter:reading",
            ),
            chip(
                12,
                prefs.chip == Some(Chip::NotStarted),
                &tr("Not started"),
                "library:filter:not-started",
            ),
            chip(
                9,
                prefs.chip == Some(Chip::Finished),
                &tr("Finished"),
                "library:filter:finished",
            ),
            chip(
                11,
                prefs.collection.is_some(),
                &tr("Collection"),
                "library:filter:collection",
            ),
        ])
        .gap(t.space.xs),
    )
}

/// The covers view: a grid of tiles, three columns in portrait and four in landscape.
/// A tile is 100 dp wide and centred in its cell, so two rows of covers fit on a
/// portrait page next to the header, the chips and the page strip.
pub fn covers(t: &Theme, screen: Size, infos: Vec<BookInfo>, loader: crate::covers::Shared) -> WidgetPod {
    let portrait = screen.h >= screen.w;
    let columns = if portrait { 3 } else { 4 };
    let grid = Grid::new(infos, columns, move |info: &BookInfo, t: &Theme| {
        let tile_w = t.scale.dp(100);
        let info = info.clone();
        let theme = t.clone();
        let id = info.id.parse().unwrap_or_default();
        let held_id = info.id.clone();
        let tile = WidgetPod::new(crate::covers::Cell::new(id, loader.clone(), move |cover| {
            let mut info = info.clone();
            info.cover = cover;
            WidgetPod::new(BookTile::new(&theme, &info).on_action("library:open"))
        }));
        let sized = WidgetPod::new(SizedBox::new(Some(tile)).width(tile_w));
        let centred = WidgetPod::new(Row::new(vec![
            WidgetPod::new(Spacer::new(1)),
            sized,
            WidgetPod::new(Spacer::new(1)),
        ]));
        let hold = Pressable::new(centred).hold("library:details", vec![Value::Str(held_id.clone())]);
        WidgetPod::new(hold).with_key(format!("book-{held_id}"))
    })
    .gap(t.space.sm);
    WidgetPod::new(grid)
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
    let list = PagedList::new(items, |(n, group): &(usize, SeriesGroup), _t: &Theme| {
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
pub fn series_books(t: &Theme, name: &str, books: Vec<Line>, loader: crate::covers::Shared) -> WidgetPod {
    WidgetPod::new(
        Column::new(vec![
            WidgetPod::new(Label::new(
                name.to_string(),
                t.font.caption.clone(),
                Tone::InkSecondary,
            )),
            WidgetPod::new(ListRow::new(tr("Up")).on_action("library:series:up")).with_key("series-up"),
            WidgetPod::new(Expanded::new(
                WidgetPod::new(list_covers(books, loader)).with_key("series-books"),
                1,
            )),
        ])
        .gap(t.space.sm)
        .cross_align(CrossAlign::Stretch),
    )
}

/// The collections tab: one row per collection, which filters the covers by it.
pub fn collections(t: &Theme, collections: &[Collection]) -> WidgetPod {
    let items: Vec<_> = collections
        .iter()
        .enumerate()
        .map(|(position, c)| (c.clone(), position))
        .collect();
    let list = PagedList::new(items, |(c, position): &(Collection, usize), t: &Theme| {
        let id = c.id;
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
                WidgetPod::new(
                    Row::new(vec![
                        button(
                            ButtonStyle::Secondary,
                            &tr("Rename"),
                            &format!("library:collections:rename:{id}"),
                        ),
                        button(
                            ButtonStyle::Secondary,
                            &tr("Delete"),
                            &format!("library:collections:delete:{id}"),
                        ),
                        button(
                            ButtonStyle::Secondary,
                            &tr("Move up"),
                            &format!("library:collections:move:{id}:{}", position.saturating_sub(1)),
                        ),
                        button(
                            ButtonStyle::Secondary,
                            &tr("Move down"),
                            &format!("library:collections:move:{id}:{}", position + 1),
                        ),
                    ])
                    .gap(t.space.sm),
                ),
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
pub fn sheet(t: &Theme, title: &str, mut children: Vec<WidgetPod>) -> WidgetPod {
    let mut all = vec![WidgetPod::new(Label::new(title, t.font.title.clone(), Tone::Ink))];
    all.append(&mut children);
    let column = Column::new(all)
        .gap(t.space.md)
        .cross_align(CrossAlign::Stretch)
        .shrink();
    let padded = WidgetPod::new(kaia_ui::components::Padding::new(
        WidgetPod::new(column),
        Insets::all(t.space.md),
    ));
    WidgetPod::new(
        Frame::new(padded)
            .fill(Tone::Surface)
            .border(Tone::Ink, t.line.hairline)
            .radius(t.radius.card),
    )
}

/// The label of a row in the sort table of `sort_panel`.
fn sort_label(label: &str) -> String {
    match label {
        "Title" => tr("Title"),
        "Author" => tr("Author"),
        "Recently read" => tr("Recently read"),
        "Recently added" => tr("Recently added"),
        "Progress" => tr("Progress"),
        "Series" => tr("Series"),
        other => other.to_string(),
    }
}

/// The sort panel: one row per choice. The current one shows a value.
pub fn sort_panel(t: &Theme, current: SortChoice) -> WidgetPod {
    let options = [
        (SortChoice::Title, "Title", "title", true),
        (SortChoice::Author, "Author", "author", true),
        (SortChoice::Read, "Recently read", "read", true),
        (SortChoice::Added, "Recently added", "added", true),
        (SortChoice::Progress, "Progress", "progress", true),
        (SortChoice::Series, "Series", "series", true),
    ];
    let rows: Vec<WidgetPod> = options
        .into_iter()
        .map(|(choice, label, key, enabled)| {
            let mut row = ListRow::new(sort_label(label))
                .enabled(enabled)
                .on_action(format!("library:sort:{key}"));
            if !enabled {
                row = row.secondary(tr("Needs series data from the book files"));
            } else if choice == current {
                row = row.value(tr("Current"));
            }
            WidgetPod::new(row).with_key(format!("sort-{key}"))
        })
        .collect();
    sheet(
        t,
        &tr("Sort by"),
        vec![
            WidgetPod::new(Column::new(rows).cross_align(CrossAlign::Stretch).shrink()),
            button(ButtonStyle::Secondary, &tr("Cancel"), "library:panel:close"),
        ],
    )
}

/// The collection picker. For a book it adds the book to a collection or starts a
/// new one; without a book it filters the covers.
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

/// The details of one book: cover and facts, the description, and the actions.
/// `remove` is the action that takes the book out of the collection being
/// viewed. None when the book is not in that collection.
pub fn details_panel(
    t: &Theme,
    book: &Book,
    cover: Option<Pixmap>,
    close: &str,
    remove: Option<String>,
) -> WidgetPod {
    let title = crate::surfaces::common::display_title(book);
    let mut facts = vec![WidgetPod::new(Label::new(
        title.clone(),
        t.font.book_title.clone(),
        Tone::Ink,
    ))];
    if let Some(author) = book.authors.as_deref().filter(|a| !a.trim().is_empty()) {
        facts.push(WidgetPod::new(Label::new(
            author,
            t.font.body.clone(),
            Tone::InkSecondary,
        )));
    }
    facts.push(WidgetPod::new(ProgressBar::new(book.percent as f32)));
    facts.push(body_text(t, &data::status_label(book)));
    let mut info = tr_format(
        &tr("%1 file, %2"),
        &[&book.format.to_uppercase(), &data::size_text(book.size)],
    );
    if let Some(pages) = book.pages {
        info.push_str(&tr_format(
            &tr_n(", %1 page", ", %1 pages", pages as i64),
            &[&pages.to_string()],
        ));
    }
    facts.push(WidgetPod::new(Label::new(
        info,
        t.font.caption.clone(),
        Tone::InkSecondary,
    )));
    if let Some(name) = book.path.file_name() {
        facts.push(WidgetPod::new(Label::new(
            name.to_string_lossy().into_owned(),
            t.font.caption.clone(),
            Tone::InkSecondary,
        )));
    }
    let cover_box = WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(CoverImage::new(title.clone(), cover))))
            .width(t.scale.dp(110))
            .height(t.scale.dp(165)),
    );
    let mut children = vec![WidgetPod::new(
        Row::new(vec![
            cover_box,
            WidgetPod::new(Expanded::new(
                WidgetPod::new(Column::new(facts).gap(t.space.xs).shrink()),
                1,
            )),
        ])
        .gap(t.space.md),
    )];
    if let Some(desc) = book.description.as_deref().filter(|d| !d.trim().is_empty()) {
        children.push(WidgetPod::new(
            Paragraph::new(desc.to_string(), t.font.body.clone(), Tone::InkSecondary).max_lines(4),
        ));
    }
    let id = book.id;
    children.push(button(
        ButtonStyle::Primary,
        &tr("Open"),
        &format!("library:open:{id}"),
    ));
    children.push(pair(
        t,
        button(
            ButtonStyle::Secondary,
            &tr("Add to collection"),
            &format!("library:book:collection:{id}"),
        ),
        button(
            ButtonStyle::Secondary,
            &tr("Mark as finished"),
            &format!("library:book:finished:{id}"),
        ),
    ));
    // The rows keep to four, so the sheet fits the page. Inside a collection the
    // removal shares the history row.
    let history = button(
        ButtonStyle::Secondary,
        &tr("Remove from history"),
        &format!("library:book:history:{id}"),
    );
    children.push(match remove {
        Some(action) => pair(
            t,
            button(ButtonStyle::Secondary, &tr("Remove from collection"), &action),
            history,
        ),
        None => history,
    });
    children.push(pair(
        t,
        button(
            ButtonStyle::Secondary,
            &tr("Delete file"),
            &format!("library:book:delete:{id}"),
        ),
        button(ButtonStyle::Secondary, &tr("Close"), close),
    ));
    sheet(t, &tr("Book details"), children)
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
