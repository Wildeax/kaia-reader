//! Smaller fixes: stepper and slider steps without float drift, text field
//! shaping, and the width of a list row value.

mod common;

use common::*;
use kaia_events::Value;
use kaia_gfx::{Point, Rect};
use kaia_input::Key;
use kaia_ui::components::{Column, CrossAlign, Frame};
use kaia_ui::composite::ListRow;
use kaia_ui::controls::{Slider, Stepper, TextField};
use kaia_ui::theme::Tone;
use kaia_ui::widget::Out;
use kaia_ui::{Harness, WidgetPod};

fn page(child: WidgetPod) -> Frame {
    Frame::new(WidgetPod::new(
        Column::new(vec![child]).cross_align(CrossAlign::Start),
    ))
    .fill(Tone::Surface)
}

fn centre(r: Rect) -> Point {
    Point::new(r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
}

fn plus_point(h: &Harness) -> Point {
    let r = h.find_rect("w").unwrap();
    Point::new(r.right() - 10, r.y + r.h as i32 / 2)
}

fn numbers(h: &Harness) -> Vec<f64> {
    h.outbox()
        .into_iter()
        .filter_map(|o| match o {
            Out::Event(e) => match e.args.first() {
                Some(Value::Number(n)) => Some(*n),
                _ => None,
            },
            Out::Action(_) => None,
        })
        .collect()
}

#[test]
fn a_stepper_with_a_decimal_step_has_no_float_drift() {
    let t = theme(false);
    let a = assets();
    let st = WidgetPod::new(Stepper::new(0.0, 0.0, 1.0, 0.1).on_event("size")).with_key("w");
    let mut h = Harness::new(page(st), SCREEN, &t, &a);
    h.frame();
    for _ in 0..10 {
        let at = plus_point(&h);
        h.tap(at);
    }
    let seen = numbers(&h);
    assert_eq!(seen, vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0]);
    let mut value = 0.0;
    h.update::<Stepper, _>("w", |s, _| value = s.value());
    assert_eq!(value, 1.0);
}

#[test]
fn a_stepper_with_a_quarter_step_keeps_two_decimals() {
    let t = theme(false);
    let a = assets();
    let st = WidgetPod::new(Stepper::new(0.0, 0.0, 2.0, 0.25).on_event("size")).with_key("w");
    let mut h = Harness::new(page(st), SCREEN, &t, &a);
    h.frame();
    for _ in 0..3 {
        let at = plus_point(&h);
        h.tap(at);
    }
    assert_eq!(numbers(&h), vec![0.25, 0.5, 0.75]);
}

#[test]
fn a_slider_steps_by_decimals_without_drift() {
    let t = theme(false);
    let a = assets();
    let sl = WidgetPod::new(Slider::new(0.5, 0.0, 1.0, 0.1).on_event("warmth"))
        .with_key("w")
        .focusable(true);
    let mut h = Harness::new(page(sl), SCREEN, &t, &a);
    h.frame();
    h.key(Key::RPgFwd);
    h.key(Key::Left);
    h.key(Key::Left);
    assert_eq!(numbers(&h), vec![0.4, 0.3]);
    let mut v = 0.0;
    h.update::<Slider, _>("w", |s, _| v = s.value());
    assert_eq!(v, 0.3);
}

#[test]
fn a_text_field_shapes_once_per_change_not_once_per_paint() {
    let t = theme(false);
    let a = assets();
    let tf = WidgetPod::new(TextField::new().with_text("hello world"))
        .with_key("w")
        .focusable(true);
    let mut h = Harness::new(page(tf), SCREEN, &t, &a);
    h.frame();
    let count = |a: &kaia_ui::SharedAssets| a.borrow().fonts.layout_count();
    let base = count(&a);
    for _ in 0..5 {
        h.update::<TextField, _>("w", |_, u| u.request_paint());
        h.frame();
    }
    assert_eq!(count(&a), base, "five repaints of unchanged text shape nothing");
}

#[test]
fn typing_shapes_a_constant_number_of_times_per_key() {
    let t = theme(false);
    let a = assets();
    let tf = WidgetPod::new(TextField::new()).with_key("w").focusable(true);
    let mut h = Harness::new(page(tf), SCREEN, &t, &a);
    h.frame();
    h.tap(centre(h.find_rect("w").unwrap()));
    let count = |a: &kaia_ui::SharedAssets| a.borrow().fonts.layout_count();
    let mut per_key = Vec::new();
    for n in 0..120 {
        let before = count(&a);
        h.key(Key::Char((b'a' + (n % 26) as u8) as char));
        h.frame();
        per_key.push(count(&a) - before);
    }
    let worst = *per_key.iter().max().unwrap();
    assert!(
        worst <= 4,
        "shaping calls per key stay constant, worst was {worst}"
    );
}

#[test]
fn a_long_list_row_value_does_not_squeeze_the_title() {
    let t = theme(false);
    let a = assets();
    let title_area = |value: &str| {
        let row = WidgetPod::new(ListRow::new("A reasonably long title").value(value)).with_key("w");
        let mut h = Harness::new(
            Column::new(vec![row]).cross_align(CrossAlign::Stretch),
            SCREEN,
            &t,
            &a,
        );
        let pm = h.frame().clone();
        let r = h.find_rect("w").unwrap();
        // The title sits at the left. A half-width value leaves it the left 40 percent.
        let mut left = Rect::new(r.x, r.y, r.w * 2 / 5, r.h);
        left.h = left.h.saturating_sub(4);
        (0..left.h as i32)
            .flat_map(|dy| (0..left.w as i32).map(move |dx| (left.x + dx, left.y + dy)))
            .map(|(x, y)| pm.get(x, y).unwrap().r)
            .collect::<Vec<u8>>()
    };
    let long = "A very long value that would take the whole row if it were measured without a limit";
    assert_eq!(
        title_area(long),
        title_area("5"),
        "the title is laid out the same with a long value"
    );
}

#[test]
fn a_window_can_resize_for_a_rotated_screen() {
    use kaia_events::DirtyQueue;
    use kaia_gfx::{PixelFormat, Pixmap, Size};
    use kaia_ui::components::Align;
    use kaia_ui::{Alignment, Window};

    let t = theme(false);
    let a = assets();
    let label = WidgetPod::new(kaia_ui::components::Label::new(
        "Centered",
        t.font.title.clone(),
        Tone::Ink,
    ))
    .with_key("w");
    let root = Frame::new(WidgetPod::new(Align::new(
        label,
        Alignment::Center,
        Alignment::Center,
    )))
    .fill(Tone::Surface);
    let portrait = Size::new(600, 800);
    let window = Window::new(root, t.clone(), a.clone(), portrait);
    let mut frame = Pixmap::new(portrait.w, portrait.h, PixelFormat::Gray8);
    let mut dirty = DirtyQueue::new(kaia_events::Rect::new(0, 0, 600, 800));
    window.paint(&mut frame, &mut dirty);
    let before = window.find_rect("w").unwrap();
    assert_eq!(before.x + before.w as i32 / 2, 300);

    let landscape = Size::new(800, 600);
    window.resize(landscape);
    assert_eq!(window.screen(), landscape);
    let mut frame = Pixmap::new(landscape.w, landscape.h, PixelFormat::Gray8);
    let mut dirty = DirtyQueue::new(kaia_events::Rect::new(0, 0, 800, 600));
    assert!(window.paint(&mut frame, &mut dirty));
    let after = window.find_rect("w").unwrap();
    assert_eq!(
        after.x + after.w as i32 / 2,
        400,
        "the label follows the new width"
    );
    assert!(dark_in(&frame, after) > 100, "and is drawn there");
}
