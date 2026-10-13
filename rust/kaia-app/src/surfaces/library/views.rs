//! Widget trees for the Library: the toolbar, the four views, the search page,
//! the panels (sort, collection picker, name, delete confirm) and the book
//! details. Each builder takes the theme and the data it shows, and returns
//! widgets. The surface decides which one is on screen.

use kaia_events::Value;
use kaia_gfx::{Pixmap, Size};
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
use crate::surfaces::library::data::{self, Prefs};
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
            detail: parts.join("  "),
            cover: info.cover.clone(),
            tap: format!("library:open:{}", info.id),
            hold: Some(format!("library:details:{}", info.id)),
            enabled: true,
        }
    }
}

fn line_row(line: &Line) -> WidgetPod {
    let mut row = ListRow::new(line.title.clone())
        .enabled(line.enabled)
        .on_action(line.tap.clone());
    if !line.detail.is_empty() {
        row = row.secondary(line.detail.clone());
    }
    if let Some(px) = &line.cover {
        row = row.cover(CoverImage::new(line.title.clone(), Some(px.clone())));
    }
    row = row.chevron();
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

/// The page header: the title, the status line and the search action.
pub fn header(theme: &Theme, title: &str, status: Option<String>) -> WidgetPod {
    let mut h = Header::new(theme, title).action("appbar.search", "library:search:open");
    if let Some(text) = status {
        h = h.status(text);
    }
    crate::surfaces::common::status_header(h).with_key("library-header")
}

/// The four view tabs and the sort button. The active tab is the primary button.
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
            tab(7, Some(View::Covers), "Covers", "library:view:covers"),
            tab(5, Some(View::List), "List", "library:view:list"),
            tab(8, Some(View::Folders), "Folders", "library:view:folders"),
            tab(
                12,
                Some(View::Collections),
                "Collections",
                "library:view:collections",
            ),
            tab(6, None, "Sort", "library:sort"),
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
                "Reading",
                "library:filter:reading",
            ),
            chip(
                12,
                prefs.chip == Some(Chip::NotStarted),
                "Not started",
                "library:filter:not-started",
            ),
            chip(
                9,
                prefs.chip == Some(Chip::Finished),
                "Finished",
                "library:filter:finished",
            ),
            chip(
                11,
                prefs.collection.is_some(),
                "Collection",
                "library:filter:collection",
            ),
        ])
        .gap(t.space.xs),
    )
}

/// The covers view: a grid of tiles, three columns in portrait and four in landscape.
/// A tile is 100 dp wide and centred in its cell, so two rows of covers fit on a
/// portrait page next to the header, the chips and the page strip.
pub fn covers(t: &Theme, screen: Size, infos: Vec<BookInfo>) -> WidgetPod {
    let portrait = screen.h >= screen.w;
    let columns = if portrait { 3 } else { 4 };
    let grid = Grid::new(infos, columns, |info: &BookInfo, t: &Theme| {
        let tile_w = t.scale.dp(100);
        let tile = WidgetPod::new(BookTile::new(t, info).on_action("library:open"));
        let sized = WidgetPod::new(SizedBox::new(Some(tile)).width(tile_w));
        let centred = WidgetPod::new(Row::new(vec![
            WidgetPod::new(Spacer::new(1)),
            sized,
            WidgetPod::new(Spacer::new(1)),
        ]));
        let hold = Pressable::new(centred).hold("library:details", vec![Value::Str(info.id.clone())]);
        WidgetPod::new(hold).with_key(format!("book-{}", info.id))
    })
    .gap(t.space.sm);
    WidgetPod::new(grid)
}

/// The list view and the search results: rows of cover, title and detail, paged.
pub fn list(lines: Vec<Line>) -> PagedList<Line> {
    PagedList::new(lines, |line: &Line, _t: &Theme| line_row(line))
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
                        .secondary(format!("{} books", c.books))
                        .on_action(format!("library:collection:{id}")),
                )
                .with_key(format!("collection-{id}")),
                WidgetPod::new(
                    Row::new(vec![
                        button(
                            ButtonStyle::Secondary,
                            "Rename",
                            &format!("library:collections:rename:{id}"),
                        ),
                        button(
                            ButtonStyle::Secondary,
                            "Delete",
                            &format!("library:collections:delete:{id}"),
                        ),
                        button(
                            ButtonStyle::Secondary,
                            "Move up",
                            &format!("library:collections:move:{id}:{}", position.saturating_sub(1)),
                        ),
                        button(
                            ButtonStyle::Secondary,
                            "Move down",
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
                button(ButtonStyle::Primary, "New collection", "library:collections:new"),
                WidgetPod::new(Expanded::new(WidgetPod::new(list).with_key("collection-list"), 1)),
                button(
                    ButtonStyle::Secondary,
                    "Clear reading history",
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
pub fn search(t: &Theme, query: &str, results: Vec<Line>) -> Vec<WidgetPod> {
    let field = WidgetPod::new(
        TextField::new()
            .placeholder("Title, author, series")
            .with_text(query)
            .on_change("library:search"),
    )
    .focusable(true)
    .with_key("search-field");
    let top = WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(field, 1)),
            button(ButtonStyle::Secondary, "Close", "library:search:close"),
        ])
        .gap(t.space.sm)
        .cross_align(CrossAlign::Center),
    );
    // A fixed hint: the results change without a rebuild, so the hint cannot depend on them.
    let hint = body_text(t, "Type a title, an author or a word from the description.");
    let list = WidgetPod::new(Expanded::new(
        WidgetPod::new(list(results)).with_key("results"),
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
            let mut row = ListRow::new(label)
                .enabled(enabled)
                .on_action(format!("library:sort:{key}"));
            if !enabled {
                row = row.secondary("Needs series data from the book files");
            } else if choice == current {
                row = row.value("Current");
            }
            WidgetPod::new(row).with_key(format!("sort-{key}"))
        })
        .collect();
    sheet(
        t,
        "Sort by",
        vec![
            WidgetPod::new(Column::new(rows).cross_align(CrossAlign::Stretch).shrink()),
            button(ButtonStyle::Secondary, "Cancel", "library:panel:close"),
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
            .secondary(format!("{} books", c.books))
            .on_action(action.clone());
        if book.is_none() && filter == Some(c.id) {
            row = row.value("Showing");
        }
        rows.push(WidgetPod::new(row).with_key(format!("collection-{}", c.id)));
    }
    if rows.is_empty() {
        rows.push(body_text(t, "No collections yet."));
    }
    let mut footer = Vec::new();
    match book {
        Some(b) => footer.push(button(
            ButtonStyle::Primary,
            "New collection",
            &format!("library:add:{b}:new"),
        )),
        None if filter.is_some() => footer.push(button(
            ButtonStyle::Secondary,
            "Show all books",
            "library:collection:none",
        )),
        None => {}
    }
    footer.push(button(ButtonStyle::Secondary, "Cancel", "library:panel:close"));
    let title = if book.is_some() {
        "Add to collection"
    } else {
        "Filter by collection"
    };
    let mut children = vec![WidgetPod::new(
        Column::new(rows).cross_align(CrossAlign::Stretch).shrink(),
    )];
    children.push(WidgetPod::new(Row::new(footer).gap(t.space.md)));
    sheet(t, title, children)
}

/// The name panel for a new collection: a field, the keyboard and two buttons.
pub fn name_panel(t: &Theme, name: &str) -> WidgetPod {
    let field = WidgetPod::new(
        TextField::new()
            .placeholder("Collection name")
            .with_text(name)
            .on_change("library:name")
            .on_submit("library:name:save"),
    )
    .focusable(true)
    .with_key("name-field");
    let keyboard = WidgetPod::new(OnScreenKeyboard::new(&ENGLISH).background(Tone::Surface));
    sheet(
        t,
        "New collection",
        vec![
            field,
            keyboard,
            WidgetPod::new(
                Row::new(vec![
                    button(ButtonStyle::Secondary, "Cancel", "library:panel:close"),
                    button(ButtonStyle::Primary, "Save", "library:name:save"),
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
        "Delete this file?",
        vec![
            body_text(
                t,
                &format!("\"{title}\" is removed from the device. The book leaves the library. This cannot be undone."),
            ),
            WidgetPod::new(
                Row::new(vec![
                    button(ButtonStyle::Secondary, "Cancel", "library:panel:close"),
                    button(ButtonStyle::Primary, "Delete", &format!("library:book:delete-confirm:{}", book.id)),
                ])
                .gap(t.space.md),
            ),
        ],
    )
}

/// The details of one book: cover and facts, the description, and the actions.
pub fn details_panel(t: &Theme, book: &Book, cover: Option<Pixmap>, close: &str) -> WidgetPod {
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
    facts.push(body_text(t, data::status_label(book)));
    let mut info = format!(
        "{} file, {}",
        book.format.to_uppercase(),
        data::size_text(book.size)
    );
    if let Some(pages) = book.pages {
        info.push_str(&format!(", {pages} pages"));
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
        "Open",
        &format!("library:open:{id}"),
    ));
    children.push(pair(
        button(
            ButtonStyle::Secondary,
            "Add to collection",
            &format!("library:book:collection:{id}"),
        ),
        button(
            ButtonStyle::Secondary,
            "Mark as finished",
            &format!("library:book:finished:{id}"),
        ),
    ));
    children.push(button(
        ButtonStyle::Secondary,
        "Remove from history",
        &format!("library:book:history:{id}"),
    ));
    children.push(pair(
        button(
            ButtonStyle::Secondary,
            "Delete file",
            &format!("library:book:delete:{id}"),
        ),
        button(ButtonStyle::Secondary, "Close", close),
    ));
    sheet(t, "Book details", children)
}

/// Two buttons side by side, each taking half the row.
fn pair(left: WidgetPod, right: WidgetPod) -> WidgetPod {
    WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(left, 1)),
            WidgetPod::new(Expanded::new(right, 1)),
        ])
        .gap(8),
    )
}
