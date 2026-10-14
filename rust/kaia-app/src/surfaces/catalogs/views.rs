use kaia_calibre::metadata::{Book, Field};
use kaia_gfx::{image::FitMode, Size};
use kaia_i18n::{format as tr_format, tr, tr_n};
use kaia_ui::components::{Column, CrossAlign, Expanded, Image, Label, Paragraph, Row, SizedBox};
use kaia_ui::composite::{Header, ListRow, PagedList, SheetPanel};
use kaia_ui::controls::{ButtonStyle, OnScreenKeyboard, TextField, ENGLISH};
use kaia_ui::{Theme, Tone, WidgetPod, Window};

use super::cache::{field_key, field_title, Cache};
use super::{store, CatalogSurface, View, CALIBRE_QUERY_FIELD};
use crate::services::Services;
use crate::surfaces::common::{body, button, page, root_window};

#[derive(Clone)]
struct Item {
    title: String,
    detail: String,
    action: String,
    edit: Option<String>,
}
fn item(title: impl Into<String>, detail: impl Into<String>, action: impl Into<String>) -> Item {
    Item {
        title: title.into(),
        detail: detail.into(),
        action: action.into(),
        edit: None,
    }
}
fn row(item: &Item, t: &Theme) -> WidgetPod {
    let main = WidgetPod::new(
        ListRow::new(&item.title)
            .secondary(&item.detail)
            .chevron()
            .on_action(&item.action),
    )
    .with_key(&item.action);
    if let Some(edit) = &item.edit {
        WidgetPod::new(
            Row::new(vec![
                WidgetPod::new(Expanded::new(main, 1)),
                button(ButtonStyle::Secondary, &tr("Edit"), edit, edit),
            ])
            .gap(t.space.sm)
            .cross_align(CrossAlign::Center),
        )
    } else {
        main
    }
}
/// The second line of a Calibre search row: the authors, then the series and its number.
fn calibre_detail(book: &Book) -> String {
    let mut parts = Vec::new();
    if !book.authors.is_empty() {
        parts.push(book.authors.join(", "));
    }
    if let Some(series) = &book.series {
        parts.push(match book.series_index {
            Some(n) if n.fract() == 0.0 => format!("{series} {n:.0}"),
            Some(n) => format!("{series} {n}"),
            None => series.clone(),
        });
    }
    parts.join(", ")
}
fn list(items: Vec<Item>) -> WidgetPod {
    WidgetPod::new(Expanded::new(
        WidgetPod::new(PagedList::new(items, row)).with_key("catalogs-list"),
        1,
    ))
}
fn action(label: &str, id: &str) -> WidgetPod {
    button(ButtonStyle::Secondary, label, id, id)
}
fn actions(t: &Theme, buttons: Vec<WidgetPod>) -> WidgetPod {
    WidgetPod::new(Row::new(buttons).gap(t.space.sm))
}
fn text_field(t: &Theme, label: &str, value: &str, name: &str, password: bool) -> WidgetPod {
    WidgetPod::new(
        Column::new(vec![
            WidgetPod::new(Label::new(label, t.font.caption.clone(), Tone::InkSecondary)),
            WidgetPod::new(
                TextField::new()
                    .with_text(value)
                    .password(password)
                    .on_change(format!("catalogs:change:{name}")),
            )
            .focusable(true)
            .with_key(format!("catalogs-field-{name}")),
        ])
        .gap(t.space.xs)
        .cross_align(CrossAlign::Stretch)
        .shrink(),
    )
}

pub(super) fn build(state: &CatalogSurface, services: &Services, screen: Size) -> Window {
    let t = &services.theme;
    let title = match &state.view {
        View::Catalogs => tr("OPDS catalogs"),
        View::Suggestions => tr("Suggested catalogs"),
        View::Editor { index, .. } => {
            if index.is_some() {
                tr("Edit catalog")
            } else {
                tr("Add catalog")
            }
        }
        View::Feed { feed, .. } => feed.title.clone(),
        View::Book { .. } => tr("Book details"),
        View::Calibre => tr("Calibre wireless"),
        View::CalibreBooks { title, .. } => title.clone(),
        View::CalibreGroups { field, .. } => field_title(*field),
        View::Downloads => tr("Downloads folder"),
    };
    let mut children = Vec::new();
    if !matches!(state.view, View::Book { .. }) {
        children.push(WidgetPod::new(Header::new(t, title.clone()).back("back")));
    }
    let active = state.worker.is_some() || state.pending.is_some();
    match &state.view {
        View::Catalogs => {
            children.push(actions(
                t,
                vec![
                    action(&tr("Add catalog"), "catalogs:add"),
                    action(&tr("Suggestions"), "catalogs:suggestions"),
                    action(&tr("Downloads"), "catalogs:downloads"),
                ],
            ));
            let catalogs = store::catalogs(&services.settings);
            if catalogs.is_empty() {
                children.push(body(services, tr("Add a catalog to browse and download books.")));
            }
            children.push(list(
                catalogs
                    .iter()
                    .enumerate()
                    .map(|(i, c)| Item {
                        title: c.title.clone(),
                        detail: c.url.clone(),
                        action: format!("catalogs:browse:{i}"),
                        edit: Some(format!("catalogs:edit:{i}")),
                    })
                    .collect(),
            ));
        }
        View::Suggestions => {
            children.push(body(
                services,
                tr("Choose a catalog to add. These are KOReader's default suggestions."),
            ));
            children.push(list(
                kaia_opds::default_catalogs()
                    .into_iter()
                    .enumerate()
                    .map(|(i, c)| item(c.title, c.url, format!("catalogs:suggest:{i}")))
                    .collect(),
            ));
        }
        View::Editor { index, catalog } => {
            if state.typing {
                let (label, value, name, password) = match state.field {
                    1 => (tr("URL"), catalog.url.as_str(), "url", false),
                    2 => (
                        tr("Username, optional"),
                        catalog.username.as_deref().unwrap_or(""),
                        "username",
                        false,
                    ),
                    3 => (
                        tr("Password, optional"),
                        catalog.password.as_deref().unwrap_or(""),
                        "password",
                        true,
                    ),
                    _ => (tr("Name"), catalog.title.as_str(), "name", false),
                };
                children.push(text_field(t, &label, value, name, password));
                if password {
                    children.push(body(services, tr("Stored on this device without encryption")));
                }
                children.push(action(&tr("Done"), "catalogs:hide-keyboard"));
                children.push(WidgetPod::new(kaia_ui::components::Spacer::new(1)));
            } else {
                children.push(list(vec![
                    item(tr("Name"), &catalog.title, "catalogs:field:0"),
                    item(tr("URL"), &catalog.url, "catalogs:field:1"),
                    item(
                        tr("Username, optional"),
                        catalog.username.as_deref().unwrap_or(""),
                        "catalogs:field:2",
                    ),
                    item(
                        tr("Password, optional"),
                        tr("Stored on this device without encryption"),
                        "catalogs:field:3",
                    ),
                ]));
                let mut buttons = vec![action(&tr("Save"), "catalogs:save")];
                if index.is_some() {
                    buttons.push(action(&tr("Delete"), "catalogs:delete"));
                }
                children.push(actions(t, buttons));
            }
        }
        View::Feed { feed, .. } => {
            if state.typing {
                children.push(text_field(t, &tr("Search catalog"), &state.query, "query", false));
                children.push(action(&tr("Search"), "catalogs:search-go"));
            } else if !active && (feed.search_template.is_some() || feed.search_description.is_some()) {
                children.push(action(&tr("Search"), "catalogs:search"));
            }
            children.push(list(
                feed.entries
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        item(
                            &e.title,
                            if e.authors.is_empty() {
                                if e.navigation.is_some() {
                                    tr("Browse catalog")
                                } else {
                                    tr("Book details")
                                }
                            } else {
                                e.authors.join(", ")
                            },
                            format!("catalogs:entry:{i}"),
                        )
                    })
                    .collect(),
            ));
            if !active {
                let mut buttons = Vec::new();
                if feed.prev.is_some() {
                    buttons.push(action(&tr("Previous feed page"), "catalogs:previous"));
                }
                if feed.next.is_some() {
                    buttons.push(action(&tr("Next feed page"), "catalogs:next"));
                }
                if !buttons.is_empty() {
                    children.push(actions(t, buttons));
                }
            }
        }
        View::Book { entry, cover, .. } => {
            let text = WidgetPod::new(
                Column::new(vec![
                    WidgetPod::new(
                        Paragraph::new(&entry.title, t.font.book_title.clone(), Tone::Ink).max_lines(3),
                    ),
                    WidgetPod::new(
                        Paragraph::new(
                            entry.authors.join(", "),
                            t.font.caption.clone(),
                            Tone::InkSecondary,
                        )
                        .max_lines(2),
                    ),
                ])
                .gap(t.space.sm)
                .shrink(),
            );
            let mut heading = Vec::new();
            if let Some(cover) = cover {
                heading.push(WidgetPod::new(
                    SizedBox::new(Some(WidgetPod::new(Image::new(cover.clone(), FitMode::Contain))))
                        .width(t.row_height)
                        .height(t.row_height * 3 / 2),
                ));
            }
            heading.push(WidgetPod::new(Expanded::new(text, 1)));
            children.push(WidgetPod::new(Row::new(heading).gap(t.space.md)));
            children.push(WidgetPod::new(
                Paragraph::new(
                    entry
                        .summary
                        .clone()
                        .unwrap_or_else(|| tr("No summary provided.")),
                    t.font.body.clone(),
                    Tone::InkSecondary,
                )
                .max_lines(5),
            ));
            children.push(list(
                entry
                    .downloads()
                    .enumerate()
                    .map(|(i, a)| {
                        let kind = a
                            .file_type()
                            .map_or_else(|| tr("book").to_uppercase(), str::to_uppercase);
                        item(
                            tr_format(&tr("Download %1"), &[&kind]),
                            a.title.clone().unwrap_or_else(|| {
                                a.size
                                    .map(|n| tr_format(&tr("%1 KB"), &[&(n / 1024).to_string()]))
                                    .unwrap_or_default()
                            }),
                            format!("catalogs:download:{i}"),
                        )
                    })
                    .collect(),
            ));
            if entry.downloads().next().is_none() {
                children.push(body(services, tr("No supported download format.")));
            }
        }
        View::Calibre => {
            if state.typing {
                if state.field == CALIBRE_QUERY_FIELD {
                    children.push(text_field(
                        t,
                        &tr("Search Calibre books"),
                        &state.calibre_query,
                        "calibre-query",
                        false,
                    ));
                    children.push(action(&tr("Search"), "catalogs:calibre-search-go"));
                } else if state.field == 1 {
                    children.push(text_field(
                        t,
                        &tr("Server password, optional"),
                        &state.password,
                        "calibre-password",
                        true,
                    ));
                } else {
                    children.push(text_field(
                        t,
                        &tr("Server address and port"),
                        &state.address,
                        "address",
                        false,
                    ));
                }
                children.push(action(&tr("Done"), "catalogs:hide-keyboard"));
            } else if !active {
                match &state.cache {
                    Cache::Loaded { .. } => {
                        children.push(actions(
                            t,
                            vec![action(&tr("Search Calibre books"), "catalogs:calibre-search")],
                        ));
                        for field in [Field::Author, Field::Series, Field::Tag] {
                            children.push(row(
                                &item(
                                    field_title(field),
                                    match field {
                                        Field::Author => tr("Find books by author"),
                                        Field::Series => tr("Find books in a series"),
                                        Field::Tag => tr("Find books with a tag"),
                                    },
                                    format!("catalogs:calibre-browse:{}", field_key(field)),
                                ),
                                t,
                            ));
                        }
                    }
                    Cache::Failed(message) => children.push(body(services, message.clone())),
                    Cache::Missing => {}
                }
                children.push(body(
                    services,
                    tr("Start Calibre's wireless device connection, then connect here to receive books."),
                ));
                children.push(actions(
                    t,
                    vec![
                        action(&tr("Discover"), "catalogs:discover"),
                        action(&tr("Connect"), "catalogs:connect"),
                        action(&tr("Downloads"), "catalogs:downloads"),
                    ],
                ));
                children.push(row(
                    &item(tr("Server address and port"), &state.address, "catalogs:field:0"),
                    t,
                ));
                children.push(row(
                    &item(
                        tr("Server password, optional"),
                        tr("Used for this connection only"),
                        "catalogs:field:1",
                    ),
                    t,
                ));
            }
            children.push(list(
                state
                    .servers
                    .iter()
                    .enumerate()
                    .map(|(i, s)| item(&s.name, s.address.to_string(), format!("catalogs:server:{i}")))
                    .collect(),
            ));
        }
        View::CalibreBooks { books, .. } => {
            let items: Vec<Item> = books
                .iter()
                .enumerate()
                .filter_map(|(pos, &index)| {
                    let book = state.cache.metadata()?.books().get(index)?;
                    Some(item(
                        book.display_title(),
                        calibre_detail(book),
                        format!("catalogs:calibre-book:{pos}"),
                    ))
                })
                .collect();
            if items.is_empty() {
                children.push(body(services, tr("No books found.")));
            }
            children.push(list(items));
        }
        View::CalibreGroups { groups, .. } => {
            if groups.is_empty() {
                children.push(body(services, tr("No values in the Calibre metadata file.")));
            }
            children.push(list(
                groups
                    .iter()
                    .enumerate()
                    .map(|(i, (value, count))| {
                        let count = *count as i64;
                        item(
                            value.clone(),
                            tr_format(&tr_n("%1 book", "%1 books", count), &[&count.to_string()]),
                            format!("catalogs:calibre-group:{i}"),
                        )
                    })
                    .collect(),
            ));
        }
        View::Downloads => {
            children.push(body(
                services,
                tr("OPDS and Calibre save books here. The default is the device's books folder."),
            ));
            children.push(text_field(
                t,
                &tr("Folder"),
                &state.download_folder,
                "folder",
                false,
            ));
            children.push(action(&tr("Save folder"), "catalogs:save-folder"));
            if !state.typing {
                children.push(action(&tr("Edit folder"), "catalogs:field:0"));
            }
            children.push(WidgetPod::new(kaia_ui::components::Spacer::new(1)));
        }
    }
    if !state.status.is_empty() {
        children.push(
            WidgetPod::new(
                Paragraph::new(&state.status, t.font.caption.clone(), Tone::InkSecondary).max_lines(2),
            )
            .with_key("catalogs-status"),
        );
    }
    if state.pending.is_some() {
        children.push(actions(
            t,
            vec![
                action(&tr("Cancel"), "catalogs:cancel"),
                action(&tr("Turn on Wi-Fi"), "catalogs:wifi"),
            ],
        ));
    } else if state.worker.is_some() {
        children.push(action(
            &if state.connection {
                tr("Disconnect")
            } else {
                tr("Cancel")
            },
            "catalogs:cancel",
        ));
    }
    if state.book.is_some() {
        children.push(action(&tr("Open"), "catalogs:open-book"));
    }
    if state.typing {
        children.push(WidgetPod::new(
            OnScreenKeyboard::new(&ENGLISH).on_hide("catalogs:hide-keyboard"),
        ));
    }
    if matches!(state.view, View::Book { .. }) {
        Window::new(
            SheetPanel::new(
                t,
                title,
                WidgetPod::new(
                    Column::new(children)
                        .gap(t.space.sm)
                        .cross_align(CrossAlign::Stretch),
                ),
            )
            .modal(true),
            t.clone(),
            services.assets.clone(),
            screen,
        )
    } else {
        root_window(services, screen, page(services, children))
    }
}
