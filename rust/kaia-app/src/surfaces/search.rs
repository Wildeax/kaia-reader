//! Local search with scopes, a shared keyboard and a retained result page.
use std::sync::mpsc::{self, Receiver};

use kaia_events::{Event, Value};
use kaia_gfx::Rect;
use kaia_i18n::{format as tr_format, tr};
use kaia_library::{Book, CoverSize};
use kaia_ui::components::{
    Column, CrossAlign, Divider, Expanded, Frame, Label, Padding, Paragraph, Row, SizedBox,
};
use kaia_ui::controls::{IconButton, OnScreenKeyboard, TextField, ENGLISH};
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, Tone, WidgetPod, Window};

use super::{
    common,
    library::{
        data, views,
        widgets::{FittedCover, Overlay},
    },
    notes,
    reader::dict,
    Route,
};
use crate::surface::{Nav, Outcome, Surface, SurfaceEnv};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Books,
    Current,
    Notes,
    Dictionary,
}
impl Scope {
    fn label(self) -> String {
        match self {
            Self::Books => tr("My books"),
            Self::Current => tr("Current book"),
            Self::Notes => tr("Annotations"),
            Self::Dictionary => tr("Dictionary"),
        }
    }
}
#[derive(Clone)]
struct Hit {
    book: i64,
    title: String,
    detail: String,
    status: String,
    location: Option<Value>,
    rank: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Popup {
    Scope,
    Sort,
    Books,
}

pub struct SearchSurface {
    query: String,
    scope: Scope,
    typing: bool,
    popup: Option<Popup>,
    hits: Vec<Hit>,
    message: String,
    page: usize,
    alphabetical: bool,
    book: Option<i64>,
    worker: Option<Receiver<Result<Vec<Hit>, String>>>,
}
impl Default for SearchSurface {
    fn default() -> Self {
        Self {
            query: String::new(),
            scope: Scope::Books,
            typing: true,
            popup: None,
            hits: Vec::new(),
            message: String::new(),
            page: 0,
            alphabetical: false,
            book: None,
            worker: None,
        }
    }
}

/// Shared by global search and the reader's search panel.
pub fn query_field(text: &str, placeholder: &str) -> TextField {
    TextField::new().with_text(text).placeholder(placeholder)
}

/// The keyboard used by search and password entry.
pub fn keyboard() -> WidgetPod {
    WidgetPod::new(OnScreenKeyboard::new(&ENGLISH).background(Tone::Surface)).with_key("search-keyboard")
}

impl SearchSurface {
    fn page_size(&self) -> usize {
        if self.scope == Scope::Current {
            4
        } else {
            5
        }
    }
    fn submit(&mut self, env: &mut SurfaceEnv<'_>) {
        self.worker = None;
        self.hits.clear();
        self.message.clear();
        self.page = 0;
        self.typing = false;
        let query = self.query.trim();
        if query.is_empty() {
            self.typing = true;
            return;
        }
        let _ = env.services.library.record_search(query);
        match self.scope {
            Scope::Books => {
                self.hits = env
                    .services
                    .library
                    .search(query, 200)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|b| b.path.exists())
                    .enumerate()
                    .map(|(rank, b)| Hit {
                        book: b.id,
                        title: data::title(&b),
                        detail: b.authors.clone().unwrap_or_default(),
                        status: format!(
                            "{}\n{}",
                            data::status_label(&b),
                            tr_format(
                                &tr("%1 · %2"),
                                &[&b.format.to_uppercase(), &data::size_text(b.size)]
                            )
                        ),
                        location: None,
                        rank,
                    })
                    .collect();
            }
            Scope::Notes => {
                let books = data::all(&env.services.library)
                    .into_iter()
                    .map(|b| (b.partial_md5.clone(), b))
                    .collect();
                let needle = query.to_lowercase();
                for book in notes::data::load(&env.services.paths.data_dir, &books) {
                    let Some(id) = book.library_id else {
                        continue;
                    };
                    for note in book.notes {
                        let text = format!("{} {}", note.primary(), note.note.as_deref().unwrap_or(""));
                        if !text.to_lowercase().contains(&needle) {
                            continue;
                        }
                        let location = match note.location {
                            notes::data::Location::Page(p) => Value::Number(p as f64),
                            notes::data::Location::Xpointer(x) => Value::from(x),
                        };
                        self.hits.push(Hit {
                            book: id,
                            title: book.title.clone(),
                            detail: text.clone(),
                            status: String::new(),
                            location: Some(location),
                            rank: usize::MAX - text.to_lowercase().matches(&needle).count(),
                        });
                    }
                }
            }
            Scope::Dictionary => {
                self.message = dict::lookup(query, env.services, &[]).text;
            }
            Scope::Current => {
                let book = self
                    .book
                    .and_then(|id| env.services.library.book(id).ok().flatten());
                if let Some(book) = book {
                    let query = query.to_owned();
                    let (send, reply) = mpsc::channel();
                    self.worker = Some(reply);
                    self.message = tr("Searching…");
                    std::thread::spawn(move || {
                        let _ = send.send(search_book(book, &query));
                    });
                } else {
                    self.message = tr("Choose a book to search.");
                }
            }
        }
        self.sort();
    }
    fn sort(&mut self) {
        if self.alphabetical {
            self.hits.sort_by_key(|h| h.title.to_lowercase());
        } else {
            self.hits.sort_by_key(|h| h.rank);
        }
        self.page = 0;
    }
}
impl Surface for SearchSurface {
    fn route(&self) -> Route {
        Route::Search
    }
    fn busy(&self) -> bool {
        self.worker.is_some()
    }
    fn build(&mut self, env: &mut SurfaceEnv<'_>) -> Option<Window> {
        let t = &env.services.theme;
        if self.book.is_none() {
            self.book = env
                .services
                .library
                .recent(1)
                .ok()
                .and_then(|b| b.first().map(|b| b.id));
        }
        let head = WidgetPod::new(
            Row::new(vec![
                WidgetPod::new(IconButton::new("chevron.left").on_action("back")).with_key("search-back"),
                WidgetPod::new(Expanded::new(
                    WidgetPod::new(Label::new(tr("Search"), t.font.caption.clone(), Tone::Ink)),
                    1,
                )),
            ])
            .gap(t.space.md),
        );
        let head = WidgetPod::new(SizedBox::new(Some(head)).height(t.scale.dp(30)));
        let field = if self.typing {
            WidgetPod::new(
                query_field(&self.query, &tr("Search"))
                    .borderless()
                    .on_change("global-search:query")
                    .on_submit("global-search:submit"),
            )
            .focusable(true)
            .with_key("search-field")
        } else {
            field_button(t, &self.query, "global-search:edit", "search-field")
        };
        let entry = WidgetPod::new(
            Frame::new(WidgetPod::new(
                Row::new(vec![
                    WidgetPod::new(IconButton::new("appbar.search").on_action("global-search:submit"))
                        .with_key("search-go"),
                    WidgetPod::new(Expanded::new(field, 1)),
                    field_button(
                        t,
                        &format!("{} ⌄", self.scope.label()),
                        "global-search:scope",
                        "search-scope",
                    ),
                ])
                .cross_align(CrossAlign::Center),
            ))
            .padding(Insets::all(2))
            .border(Tone::Ink, 2)
            .fill(Tone::Surface),
        );
        let entry = WidgetPod::new(SizedBox::new(Some(entry)).height(t.scale.dp(43)));
        let mut rows = vec![head, entry];
        if self.scope == Scope::Current {
            let title = self
                .book
                .and_then(|id| env.services.library.book(id).ok().flatten())
                .map(|b| data::title(&b))
                .unwrap_or_else(|| tr("Choose a book"));
            rows.push(views::compact(t, &title, "global-search:books", "search-book"));
        }
        if !self.typing && self.scope != Scope::Dictionary {
            rows.push(WidgetPod::new(Row::new(vec![
                WidgetPod::new(Expanded::new(
                    views::compact(
                        t,
                        &if self.alphabetical {
                            tr("Title ⌄")
                        } else {
                            tr("Relevance ⌄")
                        },
                        "global-search:sort",
                        "search-sort",
                    ),
                    1,
                )),
                WidgetPod::new(Label::new(
                    tr_format(
                        &tr(if self.hits.len() == 1 {
                            "%1 result"
                        } else {
                            "%1 results"
                        }),
                        &[&self.hits.len().to_string()],
                    ),
                    t.font.caption.clone(),
                    Tone::Ink,
                )),
            ])));
        }
        let content = if !self.message.is_empty() {
            WidgetPod::new(
                super::library::widgets::Scroll::new(
                    vec![self.message.clone()],
                    1,
                    1,
                    |text: &String, t: &Theme| {
                        WidgetPod::new(Paragraph::new(text, t.font.body.clone(), Tone::Ink))
                    },
                )
                .measure_single_row(),
            )
        } else if !self.typing && self.hits.is_empty() {
            views::body_text(t, &tr("No results. Try another word."))
        } else {
            let hits = self
                .hits
                .iter()
                .enumerate()
                .skip(self.page * self.page_size())
                .take(self.page_size())
                .map(|(n, hit)| result_row(t, env.services, n, hit))
                .collect();
            WidgetPod::new(
                Column::new(hits)
                    .gap(t.scale.dp(12))
                    .cross_align(CrossAlign::Stretch),
            )
        };
        rows.push(WidgetPod::new(Expanded::new(content, 1)));
        if self.typing {
            rows.push(
                WidgetPod::new(
                    OnScreenKeyboard::new(&ENGLISH)
                        .background(Tone::Surface)
                        .on_hide("global-search:hide"),
                )
                .with_key("search-keyboard"),
            );
        } else if !self.hits.is_empty() {
            let pages = self.hits.len().div_ceil(self.page_size());
            let pager = WidgetPod::new(Row::new(vec![
                views::compact(t, "‹", "global-search:previous", "search-previous"),
                WidgetPod::new(Expanded::new(
                    WidgetPod::new(kaia_ui::components::Align::new(
                        WidgetPod::new(Label::new(
                            tr_format(
                                &tr("Page %1 of %2"),
                                &[&(self.page + 1).to_string(), &pages.to_string()],
                            ),
                            t.font.caption.clone(),
                            Tone::Ink,
                        )),
                        kaia_ui::geometry::Alignment::Center,
                        kaia_ui::geometry::Alignment::Center,
                    )),
                    1,
                )),
                views::compact(t, "›", "global-search:next", "search-next"),
            ]));
            rows.push(WidgetPod::new(SizedBox::new(Some(pager)).height(t.scale.dp(40))));
        }
        let panel = WidgetPod::new(
            Frame::new(WidgetPod::new(Padding::new(
                WidgetPod::new(
                    Column::new(rows)
                        .gap(t.scale.dp(14))
                        .cross_align(CrossAlign::Stretch),
                ),
                Insets::all(t.scale.dp(18)),
            )))
            .fill(Tone::Surface)
            .border(Tone::Ink, 1),
        );
        let base = WidgetPod::new(Overlay::new(None, panel, Rect::new(12, 12, 608, 816), "back").dialog());
        let root = if let Some(popup) = self.popup {
            let options = match popup {
                Popup::Scope => [Scope::Books, Scope::Current, Scope::Notes, Scope::Dictionary]
                    .into_iter()
                    .enumerate()
                    .map(|(n, scope)| {
                        views::menu_row(
                            t,
                            &scope.label(),
                            &format!("global-search:scope:{n}"),
                            &format!("search-scope-{n}"),
                            scope == self.scope,
                            None,
                        )
                    })
                    .collect(),
                Popup::Sort => [tr("Relevance"), tr("Title")]
                    .into_iter()
                    .enumerate()
                    .map(|(n, label)| {
                        views::menu_row(
                            t,
                            &label,
                            &format!("global-search:sort:{n}"),
                            &format!("search-sort-{n}"),
                            self.alphabetical == (n == 1),
                            None,
                        )
                    })
                    .collect(),
                Popup::Books => data::all(&env.services.library)
                    .iter()
                    .map(|book| {
                        views::menu_row(
                            t,
                            &data::title(book),
                            &format!("global-search:book:{}", book.id),
                            &format!("search-book-{}", book.id),
                            self.book == Some(book.id),
                            None,
                        )
                    })
                    .collect(),
            };
            let panel = if popup == Popup::Books {
                WidgetPod::new(super::library::widgets::Scroll::new(
                    data::all(&env.services.library),
                    1,
                    46,
                    |book: &Book, t: &Theme| {
                        views::menu_row(
                            t,
                            &data::title(book),
                            &format!("global-search:book:{}", book.id),
                            &format!("search-book-{}", book.id),
                            false,
                            None,
                        )
                    },
                ))
            } else {
                views::menu(t, options)
            };
            WidgetPod::new(Overlay::new(
                Some(base),
                panel,
                if popup == Popup::Scope {
                    Rect::new(411, 111, 190, 194)
                } else if popup == Popup::Sort {
                    Rect::new(32, 160, 220, 102)
                } else {
                    Rect::new(32, 160, 550, 420)
                },
                "global-search:dismiss",
            ))
        } else {
            base
        };
        Some(common::root_window(
            env.services,
            env.screen,
            SizedBox::new(Some(root)),
        ))
    }
    fn on_event(&mut self, ev: &Event, _env: &mut SurfaceEnv<'_>) -> Outcome {
        if ev.name == "ReaderTick" {
            if let Some(result) = self.worker.as_ref().and_then(|r| r.try_recv().ok()) {
                self.worker = None;
                self.message.clear();
                match result {
                    Ok(hits) => self.hits = hits,
                    Err(error) => self.message = error,
                }
                self.sort();
                return Outcome::Rebuild;
            }
        }
        Outcome::NotMine
    }
    fn on_command(&mut self, cmd: &str, args: &[Value], env: &mut SurfaceEnv<'_>) -> Outcome {
        match cmd {
            "surface:back" if self.popup.is_some() => {
                self.popup = None;
            }
            "global-search:dismiss" => self.popup = None,
            "global-search:scope" => self.popup = Some(Popup::Scope),
            "global-search:sort" => self.popup = Some(Popup::Sort),
            "global-search:books" => self.popup = Some(Popup::Books),
            "global-search:query" => {
                self.query = args.first().and_then(Value::as_str).unwrap_or("").into();
                return Outcome::Handled;
            }
            "global-search:edit" => self.typing = true,
            "global-search:hide" => self.typing = false,
            "global-search:submit" => self.submit(env),
            "global-search:next" => {
                if (self.page + 1) * self.page_size() < self.hits.len() {
                    self.page += 1;
                }
            }
            "global-search:previous" => self.page = self.page.saturating_sub(1),
            _ => {
                if let Some(n) = cmd
                    .strip_prefix("global-search:scope:")
                    .and_then(|v| v.parse::<usize>().ok())
                {
                    if let Some(scope) =
                        [Scope::Books, Scope::Current, Scope::Notes, Scope::Dictionary].get(n)
                    {
                        self.scope = *scope;
                    }
                    self.popup = None;
                    self.hits.clear();
                    self.message.clear();
                    self.worker = None;
                    self.typing = true;
                } else if let Some(id) = cmd
                    .strip_prefix("global-search:book:")
                    .and_then(|v| v.parse().ok())
                {
                    self.book = Some(id);
                    self.popup = None;
                    self.typing = true;
                } else if let Some(n) = cmd.strip_prefix("global-search:sort:") {
                    self.alphabetical = n == "1";
                    self.sort();
                    self.popup = None;
                } else if let Some(n) = cmd
                    .strip_prefix("global-search:hit:")
                    .and_then(|v| v.parse::<usize>().ok())
                {
                    if let Some(hit) = self.hits.get(n) {
                        return Outcome::Navigate(Nav::OpenBook {
                            book: hit.book,
                            location: hit.location.clone(),
                        });
                    }
                } else {
                    return Outcome::NotMine;
                }
            }
        }
        Outcome::Rebuild
    }
}
fn result_row(t: &Theme, services: &crate::services::Services, index: usize, hit: &Hit) -> WidgetPod {
    let cover = data::cover(&services.library, hit.book, CoverSize::Small);
    let row = Row::new(vec![
        WidgetPod::new(
            SizedBox::new(Some(WidgetPod::new(FittedCover::new(hit.title.clone(), cover))))
                .width(t.scale.dp(62))
                .height(t.scale.dp(91)),
        ),
        WidgetPod::new(Expanded::new(
            WidgetPod::new(
                Column::new(vec![
                    WidgetPod::new(Label::new(&hit.title, t.font.book_title.clone(), Tone::Ink)),
                    WidgetPod::new(Paragraph::new(&hit.detail, t.font.body.clone(), Tone::Ink).max_lines(2)),
                ])
                .gap(t.space.sm)
                .shrink(),
            ),
            1,
        )),
        WidgetPod::new(
            SizedBox::new(Some(WidgetPod::new(
                Paragraph::new(&hit.status, t.font.caption.clone(), Tone::Ink).max_lines(2),
            )))
            .width(t.scale.dp(132)),
        ),
    ])
    .gap(t.scale.dp(11))
    .cross_align(CrossAlign::Center);
    WidgetPod::new(
        super::library::press::Pressable::new(WidgetPod::new(
            Column::new(vec![
                WidgetPod::new(row),
                WidgetPod::new(Divider::new(Tone::Divider)),
            ])
            .gap(t.scale.dp(12))
            .shrink(),
        ))
        .tap(format!("global-search:hit:{index}"), vec![]),
    )
    .with_key(format!("search-hit-{index}"))
}

fn search_book(book: Book, query: &str) -> Result<Vec<Hit>, String> {
    // This read-only document never opens or rewrites the book's saved state.
    let error = |_| tr("Cannot search this book.");
    let extension = book.path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if kaia_doc::cre::Kind::from_extension(extension).is_some() {
        let doc = kaia_doc::cre::CreDocument::open(&book.path).map_err(error)?;
        return collect_hits(&doc, &book, query, |page, quad| {
            let b = quad.bounds();
            doc.xpointer_at(page, b.x0 as i32, b.y0 as i32)?
                .map_or_else(|| doc.page_xpointer(page), Ok)
                .map(Value::from)
        });
    }
    let doc = kaia_doc::open(&book.path).map_err(error)?;
    collect_hits(doc.as_ref(), &book, query, |page, _| {
        Ok(Value::Number(page as f64))
    })
}
fn collect_hits(
    doc: &dyn kaia_doc::Document,
    book: &Book,
    query: &str,
    location: impl Fn(u32, kaia_doc::Quad) -> kaia_doc::Result<Value>,
) -> Result<Vec<Hit>, String> {
    let error = |_| tr("Cannot search this book.");
    let mut hits = Vec::new();
    for page in 0..doc.page_count() as u32 {
        let found = doc.search(page, query).map_err(error)?;
        let Some(quad) = found.first() else {
            continue;
        };
        let text = doc.page_text(page).map_err(error)?.text;
        let snippet = text
            .lines()
            .find(|line| line.to_lowercase().contains(&query.to_lowercase()))
            .unwrap_or(&text)
            .chars()
            .take(180)
            .collect();
        hits.push(Hit {
            book: book.id,
            title: data::title(book),
            detail: snippet,
            status: tr_format(&tr("Page %1"), &[&(page + 1).to_string()]),
            location: Some(location(page, *quad).map_err(error)?),
            rank: usize::MAX - found.len(),
        });
        if hits.len() == 200 {
            break;
        }
    }
    Ok(hits)
}

fn field_button(t: &Theme, text: &str, action: &str, key: &str) -> WidgetPod {
    let mut font = t.font.body.clone();
    font.size = t.scale.dp(18) as f32 * t.text_scale;
    WidgetPod::new(
        super::library::press::Pressable::new(WidgetPod::new(Padding::new(
            WidgetPod::new(Label::new(text, font, Tone::Ink)),
            Insets::symmetric(t.scale.dp(8), t.scale.dp(2)),
        )))
        .tap(action, vec![]),
    )
    .with_key(key)
    .focusable(true)
}
