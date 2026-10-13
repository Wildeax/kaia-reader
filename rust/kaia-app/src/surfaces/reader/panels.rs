//! The reader's panels: sheets that rise from the bottom edge above the overlay
//! or the page. Each panel is a surface, so the router sends it gestures and
//! keys. Its close button and its taps above the sheet pop it. The panels hold
//! kaia-ui composites: a segmented tab row, paged lists, radio groups, steppers,
//! toggles, text fields and the on-screen keyboard.

use std::cell::RefCell;
use std::rc::Rc;

use kaia_book::{Annotation, TextLayout, ZoomMode};
use kaia_events::{Refresh, RefreshMode, ShowOpts};
use kaia_gfx::Size;
use kaia_ui::components::{Column, CrossAlign, Expanded, Frame, Label, Paragraph, Row, SizedBox};
use kaia_ui::composite::{ListRow, PageLabels, PagedList};
use kaia_ui::controls::{
    ButtonStyle, IconButton, OnScreenKeyboard, RadioGroup, Stepper, TextField, Toggle, ENGLISH,
};
use kaia_ui::geometry::Insets;
use kaia_ui::{Theme, Tone, WidgetPod, Window};

use super::state::Core;
use crate::services::Services;
use crate::surface::{Surface, SurfaceEnv};
use crate::surfaces::common::{button, fill, window};
use crate::surfaces::Route;

/// Font choices of the Text and layout sheet: the label and the face name.
/// The first choice keeps the book's own font.
pub const FONT_CHOICES: [(&str, Option<&str>); 3] = [
    ("Book's font", None),
    ("Noto Serif", Some("Noto Serif")),
    ("Noto Sans", Some("Noto Sans")),
];

/// Share of the screen height that a tall sheet takes.
const TALL_PERCENT: u32 = 70;
/// Share of the screen height that the search and note sheets take.
const WRITING_PERCENT: u32 = 90;

/// One row of a paged list in the Contents panel.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentItem {
    pub label: String,
    pub detail: String,
    /// True for the entry that holds the page being read. Shown as a trailing "Now".
    pub current: bool,
    pub action: String,
    /// How deep the entry is in the table of contents. The top level is 0.
    pub level: u32,
}

/// One row of the search results.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchRow {
    pub label: String,
    pub snippet: String,
    pub action: String,
}

/// A panel surface. The reader keeps its window, and clears the reader's
/// handle when the panel pops.
pub struct Panel {
    window: Window,
    serial: u64,
    core: Rc<RefCell<Core>>,
}

impl Panel {
    pub fn new(window: Window, serial: u64, core: Rc<RefCell<Core>>) -> Self {
        Self { window, serial, core }
    }
}

impl Surface for Panel {
    fn route(&self) -> Route {
        Route::ReaderPanel
    }

    fn show_opts(&self) -> ShowOpts {
        ShowOpts {
            covers_fullscreen: false,
            refresh: Some(Refresh::new(RefreshMode::Ui)),
            ..ShowOpts::default()
        }
    }

    fn build(&mut self, _env: &mut SurfaceEnv<'_>) -> Option<Window> {
        Some(self.window.clone())
    }

    fn closes_on_unused_tap(&self) -> bool {
        true
    }

    fn on_hide(&mut self, _env: &mut SurfaceEnv<'_>) {
        let mut core = self.core.borrow_mut();
        if core.panel.as_ref().is_some_and(|l| l.serial == self.serial) {
            core.panel = None;
        }
    }
}

/// A sheet from the bottom edge: an optional title with a close button, then
/// the content. `height_percent` fixes the height; None sizes it to the content.
fn sheet(
    services: &Services,
    screen: Size,
    title: Option<&str>,
    mut content: Vec<WidgetPod>,
    height_percent: Option<u32>,
) -> Window {
    let t = &services.theme;
    let mut rows = Vec::new();
    if let Some(title) = title {
        let heading = WidgetPod::new(Label::new(title.to_string(), t.font.title.clone(), Tone::Ink));
        let close =
            WidgetPod::new(IconButton::new("close").on_action("reader:panel:close")).with_key("panel-close");
        rows.push(WidgetPod::new(
            Row::new(vec![WidgetPod::new(Expanded::new(heading, 1)), close])
                .gap(t.space.md)
                .cross_align(CrossAlign::Center),
        ));
    }
    rows.append(&mut content);
    let mut body = Column::new(rows).gap(t.space.md).cross_align(CrossAlign::Stretch);
    if height_percent.is_none() {
        body = body.shrink();
    }
    let body = WidgetPod::new(body);
    let framed = WidgetPod::new(
        Frame::new(body)
            .fill(Tone::Surface)
            .border(Tone::Ink, t.line.hairline)
            .radius(t.radius.card)
            .padding(Insets::all(t.space.lg)),
    );
    let sized = match height_percent {
        Some(p) => WidgetPod::new(SizedBox::new(Some(framed)).height(screen.h * p / 100)),
        None => framed,
    };
    window(
        services,
        screen,
        Column::new(vec![fill(), sized]).cross_align(CrossAlign::Stretch),
    )
}

/// The heading of a group of choices, such as the font list. The choices below it
/// are a radio group, which is a list of rows of its own.
fn group_heading(services: &Services, text: &str) -> WidgetPod {
    let t = &services.theme;
    WidgetPod::new(Label::new(
        text.to_string(),
        t.font.label.clone(),
        Tone::InkSecondary,
    ))
}

/// A labelled row: the label on the left, the control on the right.
fn labelled(services: &Services, label: &str, control: WidgetPod) -> WidgetPod {
    let t = &services.theme;
    WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(
                WidgetPod::new(Label::new(label.to_string(), t.font.body.clone(), Tone::Ink)),
                1,
            )),
            control,
        ])
        .gap(t.space.md)
        .cross_align(CrossAlign::Center),
    )
}

fn tab_button(index: usize, label: &str, active: bool) -> WidgetPod {
    let style = if active {
        ButtonStyle::Primary
    } else {
        ButtonStyle::Secondary
    };
    WidgetPod::new(Expanded::new(
        button(
            style,
            label,
            &format!("reader:contents:tab:{index}"),
            &format!("contents-tab-{index}"),
        ),
        1,
    ))
}

/// The Contents panel. `tab` is 0 for chapters, 1 for bookmarks and 2 for highlights.
pub fn contents(
    services: &Services,
    screen: Size,
    tab: usize,
    items: Vec<ContentItem>,
    empty: &str,
) -> Window {
    let t = &services.theme;
    let tabs = WidgetPod::new(
        Row::new(vec![
            tab_button(0, "Chapters", tab == 0),
            tab_button(1, "Bookmarks", tab == 1),
            tab_button(2, "Highlights", tab == 2),
        ])
        .gap(t.space.sm),
    );
    let mut children = vec![tabs];
    if items.is_empty() {
        children.push(WidgetPod::new(Paragraph::new(
            empty.to_string(),
            t.font.body.clone(),
            Tone::InkSecondary,
        )));
    } else {
        let list = PagedList::new(items, |item: &ContentItem, _t: &Theme| {
            let mut row = ListRow::new(item.label.clone())
                .indent(item.level)
                .on_action(item.action.clone());
            if !item.detail.is_empty() {
                row = row.secondary(item.detail.clone());
            }
            if item.current {
                row = row.value("Now");
            }
            WidgetPod::new(row).with_key(key_of(&item.action))
        })
        .labels(page_labels());
        children.push(WidgetPod::new(Expanded::new(
            WidgetPod::new(list).with_key("contents-list"),
            1,
        )));
    }
    sheet(services, screen, Some("Contents"), children, Some(TALL_PERCENT))
}

/// The key of a list row: its action id with the colons turned into dashes, so
/// `reader:search:hit:4` is `reader-search-hit-4`.
pub fn key_of(action: &str) -> String {
    action.replace(':', "-")
}

fn page_labels() -> PageLabels {
    PageLabels::new("Previous", "Next", |page, count| {
        format!("Page {page} of {count}")
    })
}

/// The Text and layout sheet. For reflowable books it shows the text controls,
/// for fixed-layout books the zoom choice.
pub fn layout(services: &Services, screen: Size, reflow: Option<&TextLayout>, zoom: ZoomMode) -> Window {
    let mut children = Vec::new();
    match reflow {
        Some(layout) => {
            let selected = FONT_CHOICES
                .iter()
                .position(|(_, face)| *face == layout.font_family.as_deref())
                .or(Some(0));
            let options = FONT_CHOICES
                .iter()
                .map(|(label, _)| (*label).to_string())
                .collect();
            let font = WidgetPod::new(RadioGroup::new(options, selected).on_action("reader:layout:font"))
                .with_key("layout-font");
            children.push(group_heading(services, "Font"));
            children.push(font);
            let size = WidgetPod::new(
                Stepper::new(
                    f64::from(layout.font_size),
                    f64::from(super::koreader_px(screen, 12.0)),
                    f64::from(super::koreader_px(screen, 44.0)),
                    1.0,
                )
                .format(|v| format!("{}", v as i64))
                .on_action("reader:layout:size"),
            )
            .with_key("layout-size");
            children.push(labelled(services, "Size", size));
            let spacing = WidgetPod::new(
                Stepper::new(f64::from(layout.line_spacing), 0.8, 2.0, 0.1)
                    .format(|v| format!("{v:.1}"))
                    .on_action("reader:layout:spacing"),
            )
            .with_key("layout-spacing");
            children.push(labelled(services, "Line spacing", spacing));
            let margins = WidgetPod::new(
                Stepper::new(f64::from(layout.margins.left), 0.0, 80.0, 5.0)
                    .format(|v| format!("{}", v as i64))
                    .on_action("reader:layout:margins"),
            )
            .with_key("layout-margins");
            children.push(labelled(services, "Margins", margins));
            // The pill switch of the Settings rows, so every on or off setting looks the same.
            let toggle = |on: bool, action: &str, key: &str| {
                WidgetPod::new(Toggle::pill(on).on_action(action)).with_key(key)
            };
            children.push(labelled(
                services,
                "Justified text",
                toggle(layout.justify, "reader:layout:justify", "layout-justify"),
            ));
            children.push(labelled(
                services,
                "Hyphenation",
                toggle(
                    layout.hyphenation,
                    "reader:layout:hyphenation",
                    "layout-hyphenation",
                ),
            ));
            children.push(labelled(
                services,
                "Book's styles",
                toggle(layout.embedded_styles, "reader:layout:styles", "layout-styles"),
            ));
        }
        None => {
            let index = match zoom {
                ZoomMode::FitPage | ZoomMode::Manual { .. } => 0,
                ZoomMode::FitWidth => 1,
            };
            let options = vec!["Fit page".to_string(), "Fit width".to_string()];
            let zoom_group =
                WidgetPod::new(RadioGroup::new(options, Some(index)).on_action("reader:layout:zoom"))
                    .with_key("layout-zoom");
            children.push(group_heading(services, "Zoom"));
            children.push(zoom_group);
        }
    }
    let title = if reflow.is_some() {
        "Text and layout"
    } else {
        "Page view"
    };
    sheet(services, screen, Some(title), children, Some(WRITING_PERCENT))
}

/// The More panel: go to either end and mark the book as finished.
pub fn more(services: &Services, screen: Size) -> Window {
    let children = vec![WidgetPod::new(
        Column::new(vec![
            button(
                ButtonStyle::Secondary,
                "Go to the start",
                "reader:more:start",
                "more-start",
            ),
            button(
                ButtonStyle::Secondary,
                "Go to the end",
                "reader:more:end",
                "more-end",
            ),
            button(
                ButtonStyle::Secondary,
                "Mark as finished",
                "reader:finish",
                "more-finish",
            ),
        ])
        .gap(services.theme.space.sm)
        .cross_align(CrossAlign::Stretch),
    )];
    sheet(services, screen, Some("More"), children, None)
}

/// The selection popover: the word and five actions.
pub fn popover(services: &Services, screen: Size, word: &str) -> Window {
    let t = &services.theme;
    let shown = if word.chars().count() > 40 {
        let cut: String = word.chars().take(39).collect();
        format!("{cut}\u{2026}")
    } else {
        word.to_string()
    };
    let heading =
        WidgetPod::new(Label::new(shown, t.font.label.clone(), Tone::InkSecondary)).with_key("sel-word");
    let row = |items: [(&str, &str, &str); 3]| {
        WidgetPod::new(
            Row::new(
                items
                    .into_iter()
                    .map(|(label, action, key)| {
                        WidgetPod::new(Expanded::new(
                            button(ButtonStyle::Secondary, label, action, key),
                            1,
                        ))
                    })
                    .collect(),
            )
            .gap(t.space.sm),
        )
    };
    let children = vec![
        heading,
        row([
            ("Highlight", "reader:sel:highlight", "sel-highlight"),
            ("Note", "reader:sel:note", "sel-note"),
            ("Copy", "reader:sel:copy", "sel-copy"),
        ]),
        WidgetPod::new(
            Row::new(vec![
                WidgetPod::new(Expanded::new(
                    button(
                        ButtonStyle::Secondary,
                        "Dictionary",
                        "reader:sel:dict",
                        "sel-dict",
                    ),
                    1,
                )),
                WidgetPod::new(Expanded::new(
                    button(
                        ButtonStyle::Secondary,
                        "Search",
                        "reader:sel:search",
                        "sel-search",
                    ),
                    1,
                )),
            ])
            .gap(t.space.sm),
        ),
    ];
    sheet(services, screen, None, children, None)
}

/// The dictionary sheet: the word and its definitions, or the install instructions.
pub fn dictionary(services: &Services, screen: Size, word: &str, text: &str) -> Window {
    let t = &services.theme;
    let children = vec![
        WidgetPod::new(Label::new(word.to_string(), t.font.title.clone(), Tone::Ink)),
        WidgetPod::new(Paragraph::new(text.to_string(), t.font.body.clone(), Tone::Ink).max_lines(40))
            .with_key("dict-body"),
    ];
    sheet(services, screen, Some("Dictionary"), children, None)
}

/// The search panel: a text field with a Search button, the status, the results
/// and the on-screen keyboard.
pub fn search(services: &Services, screen: Size, text: &str, rows: Vec<SearchRow>, status: &str) -> Window {
    let t = &services.theme;
    let field = WidgetPod::new(
        TextField::new()
            .with_text(text.to_string())
            .placeholder("Search this book"),
    )
    .with_key("search-field")
    .focusable(true);
    let go = button(ButtonStyle::Primary, "Search", "reader:search:go", "search-go");
    let entry = WidgetPod::new(
        Row::new(vec![WidgetPod::new(Expanded::new(field, 1)), go])
            .gap(t.space.sm)
            .cross_align(CrossAlign::Center),
    );
    let status_label = WidgetPod::new(Label::new(
        status.to_string(),
        t.font.caption.clone(),
        Tone::InkSecondary,
    ))
    .with_key("search-status");
    let list = PagedList::new(rows, |row: &SearchRow, _t: &Theme| {
        WidgetPod::new(
            ListRow::new(row.label.clone())
                .secondary(row.snippet.clone())
                .on_action(row.action.clone()),
        )
        .with_key(key_of(&row.action))
    })
    .labels(page_labels());
    let children = vec![
        entry,
        status_label,
        WidgetPod::new(Expanded::new(WidgetPod::new(list).with_key("search-results"), 1)),
        WidgetPod::new(OnScreenKeyboard::new(&ENGLISH)),
    ];
    sheet(
        services,
        screen,
        Some("Search this book"),
        children,
        Some(WRITING_PERCENT),
    )
}

/// The note editor for a selection: a text field and the keyboard.
pub fn note(services: &Services, screen: Size, word: &str) -> Window {
    let t = &services.theme;
    let field = WidgetPod::new(TextField::new().placeholder("Note"))
        .with_key("note-field")
        .focusable(true);
    let save = button(ButtonStyle::Primary, "Save", "reader:note:save", "note-save");
    let children = vec![
        WidgetPod::new(Label::new(
            word.to_string(),
            t.font.label.clone(),
            Tone::InkSecondary,
        )),
        WidgetPod::new(
            Row::new(vec![WidgetPod::new(Expanded::new(field, 1)), save])
                .gap(t.space.sm)
                .cross_align(CrossAlign::Center),
        ),
        WidgetPod::new(OnScreenKeyboard::new(&ENGLISH)),
    ];
    sheet(services, screen, Some("Note"), children, Some(WRITING_PERCENT))
}

/// The end of book sheet: mark the book as finished, or go back to the Library.
pub fn end_of_book(services: &Services, screen: Size) -> Window {
    let t = &services.theme;
    let children = vec![
        WidgetPod::new(Paragraph::new(
            "You have reached the end of this book.".to_string(),
            t.font.body.clone(),
            Tone::Ink,
        )),
        button(
            ButtonStyle::Primary,
            "Mark as finished",
            "reader:finish",
            "end-finish",
        ),
        button(
            ButtonStyle::Secondary,
            "Back to Library",
            "nav:library",
            "end-library",
        ),
    ];
    sheet(services, screen, Some("End of book"), children, None)
}

/// The rows of the Contents panel for bookmarks or highlights.
pub fn annotation_items(annotations: &[Annotation], bookmarks: bool) -> Vec<ContentItem> {
    annotations
        .iter()
        .map(|a| {
            let label = if bookmarks {
                a.chapter.clone().unwrap_or_else(|| "Bookmark".to_string())
            } else {
                match a.text.as_deref() {
                    Some(text) if !text.trim().is_empty() => shorten(text, 80),
                    _ => "Highlight".to_string(),
                }
            };
            let detail = if bookmarks {
                a.datetime.clone()
            } else {
                a.note
                    .clone()
                    .unwrap_or_else(|| a.chapter.clone().unwrap_or_default())
            };
            ContentItem {
                label,
                detail,
                current: false,
                action: format!("reader:contents:annot:{}", a.id),
                level: 0,
            }
        })
        .collect()
}

/// `text` cut to `max` characters with an ellipsis.
pub fn shorten(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

/// The search rows for the results so far.
pub fn search_rows(results: &[kaia_book::SearchResult]) -> Vec<SearchRow> {
    results
        .iter()
        .map(|r| SearchRow {
            label: format!("Page {}", r.page + 1),
            snippet: r.snippet.clone(),
            action: format!("reader:search:hit:{}", r.index),
        })
        .collect()
}

/// Opens a panel as a surface, with the serial the reader uses to find it.
pub fn panel_surface(window: Window, serial: u64, core: Rc<RefCell<Core>>) -> Box<dyn Surface> {
    Box::new(Panel::new(window, serial, core))
}
