//! Highlights on the reader page. On a color panel a mark with a color is drawn in
//! that color. A grayscale panel keeps the shape alone. A box is dimmed and a line is ink.
//! Most tests paint the page headless and read the pixels of the frame. The last test
//! makes a highlight in the reader and checks the mark the page gets from it.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use kaia_app::surfaces::reader::page::{Mark, MarkStyle, PageView, PAGE_KEY};
use kaia_app::{App, HeadlessBackend};
use kaia_display::InputEvent;
use kaia_gfx::{Color, PixelFormat, Pixmap, Point, Rect, Size};
use kaia_input::{Key, Phase, TouchEvent};
use kaia_ui::assets::{Assets, SharedAssets};
use kaia_ui::harness::render;
use kaia_ui::theme::{Scale, Theme, Tone};

mod common;

use common::*;

/// The frame. The page is centered, so its top-left corner is at (50, 50).
const SCREEN: Size = Size::new(300, 300);
const PAGE: Size = Size::new(200, 200);
/// The box of every mark in these tests. It sits inside the page.
const BOX: Rect = Rect::new(80, 80, 60, 20);
const WHITE: [u8; 3] = [255, 255, 255];
/// KOReader's yellow, the `yellow` entry of its highlight colors.
const YELLOW: [u8; 3] = [0xFF, 0xFF, 0x33];
/// White dimmed by 40, which is 255 * (255 - 40) / 255.
const DIMMED: [u8; 3] = [215, 215, 215];

fn assets() -> SharedAssets {
    let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources");
    Assets::load_icons(
        &resources.join("fonts/noto"),
        &kaia_app::services::icon_dirs(&resources),
    )
    .unwrap_or_else(|e| panic!("cannot load assets: {e}"))
    .shared()
}

fn theme(color_panel: bool) -> Theme {
    Theme::new(
        Scale {
            width: SCREEN.w,
            height: SCREEN.h,
            dpi: Some(300),
        },
        color_panel,
    )
}

fn white() -> Pixmap {
    let mut page = Pixmap::new(PAGE.w, PAGE.h, PixelFormat::Gray8);
    page.fill_rect(Rect::from_size(PAGE), Color::WHITE);
    page
}

/// Paints `page` with `marks` on it, into a frame in the format of the panel.
fn paint(page: Pixmap, color_panel: bool, marks: Vec<Mark>) -> Pixmap {
    let mut view = PageView::new();
    view.page = Some(Arc::new(page));
    view.marks = marks;
    render(view, SCREEN, &theme(color_panel), &assets())
}

/// Paints `page` with `marks` at the highlight shade `shade`.
fn paint_at_shade(page: Pixmap, color_panel: bool, marks: Vec<Mark>, shade: f64) -> Pixmap {
    let mut view = PageView::new();
    view.page = Some(Arc::new(page));
    view.marks = marks;
    view.highlight_shade = shade;
    render(view, SCREEN, &theme(color_panel), &assets())
}

fn mark(style: MarkStyle, color: Option<[u8; 3]>) -> Mark {
    Mark {
        rect: BOX,
        style,
        color: color.map(|[r, g, b]| Color::rgb(r, g, b)),
    }
}

/// The RGB value of a pixel. A gray pixel has the same value in each channel.
fn at(pm: &Pixmap, x: i32, y: i32) -> [u8; 3] {
    let c = pm.get(x, y).expect("the pixel is in the frame");
    [c.r, c.g, c.b]
}

/// The ink color of the panel, as a pixel value.
fn ink(color_panel: bool) -> [u8; 3] {
    let c = theme(color_panel).color(Tone::Ink);
    [c.r, c.g, c.b]
}

#[test]
fn a_box_is_tinted_by_its_color_on_a_color_panel() {
    let frame = paint(white(), true, vec![mark(MarkStyle::Box, Some(YELLOW))]);
    // White times the color, channel by channel, is the color.
    assert_eq!(
        at(&frame, 80, 80),
        YELLOW,
        "the top-left corner is inside the box"
    );
    assert_eq!(
        at(&frame, 139, 99),
        YELLOW,
        "the bottom-right corner is inside the box"
    );
    assert_eq!(at(&frame, 79, 80), WHITE, "the column left of the box is page");
    assert_eq!(at(&frame, 140, 80), WHITE, "the column right of the box is page");
    assert_eq!(at(&frame, 100, 100), WHITE, "the row below the box is page");
}

#[test]
fn dark_text_under_a_colored_box_stays_dark() {
    let mut page = white();
    // Screen (85, 85) is page pixel (35, 35): a black stroke of text.
    page.put(35, 35, Color::BLACK);
    let frame = paint(page, true, vec![mark(MarkStyle::Box, Some(YELLOW))]);
    assert_eq!(
        at(&frame, 85, 85),
        [0, 0, 0],
        "black stays black under the highlight"
    );
    assert_eq!(at(&frame, 86, 85), YELLOW, "the white next to it takes the color");
}

#[test]
fn a_box_without_a_color_is_dimmed_on_a_color_panel() {
    let frame = paint(white(), true, vec![mark(MarkStyle::Box, None)]);
    assert_eq!(at(&frame, 85, 85), DIMMED);
    assert_eq!(at(&frame, 79, 80), WHITE);
}

#[test]
fn a_grayscale_panel_dims_the_box_and_ignores_its_color() {
    let colored = paint(white(), false, vec![mark(MarkStyle::Box, Some(YELLOW))]);
    let plain = paint(white(), false, vec![mark(MarkStyle::Box, None)]);
    assert_eq!(
        colored.data(),
        plain.data(),
        "the color does not reach a grayscale panel"
    );
    assert_eq!(at(&colored, 85, 85), DIMMED, "white dimmed by 40 is 215");
    assert_eq!(at(&colored, 79, 80), WHITE);
}

#[test]
fn the_default_shade_draws_the_pixels_of_the_box_before_the_setting() {
    // The default shade, 0.2, dims a gray box by 40 and tints a color box by its color.
    let gray = vec![mark(MarkStyle::Box, None)];
    let colored = vec![mark(MarkStyle::Box, Some(YELLOW))];
    for color_panel in [false, true] {
        for marks in [&gray, &colored] {
            let default = paint(white(), color_panel, marks.clone());
            let explicit = paint_at_shade(white(), color_panel, marks.clone(), 0.2);
            assert_eq!(
                default.data(),
                explicit.data(),
                "0.2 is the default on color_panel={color_panel}"
            );
        }
    }
    let dimmed = paint_at_shade(white(), true, gray, 0.2);
    assert_eq!(at(&dimmed, 85, 85), DIMMED, "white dimmed by 40 is 215");
    let tinted = paint_at_shade(white(), true, colored, 0.2);
    assert_eq!(at(&tinted, 85, 85), YELLOW, "white times the color is the color");
}

#[test]
fn a_higher_shade_darkens_a_gray_box() {
    // White dimmed by 20, 40, 120 and 200: 235, 215, 135 and 55.
    let values: Vec<u8> = [0.1, 0.2, 0.6, 1.0]
        .into_iter()
        .map(|shade| {
            let frame = paint_at_shade(white(), true, vec![mark(MarkStyle::Box, None)], shade);
            at(&frame, 85, 85)[0]
        })
        .collect();
    assert_eq!(values, [235, 215, 135, 55], "each step darkens the box");

    let gray_panel = paint_at_shade(white(), false, vec![mark(MarkStyle::Box, None)], 0.6);
    assert_eq!(
        at(&gray_panel, 85, 85),
        [135, 135, 135],
        "a grayscale panel follows the shade too"
    );
    assert_eq!(
        at(&gray_panel, 79, 80),
        WHITE,
        "the page outside the box is not dimmed"
    );
}

#[test]
fn a_lower_shade_pales_a_color_on_a_color_panel() {
    // The yellow factor is (255, 255, 51). At 0.1 it moves halfway to white in blue: 153.
    let pale = paint_at_shade(white(), true, vec![mark(MarkStyle::Box, Some(YELLOW))], 0.1);
    assert_eq!(
        at(&pale, 85, 85),
        [255, 255, 153],
        "a lighter shade is a paler yellow"
    );
    assert_eq!(at(&pale, 79, 80), WHITE);

    // At 0.4 the blue factor goes past black and stops at 0.
    let deep = paint_at_shade(white(), true, vec![mark(MarkStyle::Box, Some(YELLOW))], 0.4);
    assert_eq!(
        at(&deep, 85, 85),
        [255, 255, 0],
        "a higher shade is a deeper yellow"
    );
}

#[test]
fn an_invert_box_flips_the_pixels_and_ignores_its_color() {
    let mut page = white();
    // Screen (85, 85) is page pixel (35, 35): a black stroke of text.
    page.put(35, 35, Color::BLACK);
    let frame = paint(page, true, vec![mark(MarkStyle::Invert, Some(YELLOW))]);
    assert_eq!(at(&frame, 86, 85), [0, 0, 0], "white turns black, as in KOReader");
    assert_eq!(at(&frame, 85, 85), WHITE, "the text turns white");
    assert_eq!(at(&frame, 79, 80), WHITE, "the column left of the box is page");
}

#[test]
fn an_underline_is_drawn_in_its_color_on_a_color_panel() {
    let frame = paint(white(), true, vec![mark(MarkStyle::Underline, Some(YELLOW))]);
    // The line is the bottom three rows of the box.
    for y in 97..100 {
        assert_eq!(at(&frame, 80, y), YELLOW, "row {y} is the line");
    }
    assert_eq!(at(&frame, 139, 99), YELLOW, "the line reaches the right edge");
    assert_eq!(at(&frame, 85, 96), WHITE, "the row above the line is page");
}

#[test]
fn a_strikeout_is_drawn_in_its_color_on_a_color_panel() {
    let frame = paint(white(), true, vec![mark(MarkStyle::Strike, Some(YELLOW))]);
    // The line is two rows through the middle of the box, starting at y = 90.
    assert_eq!(at(&frame, 85, 90), YELLOW);
    assert_eq!(at(&frame, 85, 91), YELLOW);
    assert_eq!(at(&frame, 85, 89), WHITE, "the row above the line is page");
    assert_eq!(at(&frame, 85, 92), WHITE, "the row below the line is page");
}

#[test]
fn a_line_is_ink_without_a_color_and_on_a_grayscale_panel() {
    for (style, row) in [(MarkStyle::Underline, 99), (MarkStyle::Strike, 90)] {
        let plain = paint(white(), true, vec![mark(style, None)]);
        assert_eq!(at(&plain, 85, row), ink(true), "{style:?} without a color is ink");

        let colored = paint(white(), false, vec![mark(style, Some(YELLOW))]);
        let plain_gray = paint(white(), false, vec![mark(style, None)]);
        assert_eq!(
            colored.data(),
            plain_gray.data(),
            "{style:?} ignores the color on grayscale"
        );
        assert_eq!(at(&colored, 85, row), ink(false), "{style:?} on grayscale is ink");
    }
}

/// Opens the reader on `name` and waits for the library scan.
fn open_reader(name: &str, dir: &Path) -> App<HeadlessBackend> {
    let lib = library_with_books(dir);
    let mut app = make_app(dir, vec![lib]);
    app.start().unwrap();
    wait_for(&mut app, "the library scan", |a| !a.scan_running());
    app.iterate(Some(Duration::from_millis(10))).unwrap();
    open_from_library(&mut app, name);
    app
}

/// Turns to a page of juliet with text. Page 0 is the cover.
fn turn_to_text(app: &mut App<HeadlessBackend>) {
    for _ in 0..8 {
        app.key(Key::RPgFwd).unwrap();
        wait_reader(app);
    }
}

/// A point on a word of the text page that `turn_to_text` shows.
fn word_point() -> Point {
    Point::new(200, 120)
}

/// A hold at `p`, which selects the word there and opens the selection popover.
fn hold_at(app: &mut App<HeadlessBackend>, p: Point) {
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Down,
        pos: p,
        time_us: 1,
    })])
    .unwrap();
    std::thread::sleep(Duration::from_millis(700));
    app.iterate(Some(Duration::from_millis(1))).unwrap();
    app.process(vec![InputEvent::Touch(TouchEvent {
        slot: 0,
        phase: Phase::Up,
        pos: p,
        time_us: 2_000_000,
    })])
    .unwrap();
    wait_reader(app);
}

#[test]
fn a_highlight_made_in_the_reader_carries_its_color_to_the_page() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("highlight-color");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    // The yellow dot is the default highlight: lighten in yellow on a color panel.
    tap_key(&mut app, "color-yellow");
    wait_reader(&mut app);

    let mut marks: Vec<Mark> = Vec::new();
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| marks = view.marks.clone());
    let yellow = Color::rgb(0xFF, 0xFF, 0x33);
    assert!(
        marks
            .iter()
            .any(|m| m.style == MarkStyle::Box && m.color == Some(yellow)),
        "the page draws the new highlight in yellow, got {marks:?}"
    );
}

#[test]
fn the_shade_stepper_in_more_saves_the_shade_and_the_page_draws_it() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let dir = scratch("highlight-shade");
    let mut app = open_reader("juliet.epub", &dir);
    turn_to_text(&mut app);
    hold_at(&mut app, word_point());
    tap_key(&mut app, "sel-more");

    // The right edge of the stepper is its plus button. The default shade is 20 percent.
    let stepper = app.find_rect("sel-shade").expect("the stepper is in More");
    let plus = Point::new(stepper.x + stepper.w as i32 - 4, stepper.y + stepper.h as i32 / 2);
    app.tap(plus).unwrap();
    wait_reader(&mut app);
    assert_eq!(
        app.services().settings.read("reading.highlight_shade").as_f64(),
        Some(0.3),
        "one step up from the default 0.2"
    );

    // The sheet closes, and the page under it is drawn with the new shade.
    tap_key(&mut app, "panel-close");
    wait_reader(&mut app);
    let mut shade = 0.0;
    app.update_top::<PageView, _>(PAGE_KEY, |view, _| shade = view.highlight_shade);
    assert_eq!(shade, 0.3, "the page holds the new shade");
}
