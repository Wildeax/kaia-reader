//! Widgets draw only inside their bounds (UI-M8), long button text is cut
//! with an ellipsis, and content widgets clear their bounds first (UI-M9).

mod common;

use common::*;
use kaia_gfx::{Color, Pixmap, Rect, Size};
use kaia_ui::components::{Column, CrossAlign, Frame, Icon, Label, Paragraph, ProgressBar, Row, SizedBox};
use kaia_ui::controls::{Button, ButtonStyle};
use kaia_ui::geometry::Constraints;
use kaia_ui::theme::Tone;
use kaia_ui::widget::{LayoutCx, PaintCx, Widget};
use kaia_ui::{render, Harness, WidgetPod};

/// Fills a rectangle far bigger than its bounds, and writes text and a circle outside them.
struct Scribbler;

impl Widget for Scribbler {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(100, 100))
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        let ink = cx.color(Tone::Ink);
        cx.fill_rect(Rect::new(b.x - 500, b.y - 500, 2000, 2000), ink);
        cx.fill_rrect(Rect::new(b.x - 500, b.y - 500, 2000, 2000), 20, ink);
        cx.border(Rect::new(b.x - 500, b.y - 500, 2000, 2000), 0, 30, ink);
        cx.fill_circle(kaia_gfx::Point::new(b.x + 50, b.y + 50), 400.0, ink);
        cx.dim(Rect::new(b.x - 500, b.y - 500, 2000, 2000), 255);
    }
}

#[test]
fn a_widget_cannot_draw_outside_its_bounds() {
    let t = theme(false);
    let a = assets();
    let row = Row::new(vec![WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(Scribbler)))
            .width(100)
            .height(100),
    )]);
    let root = Frame::new(WidgetPod::new(Column::new(vec![WidgetPod::new(row)]))).fill(Tone::Surface);
    let pm = render(root, SCREEN, &t, &a);
    assert_eq!(
        dark_in(&pm, Rect::new(0, 0, 100, 100)),
        100 * 100,
        "it fills its own bounds"
    );
    assert_eq!(
        dark_in(&pm, Rect::new(100, 0, 1164, 1680)),
        0,
        "nothing right of its bounds"
    );
    assert_eq!(
        dark_in(&pm, Rect::new(0, 100, 1264, 1580)),
        0,
        "nothing below its bounds"
    );
}

#[test]
fn long_button_text_is_cut_inside_the_button() {
    let t = theme(false);
    let a = assets();
    let row = Row::new(vec![
        WidgetPod::new(
            SizedBox::new(Some(WidgetPod::new(
                Button::new(ButtonStyle::Text).text("Mark every book in this collection as finished"),
            )))
            .width(300),
        )
        .with_key("b"),
        WidgetPod::new(SizedBox::new(None).width(600).height(10)),
    ])
    .cross_align(CrossAlign::Start);
    let root = Frame::new(WidgetPod::new(Column::new(vec![WidgetPod::new(row)]))).fill(Tone::Surface);
    let mut h = Harness::new(root, SCREEN, &t, &a);
    let pm = h.frame().clone();
    let r = h.find_rect("b").unwrap();
    let (lo, hi) = dark_span(&pm, Rect::new(0, r.y, SCREEN.w, r.h)).expect("the button draws text");
    assert!(
        lo >= r.x && hi < r.right(),
        "ink {lo}..={hi} outside the button {r:?}"
    );
    // The text ends with an ellipsis that fits inside the outline, so it is not cut at the edge.
    let (ring, pad) = (t.line.focus as i32, t.space.md as i32);
    assert!(
        hi < r.right() - 2 * ring,
        "ink ends at {hi}, the margin edge is {}",
        r.right() - 2 * ring
    );
    assert!(
        hi > r.right() - 4 * pad,
        "the text still uses the width: ink ends at {hi}, button at {}",
        r.right()
    );
}

#[test]
fn a_button_with_an_icon_cuts_its_text_inside_the_button() {
    let t = theme(false);
    let a = assets();
    let row = Row::new(vec![WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(
            Button::new(ButtonStyle::Secondary)
                .icon("check")
                .text("A label that is much too long for the box"),
        )))
        .width(300),
    )
    .with_key("b")])
    .cross_align(CrossAlign::Start);
    let root = Frame::new(WidgetPod::new(Column::new(vec![WidgetPod::new(row)]))).fill(Tone::Surface);
    let mut h = Harness::new(root, SCREEN, &t, &a);
    let pm = h.frame().clone();
    let r = h.find_rect("b").unwrap();
    let all = dark_in(&pm, Rect::from_size(SCREEN));
    assert_eq!(dark_in(&pm, r), all, "ink outside {r:?}");
}

/// Pixels in `r` that differ between two frames.
fn differing(a: &Pixmap, b: &Pixmap, r: Rect) -> usize {
    let mut n = 0;
    for y in r.y..r.bottom() {
        for x in r.x..r.right() {
            if a.get(x, y) != b.get(x, y) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn a_label_that_changes_leaves_no_old_text_behind() {
    let t = theme(false);
    let a = assets();
    let col = || {
        Column::new(vec![WidgetPod::new(Label::new(
            "Scanning 888 of 999",
            t.font.title.clone(),
            Tone::Ink,
        ))
        .with_key("w")])
    };
    let mut h = Harness::new(col(), SCREEN, &t, &a);
    h.frame();
    h.update::<Label, _>("w", |l, u| {
        l.set_text("Done");
        u.request_paint();
    });
    let pm = h.frame().clone();
    let fresh = render(
        Column::new(vec![WidgetPod::new(Label::new(
            "Done",
            t.font.title.clone(),
            Tone::Ink,
        ))
        .with_key("w")]),
        SCREEN,
        &t,
        &a,
    );
    assert_eq!(differing(&pm, &fresh, h.find_rect("w").unwrap()), 0);
}

#[test]
fn a_paragraph_that_changes_leaves_no_old_text_behind() {
    let t = theme(false);
    let a = assets();
    let make = |text: &str| {
        Column::new(vec![WidgetPod::new(Paragraph::new(
            text,
            t.font.title.clone(),
            Tone::Ink,
        ))
        .with_key("w")])
    };
    let mut h = Harness::new(make("A long first sentence here"), SCREEN, &t, &a);
    h.frame();
    let old = h.find_rect("w").unwrap();
    h.update::<Paragraph, _>("w", |p, u| {
        p.set_text("Hi");
        u.request_paint();
    });
    let pm = h.frame().clone();
    let fresh = render(make("Hi"), SCREEN, &t, &a);
    assert_eq!(differing(&pm, &fresh, old), 0);
}

#[test]
fn a_progress_bar_that_changes_leaves_no_old_percent_behind() {
    let t = theme(false);
    let a = assets();
    let make = |v: f32| Column::new(vec![WidgetPod::new(ProgressBar::new(v)).with_key("w")]);
    let mut h = Harness::new(make(1.0), SCREEN, &t, &a);
    h.frame();
    h.update::<ProgressBar, _>("w", |p, u| {
        p.set_value(0.05);
        u.request_paint();
    });
    let pm = h.frame().clone();
    let fresh = render(make(0.05), SCREEN, &t, &a);
    assert_eq!(differing(&pm, &fresh, h.find_rect("w").unwrap()), 0);
}

/// Multiplies a rectangle far bigger than its bounds by a highlight color. The clip
/// keeps the tint inside the bounds.
struct Tinter;

impl Widget for Tinter {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(100, 100))
    }
    fn paint(&mut self, cx: &mut PaintCx) {
        let b = cx.bounds();
        cx.multiply_rect(
            Rect::new(b.x - 500, b.y - 500, 2000, 2000),
            Color::rgb(255, 51, 0),
        );
    }
}

#[test]
fn a_multiply_is_cut_at_the_bounds() {
    let t = theme(true);
    let a = assets();
    let row = Row::new(vec![WidgetPod::new(
        SizedBox::new(Some(WidgetPod::new(Tinter))).width(100).height(100),
    )]);
    let root = Frame::new(WidgetPod::new(Column::new(vec![WidgetPod::new(row)]))).fill(Tone::Surface);
    let pm = render(root, SCREEN, &t, &a);
    let surface = t.color(Tone::Surface);
    // Each channel is in * color / 255: the red and blue factors are 255 and 0, the green is 51.
    let tinted = Color::rgb(surface.r, (u32::from(surface.g) * 51 / 255) as u8, 0);
    assert_eq!(
        pm.get(0, 0),
        Some(tinted),
        "inside the bounds the surface takes the tint"
    );
    assert_eq!(pm.get(99, 99), Some(tinted), "the last pixel inside the bounds");
    assert_eq!(pm.get(100, 0), Some(surface), "right of the bounds is untouched");
    assert_eq!(pm.get(0, 100), Some(surface), "below the bounds is untouched");
}

#[test]
fn an_icon_that_changes_leaves_no_old_shape_behind() {
    let t = theme(false);
    let a = assets();
    let make = |name: &str| Column::new(vec![WidgetPod::new(Icon::new(name, 96, Tone::Ink)).with_key("w")]);
    let mut h = Harness::new(make("check"), SCREEN, &t, &a);
    h.frame();
    h.update::<Icon, _>("w", |i, u| {
        i.set_name("close");
        u.request_paint();
    });
    let pm = h.frame().clone();
    let fresh = render(make("close"), SCREEN, &t, &a);
    assert_eq!(differing(&pm, &fresh, h.find_rect("w").unwrap()), 0);
}
