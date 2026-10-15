//! The reader's panels: sheets that rise from the bottom edge above the overlay
//! or the page. Each panel is a surface, so the router sends it gestures and
//! keys. Its close button and its taps above the sheet pop it. The panels hold
//! kaia-ui composites: a segmented tab row, paged lists, radio groups, steppers,
//! toggles, text fields and the on-screen keyboard.

use std::cell::RefCell;
use std::rc::Rc;

use kaia_book::{Annotation, Selection, TextLayout, ZoomMode};
use kaia_events::ShowOpts;
use kaia_gfx::{Color, Point, Size};
use kaia_i18n::{format as tr_format, tr};
use kaia_input::Gesture;
use kaia_ui::components::{Column, CrossAlign, Expanded, Frame, Label, Paragraph, Row, SizedBox};
use kaia_ui::composite::{ListRow, PageLabels, PagedList};
use kaia_ui::controls::{
    ButtonStyle, IconButton, OnScreenKeyboard, RadioGroup, Stepper, TextField, Toggle, ENGLISH,
};
use kaia_ui::geometry::Insets;
use kaia_ui::{
    AppAction, Constraints, EventCx, Handled, LayoutCx, PaintCx, Theme, Tone, UiEvent, Widget, WidgetPod,
    Window,
};

use super::footer::{Footer, Item};
use super::state::Core;
use crate::services::Services;
use crate::surface::{Surface, SurfaceEnv};
use crate::surfaces::common::{button, fill, window};
use crate::surfaces::settings::commands;
use crate::surfaces::Route;

/// The built-in font choices of the Text and layout sheet: the label and the face
/// name. The first choice keeps the book's own font. The user faces follow these.
pub const FONT_CHOICES: [(&str, Option<&str>); 5] = [
    ("Book's font", None),
    ("Noto Serif", Some("Noto Serif")),
    ("Noto Sans", Some("Noto Sans")),
    ("Atkinson Hyperlegible", Some("Atkinson Hyperlegible")),
    ("OpenDyslexic", Some("OpenDyslexic")),
];

fn font_label(label: &str) -> String {
    match label {
        "Book's font" => tr("Book's font"),
        "Noto Serif" => tr("Noto Serif"),
        "Noto Sans" => tr("Noto Sans"),
        other => other.to_string(),
    }
}

/// The font choices in list order: the built-in choices, then the user faces in
/// `user`. Each choice is its label and its face (None for the book's own font).
fn choices_with(user: &[String]) -> Vec<(String, Option<String>)> {
    let builtin = FONT_CHOICES
        .iter()
        .map(|(label, face)| (font_label(label), face.map(str::to_string)));
    let extra = commands::extra_font_faces(user)
        .into_iter()
        .map(|face| (face.clone(), Some(face)));
    builtin.chain(extra).collect()
}

/// The font choices, with the user faces the engine has registered.
fn font_choices() -> Vec<(String, Option<String>)> {
    choices_with(&kaia_doc::cre::user_font_faces())
}

/// The index in the font choices of a layout's face. A book with no face, or a face
/// that is not in the list, gets the first choice, the book's own font.
pub fn font_index(face: Option<&str>) -> usize {
    font_choices()
        .iter()
        .position(|(_, name)| name.as_deref() == face)
        .unwrap_or(0)
}

/// The label of font choice `index`, as the Font row and the font list show it.
pub fn font_choice_label(index: usize) -> String {
    font_choices()
        .into_iter()
        .nth(index)
        .map_or_else(String::new, |(label, _)| label)
}

/// The face of font choice `index` when the index is past the built-in choices.
/// The built-in faces are not here: `state::font_face` maps them.
pub fn user_face_at(index: usize) -> Option<String> {
    if index < FONT_CHOICES.len() {
        return None;
    }
    font_choices().into_iter().nth(index).and_then(|(_, face)| face)
}

/// The labels of the weight choices, in the order of `kaia_book::layout::FONT_WEIGHT_VALUES`.
/// The regular choice has its own translated label.
const WEIGHT_LABELS: [&str; 7] = ["-1", "-½", "Regular", "+½", "+1", "+1½", "+3"];
/// The index of regular text in the weight choices.
pub const REGULAR_WEIGHT_INDEX: usize = 2;

/// The weight choice at a stepper position. Positions outside the choices clamp.
pub fn font_weight_at(index: f64) -> f32 {
    let i = index.round().clamp(0.0, (WEIGHT_LABELS.len() - 1) as f64) as usize;
    kaia_book::layout::FONT_WEIGHT_VALUES[i]
}

/// The stepper position of the choice nearest to a weight value. A value
/// between choices, or one that is not a number, takes the nearest choice.
pub fn font_weight_index(value: f32) -> usize {
    if !value.is_finite() {
        return REGULAR_WEIGHT_INDEX;
    }
    let values = kaia_book::layout::FONT_WEIGHT_VALUES;
    (0..values.len())
        .min_by(|a, b| (values[*a] - value).abs().total_cmp(&(values[*b] - value).abs()))
        .unwrap_or(REGULAR_WEIGHT_INDEX)
}

/// The label of the weight choice at a stepper position.
pub fn font_weight_label(index: f64) -> String {
    let i = index.round().clamp(0.0, (WEIGHT_LABELS.len() - 1) as f64) as usize;
    if i == REGULAR_WEIGHT_INDEX {
        tr("Regular")
    } else {
        WEIGHT_LABELS[i].to_string()
    }
}

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

/// The screen edge that a panel sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Edge {
    Top,
    Bottom,
}

/// The edge for a panel over `selection`. The panel goes to the top when the
/// selection is in the lower half of the screen, so it does not cover the word.
/// Otherwise it stays at the bottom.
pub(super) fn edge_for(screen: Size, selection: Option<&Selection>, top_offset: u32) -> Edge {
    let rects = selection.map_or(&[][..], |s| s.screen_rects.as_slice());
    let top = rects.iter().map(|r| r.y).min();
    let bottom = rects.iter().map(|r| r.y + r.h as i32).max();
    match (top, bottom) {
        // The middle of the selection is at or below the middle of the screen.
        (Some(top), Some(bottom)) if top + bottom + 2 * top_offset as i32 >= screen.h as i32 => Edge::Top,
        _ => Edge::Bottom,
    }
}

/// A sheet from the bottom edge: an optional title with a close button, then
/// the content. `height_percent` fixes the height; None sizes it to the content.
fn sheet(
    services: &Services,
    screen: Size,
    title: Option<&str>,
    content: Vec<WidgetPod>,
    height_percent: Option<u32>,
) -> Window {
    framed(
        services,
        screen,
        Edge::Bottom,
        services.theme.space.lg,
        title,
        content,
        height_percent,
    )
}

/// A panel on `edge` with `pad` around its content. See [`sheet`].
fn framed(
    services: &Services,
    screen: Size,
    edge: Edge,
    pad: u32,
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
    let card = WidgetPod::new(
        Frame::new(body)
            .fill(Tone::Surface)
            .border(Tone::Ink, t.line.hairline)
            .radius(t.radius.card)
            .padding(Insets::all(pad)),
    );
    let sized = match height_percent {
        Some(p) => WidgetPod::new(SizedBox::new(Some(card)).height(screen.h * p / 100)),
        None => card,
    };
    let stack = match edge {
        Edge::Bottom => vec![fill(), sized],
        Edge::Top => vec![sized, fill()],
    };
    window(
        services,
        screen,
        Column::new(stack).cross_align(CrossAlign::Stretch),
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
            tab_button(0, &tr("Chapters"), tab == 0),
            tab_button(1, &tr("Bookmarks"), tab == 1),
            tab_button(2, &tr("Highlights"), tab == 2),
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
                row = row.value(tr("Now"));
            }
            WidgetPod::new(row).with_key(key_of(&item.action))
        })
        .labels(page_labels());
        children.push(WidgetPod::new(Expanded::new(
            WidgetPod::new(list).with_key("contents-list"),
            1,
        )));
    }
    sheet(
        services,
        screen,
        Some(&tr("Contents")),
        children,
        Some(TALL_PERCENT),
    )
}

/// The key of a list row: its action id with the colons turned into dashes, so
/// `reader:search:hit:4` is `reader-search-hit-4`.
pub fn key_of(action: &str) -> String {
    action.replace(':', "-")
}

fn page_labels() -> PageLabels {
    PageLabels::new(tr("Previous"), tr("Next"), |page, count| {
        tr_format(&tr("Page %1 of %2"), &[&page.to_string(), &count.to_string()])
    })
}

/// The Text and layout sheet. For reflowable books it shows the text controls,
/// for fixed-layout books the zoom choice.
pub fn layout(services: &Services, screen: Size, reflow: Option<&TextLayout>, zoom: ZoomMode) -> Window {
    let mut children = Vec::new();
    match reflow {
        Some(layout) => {
            // The face shows on the button. Tapping it opens the font list.
            let face = font_choice_label(font_index(layout.font_family.as_deref()));
            let font = button(
                ButtonStyle::Secondary,
                &face,
                "reader:layout:fonts",
                "layout-font",
            );
            children.push(labelled(services, &tr("Font"), font));
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
            children.push(labelled(services, &tr("Size"), size));
            let weight_index = font_weight_index(layout.font_weight);
            let weight = WidgetPod::new(
                Stepper::new(weight_index as f64, 0.0, (WEIGHT_LABELS.len() - 1) as f64, 1.0)
                    .format(font_weight_label)
                    .on_action("reader:layout:weight"),
            )
            .with_key("layout-weight");
            children.push(labelled(services, &tr("Weight"), weight));
            let spacing = WidgetPod::new(
                Stepper::new(f64::from(layout.line_spacing), 0.8, 2.0, 0.1)
                    .format(|v| format!("{v:.1}"))
                    .on_action("reader:layout:spacing"),
            )
            .with_key("layout-spacing");
            children.push(labelled(services, &tr("Line spacing"), spacing));
            let margins = WidgetPod::new(
                Stepper::new(f64::from(layout.margins.left), 0.0, 80.0, 5.0)
                    .format(|v| format!("{}", v as i64))
                    .on_action("reader:layout:margins"),
            )
            .with_key("layout-margins");
            children.push(labelled(services, &tr("Margins"), margins));
            // The pill switch of the Settings rows, so every on or off setting looks the same.
            let toggle = |on: bool, action: &str, key: &str| {
                WidgetPod::new(Toggle::pill(on).on_action(action)).with_key(key)
            };
            children.push(labelled(
                services,
                &tr("Justified text"),
                toggle(layout.justify, "reader:layout:justify", "layout-justify"),
            ));
            children.push(labelled(
                services,
                &tr("Hyphenation"),
                toggle(
                    layout.hyphenation,
                    "reader:layout:hyphenation",
                    "layout-hyphenation",
                ),
            ));
            let hyph_lang = button(
                ButtonStyle::Secondary,
                &hyphenation_choice_label(layout.hyphenation_lang.as_deref()),
                "reader:layout:hyphlang",
                "layout-hyphlang",
            );
            children.push(labelled(services, &tr("Hyphenation language"), hyph_lang));
            children.push(labelled(
                services,
                &tr("Book's styles"),
                toggle(layout.embedded_styles, "reader:layout:styles", "layout-styles"),
            ));
        }
        None => {
            let index = match zoom {
                ZoomMode::FitPage => Some(0),
                ZoomMode::FitWidth => Some(1),
                ZoomMode::FitColumns => Some(2),
                ZoomMode::Manual { .. } | ZoomMode::Region => None,
            };
            let options = vec![tr("Fit page"), tr("Fit width"), tr("Fit columns")];
            let zoom_group = WidgetPod::new(RadioGroup::new(options, index).on_action("reader:layout:zoom"))
                .with_key("layout-zoom");
            children.push(group_heading(services, &tr("Zoom")));
            children.push(zoom_group);
            children.push(button(
                ButtonStyle::Secondary,
                &tr("Zoom to area"),
                "reader:zoom:area",
                "zoom-area",
            ));
            children.push(button(
                ButtonStyle::Secondary,
                &tr("Reset"),
                "reader:zoom:reset",
                "zoom-reset",
            ));
        }
    }
    let title = if reflow.is_some() {
        tr("Text and layout")
    } else {
        tr("Zoom")
    };
    sheet(services, screen, Some(&title), children, Some(WRITING_PERCENT))
}

/// The font list, opened from the Font row of the Text and layout sheet. One row
/// per font choice, in the order of `font_choices`. The row of choice `current`
/// shows "Current". Tapping a row picks that face.
pub fn font_list(services: &Services, screen: Size, current: usize) -> Window {
    let labels: Vec<String> = font_choices().into_iter().map(|(label, _)| label).collect();
    let indices: Vec<usize> = (0..labels.len()).collect();
    let list = PagedList::new(indices, move |index: &usize, _t: &Theme| {
        let action = format!("reader:layout:font:{index}");
        let mut row = ListRow::new(labels[*index].clone()).on_action(action.clone());
        if *index == current {
            row = row.value(tr("Current"));
        }
        WidgetPod::new(row).with_key(key_of(&action))
    })
    .labels(page_labels());
    let children = vec![WidgetPod::new(Expanded::new(
        WidgetPod::new(list).with_key("font-list"),
        1,
    ))];
    sheet(
        services,
        screen,
        Some(&tr("Font")),
        children,
        Some(WRITING_PERCENT),
    )
}

/// The hyphenation language choices. Index 0 is the book language, which is the
/// `None` of the layout. Each later index is a language with a pattern, in the
/// order of `kaia_book::layout::HYPHENATION_LANGUAGES`.
fn hyphenation_choices() -> Vec<Option<&'static kaia_book::layout::HyphenationLanguage>> {
    std::iter::once(None)
        .chain(kaia_book::layout::HYPHENATION_LANGUAGES.iter().map(Some))
        .collect()
}

/// The index of a layout's hyphenation language in the list. The book language,
/// and a tag that names no pattern, take index 0.
pub fn hyphenation_index(choice: Option<&str>) -> usize {
    choice
        .and_then(kaia_book::layout::language_for_tag)
        .and_then(|lang| {
            kaia_book::layout::HYPHENATION_LANGUAGES
                .iter()
                .position(|candidate| candidate == lang)
        })
        .map_or(0, |i| i + 1)
}

/// The label of a hyphenation choice, as the row and the list show it.
pub fn hyphenation_choice_label(choice: Option<&str>) -> String {
    match choice.and_then(kaia_book::layout::language_for_tag) {
        Some(lang) => kaia_i18n::tr_known(lang.name),
        None => tr("Book language"),
    }
}

/// The language tag that a hyphenation list index saves. None is the book language.
pub fn hyphenation_tag_at(index: usize) -> Option<String> {
    index
        .checked_sub(1)
        .and_then(|i| kaia_book::layout::HYPHENATION_LANGUAGES.get(i))
        .map(|lang| lang.tag.to_string())
}

/// The hyphenation language list, opened from the row of the Text and layout
/// sheet. The row of choice `current` shows "Current".
pub fn hyphenation_list(services: &Services, screen: Size, current: usize) -> Window {
    let choices = hyphenation_choices();
    let indices: Vec<usize> = (0..choices.len()).collect();
    let list = PagedList::new(indices, move |index: &usize, _t: &Theme| {
        let label = match choices[*index] {
            Some(lang) => kaia_i18n::tr_known(lang.name),
            None => tr("Book language"),
        };
        let action = format!("reader:layout:hyphlang:{index}");
        let mut row = ListRow::new(label).on_action(action.clone());
        if *index == current {
            row = row.value(tr("Current"));
        }
        WidgetPod::new(row).with_key(key_of(&action))
    })
    .labels(page_labels());
    let children = vec![WidgetPod::new(Expanded::new(
        WidgetPod::new(list).with_key("hyphenation-list"),
        1,
    ))];
    sheet(
        services,
        screen,
        Some(&tr("Hyphenation language")),
        children,
        Some(WRITING_PERCENT),
    )
}

/// The More panel: go to either end and mark the book as finished.
pub fn more(services: &Services, screen: Size) -> Window {
    let children = vec![WidgetPod::new(
        Column::new(vec![
            button(
                ButtonStyle::Secondary,
                &tr("Ink mode"),
                "reader:ink:toggle",
                "more-ink",
            ),
            button(
                ButtonStyle::Secondary,
                &tr("Go to page"),
                "reader:goto:open",
                "more-goto",
            ),
            button(
                ButtonStyle::Secondary,
                &tr("Go to the start"),
                "reader:more:start",
                "more-start",
            ),
            button(
                ButtonStyle::Secondary,
                &tr("Go to the end"),
                "reader:more:end",
                "more-end",
            ),
            button(
                ButtonStyle::Secondary,
                &tr("Mark as finished"),
                "reader:finish",
                "more-finish",
            ),
            button(
                ButtonStyle::Secondary,
                &tr("Header and footer"),
                "reader:panel:footer",
                "more-footer",
            ),
            button(
                ButtonStyle::Secondary,
                &tr("Word list"),
                "reader:words:open",
                "more-words",
            ),
            button(
                ButtonStyle::Secondary,
                &tr("Plugins"),
                "reader:plugins",
                "more-plugins",
            ),
        ])
        .gap(services.theme.space.sm)
        .cross_align(CrossAlign::Stretch),
    )];
    sheet(services, screen, Some(&tr("More")), children, None)
}

/// Both groups sit side by side so all ten switches fit on the reader screen.
pub fn footer_sheet(
    services: &Services,
    screen: Size,
    header: &super::header::Header,
    footer: &Footer,
) -> Window {
    let mut header_rows = vec![group_heading(services, &tr("Header"))];
    for item in super::header::Item::ALL {
        let action = format!("reader:header:{}", item.id());
        let key = format!("header-{}", item.id());
        let toggle = WidgetPod::new(Toggle::pill(header.is_on(item)).on_action(&action)).with_key(&key);
        header_rows.push(status_switch(services, &item.label(), toggle));
    }
    let mut footer_rows = vec![group_heading(services, &tr("Footer"))];
    for item in Item::ALL {
        let action = format!("reader:footer:{}", item.id());
        let key = format!("footer-{}", item.id());
        let toggle = WidgetPod::new(Toggle::pill(footer.is_on(item)).on_action(&action)).with_key(&key);
        let label = match item {
            Item::TimeChapter => tr("Chapter time left"),
            Item::TimeBook => tr("Book time left"),
            _ => item.label(),
        };
        footer_rows.push(status_switch(services, &label, toggle));
    }
    let groups = [header_rows, footer_rows]
        .into_iter()
        .map(|rows| {
            WidgetPod::new(Expanded::new(
                WidgetPod::new(
                    Column::new(rows)
                        .gap(services.theme.space.md)
                        .cross_align(CrossAlign::Stretch)
                        .shrink(),
                ),
                1,
            ))
        })
        .collect();
    let groups = WidgetPod::new(Row::new(groups).gap(services.theme.space.lg));
    sheet(
        services,
        screen,
        Some(&tr("Header and footer")),
        vec![groups],
        None,
    )
}

/// Wrap long switch labels within their half of the sheet.
fn status_switch(services: &Services, label: &str, toggle: WidgetPod) -> WidgetPod {
    let t = &services.theme;
    WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(
                WidgetPod::new(Paragraph::new(label, t.font.body.clone(), Tone::Ink).max_lines(2)),
                1,
            )),
            toggle,
        ])
        .gap(t.space.md)
        .cross_align(CrossAlign::Center),
    )
}

/// A highlight color: a square, or a round dot when `round` is set. It is framed.
/// The selected one has a thick black frame. A tap sends its action when it has
/// one. The fill goes through the page's pixel format, so a grayscale panel shows
/// the luminance of the color.
struct Swatch {
    color: Color,
    selected: bool,
    action: Option<String>,
    round: bool,
}

impl Widget for Swatch {
    kaia_ui::impl_as_any!();

    fn layout(&mut self, cx: &mut LayoutCx, c: Constraints) -> kaia_gfx::Size {
        let height = cx.theme.touch_target;
        c.constrain(Size::new(c.max.w, height))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let t = cx.theme;
        let (width, frame) = if self.selected {
            (t.line.focus.max(4), Color::BLACK)
        } else {
            (t.line.hairline, Color::gray(0x66))
        };
        if self.round {
            // The ring sits inside the touch target, and the dot inside the ring.
            let bounds = cx.bounds();
            let center = Point::new(bounds.x + bounds.w as i32 / 2, bounds.y + bounds.h as i32 / 2);
            let outer = bounds.w.min(bounds.h) as f32 / 2.0 - 1.0;
            let ring = width as f32;
            cx.fill_circle(center, outer - ring, self.color);
            cx.stroke_circle(center, outer - ring / 2.0, ring, frame);
            return;
        }
        let inner = cx.bounds().inset(t.line.focus as i32);
        cx.fill_rect(inner, self.color);
        cx.border(inner, 0, width, frame);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        match (ev, &self.action) {
            (UiEvent::Gesture(Gesture::Tap { .. }), Some(id)) => {
                cx.action(AppAction {
                    id: id.clone(),
                    args: Vec::new(),
                });
                Handled::Yes
            }
            // An inert swatch keeps the tap, so the sheet does not close on it.
            (UiEvent::Gesture(Gesture::Tap { .. }), None) => Handled::Yes,
            _ => Handled::No,
        }
    }
}

/// The name of a highlight color, as KOReader writes it.
pub fn color_label(name: &str) -> String {
    match name {
        "red" => tr("Red"),
        "orange" => tr("Orange"),
        "yellow" => tr("Yellow"),
        "green" => tr("Green"),
        "olive" => tr("Olive"),
        "cyan" => tr("Cyan"),
        "blue" => tr("Blue"),
        "purple" => tr("Purple"),
        "gray" => tr("Gray"),
        other => other.to_string(),
    }
}

/// The name of a drawer style, as KOReader writes it.
pub fn style_label(drawer: &str) -> String {
    match drawer {
        "lighten" => tr("Lighten"),
        "underscore" => tr("Underline"),
        "strikeout" => tr("Strikethrough"),
        "invert" => tr("Invert"),
        other => other.to_string(),
    }
}

/// The color and style choices: a row of color swatches with their names, then a
/// row of drawer styles. `prefix` starts each action id, such as `reader:highlight`,
/// so the action for a color is `prefix:color:<name>`. Invert draws no color, so the
/// swatches are shown without actions while it is chosen.
fn style_rows(services: &Services, prefix: &str, drawer: &str, color: &str) -> Vec<WidgetPod> {
    let t = &services.theme;
    let swatches: Vec<WidgetPod> = kaia_book::HIGHLIGHT_COLORS
        .iter()
        .map(|name| {
            let rgb = kaia_book::highlight_rgb(name).unwrap_or([0, 0, 0]);
            let swatch = Swatch {
                color: Color::rgb(rgb[0], rgb[1], rgb[2]),
                selected: *name == color,
                action: (drawer != "invert").then(|| format!("{prefix}:color:{name}")),
                round: false,
            };
            WidgetPod::new(Expanded::new(
                WidgetPod::new(
                    Column::new(vec![
                        WidgetPod::new(swatch).with_key(format!("color-{name}")),
                        WidgetPod::new(Label::new(
                            color_label(name),
                            t.font.caption.clone(),
                            Tone::InkSecondary,
                        )),
                    ])
                    .shrink()
                    .gap(t.space.xs)
                    .cross_align(CrossAlign::Center),
                ),
                1,
            ))
        })
        .collect();
    let mut children = vec![
        group_heading(services, &tr("Color")),
        WidgetPod::new(Row::new(swatches).gap(t.space.xs)),
        group_heading(services, &tr("Style")),
    ];
    children.extend(drawer_rows(services, prefix, drawer));
    children
}

/// The four drawer styles in two rows of two, so the longest name, Strikethrough,
/// fits its button. The chosen drawer is primary. A tap sends `prefix:drawer:<name>`.
fn drawer_rows(services: &Services, prefix: &str, drawer: &str) -> Vec<WidgetPod> {
    let t = &services.theme;
    let styles: Vec<WidgetPod> = kaia_book::HIGHLIGHT_DRAWERS
        .iter()
        .map(|style| {
            let name = style.as_str();
            let look = if name == drawer {
                ButtonStyle::Primary
            } else {
                ButtonStyle::Secondary
            };
            WidgetPod::new(Expanded::new(
                button(
                    look,
                    &style_label(name),
                    &format!("{prefix}:drawer:{name}"),
                    &format!("style-{name}"),
                ),
                1,
            ))
        })
        .collect();
    let mut styles = styles.into_iter();
    let first: Vec<WidgetPod> = styles.by_ref().take(2).collect();
    let second: Vec<WidgetPod> = styles.collect();
    vec![
        WidgetPod::new(Row::new(first).gap(t.space.sm)),
        WidgetPod::new(Row::new(second).gap(t.space.sm)),
    ]
}

/// The sheet of an existing highlight: its text, and the color and style to change it to.
pub fn highlight_sheet(services: &Services, screen: Size, annotation: &Annotation) -> Window {
    let t = &services.theme;
    let text = annotation.text.clone().unwrap_or_default();
    let mut children = vec![WidgetPod::new(
        Paragraph::new(shorten(&text, 200), t.font.label.clone(), Tone::InkSecondary).max_lines(3),
    )];
    children.extend(style_rows(
        services,
        &format!("reader:annot:{}", annotation.id),
        annotation.drawer.as_deref().unwrap_or("lighten"),
        annotation.color.as_deref().unwrap_or("yellow"),
    ));
    sheet(services, screen, Some(&tr("Highlight")), children, None)
}

/// The selection popover on the bottom edge. `style` is the drawer and the color
/// of the saved choice.
pub fn popover(services: &Services, screen: Size, word: &str) -> Window {
    selection_popover(services, screen, word, false, ("lighten", "yellow"), Edge::Bottom)
}

/// The selection panel: the word, one row of color dots that highlight at once, and
/// the actions Note, Dictionary, Copy, Search and More. A grayscale panel has no
/// color dots, so it shows one Highlight button instead. `style` is the drawer and
/// the color of the saved choice.
pub(super) fn selection_popover(
    services: &Services,
    screen: Size,
    word: &str,
    moving: bool,
    style: (&str, &str),
    edge: Edge,
) -> Window {
    let t = &services.theme;
    if word.is_empty() {
        let mut children = vec![button(
            ButtonStyle::Primary,
            &tr("Sticker"),
            "reader:sel:sticker",
            "sel-sticker",
        )];
        if moving {
            children.push(button(
                ButtonStyle::Secondary,
                &tr("Cancel move"),
                "reader:sticker:cancel-move",
                "sticker-cancel-move",
            ));
        }
        return framed(
            services,
            screen,
            edge,
            t.space.lg,
            Some(&tr("Page sticker")),
            children,
            None,
        );
    }
    let shown = if word.chars().count() > 40 {
        let cut: String = word.chars().take(39).collect();
        format!("{cut}\u{2026}")
    } else {
        word.to_string()
    };
    let heading =
        WidgetPod::new(Label::new(shown, t.font.label.clone(), Tone::InkSecondary)).with_key("sel-word");
    let (drawer, color) = style;
    let mut children = vec![heading];
    // Invert ignores the color, so it gets the single Highlight button, as a
    // grayscale panel does. Dots without an action would leave nothing to tap.
    if t.color_panel && drawer != "invert" {
        children.push(color_dots(services, color));
    } else {
        children.push(button(
            ButtonStyle::Primary,
            &tr("Highlight"),
            "reader:sel:highlight",
            "sel-highlight",
        ));
    }
    // Dictionary is the longest label, so it takes a larger share of the row.
    let action = |label: &str, id: &str, key: &str, share: u32| {
        WidgetPod::new(Expanded::new(
            button(ButtonStyle::Secondary, label, id, key),
            share,
        ))
    };
    children.push(WidgetPod::new(
        Row::new(vec![
            action(&tr("Note"), "reader:sel:note", "sel-note", 2),
            action(&tr("Dictionary"), "reader:sel:dict", "sel-dict", 3),
            action(&tr("Copy"), "reader:sel:copy", "sel-copy", 2),
            action(&tr("Search"), "reader:sel:search", "sel-search", 2),
            action(&tr("More"), "reader:sel:more", "sel-more", 2),
        ])
        .gap(t.space.sm),
    ));
    if moving {
        children.push(button(
            ButtonStyle::Secondary,
            &tr("Cancel move"),
            "reader:sticker:cancel-move",
            "sticker-cancel-move",
        ));
    }
    framed(services, screen, edge, t.space.md, None, children, None)
}

/// One row of the nine highlight colors as dots. A tap highlights the selection in
/// that color. The saved color is marked.
fn color_dots(services: &Services, color: &str) -> WidgetPod {
    let dots: Vec<WidgetPod> = kaia_book::HIGHLIGHT_COLORS
        .iter()
        .map(|name| {
            let rgb = kaia_book::highlight_rgb(name).unwrap_or([0, 0, 0]);
            let dot = Swatch {
                color: Color::rgb(rgb[0], rgb[1], rgb[2]),
                selected: *name == color,
                action: Some(format!("reader:sel:color:{name}")),
                round: true,
            };
            WidgetPod::new(Expanded::new(
                WidgetPod::new(dot).with_key(format!("color-{name}")),
                1,
            ))
        })
        .collect();
    WidgetPod::new(Row::new(dots).gap(services.theme.space.xs))
}

/// The More sheet of a selection: the drawer styles with the saved one marked, the
/// shade of box highlights as a percent, then Sticker and Share to Notes. Choosing a
/// style returns to the selection panel. The shade stepper redraws the page.
pub(super) fn selection_more(
    services: &Services,
    screen: Size,
    edge: Edge,
    drawer: &str,
    shade: f64,
) -> Window {
    let t = &services.theme;
    let mut children = vec![group_heading(services, &tr("Style"))];
    children.extend(drawer_rows(services, "reader:highlight", drawer));
    let shade = WidgetPod::new(
        Stepper::new(shade * 100.0, 0.0, 100.0, 10.0)
            .format(|v| format!("{}%", v.round() as i64))
            .on_action("reader:sel:shade"),
    )
    .with_key("sel-shade");
    children.push(labelled(services, &tr("Shade"), shade));
    children.push(WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(
                button(
                    ButtonStyle::Secondary,
                    &tr("Sticker"),
                    "reader:sel:sticker",
                    "sel-sticker",
                ),
                1,
            )),
            WidgetPod::new(Expanded::new(
                button(
                    ButtonStyle::Secondary,
                    &tr("Share to Notes"),
                    "reader:sel:share",
                    "sel-share",
                ),
                1,
            )),
        ])
        .gap(t.space.sm),
    ));
    framed(
        services,
        screen,
        edge,
        t.space.md,
        Some(&tr("More")),
        children,
        None,
    )
}

/// The dictionary sheet without the word actions: the word and its definitions, or
/// the install instructions.
pub fn dictionary(services: &Services, screen: Size, word: &str, text: &str) -> Window {
    dictionary_sheet(services, screen, word, text, None)
}

/// What the dictionary sheet offers beyond the definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictActions<'a> {
    /// The dictionary of the first definition, if the lookup found one.
    pub first: Option<&'a str>,
    /// True when the book already uses that dictionary first.
    pub preferred: bool,
}

/// The dictionary sheet: the word and its definitions, or the install instructions.
/// With `actions`, it also offers the word list, the dictionary settings and the
/// dictionary to use first for the book.
pub fn dictionary_sheet(
    services: &Services,
    screen: Size,
    word: &str,
    text: &str,
    actions: Option<DictActions<'_>>,
) -> Window {
    let t = &services.theme;
    let width = screen
        .w
        .saturating_sub(2 * t.space.lg + 2 * t.line.hairline)
        .max(1);
    let style = t.style(&t.font.body, Tone::Ink);
    let layout = services
        .assets
        .borrow_mut()
        .fonts
        .layout(text, &style, width, None, false);
    let heading = WidgetPod::new(Label::new(word.to_string(), t.font.title.clone(), Tone::Ink));
    let extra = dictionary_actions(services, actions);
    if layout.height < screen.h / 3 {
        let mut children = vec![
            heading,
            WidgetPod::new(
                Paragraph::new(text.to_string(), t.font.body.clone(), Tone::Ink).max_lines(usize::MAX),
            )
            .with_key("dict-body"),
        ];
        children.extend(extra);
        return sheet(
            services,
            screen,
            Some(&tr("Dictionary")),
            dictionary_plugins(services, children),
            None,
        );
    }
    let lines = dictionary_lines(services, text, width);
    let list = PagedList::new(lines, |line: &String, t| {
        WidgetPod::new(
            Paragraph::new(line.trim_end_matches('\n'), t.font.body.clone(), Tone::Ink).max_lines(usize::MAX),
        )
    });
    let mut children = vec![
        heading,
        WidgetPod::new(Expanded::new(WidgetPod::new(list).with_key("dict-pages"), 1)),
    ];
    children.extend(extra);
    sheet(
        services,
        screen,
        Some(&tr("Dictionary")),
        dictionary_plugins(services, children),
        Some(TALL_PERCENT),
    )
}

/// The word actions of the dictionary sheet: the word list and the dictionary
/// settings, then the book's first dictionary when the lookup found one.
fn dictionary_actions(services: &Services, actions: Option<DictActions<'_>>) -> Vec<WidgetPod> {
    let Some(actions) = actions else {
        return Vec::new();
    };
    let t = &services.theme;
    let mut rows = vec![WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(Expanded::new(
                button(
                    ButtonStyle::Secondary,
                    &tr("Add to word list"),
                    "reader:words:add",
                    "dict-add-word",
                ),
                1,
            )),
            WidgetPod::new(Expanded::new(
                button(
                    ButtonStyle::Secondary,
                    &tr("Dictionary settings"),
                    "reader:dicts:open",
                    "dict-settings",
                ),
                1,
            )),
        ])
        .gap(t.space.sm),
    )];
    if actions.first.is_some() {
        let label = if actions.preferred {
            tr("Stop using this dictionary first for this book")
        } else {
            tr("Use this dictionary first for this book")
        };
        rows.push(button(
            ButtonStyle::Secondary,
            &label,
            "reader:dict:first",
            "dict-first",
        ));
    }
    rows
}

/// One installed dictionary on the Dictionary settings screen. `index` is its place
/// in the lookup order, which the actions use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DictRow {
    pub index: usize,
    pub name: String,
    pub enabled: bool,
}

/// The Dictionary settings screen: the installed dictionaries in lookup order. Each
/// row has an on or off switch and move up and move down buttons. `empty` is shown
/// when no dictionary is installed.
pub fn dictionary_settings(services: &Services, screen: Size, rows: Vec<DictRow>, empty: &str) -> Window {
    let t = &services.theme;
    if rows.is_empty() {
        return sheet(
            services,
            screen,
            Some(&tr("Dictionary settings")),
            vec![WidgetPod::new(Paragraph::new(
                empty.to_string(),
                t.font.body.clone(),
                Tone::InkSecondary,
            ))],
            None,
        );
    }
    let list = PagedList::new(rows, |row: &DictRow, t: &Theme| {
        let toggle =
            WidgetPod::new(Toggle::pill(row.enabled).on_action(format!("reader:dicts:toggle:{}", row.index)))
                .with_key(format!("dict-toggle-{}", row.index));
        let name = WidgetPod::new(Expanded::new(
            WidgetPod::new(Label::new(row.name.clone(), t.font.body.clone(), Tone::Ink)),
            1,
        ));
        let up = WidgetPod::new(Expanded::new(
            button(
                ButtonStyle::Secondary,
                &tr("Move up"),
                &format!("reader:dicts:up:{}", row.index),
                &format!("dict-up-{}", row.index),
            ),
            1,
        ));
        let down = WidgetPod::new(Expanded::new(
            button(
                ButtonStyle::Secondary,
                &tr("Move down"),
                &format!("reader:dicts:down:{}", row.index),
                &format!("dict-down-{}", row.index),
            ),
            1,
        ));
        WidgetPod::new(
            Column::new(vec![
                WidgetPod::new(
                    Row::new(vec![name, toggle])
                        .gap(t.space.md)
                        .cross_align(CrossAlign::Center),
                ),
                WidgetPod::new(Row::new(vec![up, down]).gap(t.space.sm)),
            ])
            .shrink()
            .gap(t.space.sm)
            .cross_align(CrossAlign::Stretch),
        )
        .with_key(format!("dict-row-{}", row.index))
    })
    .labels(page_labels());
    let children = vec![
        WidgetPod::new(Paragraph::new(
            tr("Dictionaries are searched in this order. A dictionary that is off is skipped."),
            t.font.caption.clone(),
            Tone::InkSecondary,
        )),
        WidgetPod::new(Expanded::new(
            WidgetPod::new(list).with_key("dict-settings-list"),
            1,
        )),
    ];
    sheet(
        services,
        screen,
        Some(&tr("Dictionary settings")),
        children,
        Some(TALL_PERCENT),
    )
}

/// One saved word on the word list. `detail` is the book and the sentence, and
/// `date` is the local date it was last saved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordRow {
    pub id: i64,
    pub word: String,
    pub detail: String,
    pub date: String,
}

/// The word list, newest first. Each row has a Delete button. `rows` is empty when
/// no word is saved yet.
pub fn words(services: &Services, screen: Size, rows: Vec<WordRow>) -> Window {
    let t = &services.theme;
    if rows.is_empty() {
        return sheet(
            services,
            screen,
            Some(&tr("Word list")),
            vec![WidgetPod::new(Paragraph::new(
                tr("No words yet. Hold a word in a book, then choose Dictionary and Add to word list."),
                t.font.body.clone(),
                Tone::InkSecondary,
            ))],
            None,
        );
    }
    let list = PagedList::new(rows, |row: &WordRow, t: &Theme| {
        let mut item = ListRow::new(row.word.clone()).value(row.date.clone());
        if !row.detail.is_empty() {
            item = item.secondary(row.detail.clone());
        }
        let delete = button(
            ButtonStyle::Secondary,
            &tr("Delete"),
            &format!("reader:words:delete:{}", row.id),
            &format!("word-delete-{}", row.id),
        );
        WidgetPod::new(
            Row::new(vec![
                WidgetPod::new(Expanded::new(WidgetPod::new(item), 1)),
                delete,
            ])
            .gap(t.space.sm)
            .cross_align(CrossAlign::Center),
        )
        .with_key(format!("word-row-{}", row.id))
    })
    .labels(page_labels());
    let children = vec![WidgetPod::new(Expanded::new(
        WidgetPod::new(list).with_key("words-list"),
        1,
    ))];
    sheet(
        services,
        screen,
        Some(&tr("Word list")),
        children,
        Some(TALL_PERCENT),
    )
}

/// Split at measured line widths. Every byte remains reachable through the pager.
fn dictionary_lines(services: &Services, text: &str, width: u32) -> Vec<String> {
    let style = services.theme.style(&services.theme.font.body, Tone::Ink);
    let mut assets = services.assets.borrow_mut();
    let mut lines = Vec::new();
    for paragraph in text.split_inclusive('\n') {
        let mut rest = paragraph;
        while !rest.is_empty() {
            let ends: Vec<_> = rest
                .char_indices()
                .skip(1)
                .map(|(i, _)| i)
                .chain(std::iter::once(rest.len()))
                .collect();
            let mut low = 1;
            let mut high = ends.len();
            while low < high {
                let mid = (low + high).div_ceil(2);
                let candidate = rest[..ends[mid - 1]].trim_end_matches('\n');
                if assets
                    .fonts
                    .layout(candidate, &style, width, None, false)
                    .lines
                    .len()
                    <= 1
                {
                    low = mid;
                } else {
                    high = mid - 1;
                }
            }
            let mut end = ends[low - 1];
            if end < rest.len() {
                if let Some((at, ch)) = rest[..end]
                    .char_indices()
                    .rev()
                    .find(|(_, ch)| ch.is_whitespace())
                {
                    if at > 0 {
                        end = at + ch.len_utf8();
                    }
                }
            }
            lines.push(rest[..end].to_owned());
            rest = &rest[end..];
        }
    }
    lines
}

/// The search panel: a text field with a Search button, the status, the results
/// and the on-screen keyboard.
pub fn search(services: &Services, screen: Size, text: &str, rows: Vec<SearchRow>, status: &str) -> Window {
    let t = &services.theme;
    let field = WidgetPod::new(
        TextField::new()
            .with_text(text.to_string())
            .placeholder(tr("Search this book")),
    )
    .with_key("search-field")
    .focusable(true);
    let go = button(
        ButtonStyle::Primary,
        &tr("Search"),
        "reader:search:go",
        "search-go",
    );
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
        Some(&tr("Search this book")),
        children,
        Some(WRITING_PERCENT),
    )
}

/// The note editor for a selection: a text field and the keyboard.
pub fn note(services: &Services, screen: Size, word: &str) -> Window {
    let t = &services.theme;
    let field = WidgetPod::new(TextField::new().placeholder(tr("Note")))
        .with_key("note-field")
        .focusable(true);
    let save = button(ButtonStyle::Primary, &tr("Save"), "reader:note:save", "note-save");
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
    sheet(
        services,
        screen,
        Some(&tr("Note")),
        children,
        Some(WRITING_PERCENT),
    )
}

/// The end of book sheet: finish, keep reading, or close the book.
pub fn end_of_book(services: &Services, screen: Size) -> Window {
    let t = &services.theme;
    let children = vec![
        WidgetPod::new(Paragraph::new(
            tr("You've reached the end of the document.\nWhat would you like to do?"),
            t.font.body.clone(),
            Tone::Ink,
        )),
        button(
            ButtonStyle::Primary,
            &tr("Mark as finished"),
            "reader:finish",
            "end-finish",
        ),
        button(
            ButtonStyle::Secondary,
            &tr("Close book"),
            "nav:library",
            "end-library",
        ),
        button(
            ButtonStyle::Secondary,
            &tr("Keep reading"),
            "reader:panel:close",
            "end-keep",
        ),
    ];
    sheet(services, screen, Some(&tr("End of book")), children, None)
}

/// The rows of the Contents panel for bookmarks or highlights.
pub fn annotation_items(annotations: &[Annotation], bookmarks: bool) -> Vec<ContentItem> {
    annotations
        .iter()
        .map(|a| {
            let label = if bookmarks {
                a.chapter.clone().unwrap_or_else(|| tr("Bookmark"))
            } else {
                match a.text.as_deref() {
                    Some(text) if !text.trim().is_empty() => shorten(text, 80),
                    _ => tr("Highlight"),
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
            label: tr_format(&tr("Page %1"), &[&(r.page + 1).to_string()]),
            snippet: r.snippet.clone(),
            action: format!("reader:search:hit:{}", r.index),
        })
        .collect()
}

/// Opens a panel as a surface, with the serial the reader uses to find it.
pub fn panel_surface(window: Window, serial: u64, core: Rc<RefCell<Core>>) -> Box<dyn Surface> {
    Box::new(Panel::new(window, serial, core))
}

fn dictionary_plugins(services: &Services, children: Vec<WidgetPod>) -> Vec<WidgetPod> {
    #[cfg(feature = "lua")]
    if services.plugins.running() {
        let mut children = children;
        children.push(button(
            ButtonStyle::Secondary,
            &tr("Plugin actions"),
            "plugins:dictionary",
            "dictionary-plugins",
        ));
        return children;
    }
    let _ = services;
    children
}

/// Bundled image stickers and a margin note.
pub fn sticker_picker(services: &Services, screen: Size) -> Window {
    let t = &services.theme;
    let choices = super::stickers::BUNDLED
        .iter()
        .map(|(name, label)| {
            let mut children = Vec::new();
            if let Some(path) = super::stickers::bundled_path(services, name) {
                if let Some(image) = sticker_image(&path, t.scale.dp(70)) {
                    children.push(image);
                }
            }
            children.push(button(
                ButtonStyle::Secondary,
                &sticker_label(label),
                &format!("reader:sticker:add:{name}"),
                &format!("sticker-pick-{name}"),
            ));
            WidgetPod::new(Expanded::new(
                WidgetPod::new(Column::new(children).shrink().gap(t.space.sm)),
                1,
            ))
        })
        .collect();
    sheet(
        services,
        screen,
        Some(&tr("Choose a sticker")),
        vec![
            WidgetPod::new(Row::new(choices).gap(t.space.sm)),
            button(
                ButtonStyle::Secondary,
                &tr("Text note"),
                "reader:sticker:text",
                "sticker-text",
            ),
        ],
        None,
    )
}

fn sticker_label(label: &str) -> String {
    match label {
        "Star" => tr("Star"),
        "Heart" => tr("Heart"),
        "Question" => tr("Question"),
        "Check" => tr("Check"),
        other => other.to_string(),
    }
}

fn sticker_image(path: &std::path::Path, height: u32) -> Option<WidgetPod> {
    let bytes = std::fs::read(path).ok()?;
    let image = kaia_gfx::image::decode_rgb(&bytes, kaia_gfx::image::DecodeLimits::default()).ok()?;
    Some(WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(kaia_ui::components::Image::new(
            image,
            kaia_gfx::image::FitMode::Contain,
        ))))
        .height(height),
    ))
}

pub fn sticker_sheet(services: &Services, screen: Size, sticker: &kaia_storage::Sticker) -> Window {
    let id = sticker.id.as_deref().unwrap_or("");
    let mut children = vec![button(
        ButtonStyle::Secondary,
        &tr("View"),
        &format!("reader:sticker:view:{id}"),
        "sticker-view",
    )];
    if sticker.asset.is_none() {
        children.push(button(
            ButtonStyle::Secondary,
            &tr("Edit"),
            &format!("reader:sticker:edit:{id}"),
            "sticker-edit",
        ));
    }
    children.push(button(
        ButtonStyle::Secondary,
        &tr("Move to another place"),
        &format!("reader:sticker:move:{id}"),
        "sticker-move",
    ));
    children.push(button(
        ButtonStyle::Secondary,
        &tr("Delete"),
        &format!("reader:sticker:delete:{id}"),
        "sticker-delete",
    ));
    sheet(services, screen, Some(&tr("Sticker")), children, None)
}

pub fn sticker_view(
    services: &Services,
    screen: Size,
    sticker: &kaia_storage::Sticker,
    path: Option<&std::path::Path>,
) -> Window {
    let mut children = Vec::new();
    if let Some(image) = path.and_then(|p| sticker_image(p, screen.h / 4)) {
        children.push(image);
    }
    if let Some(text) = &sticker.text {
        let lines = dictionary_lines(
            services,
            text,
            screen.w.saturating_sub(2 * services.theme.space.lg),
        );
        let list = PagedList::new(lines, |line: &String, t| {
            WidgetPod::new(Paragraph::new(line, t.font.body.clone(), Tone::Ink).max_lines(usize::MAX))
        });
        children.push(WidgetPod::new(Expanded::new(
            WidgetPod::new(list).with_key("sticker-view-pages"),
            1,
        )));
    }
    sheet(services, screen, Some(&tr("Sticker")), children, Some(60))
}

pub fn sticker_note(services: &Services, screen: Size, text: &str) -> Window {
    let mut field = TextField::new().placeholder(tr("Text note"));
    field.set_text(text);
    let children = vec![
        WidgetPod::new(field)
            .with_key("sticker-note-field")
            .focusable(true),
        button(
            ButtonStyle::Primary,
            &tr("Save"),
            "reader:sticker:save",
            "sticker-note-save",
        ),
        WidgetPod::new(OnScreenKeyboard::new(&ENGLISH)),
    ];
    sheet(
        services,
        screen,
        Some(&tr("Text note")),
        children,
        Some(WRITING_PERCENT),
    )
}

/// Page or percent entry, using KOReader's two explicit actions.
pub fn go_to(services: &Services, screen: Size) -> Window {
    sheet(
        services,
        screen,
        Some(&tr("Enter page number or percentage")),
        vec![
            WidgetPod::new(TextField::new().placeholder(tr("Enter page number or percentage")))
                .with_key("goto-field")
                .focusable(true),
            button(
                ButtonStyle::Primary,
                &tr("Go to page"),
                "reader:goto:page",
                "goto-page",
            ),
            button(
                ButtonStyle::Secondary,
                &tr("Go to %"),
                "reader:goto:percent",
                "goto-percent",
            ),
            WidgetPod::new(OnScreenKeyboard::new(&ENGLISH)),
        ],
        Some(WRITING_PERCENT),
    )
}

/// Notes and addresses use a paged sheet so all of the text remains reachable.
pub fn link_text(services: &Services, screen: Size, title: &str, text: &str) -> Window {
    let width = screen
        .w
        .saturating_sub(2 * services.theme.space.lg + 2 * services.theme.line.hairline)
        .max(1);
    let lines = dictionary_lines(services, text, width);
    let list = PagedList::new(lines, |line: &String, t| {
        WidgetPod::new(
            Paragraph::new(line.trim_end_matches('\n'), t.font.body.clone(), Tone::Ink).max_lines(usize::MAX),
        )
    })
    .labels(page_labels());
    sheet(
        services,
        screen,
        Some(title),
        vec![WidgetPod::new(Expanded::new(
            WidgetPod::new(list).with_key("link-text"),
            1,
        ))],
        Some(TALL_PERCENT),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        choices_with, hyphenation_choice_label, hyphenation_index, hyphenation_tag_at, FONT_CHOICES,
    };
    use crate::surfaces::settings::commands::font_options_with;
    use kaia_i18n::tr;

    /// The Text sheet and the Settings radio list offer the same faces, in the same order.
    #[test]
    fn reader_font_choices_match_the_settings_options() {
        let user = ["Zeta Mono", "Atkinson Hyperlegible", "alpha sans"].map(str::to_string);
        let reader: Vec<String> = choices_with(&user)
            .into_iter()
            .filter_map(|(_, face)| face)
            .collect();
        let settings: Vec<String> = font_options_with(&user)
            .into_iter()
            .map(|(value, _)| value)
            .filter(|value| !value.is_empty())
            .collect();
        assert_eq!(reader, settings);
        assert_eq!(
            reader,
            [
                "Noto Serif",
                "Noto Sans",
                "Atkinson Hyperlegible",
                "OpenDyslexic",
                "alpha sans",
                "Zeta Mono",
            ]
        );
    }

    #[test]
    fn book_font_comes_first_and_built_in_faces_keep_their_places() {
        let choices = choices_with(&["Zeta Mono".to_string()]);
        assert_eq!(choices.len(), FONT_CHOICES.len() + 1);
        assert_eq!(choices[0].1, None);
        assert_eq!(choices[FONT_CHOICES.len()].1.as_deref(), Some("Zeta Mono"));
    }

    #[test]
    fn hyphenation_list_index_zero_is_the_book_language_and_round_trips() {
        assert_eq!(hyphenation_index(None), 0);
        assert_eq!(hyphenation_tag_at(0), None);
        let spanish = hyphenation_index(Some("es"));
        assert!(spanish > 0);
        assert_eq!(hyphenation_tag_at(spanish).as_deref(), Some("es"));
        assert_eq!(hyphenation_index(Some("xx-unknown")), 0);
        assert_eq!(
            hyphenation_index(Some("ja")),
            0,
            "no pattern, so the book language"
        );
        assert_eq!(hyphenation_tag_at(usize::MAX), None);
    }

    #[test]
    fn hyphenation_choice_labels_name_the_language_or_the_book() {
        assert_eq!(hyphenation_choice_label(None), tr("Book language"));
        assert_eq!(hyphenation_choice_label(Some("xx-unknown")), tr("Book language"));
        assert_eq!(hyphenation_choice_label(Some("fr")), tr("French"));
    }
}
