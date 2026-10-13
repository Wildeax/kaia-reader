//! Background tones: a control clears its bounds to the tone behind it. The
//! tone comes from its own `background` setting, or from the nearest parent
//! `Frame` with a fill, or from `Surface`.

mod common;

use common::*;
use kaia_gfx::{Point, Rect, Size};
use kaia_ui::components::{Column, Frame, Padding};
use kaia_ui::controls::{Button, ButtonStyle, Toggle};
use kaia_ui::{Harness, Insets, Tone, WidgetPod};

const SCREEN: Size = Size::new(400, 300);

fn pixel(h: &Harness, p: Point) -> kaia_gfx::Color {
    h.screen().get(p.x, p.y).expect("the point is on screen")
}

/// A probe spot inside a button: the top left corner of its bounds, which
/// no content covers.
fn corner(h: &mut Harness, key: &str) -> Point {
    let r: Rect = h.find_rect(key).expect("keyed widget is laid out");
    Point::new(r.x + 2, r.y + 2)
}

#[test]
fn a_control_clears_to_surface_by_default() {
    let t = theme(false);
    let button = WidgetPod::new(Button::new(ButtonStyle::Text).text("Open")).with_key("b");
    let mut h = Harness::new(Column::new(vec![button]).shrink(), SCREEN, &t, &assets());
    h.frame();
    let p = corner(&mut h, "b");
    assert_eq!(pixel(&h, p), t.color(Tone::Surface));
}

#[test]
fn a_control_inherits_the_fill_of_its_parent_frame() {
    let t = theme(false);
    let button = WidgetPod::new(Button::new(ButtonStyle::Text).text("Open")).with_key("b");
    let card = Frame::new(WidgetPod::new(Padding::new(button, Insets::all(40))))
        .fill(Tone::SurfaceAlt)
        .radius(0);
    let mut h = Harness::new(
        Column::new(vec![WidgetPod::new(card)]).shrink(),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let p = corner(&mut h, "b");
    assert_eq!(
        pixel(&h, p),
        t.color(Tone::SurfaceAlt),
        "the button clears to the card color"
    );
}

#[test]
fn a_control_background_overrides_the_parent() {
    let t = theme(false);
    let button = WidgetPod::new(
        Button::new(ButtonStyle::Text)
            .text("Open")
            .background(Tone::Divider),
    )
    .with_key("b");
    let card = Frame::new(WidgetPod::new(Padding::new(button, Insets::all(40))))
        .fill(Tone::SurfaceAlt)
        .radius(0);
    let mut h = Harness::new(
        Column::new(vec![WidgetPod::new(card)]).shrink(),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let p = corner(&mut h, "b");
    assert_eq!(pixel(&h, p), t.color(Tone::Divider));
}

#[test]
fn the_inactive_half_of_a_toggle_uses_the_inherited_background() {
    let t = theme(false);
    let toggle = WidgetPod::new(Toggle::new(true)).with_key("t");
    let card = Frame::new(WidgetPod::new(Padding::new(toggle, Insets::all(40))))
        .fill(Tone::SurfaceAlt)
        .radius(0);
    let mut h = Harness::new(
        Column::new(vec![WidgetPod::new(card)]).shrink(),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    // With "on" the left half is the inactive paper half. The probe sits inside
    // the track, past its rounded corner and clear of the border and the label.
    let r = h.find_rect("t").expect("toggle is laid out");
    let ring = t.line.focus as i32;
    let p = Point::new(r.x + ring + 25, r.y + ring + 8);
    assert_eq!(pixel(&h, p), t.color(Tone::SurfaceAlt));
}
