//! Right-to-left text must land inside the widget that draws it (UI-C3).
//! Cosmic-text aligns an RTL line to the width it shapes at. A widget that
//! shapes at a wide limit and then sizes itself to the text would draw far to
//! the right of its own bounds.

mod common;

use common::*;
use kaia_gfx::{Pixmap, Rect};
use kaia_ui::components::{Align, Column, CrossAlign, Frame, Label, Paragraph};
use kaia_ui::controls::{Button, ButtonStyle, Slider, Stepper};
use kaia_ui::theme::Tone;
use kaia_ui::{Alignment, Harness, WidgetPod};

const SAMPLES: [(&str, &str); 2] = [("arabic", "مرحبا بالعالم"), ("hebrew", "שלום עולם")];

/// Renders `root`, finds the widget keyed "w", and checks that every dark
/// pixel on the screen is inside its bounds and that some are.
fn assert_ink_inside(root: impl kaia_ui::Widget + 'static, what: &str) {
    let t = theme(false);
    let a = assets();
    let mut h = Harness::new(root, SCREEN, &t, &a);
    let pm: Pixmap = h.frame().clone();
    let r = h.find_rect("w").unwrap_or_else(|| panic!("{what}: no widget"));
    let inside = dark_in(&pm, r);
    let everywhere = dark_in(&pm, Rect::from_size(SCREEN));
    assert!(inside > 40, "{what}: only {inside} dark pixels inside {r:?}");
    assert_eq!(
        inside,
        everywhere,
        "{what}: {} dark pixels outside the bounds {r:?}, ink spans {:?}",
        everywhere - inside,
        dark_span(&pm, Rect::from_size(SCREEN))
    );
}

fn framed(child: WidgetPod) -> Frame {
    Frame::new(WidgetPod::new(
        Column::new(vec![child]).cross_align(CrossAlign::Start),
    ))
    .fill(Tone::Surface)
}

#[test]
fn a_label_draws_inside_its_bounds() {
    let t = theme(false);
    for (name, text) in SAMPLES {
        let label = WidgetPod::new(Label::new(text, t.font.title.clone(), Tone::Ink)).with_key("w");
        assert_ink_inside(framed(label), &format!("label {name}"));
    }
}

#[test]
fn a_paragraph_draws_inside_its_bounds_at_every_alignment() {
    let t = theme(false);
    for (name, text) in SAMPLES {
        for (h, hname) in [
            (Alignment::Start, "left"),
            (Alignment::Center, "center"),
            (Alignment::End, "right"),
        ] {
            let p = WidgetPod::new(Paragraph::new(text, t.font.title.clone(), Tone::Ink)).with_key("w");
            let root = Frame::new(WidgetPod::new(Align::new(p, h, Alignment::Start))).fill(Tone::Surface);
            assert_ink_inside(root, &format!("paragraph {name} {hname}"));
        }
    }
}

#[test]
fn a_wrapped_paragraph_draws_inside_its_bounds() {
    let t = theme(false);
    let text = "مرحبا بالعالم ".repeat(12);
    let p = WidgetPod::new(Paragraph::new(text, t.font.title.clone(), Tone::Ink)).with_key("w");
    let root =
        Frame::new(WidgetPod::new(Align::new(p, Alignment::Center, Alignment::Start))).fill(Tone::Surface);
    assert_ink_inside(root, "wrapped paragraph");
}

#[test]
fn a_button_draws_its_text_inside_its_bounds() {
    for (name, text) in SAMPLES {
        for style in [ButtonStyle::Text, ButtonStyle::Secondary, ButtonStyle::Primary] {
            let b = WidgetPod::new(Button::new(style).text(text)).with_key("w");
            let t = theme(false);
            let a = assets();
            let mut h = Harness::new(framed(b), SCREEN, &t, &a);
            let pm = h.frame().clone();
            let r = h.find_rect("w").unwrap();
            let everywhere = dark_in(&pm, Rect::from_size(SCREEN));
            let inside = dark_in(&pm, r);
            assert_eq!(inside, everywhere, "button {name} {style:?}: ink outside {r:?}");
            // The text itself, not just the outline: look at the middle third of the rows.
            let mid = Rect::new(r.x + 12, r.y + r.h as i32 / 3, r.w - 24, r.h / 3);
            let (primary_text_ink, _) = (dark_in(&pm, mid), ());
            assert!(
                primary_text_ink > 20 || style == ButtonStyle::Primary,
                "button {name} {style:?}: no text"
            );
        }
    }
}

#[test]
fn a_primary_button_shows_light_text_inside_its_bounds() {
    let b = WidgetPod::new(Button::new(ButtonStyle::Primary).text("إلغاء")).with_key("w");
    let t = theme(false);
    let a = assets();
    let mut h = Harness::new(framed(b), SCREEN, &t, &a);
    let pm = h.frame().clone();
    let r = h.find_rect("w").unwrap();
    // White text on the ink fill: some light pixels sit inside the fill, away from its edge.
    let core = Rect::new(r.x + r.w as i32 / 4, r.y + r.h as i32 / 3, r.w / 2, r.h / 3);
    let light = (core.y..core.bottom())
        .flat_map(|y| (core.x..core.right()).map(move |x| (x, y)))
        .filter(|&(x, y)| pm.get(x, y).is_some_and(|c| c.r > 128))
        .count();
    assert!(
        light > 15,
        "{light} light pixels of text in the middle of the button"
    );
}

#[test]
fn stepper_and_slider_values_draw_inside_their_bounds() {
    let t = theme(false);
    let (ring, touch) = (t.line.focus as i32, t.touch_target as i32);
    for (name, text) in SAMPLES {
        let label = text.to_string();
        let st = WidgetPod::new(Stepper::new(3.0, 0.0, 9.0, 1.0).format({
            let label = label.clone();
            move |v| format!("{label} {v}")
        }))
        .with_key("w");
        assert_ink_inside(framed(st), &format!("stepper {name}"));
        // The value sits between the two buttons.
        let st = WidgetPod::new(Stepper::new(3.0, 0.0, 9.0, 1.0).format({
            let label = label.clone();
            move |v| format!("{label} {v}")
        }))
        .with_key("w");
        let mut h = Harness::new(framed(st), SCREEN, &t, &assets());
        let pm = h.frame().clone();
        let r = h.find_rect("w").unwrap();
        let value_box = Rect::new(r.x + ring + touch, r.y, r.w - 2 * (ring + touch) as u32, r.h);
        assert!(
            dark_in(&pm, value_box) > 40,
            "stepper {name}: the value is not visible"
        );

        let sl = WidgetPod::new(Slider::new(3.0, 0.0, 9.0, 1.0).format(move |v| format!("{label} {v}")))
            .with_key("w");
        assert_ink_inside(framed(sl), &format!("slider {name}"));
        let sl = WidgetPod::new(Slider::new(3.0, 0.0, 9.0, 1.0).format(move |v| format!("{text} {v}")))
            .with_key("w");
        let mut h = Harness::new(framed(sl), SCREEN, &t, &assets());
        let pm = h.frame().clone();
        let r = h.find_rect("w").unwrap();
        let tail = Rect::new(r.right() - 120, r.y, 100, r.h);
        assert!(dark_in(&pm, tail) > 40, "slider {name}: the value is not visible");
    }
}
