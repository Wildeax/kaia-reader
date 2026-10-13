//! Pointer capture (UI-H6), focus requests (UI-H7) and page-turn keys (UI-M10).

mod common;

use common::*;
use kaia_gfx::Point;
use kaia_input::{Gesture, Key};
use kaia_ui::components::{Column, CrossAlign, Frame, Label};
use kaia_ui::controls::{OnScreenKeyboard, Slider, TextField, ENGLISH};
use kaia_ui::theme::Tone;
use kaia_ui::widget::{Handled, UiEvent};
use kaia_ui::{Harness, WidgetPod};

fn two_sliders() -> Frame {
    let col = Column::new(vec![
        WidgetPod::new(Slider::new(50.0, 0.0, 100.0, 1.0)).with_key("light"),
        WidgetPod::new(Slider::new(50.0, 0.0, 100.0, 1.0)).with_key("warmth"),
    ])
    .cross_align(CrossAlign::Stretch);
    Frame::new(WidgetPod::new(col)).fill(Tone::Surface)
}

fn values(h: &mut Harness) -> (f64, f64) {
    let (mut light, mut warmth) = (0.0, 0.0);
    h.update::<Slider, _>("light", |s, _| light = s.value());
    h.update::<Slider, _>("warmth", |s, _| warmth = s.value());
    (light, warmth)
}

#[test]
fn a_drag_that_leaves_a_slider_keeps_moving_that_slider() {
    let t = theme(false);
    let a = assets();
    let mut h = Harness::new(two_sliders(), SCREEN, &t, &a);
    h.frame();
    let l = h.find_rect("light").unwrap();
    let w = h.find_rect("warmth").unwrap();
    // Drag the light slider to the right while the finger drifts down onto the next slider.
    let from = Point::new(l.x + l.w as i32 / 2, l.y + l.h as i32 / 2);
    let to = Point::new(l.x + l.w as i32 * 3 / 4, w.y + w.h as i32 / 2);
    h.swipe(from, to);
    let (light, warmth) = values(&mut h);
    assert!(
        light > 50.0,
        "the slider under the touch followed the drag: {light}"
    );
    assert_eq!(warmth, 50.0, "the slider the finger drifted over did not change");
}

#[test]
fn pan_gestures_follow_the_touch_not_the_finger() {
    let t = theme(false);
    let a = assets();
    let mut h = Harness::new(two_sliders(), SCREEN, &t, &a);
    h.frame();
    let l = h.find_rect("light").unwrap();
    let w = h.find_rect("warmth").unwrap();
    let start = Point::new(l.x + 200, l.y + l.h as i32 / 2);
    let over_warmth = Point::new(l.x + 900, w.y + w.h as i32 / 2);
    let win = h.window().clone();
    let send = |h: &mut Harness, g: Gesture| win.dispatch(UiEvent::Gesture(g), h.ctx());
    send(
        &mut h,
        Gesture::Touch {
            pos: start,
            time_us: 0,
        },
    );
    send(
        &mut h,
        Gesture::Pan {
            pos: over_warmth,
            start_pos: start,
            direction: None,
            distance: 900.0,
            time_us: 1000,
        },
    );
    send(
        &mut h,
        Gesture::PanRelease {
            pos: over_warmth,
            time_us: 2000,
        },
    );
    let (light, warmth) = values(&mut h);
    assert!(light > 50.0, "light followed the pan: {light}");
    assert_eq!(warmth, 50.0);
    // The capture ended with the release: the next touch on warmth reaches warmth.
    let on_warmth = Point::new(w.x + w.w as i32 / 4, w.y + w.h as i32 / 2);
    h.tap(on_warmth);
    let (_, warmth) = values(&mut h);
    assert_ne!(warmth, 50.0, "a later tap reaches the slider under it");
}

/// A text field in a column with the on-screen keyboard. `focusable` marks the pod.
fn typing_page(focusable: bool) -> Harness {
    let t = theme(false);
    let a = assets();
    let col = Column::new(vec![
        WidgetPod::new(TextField::new())
            .with_key("f")
            .focusable(focusable),
        WidgetPod::new(OnScreenKeyboard::new(&ENGLISH)).with_key("kb"),
    ])
    .cross_align(CrossAlign::Stretch);
    Harness::new(
        Frame::new(WidgetPod::new(col)).fill(Tone::Surface),
        SCREEN,
        &t,
        &a,
    )
}

fn type_first_key(h: &mut Harness) -> String {
    let t = theme(false);
    h.frame();
    let f = h.find_rect("f").unwrap();
    h.tap(Point::new(f.x + 20, f.y + f.h as i32 / 2));
    let kb = h.find_rect("kb").unwrap();
    let boxes = OnScreenKeyboard::new(&ENGLISH).key_boxes(&t, kb.w);
    let k = &boxes[0];
    h.tap(Point::new(
        kb.x + k.rect.x + k.rect.w as i32 / 2,
        kb.y + k.rect.y + k.rect.h as i32 / 2,
    ));
    let mut text = String::new();
    h.update::<TextField, _>("f", |tf, _| text = tf.text().to_string());
    text
}

#[test]
fn the_keyboard_types_into_a_text_field_that_was_not_marked_focusable() {
    let typed = type_first_key(&mut typing_page(false));
    assert!(
        !typed.is_empty(),
        "the tap focused the field, so the key reached it"
    );
    assert_eq!(typed, type_first_key(&mut typing_page(true)));
}

#[test]
fn a_page_turn_key_with_no_focusable_widget_is_not_handled() {
    let t = theme(false);
    let a = assets();
    let col = Column::new(vec![WidgetPod::new(Label::new(
        "Plain",
        t.font.body.clone(),
        Tone::Ink,
    ))]);
    let mut h = Harness::new(col, SCREEN, &t, &a);
    for key in [Key::LPgFwd, Key::RPgFwd, Key::LPgBack, Key::RPgBack] {
        assert_eq!(h.key(key), Handled::No, "{key:?} is left to the app");
    }
    assert_eq!(h.focus(), None);
}

#[test]
fn a_page_turn_key_with_a_focusable_widget_moves_focus() {
    let t = theme(false);
    let a = assets();
    let col = Column::new(vec![WidgetPod::new(TextField::new())
        .with_key("f")
        .focusable(true)]);
    let mut h = Harness::new(col, SCREEN, &t, &a);
    assert_eq!(h.key(Key::RPgFwd), Handled::Yes);
    assert!(h.focus().is_some());
}
