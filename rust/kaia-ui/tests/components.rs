//! Behavior of the content and overlay components, checked on rendered pixels.

mod common;

use common::*;
use kaia_gfx::image::FitMode as GfxFit;
use kaia_gfx::{Color, PixelFormat, Pixmap, Rect, Size};
use kaia_ui::components::{Image, Label, SizedBox, Stack};
use kaia_ui::{Harness, Tone, WidgetPod};

const SCREEN: Size = Size::new(300, 300);

/// Bounding box of the pixels that differ from `background`.
fn ink_extent(pm: &Pixmap, background: Color) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    for y in 0..pm.height() as i32 {
        for x in 0..pm.width() as i32 {
            if pm.get(x, y) != Some(background) {
                let px = Rect::new(x, y, 1, 1);
                out = Some(match out {
                    Some(r) => r.union(&px),
                    None => px,
                });
            }
        }
    }
    out
}

fn black_source(w: u32, h: u32) -> Pixmap {
    let mut src = Pixmap::new(w, h, PixelFormat::Gray8);
    src.fill_rect(Rect::from_size(Size::new(w, h)), Color::BLACK);
    src
}

#[test]
fn image_contain_keeps_the_aspect_ratio_and_centers() {
    let t = theme(false);
    let img = Image::new(black_source(20, 10), GfxFit::Contain);
    let pm = kaia_ui::render(img, SCREEN, &t, &assets());
    // 20 x 10 scaled to fit 300 x 300 is 300 x 150, centered vertically.
    assert_eq!(
        ink_extent(&pm, t.color(Tone::Surface)),
        Some(Rect::new(0, 75, 300, 150))
    );
}

#[test]
fn image_cover_fills_the_box() {
    let t = theme(false);
    let img = Image::new(black_source(20, 10), GfxFit::Cover);
    let pm = kaia_ui::render(img, SCREEN, &t, &assets());
    assert_eq!(
        ink_extent(&pm, t.color(Tone::Surface)),
        Some(Rect::new(0, 0, 300, 300))
    );
}

#[test]
fn stack_draws_children_on_top_of_each_other_at_the_origin() {
    let b = new_bounds();
    let stack = Stack::new(vec![
        WidgetPod::new(Probe::new("back", 50, 50, &b)),
        WidgetPod::new(Probe::new("front", 100, 20, &b)),
    ]);
    kaia_ui::render(stack, SCREEN, &theme(false), &assets());
    assert_eq!(bounds_of(&b, "back"), Rect::new(0, 0, 50, 50));
    assert_eq!(bounds_of(&b, "front"), Rect::new(0, 0, 100, 20));
}

#[test]
fn a_long_label_is_cut_with_an_ellipsis_inside_its_width() {
    let t = theme(false);
    let label = Label::new(
        "A title much too long for one line of this narrow box",
        t.font.body.clone(),
        Tone::Ink,
    );
    let boxed = SizedBox::new(Some(WidgetPod::new(label))).width(120);
    let pm = kaia_ui::render(boxed, SCREEN, &t, &assets());
    let ink = ink_extent(&pm, t.color(Tone::Surface)).expect("the label draws something");
    assert!(ink.right() <= 120, "ink ends at {}", ink.right());
    assert!(
        ink.h <= t.font.body.size as u32 * 2,
        "one line tall, got {}",
        ink.h
    );
}

#[test]
fn a_label_with_plenty_of_room_keeps_its_full_text() {
    let t = theme(false);
    let short = Label::new("Kaia", t.font.title.clone(), Tone::Ink);
    let pm = kaia_ui::render(short, SCREEN, &t, &assets());
    let ink = ink_extent(&pm, t.color(Tone::Surface)).expect("the label draws something");
    assert!(ink.w > 30 && ink.w < 200, "four letters, got width {}", ink.w);
}

/// A holder with one 300 px Tapper, set up by `setup`.
fn holder_with(setup: impl FnOnce(&mut Tapper), focusable: bool) -> Harness {
    let log = new_log();
    let mut b = Tapper::new("B", 300, 300, &log);
    setup(&mut b);
    let root = Holder {
        children: vec![WidgetPod::new(b).focusable(focusable)],
        size: SCREEN,
        name: "root",
        log: log.clone(),
        handle: false,
    };
    Harness::new(root, SCREEN, &theme(false), &assets())
}

#[test]
fn a_focus_request_makes_the_widget_focusable() {
    let mut h = holder_with(|b| b.focus_on_tap = true, false);
    h.tap(kaia_gfx::Point::new(5, 5));
    assert!(h.focus().is_some(), "the request must not be dropped");
}

#[test]
fn a_soft_focus_request_needs_a_focusable_widget() {
    let mut h = holder_with(|b| b.soft_focus_on_tap = true, false);
    h.tap(kaia_gfx::Point::new(5, 5));
    assert_eq!(
        h.focus(),
        None,
        "a plain control is not pulled into the focus order"
    );
    let mut h = holder_with(|b| b.soft_focus_on_tap = true, true);
    h.tap(kaia_gfx::Point::new(5, 5));
    assert!(h.focus().is_some());
}
