//! Commands of the reader surface. Ids are `reader:<action>[:<arg>]`. Numbers
//! that a control sends (a slider value, a stepper value, a radio index) come in
//! the action args. Taps and holds send their position in the args too.

use kaia_events::Value;
use kaia_gfx::Size;
use kaia_input::Axis;

use super::footer::Item;

/// Gesture updates bypass the reader's queued document commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum GestureCommand {
    Brightness { delta: f64, release: bool },
    Flip(u8),
    FlipStop,
}

pub(super) fn gesture(id: &str, args: &[Value]) -> Option<GestureCommand> {
    match id {
        "reader:brightness" => {
            let delta = number(args, 0).filter(|n| n.is_finite())?;
            Some(GestureCommand::Brightness {
                delta,
                release: args.get(1).and_then(Value::as_bool) == Some(true),
            })
        }
        "reader:flip" => {
            let zone = number(args, 0)?;
            (zone.is_finite() && (0.0..=4.0).contains(&zone) && zone.fract() == 0.0)
                .then_some(GestureCommand::Flip(zone as u8))
        }
        "reader:flip:stop" => Some(GestureCommand::FlipStop),
        _ => None,
    }
}

/// One change from the Text and layout sheet, with the value the control sent.
/// `Font` carries the index of the face in the font list, and `Zoom` the index of
/// the radio choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayoutChange {
    Font(usize),
    /// The index of the language in the hyphenation list. 0 is the book language.
    HyphenationLang(usize),
    Size(f64),
    /// The index of the weight choice in `kaia_book::layout::FONT_WEIGHT_VALUES`.
    Weight(f64),
    Spacing(f64),
    Margins(f64),
    Justify(bool),
    Hyphenation(bool),
    Styles(bool),
    Zoom(usize),
}

/// A panel the reader opens above itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelName {
    Contents,
    Layout,
    More,
    /// The Footer sheet, where the footer items are turned on or off.
    Footer,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// A tap on the page, in pixels from the top left of the screen.
    Tap(f64, f64),
    /// Turn to the next page: page keys, taps on the right, swipes.
    Next,
    /// Turn to the previous page.
    Prev,
    Jump(bool),
    SelSearchMenu,
    /// Hold starts a word selection at a point.
    Hold(f64, f64),
    /// A drag while held extends the selection to a point.
    Drag(f64, f64),
    /// The hold ends. A selection opens the popover.
    Release,
    /// Opens the overlay with the bars.
    Overlay,
    /// Adds or removes the bookmark on the current page.
    Bookmark,
    /// Turns a footer item on or off. The flag is the new state.
    Footer(Item, bool),
    /// Turns a header item on or off.
    Header(super::header::Item, bool),
    /// The page slider moved to a page number (1-based).
    Slider(f64),
    /// Opens the search panel.
    SearchOpen,
    /// Starts the search with the text in the search field.
    SearchGo,
    /// Jumps to a search hit by its index.
    SearchHit(usize),
    /// Runs a recent search of the book by its index in the list.
    SearchRecent(usize),
    /// Empties the recent searches of the book.
    SearchClearHistory,
    /// Opens a panel above the overlay.
    Panel(PanelName),
    /// Closes the top panel.
    PanelClose,
    /// Switches the tab of the Contents panel (0 chapters, 1 bookmarks, 2 highlights).
    ContentsTab(usize),
    /// Jumps to a table of contents entry by its index.
    ContentsGoto(usize),
    /// Jumps to an annotation by its id.
    ContentsAnnotation(u64),
    /// A change from the Text and layout sheet.
    Layout(LayoutChange),
    /// Opens the font list over the Text and layout sheet.
    FontList,
    /// Opens the hyphenation language list over the Text and layout sheet.
    HyphenationList,
    /// A two-finger pinch (`larger` false) or spread (`larger` true). `distance` is
    /// the sum of both fingers' travel in pixels, as KOReader's `ges.distance`.
    Pinch {
        larger: bool,
        distance: f64,
        axis: Option<Axis>,
        view: Option<(f32, kaia_gfx::Point, kaia_gfx::Point)>,
    },
    ZoomArea,
    ViewDrag {
        start: kaia_gfx::Point,
        end: kaia_gfx::Point,
        area: bool,
    },
    /// Highlights the selection.
    SelHighlight,
    /// Highlights the selection in this color and saves the color as the last choice.
    SelColor(String),
    /// Opens the More sheet of the selection.
    SelMore,
    /// Saves the highlight shade from the stepper of the More sheet, in percent.
    SelShade(f64),
    /// Opens the note editor for the selection.
    SelNote,
    /// Copies the selected text.
    SelCopy,
    /// Looks up the selected word in the dictionaries.
    SelDict,
    /// Searches the book for the selected text.
    SelSearch,
    /// Saves the note in the note editor as a highlight.
    NoteSave,
    /// The More panel: go to the start.
    MoreStart,
    /// The More panel: go to the end.
    MoreEnd,
    /// The More panel and the end of book sheet: mark the book as finished.
    Finish,
    Back,
    GotoOpen,
    GotoPage,
    GotoPercent,
}

fn number(args: &[Value], index: usize) -> Option<f64> {
    match args.get(index) {
        Some(Value::Number(n)) => Some(*n),
        _ => None,
    }
}

fn position(args: &[Value]) -> (f64, f64) {
    (number(args, 0).unwrap_or(0.0), number(args, 1).unwrap_or(0.0))
}

/// The axis a pinch or spread sends as its name. An empty name means no axis.
fn axis(args: &[Value]) -> Option<Axis> {
    match args.get(1) {
        Some(Value::Str(name)) => match name.as_str() {
            "horizontal" => Some(Axis::Horizontal),
            "vertical" => Some(Axis::Vertical),
            "diagonal" => Some(Axis::Diagonal),
            _ => None,
        },
        _ => None,
    }
}

fn pinch(args: &[Value], larger: bool) -> Option<Command> {
    let distance = number(args, 0).filter(|n| n.is_finite() && *n >= 0.0)?;
    Some(Command::Pinch {
        larger,
        distance,
        axis: axis(args),
        view: None,
    })
}

fn flag(args: &[Value]) -> bool {
    matches!(args.first(), Some(Value::Bool(true)))
}

fn index(args: &[Value]) -> usize {
    number(args, 0).unwrap_or(0.0).max(0.0) as usize
}

/// KOReader's `ReaderFont.steps`, in its 1-based order: the size change for each
/// step of a gesture. Lua indexes from 1, so step `n` is entry `n - 1` here.
const PINCH_STEPS: [f64; 15] = [
    0.0, 1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 4.0, 4.0, 5.0,
];

/// The size change in KOReader units for a pinch or spread. This copies
/// `ReaderFont:gesToFontSize`. The step is `ceil(2 * 15 * distance / length)`, where
/// the length is the screen side for the gesture's axis, or the shorter side when
/// the axis is unknown. A step outside 1 to 15 gives the last entry, 5, as Lua's
/// `steps[step] or steps[#steps]` does. So a distance of zero gives 5.
pub(super) fn pinch_step(distance: f64, axis: Option<Axis>, screen: Size) -> f64 {
    let (w, h) = (f64::from(screen.w), f64::from(screen.h));
    let length = match axis {
        Some(Axis::Vertical) => h,
        Some(Axis::Horizontal) => w,
        Some(Axis::Diagonal) => w.hypot(h),
        None => w.min(h),
    };
    let count = PINCH_STEPS.len();
    let step = (2.0 * count as f64 * distance / length).ceil();
    if (1.0..=count as f64).contains(&step) {
        PINCH_STEPS[step as usize - 1]
    } else {
        PINCH_STEPS[count - 1]
    }
}

/// The whole KOReader units that give `px` pixels, or the exact units when no
/// whole value does. The steps move in whole units, as KOReader's sizes do.
pub(super) fn whole_units(screen: Size, px: f32) -> f64 {
    let exact = f64::from(px) * 600.0 / f64::from(screen.w.min(screen.h).max(1));
    [exact.round(), exact.floor(), exact.ceil()]
        .into_iter()
        .find(|units| super::koreader_px(screen, *units) == px)
        .unwrap_or(exact)
}

/// Parses a reader action id. None for every id outside the reader's namespace.
pub fn parse(id: &str, args: &[Value]) -> Option<Command> {
    let rest = id.strip_prefix("reader:")?;
    let parts: Vec<&str> = rest.split(':').collect();
    let command = match parts.as_slice() {
        ["tap"] => {
            let (x, y) = position(args);
            Command::Tap(x, y)
        }
        ["zoom", "area"] => Command::ZoomArea,
        ["zoom", "reset"] => Command::Layout(LayoutChange::Zoom(0)),
        ["zoom", "pinch"] => {
            let ratio = number(args, 0)? as f32;
            if !ratio.is_finite() || ratio <= 0.0 {
                return None;
            }
            let point = |i| {
                Some(kaia_gfx::Point::new(
                    number(args, i)? as i32,
                    number(args, i + 1)? as i32,
                ))
            };
            Command::Pinch {
                larger: ratio > 1.0,
                distance: 0.0,
                axis: None,
                view: Some((ratio, point(1)?, point(3)?)),
            }
        }
        ["zoom", "drag"] => {
            let point = |i| {
                Some(kaia_gfx::Point::new(
                    number(args, i)? as i32,
                    number(args, i + 1)? as i32,
                ))
            };
            Command::ViewDrag {
                start: point(0)?,
                end: point(2)?,
                area: args.get(4).and_then(Value::as_bool) == Some(true),
            }
        }
        ["next"] => Command::Next,
        ["prev"] => Command::Prev,
        ["hold"] => {
            let (x, y) = position(args);
            Command::Hold(x, y)
        }
        ["drag"] => {
            let (x, y) = position(args);
            Command::Drag(x, y)
        }
        ["release"] => Command::Release,
        ["overlay"] => Command::Overlay,
        ["bookmark"] => Command::Bookmark,
        ["slider"] => Command::Slider(number(args, 0)?),
        ["search", "open"] => Command::SearchOpen,
        ["search", "go"] => Command::SearchGo,
        ["search", "hit", n] => Command::SearchHit(n.parse().ok()?),
        ["search", "recent", n] => Command::SearchRecent(n.parse().ok()?),
        ["search", "history", "clear"] => Command::SearchClearHistory,
        ["jump", "previous"] => Command::Jump(false),
        ["jump", "next"] => Command::Jump(true),
        ["sel", "search-menu"] => Command::SelSearchMenu,
        ["panel", "contents"] => Command::Panel(PanelName::Contents),
        ["panel", "layout"] => Command::Panel(PanelName::Layout),
        ["panel", "more"] => Command::Panel(PanelName::More),
        ["panel", "footer"] => Command::Panel(PanelName::Footer),
        ["footer", id] => Command::Footer(Item::from_id(id)?, flag(args)),
        ["header", id] => Command::Header(super::header::Item::from_id(id)?, flag(args)),
        ["panel", "close"] => Command::PanelClose,
        ["contents", "tab", n] => Command::ContentsTab(n.parse().ok()?),
        ["contents", "goto", n] => Command::ContentsGoto(n.parse().ok()?),
        ["contents", "annot", n] => Command::ContentsAnnotation(n.parse().ok()?),
        ["layout", "fonts"] => Command::FontList,
        ["layout", "font", n] => Command::Layout(LayoutChange::Font(n.parse().ok()?)),
        ["layout", "hyphlang"] => Command::HyphenationList,
        ["layout", "hyphlang", n] => Command::Layout(LayoutChange::HyphenationLang(n.parse().ok()?)),
        ["layout", "size"] => Command::Layout(LayoutChange::Size(number(args, 0)?)),
        ["layout", "weight"] => Command::Layout(LayoutChange::Weight(number(args, 0)?)),
        ["layout", "spacing"] => Command::Layout(LayoutChange::Spacing(number(args, 0)?)),
        ["layout", "margins"] => Command::Layout(LayoutChange::Margins(number(args, 0)?)),
        ["layout", "justify"] => Command::Layout(LayoutChange::Justify(flag(args))),
        ["layout", "hyphenation"] => Command::Layout(LayoutChange::Hyphenation(flag(args))),
        ["layout", "styles"] => Command::Layout(LayoutChange::Styles(flag(args))),
        ["layout", "zoom"] => Command::Layout(LayoutChange::Zoom(index(args))),
        ["size", "smaller"] => pinch(args, false)?,
        ["size", "larger"] => pinch(args, true)?,
        ["sel", "highlight"] => Command::SelHighlight,
        ["sel", "color", name] => Command::SelColor((*name).to_string()),
        ["sel", "more"] => Command::SelMore,
        ["sel", "shade"] => Command::SelShade(number(args, 0)?),
        ["sel", "note"] => Command::SelNote,
        ["sel", "copy"] => Command::SelCopy,
        ["sel", "dict"] => Command::SelDict,
        ["sel", "search"] => Command::SelSearch,
        ["note", "save"] => Command::NoteSave,
        ["more", "start"] => Command::MoreStart,
        ["more", "end"] => Command::MoreEnd,
        ["finish"] => Command::Finish,
        ["back"] => Command::Back,
        ["goto", "open"] => Command::GotoOpen,
        ["goto", "page"] => Command::GotoPage,
        ["goto", "percent"] => Command::GotoPercent,
        _ => return None,
    };
    Some(command)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_page_turns_and_taps() {
        assert_eq!(parse("reader:next", &[]), Some(Command::Next));
        assert_eq!(parse("reader:prev", &[]), Some(Command::Prev));
        let args = [Value::Number(10.0), Value::Number(20.0)];
        assert_eq!(parse("reader:tap", &args), Some(Command::Tap(10.0, 20.0)));
    }

    #[test]
    fn parses_selection_ids() {
        assert_eq!(
            parse("reader:sel:color:red", &[]),
            Some(Command::SelColor("red".to_string()))
        );
        assert_eq!(parse("reader:sel:more", &[]), Some(Command::SelMore));
        assert_eq!(
            parse("reader:sel:shade", &[Value::Number(70.0)]),
            Some(Command::SelShade(70.0))
        );
        assert_eq!(parse("reader:sel:highlight", &[]), Some(Command::SelHighlight));
        assert_eq!(parse("reader:sel:color", &[]), None);
    }

    #[test]
    fn parses_numbered_ids() {
        assert_eq!(parse("reader:search:hit:4", &[]), Some(Command::SearchHit(4)));
        assert_eq!(
            parse("reader:contents:goto:12", &[]),
            Some(Command::ContentsGoto(12))
        );
        assert_eq!(
            parse("reader:contents:annot:7", &[]),
            Some(Command::ContentsAnnotation(7))
        );
        assert_eq!(parse("reader:search:hit:x", &[]), None);
    }

    #[test]
    fn parses_search_history_commands() {
        assert_eq!(
            parse("reader:search:recent:2", &[]),
            Some(Command::SearchRecent(2))
        );
        assert_eq!(parse("reader:search:recent:x", &[]), None);
        assert_eq!(
            parse("reader:search:history:clear", &[]),
            Some(Command::SearchClearHistory)
        );
    }

    #[test]
    fn parses_footer_items() {
        assert_eq!(
            parse("reader:footer:clock", &[Value::Bool(true)]),
            Some(Command::Footer(Item::Clock, true))
        );
        assert_eq!(
            parse("reader:footer:page", &[Value::Bool(false)]),
            Some(Command::Footer(Item::Page, false))
        );
        assert_eq!(parse("reader:footer:nope", &[]), None);
        assert_eq!(
            parse("reader:panel:footer", &[]),
            Some(Command::Panel(PanelName::Footer))
        );
    }

    #[test]
    fn parses_layout_values() {
        assert_eq!(
            parse("reader:layout:size", &[Value::Number(18.0)]),
            Some(Command::Layout(LayoutChange::Size(18.0)))
        );
        assert_eq!(
            parse("reader:layout:weight", &[Value::Number(2.0)]),
            Some(Command::Layout(LayoutChange::Weight(2.0)))
        );
        assert_eq!(
            parse("reader:layout:justify", &[Value::Bool(true)]),
            Some(Command::Layout(LayoutChange::Justify(true)))
        );
        assert_eq!(
            parse("reader:layout:font:2", &[]),
            Some(Command::Layout(LayoutChange::Font(2)))
        );
        assert_eq!(parse("reader:layout:font:x", &[]), None);
        assert_eq!(parse("reader:layout:fonts", &[]), Some(Command::FontList));
        assert_eq!(
            parse("reader:layout:hyphlang", &[]),
            Some(Command::HyphenationList)
        );
        assert_eq!(
            parse("reader:layout:hyphlang:3", &[]),
            Some(Command::Layout(LayoutChange::HyphenationLang(3)))
        );
        assert_eq!(parse("reader:layout:hyphlang:x", &[]), None);
    }

    #[test]
    fn parses_pinch_and_spread() {
        let args = [Value::Number(42.0), Value::Str("vertical".into())];
        assert_eq!(
            parse("reader:size:smaller", &args),
            Some(Command::Pinch {
                larger: false,
                distance: 42.0,
                axis: Some(Axis::Vertical),
                view: None,
            })
        );
        assert_eq!(
            parse(
                "reader:size:larger",
                &[Value::Number(3.0), Value::Str(String::new())]
            ),
            Some(Command::Pinch {
                larger: true,
                distance: 3.0,
                axis: None,
                view: None,
            })
        );
        assert_eq!(parse("reader:size:larger", &[]), None);
        assert_eq!(parse("reader:size:larger", &[Value::Number(-1.0)]), None);
    }

    #[test]
    fn pinch_steps_follow_koreader_table() {
        let screen = Size::new(1264, 1680);
        // Vertical uses the height (1680): 30 * 420 / 1680 = 7.5, step 8, entry 2.
        assert_eq!(pinch_step(420.0, Some(Axis::Vertical), screen), 2.0);
        // Horizontal uses the width (1264): 30 * 100 / 1264 = 2.37, step 3, entry 1.
        assert_eq!(pinch_step(100.0, Some(Axis::Horizontal), screen), 1.0);
        // A distance that is less than one step gives 0.
        assert_eq!(pinch_step(10.0, Some(Axis::Vertical), screen), 0.0);
        // Past the table, the last entry (5) applies.
        assert_eq!(pinch_step(2000.0, Some(Axis::Vertical), screen), 5.0);
        // No movement: Lua's step 0 is nil, so the last entry applies.
        assert_eq!(pinch_step(0.0, Some(Axis::Vertical), screen), 5.0);
        // No axis uses the shorter side, the width here.
        assert_eq!(
            pinch_step(420.0, None, screen),
            pinch_step(420.0, Some(Axis::Horizontal), screen)
        );
    }

    #[test]
    fn whole_units_inverts_koreader_px() {
        let screen = Size::new(1264, 1680);
        assert_eq!(whole_units(screen, 47.0), 22.0);
        assert_eq!(whole_units(screen, super::super::koreader_px(screen, 30.0)), 30.0);
    }

    #[test]
    fn leaves_other_namespaces_alone() {
        assert_eq!(parse("nav:home", &[]), None);
        assert_eq!(parse("library:next", &[]), None);
        assert_eq!(parse("reader:unknown", &[]), None);
    }
}
