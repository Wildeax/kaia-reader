//! Behavior of the controls, driven through the harness: taps, keys, the
//! events and actions they emit, and the pixels they draw.

mod common;

use common::*;
use kaia_events::{Event, RefreshMode, Value};
use kaia_gfx::{Color, Point, Rect, Size};
use kaia_input::Key;
use kaia_ui::components::{Column, Padding};
use kaia_ui::controls::{
    Button, ButtonStyle, Checkbox, IconButton, KeyAction, Layer, OnScreenKeyboard, RadioGroup, Slider,
    Stepper, TextField, Toggle, ENGLISH,
};
use kaia_ui::theme::Theme;
use kaia_ui::{AppAction, Harness, Insets, Out, Tone, WidgetPod};

const SCREEN: Size = Size::new(300, 300);

/// One child in a shrink-wrapped column at the top left, so the child keeps its natural size.
fn at_top(child: WidgetPod) -> Column {
    Column::new(vec![child]).shrink()
}

fn centre(r: Rect) -> Point {
    Point::new(r.x + r.w as i32 / 2, r.y + r.h as i32 / 2)
}

fn pixel(h: &Harness, x: i32, y: i32) -> Color {
    h.screen().get(x, y).expect("the point is on the screen")
}

fn outbox_events(h: &Harness) -> Vec<Event> {
    h.outbox()
        .into_iter()
        .filter_map(|o| match o {
            Out::Event(e) => Some(e),
            Out::Action(_) => None,
        })
        .collect()
}

// Buttons ---------------------------------------------------------------

#[test]
fn a_tap_emits_the_action_and_the_press_ends_after_the_frame() {
    let t = theme(false);
    let button = Button::new(ButtonStyle::Secondary).text("Open").on_action("open");
    let mut h = Harness::new(
        at_top(WidgetPod::new(button).with_key("btn")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("btn").expect("the button is laid out");
    let probe = (r.x + 20, r.y + r.h as i32 / 2);

    assert!(h.tap(centre(r)));
    assert_eq!(h.outbox(), vec![Out::Action(AppAction::new("open"))]);

    // The press frame inverts the outline button: its interior is ink.
    h.frame();
    assert_eq!(pixel(&h, probe.0, probe.1), t.color(Tone::Ink));
    assert_eq!(h.refresh_requests().len(), 1);
    assert_eq!(h.refresh_requests()[0].mode, RefreshMode::Fast);

    // The release runs on the scheduler, then the frame returns to normal.
    assert_eq!(h.run_tasks(), 1);
    h.frame();
    assert_eq!(pixel(&h, probe.0, probe.1), t.color(Tone::Surface));
    assert_eq!(h.refresh_requests()[0].mode, RefreshMode::Ui);
}

#[test]
fn a_primary_button_inverts_when_pressed() {
    let t = theme(false);
    let button = Button::new(ButtonStyle::Primary).text("Save").on_action("save");
    let mut h = Harness::new(
        at_top(WidgetPod::new(button).with_key("btn")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("btn").expect("laid out");
    let probe = (r.x + 20, r.y + r.h as i32 / 2);
    assert_eq!(pixel(&h, probe.0, probe.1), t.color(Tone::Ink), "filled at rest");
    h.tap(centre(r));
    h.frame();
    assert_eq!(
        pixel(&h, probe.0, probe.1),
        t.color(Tone::Surface),
        "paper while pressed"
    );
    h.run_tasks();
    h.frame();
    assert_eq!(
        pixel(&h, probe.0, probe.1),
        t.color(Tone::Ink),
        "filled again after the release"
    );
}

#[test]
fn a_disabled_button_takes_no_tap_and_emits_nothing() {
    let t = theme(false);
    let button = Button::new(ButtonStyle::Primary)
        .text("Save")
        .enabled(false)
        .on_action("save");
    let mut h = Harness::new(
        at_top(WidgetPod::new(button).with_key("btn")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("btn").expect("laid out");
    h.tap(centre(r));
    assert!(h.outbox().is_empty());
    h.frame();
    assert!(h.refresh_requests().is_empty(), "a disabled tap changes nothing");
}

#[test]
fn a_button_is_at_least_one_touch_target_high_and_wide() {
    let t = theme(false);
    let button = Button::new(ButtonStyle::Text).icon("plus");
    let mut h = Harness::new(
        at_top(WidgetPod::new(button).with_key("btn")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("btn").expect("laid out");
    assert!(r.w >= t.touch_target, "width {}", r.w);
    assert!(r.h >= t.touch_target, "height {}", r.h);
}

#[test]
fn a_focused_button_draws_the_focus_ring_on_its_edge() {
    let t = theme(false);
    let button = Button::new(ButtonStyle::Text).text("More");
    let root = Column::new(vec![WidgetPod::new(button).with_key("btn").focusable(true)]).shrink();
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("btn").expect("laid out");
    let edge = (r.x + 2, r.y + r.h as i32 / 2);
    assert_eq!(
        pixel(&h, edge.0, edge.1),
        t.color(Tone::Surface),
        "no ring before focus"
    );

    h.key(Key::RPgFwd);
    h.frame();
    assert_eq!(pixel(&h, edge.0, edge.1), t.color(Tone::Ink), "ring after focus");
}

#[test]
fn press_on_a_focused_button_emits_its_action() {
    let t = theme(false);
    let button = Button::new(ButtonStyle::Secondary).text("OK").on_action("ok");
    let root = Column::new(vec![WidgetPod::new(button).focusable(true)]).shrink();
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    h.key(Key::RPgFwd);
    assert_eq!(h.key(Key::Press), kaia_ui::Handled::Yes);
    assert_eq!(h.outbox(), vec![Out::Action(AppAction::new("ok"))]);
}

#[test]
fn an_icon_button_emits_a_named_event() {
    let t = theme(false);
    let button = IconButton::new("home").on_event("go.home");
    let mut h = Harness::new(
        at_top(WidgetPod::new(button).with_key("btn")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("btn").expect("laid out");
    assert_eq!((r.w, r.h), (t.touch_target, t.touch_target));
    h.tap(centre(r));
    assert_eq!(outbox_events(&h), vec![Event::new("go.home")]);
}

// Toggle and checkbox ----------------------------------------------------

#[test]
fn a_toggle_flips_its_fill_and_emits_the_new_state() {
    let t = theme(false);
    let toggle = Toggle::new(false).labels("Off", "On").on_event("wifi");
    let mut h = Harness::new(
        at_top(WidgetPod::new(toggle).with_key("sw")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("sw").expect("laid out");
    let left = (r.x + 20, r.y + r.h as i32 / 2);
    let right = (r.right() - 20, r.y + r.h as i32 / 2);
    assert_eq!(
        pixel(&h, left.0, left.1),
        t.color(Tone::Ink),
        "off: the left half is inked"
    );
    assert_eq!(pixel(&h, right.0, right.1), t.color(Tone::Surface));

    h.tap(centre(r));
    assert_eq!(
        outbox_events(&h),
        vec![Event::with_args("wifi", vec![Value::Bool(true)])]
    );
    h.frame();
    assert_eq!(
        pixel(&h, left.0, left.1),
        t.color(Tone::Surface),
        "on: the left half is paper"
    );
    assert_eq!(pixel(&h, right.0, right.1), t.color(Tone::Ink));
}

#[test]
fn a_checkbox_toggles_on_tap_and_draws_a_check_mark() {
    let t = theme(false);
    let box_ = Checkbox::new(false).label("Notify").on_event("notify");
    let mut h = Harness::new(at_top(WidgetPod::new(box_).with_key("cb")), SCREEN, &t, &assets());
    h.frame();
    let before = h.screen().clone();
    let r = h.find_rect("cb").expect("laid out");
    h.tap(Point::new(r.x + 10, r.y + 10));
    assert_eq!(
        outbox_events(&h),
        vec![Event::with_args("notify", vec![Value::Bool(true)])]
    );
    let after = h.frame().clone();
    let changed = changed_bounds(&before, &after).expect("the check mark is drawn");
    assert!(
        changed.x >= r.x && changed.right() <= r.x + t.touch_target as i32,
        "{changed:?}"
    );
}

// Radio and stepper -----------------------------------------------------

#[test]
fn a_radio_group_selects_the_tapped_row() {
    let t = theme(false);
    let options = vec!["Serif".to_string(), "Sans".to_string(), "Mono".to_string()];
    let group = RadioGroup::new(options, Some(0)).on_event("font");
    let mut h = Harness::new(
        at_top(WidgetPod::new(group).with_key("rg")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("rg").expect("laid out");
    let row = t.touch_target as i32;
    h.tap(Point::new(r.x + 40, r.y + 2 * row + row / 2));
    assert_eq!(
        outbox_events(&h),
        vec![Event::with_args("font", vec![Value::Number(2.0)])]
    );
    h.update::<RadioGroup, _>("rg", |g, _| assert_eq!(g.selected(), Some(2)));
}

#[test]
fn a_focused_radio_group_moves_with_up_and_down() {
    let t = theme(false);
    let options = vec!["A".to_string(), "B".to_string()];
    let group = RadioGroup::new(options, Some(0)).on_event("pick");
    let root = Column::new(vec![WidgetPod::new(group).focusable(true)]).shrink();
    let mut h = Harness::new(root, SCREEN, &t, &assets());
    h.frame();
    h.key(Key::RPgFwd);
    h.key(Key::Down);
    assert_eq!(
        outbox_events(&h),
        vec![Event::with_args("pick", vec![Value::Number(1.0)])]
    );
    h.key(Key::Down);
    assert!(outbox_events(&h).is_empty(), "the last option stays selected");
}

#[test]
fn a_stepper_moves_by_step_and_stops_at_its_limits() {
    let t = theme(false);
    let stepper = Stepper::new(9.0, 0.0, 10.0, 1.0)
        .format(|v| format!("{v:.0} pt"))
        .on_event("size");
    let mut h = Harness::new(
        at_top(WidgetPod::new(stepper).with_key("st")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("st").expect("laid out");
    let ring = t.line.focus as i32;
    let sq = t.touch_target as i32;
    let plus = Point::new(r.right() - ring - sq / 2, r.y + r.h as i32 / 2);
    let minus = Point::new(r.x + ring + sq / 2, r.y + r.h as i32 / 2);

    h.tap(plus);
    assert_eq!(
        outbox_events(&h),
        vec![Event::with_args("size", vec![Value::Number(10.0)])]
    );
    h.tap(plus);
    assert!(outbox_events(&h).is_empty(), "plus stops at the maximum");
    h.tap(minus);
    h.tap(minus);
    assert_eq!(
        outbox_events(&h),
        vec![
            Event::with_args("size", vec![Value::Number(9.0)]),
            Event::with_args("size", vec![Value::Number(8.0)]),
        ]
    );
    h.update::<Stepper, _>("st", |s, _| assert_eq!(s.value(), 8.0));
}

// Slider ---------------------------------------------------------------

#[test]
fn a_slider_snaps_taps_and_drags_to_steps() {
    let t = theme(false);
    let slider = Slider::new(0.0, 0.0, 10.0, 1.0).on_event("volume");
    let mut h = Harness::new(
        at_top(WidgetPod::new(slider).with_key("sl")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("sl").expect("laid out");
    let y = r.y + r.h as i32 / 2;

    // Left of the track clamps to the minimum. Right of it clamps to the maximum.
    h.tap(Point::new(r.x + 1, y));
    assert!(outbox_events(&h).is_empty(), "the minimum does not change");
    h.tap(Point::new(r.right() - 1, y));
    assert_eq!(
        outbox_events(&h),
        vec![Event::with_args("volume", vec![Value::Number(10.0)])]
    );

    // A drag from the left edge to the right edge moves the value to the maximum.
    h.swipe(Point::new(r.x + 2, y), Point::new(r.right() - 2, y));
    h.update::<Slider, _>("sl", |s, _| assert_eq!(s.value(), 10.0));
}

#[test]
fn a_slider_value_is_always_a_whole_step() {
    let t = theme(false);
    let slider = Slider::new(0.0, 0.0, 10.0, 2.0);
    let mut h = Harness::new(
        at_top(WidgetPod::new(slider).with_key("sl")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    h.update::<Slider, _>("sl", |s, _| assert_eq!(s.value(), 0.0));
    h.update::<Slider, _>("sl", |s, _| s.set_value(5.2));
    h.update::<Slider, _>("sl", |s, _| {
        assert_eq!(s.value(), 6.0, "5.2 snaps to 6 on a step of 2")
    });
}

// Text field -----------------------------------------------------------

/// The x coordinate of the text area's left edge for a field at `r`.
fn text_left(t: &Theme, r: Rect) -> i32 {
    r.x + t.line.focus as i32 + t.space.sm as i32
}

fn field_root(field: TextField) -> Column {
    at_top(WidgetPod::new(field).with_key("tf").focusable(true))
}

#[test]
fn typed_characters_insert_at_the_caret_and_emit_changes() {
    let t = theme(false);
    let mut h = Harness::new(
        field_root(TextField::new().on_change("text.changed")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("tf").expect("laid out");
    h.tap(centre(r));
    h.key(Key::Char('a'));
    h.key(Key::Char('b'));
    assert_eq!(h.focus(), h.window().focus(), "the field keeps focus");
    h.update::<TextField, _>("tf", |f, _| assert_eq!(f.text(), "ab"));
    assert_eq!(
        outbox_events(&h),
        vec![
            Event::with_args("text.changed", vec![Value::Str("a".into())]),
            Event::with_args("text.changed", vec![Value::Str("ab".into())]),
        ]
    );
}

#[test]
fn backspace_and_delete_remove_characters_around_the_caret() {
    let t = theme(false);
    let field = TextField::new().with_text("abc");
    let mut h = Harness::new(field_root(field), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("tf").expect("laid out");
    h.tap(centre(r));
    h.key(Key::Backspace);
    h.update::<TextField, _>("tf", |f, _| assert_eq!(f.text(), "ab"));
    h.key(Key::Left);
    h.key(Key::Del);
    h.update::<TextField, _>("tf", |f, _| assert_eq!(f.text(), "a"));
}

#[test]
fn a_tap_places_the_caret_at_the_nearest_character() {
    let t = theme(false);
    let mut h = Harness::new(
        field_root(TextField::new().with_text("hello")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("tf").expect("laid out");
    let y = centre(r).y;
    let style = t.style(&t.font.body, Tone::Ink);
    let two = assets()
        .borrow_mut()
        .fonts
        .layout("he", &style, 1 << 20, None, false)
        .width as i32;

    h.tap(Point::new(text_left(&t, r) + two + 1, y));
    h.update::<TextField, _>("tf", |f, _| assert_eq!(f.caret(), 2));
    h.tap(Point::new(r.x + 300 - 1, y));
    h.update::<TextField, _>("tf", |f, _| assert_eq!(f.caret(), 5, "past the end is the end"));
}

#[test]
fn max_length_stops_typing() {
    let t = theme(false);
    let mut h = Harness::new(field_root(TextField::new().max_length(3)), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("tf").expect("laid out");
    h.tap(centre(r));
    for c in "abcdef".chars() {
        h.key(Key::Char(c));
    }
    h.update::<TextField, _>("tf", |f, _| assert_eq!(f.text(), "abc"));
}

#[test]
fn password_mode_draws_dots_and_keeps_the_text() {
    let t = theme(false);
    let plain = TextField::new().with_text("secret");
    let masked = TextField::new().with_text("secret").password(true);
    let mut a = Harness::new(field_root(plain), SCREEN, &t, &assets());
    let mut b = Harness::new(field_root(masked), SCREEN, &t, &assets());
    a.frame();
    b.frame();
    let differs = changed_bounds(a.screen(), b.screen());
    assert!(differs.is_some(), "the dots look different from the letters");
    b.update::<TextField, _>("tf", |f, _| assert_eq!(f.text(), "secret"));
}

#[test]
fn a_long_text_scrolls_so_the_caret_stays_visible() {
    let t = theme(false);
    let mut h = Harness::new(field_root(TextField::new()), SCREEN, &t, &assets());
    h.frame();
    let r = h.find_rect("tf").expect("laid out");
    h.tap(centre(r));
    for _ in 0..60 {
        h.key(Key::Char('w'));
    }
    h.frame();
    h.update::<TextField, _>("tf", |f, _| {
        assert_eq!(f.caret(), 60);
        assert!(f.scroll() > 0.0, "the text scrolled, got {}", f.scroll());
    });
}

#[test]
fn press_submits_the_text() {
    let t = theme(false);
    let mut h = Harness::new(
        field_root(TextField::new().with_text("go").on_submit("submit")),
        SCREEN,
        &t,
        &assets(),
    );
    h.frame();
    let r = h.find_rect("tf").expect("laid out");
    h.tap(centre(r));
    h.key(Key::Press);
    assert_eq!(
        outbox_events(&h),
        vec![Event::with_args("submit", vec![Value::Str("go".into())])]
    );
}

// On-screen keyboard ---------------------------------------------------

#[test]
fn every_layer_of_the_english_layout_fits_ten_units_per_row() {
    for layer in [ENGLISH.letters, ENGLISH.numbers, ENGLISH.symbols] {
        assert_eq!(layer.len(), 4, "four rows per layer");
        for row in layer {
            let units: f32 = row.iter().map(|k| k.width).sum();
            assert!(units <= 10.0 + 1e-6, "a row is {units} units wide");
        }
    }
}

#[test]
fn every_key_is_at_least_one_touch_target_wide_on_the_panel() {
    let t = theme(false);
    for k in OnScreenKeyboard::new(&ENGLISH).key_boxes(&t, SCREEN_W) {
        assert!(
            k.rect.w >= t.touch_target,
            "{:?} is {} px wide",
            k.action,
            k.rect.w
        );
        assert!(
            k.rect.h >= t.touch_target,
            "{:?} is {} px high",
            k.action,
            k.rect.h
        );
    }
}

const SCREEN_W: u32 = 1264;

/// A text field above the keyboard, both in a shrink column at the top.
fn keyboard_root() -> Padding {
    let column = Column::new(vec![
        WidgetPod::new(TextField::new()).with_key("tf").focusable(true),
        WidgetPod::new(OnScreenKeyboard::new(&ENGLISH).on_hide("kb.hide")).with_key("kb"),
    ])
    .shrink();
    Padding::new(WidgetPod::new(column), Insets::all(0))
}

/// The screen point of the key with `action`, on the keyboard at `kb`.
fn key_point(h: &mut Harness, t: &Theme, action: KeyAction) -> Point {
    let kb = h.find_rect("kb").expect("keyboard laid out");
    let mut boxes = Vec::new();
    h.update::<OnScreenKeyboard, _>("kb", |k, _| boxes = k.key_boxes(t, kb.w));
    let key = boxes
        .iter()
        .find(|k| k.action == action)
        .unwrap_or_else(|| panic!("no key for {action:?} on this layer"));
    centre(Rect::new(
        kb.x + key.rect.x,
        kb.y + key.rect.y,
        key.rect.w,
        key.rect.h,
    ))
}

fn focus_field(h: &mut Harness) {
    let r = h.find_rect("tf").expect("laid out");
    h.tap(centre(r));
}

fn field_text(h: &mut Harness) -> String {
    let mut out = String::new();
    h.update::<TextField, _>("tf", |f, _| out = f.text().to_string());
    out
}

#[test]
fn keys_type_into_the_focused_field() {
    let t = theme(false);
    let mut h = Harness::new(keyboard_root(), Size::new(1264, 1680), &t, &assets());
    h.frame();
    focus_field(&mut h);
    let hp = key_point(&mut h, &t, KeyAction::Char('h'));
    h.tap(hp);
    let ip = key_point(&mut h, &t, KeyAction::Char('i'));
    h.tap(ip);
    assert_eq!(field_text(&mut h), "hi");
    let sp = key_point(&mut h, &t, KeyAction::Space);
    h.tap(sp);
    let bp = key_point(&mut h, &t, KeyAction::Backspace);
    h.tap(bp);
    assert_eq!(
        field_text(&mut h),
        "hi",
        "space then delete leaves the text as it was"
    );
}

#[test]
fn shift_capitalises_one_letter_and_caps_lock_stays_on() {
    let t = theme(false);
    let mut h = Harness::new(keyboard_root(), Size::new(1264, 1680), &t, &assets());
    h.frame();
    focus_field(&mut h);
    let shift = key_point(&mut h, &t, KeyAction::Shift);
    let a = key_point(&mut h, &t, KeyAction::Char('a'));
    let b = key_point(&mut h, &t, KeyAction::Char('b'));
    h.tap(shift);
    h.tap(a);
    h.tap(b);
    assert_eq!(field_text(&mut h), "Ab", "shift lasts one letter");

    h.tap(shift);
    h.tap(shift);
    h.tap(a);
    h.tap(b);
    assert_eq!(field_text(&mut h), "AbAB", "the second shift tap locks caps");
    let mut caps = false;
    h.update::<OnScreenKeyboard, _>("kb", |k, _| caps = k.is_caps());
    assert!(caps);
}

#[test]
fn the_layer_keys_switch_to_numbers_and_symbols() {
    let t = theme(false);
    let mut h = Harness::new(keyboard_root(), Size::new(1264, 1680), &t, &assets());
    h.frame();
    focus_field(&mut h);
    let to_numbers = key_point(&mut h, &t, KeyAction::Layer(Layer::Numbers));
    h.tap(to_numbers);
    let mut layer = Layer::Letters;
    h.update::<OnScreenKeyboard, _>("kb", |k, _| layer = k.layer());
    assert_eq!(layer, Layer::Numbers);
    let one = key_point(&mut h, &t, KeyAction::Char('1'));
    h.tap(one);
    assert_eq!(field_text(&mut h), "1");

    let to_symbols = key_point(&mut h, &t, KeyAction::Layer(Layer::Symbols));
    h.tap(to_symbols);
    let hash = key_point(&mut h, &t, KeyAction::Char('#'));
    h.tap(hash);
    assert_eq!(field_text(&mut h), "1#");
}

#[test]
fn enter_submits_and_hide_emits_its_event() {
    let t = theme(false);
    let mut h = Harness::new(keyboard_root(), Size::new(1264, 1680), &t, &assets());
    h.frame();
    focus_field(&mut h);
    let hide = key_point(&mut h, &t, KeyAction::Hide);
    h.tap(hide);
    assert_eq!(outbox_events(&h), vec![Event::new("kb.hide")]);
}

/// Bounding box of the pixels that differ between two frames.
fn changed_bounds(before: &kaia_gfx::Pixmap, after: &kaia_gfx::Pixmap) -> Option<Rect> {
    let mut out: Option<Rect> = None;
    for y in 0..before.height() as i32 {
        for x in 0..before.width() as i32 {
            if before.get(x, y) != after.get(x, y) {
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

#[test]
fn an_accented_choice_keeps_its_color_after_a_press() {
    for color_panel in [false, true] {
        let t = theme(color_panel);
        let button = Button::new(ButtonStyle::Primary)
            .accent(true)
            .text("Reading")
            .on_action("filter");
        let mut h = Harness::new(
            at_top(WidgetPod::new(button).with_key("choice")),
            SCREEN,
            &t,
            &assets(),
        );
        h.frame();
        let r = h.find_rect("choice").unwrap();
        let probe = Point::new(r.x + 20, r.y + r.h as i32 / 2);
        assert_eq!(pixel(&h, probe.x, probe.y), t.color(Tone::Accent));
        h.tap(centre(r));
        h.frame();
        assert_eq!(pixel(&h, probe.x, probe.y), t.color(Tone::Surface));
        h.run_tasks();
        h.frame();
        assert_eq!(pixel(&h, probe.x, probe.y), t.color(Tone::Accent));
    }
}
