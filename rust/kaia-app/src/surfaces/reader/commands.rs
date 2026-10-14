//! Commands of the reader surface. Ids are `reader:<action>[:<arg>]`. Numbers
//! that a control sends (a slider value, a stepper value, a radio index) come in
//! the action args. Taps and holds send their position in the args too.

use kaia_events::Value;

/// One change from the Text and layout sheet, with the value the control sent.
/// `Font` and `Zoom` carry the index of the radio choice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LayoutChange {
    Font(usize),
    Size(f64),
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// A tap on the page, in pixels from the top left of the screen.
    Tap(f64, f64),
    /// Turn to the next page: page keys, taps on the right, swipes.
    Next,
    /// Turn to the previous page.
    Prev,
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
    /// The page slider moved to a page number (1-based).
    Slider(f64),
    /// Opens the search panel.
    SearchOpen,
    /// Starts the search with the text in the search field.
    SearchGo,
    /// Jumps to a search hit by its index.
    SearchHit(usize),
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
    /// Highlights the selection.
    SelHighlight,
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

fn flag(args: &[Value]) -> bool {
    matches!(args.first(), Some(Value::Bool(true)))
}

fn index(args: &[Value]) -> usize {
    number(args, 0).unwrap_or(0.0).max(0.0) as usize
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
        ["panel", "contents"] => Command::Panel(PanelName::Contents),
        ["panel", "layout"] => Command::Panel(PanelName::Layout),
        ["panel", "more"] => Command::Panel(PanelName::More),
        ["panel", "close"] => Command::PanelClose,
        ["contents", "tab", n] => Command::ContentsTab(n.parse().ok()?),
        ["contents", "goto", n] => Command::ContentsGoto(n.parse().ok()?),
        ["contents", "annot", n] => Command::ContentsAnnotation(n.parse().ok()?),
        ["layout", "font"] => Command::Layout(LayoutChange::Font(index(args))),
        ["layout", "size"] => Command::Layout(LayoutChange::Size(number(args, 0)?)),
        ["layout", "spacing"] => Command::Layout(LayoutChange::Spacing(number(args, 0)?)),
        ["layout", "margins"] => Command::Layout(LayoutChange::Margins(number(args, 0)?)),
        ["layout", "justify"] => Command::Layout(LayoutChange::Justify(flag(args))),
        ["layout", "hyphenation"] => Command::Layout(LayoutChange::Hyphenation(flag(args))),
        ["layout", "styles"] => Command::Layout(LayoutChange::Styles(flag(args))),
        ["layout", "zoom"] => Command::Layout(LayoutChange::Zoom(index(args))),
        ["sel", "highlight"] => Command::SelHighlight,
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
    fn parses_layout_values() {
        assert_eq!(
            parse("reader:layout:size", &[Value::Number(18.0)]),
            Some(Command::Layout(LayoutChange::Size(18.0)))
        );
        assert_eq!(
            parse("reader:layout:justify", &[Value::Bool(true)]),
            Some(Command::Layout(LayoutChange::Justify(true)))
        );
        assert_eq!(
            parse("reader:layout:font", &[Value::Number(2.0)]),
            Some(Command::Layout(LayoutChange::Font(2)))
        );
    }

    #[test]
    fn leaves_other_namespaces_alone() {
        assert_eq!(parse("nav:home", &[]), None);
        assert_eq!(parse("library:next", &[]), None);
        assert_eq!(parse("reader:unknown", &[]), None);
    }
}
