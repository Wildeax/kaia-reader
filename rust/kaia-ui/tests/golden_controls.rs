//! Golden PNG tests for the controls, on the Libra Colour panel.
//!
//! `controls_gray.png` shows every control in one screen, with the first
//! focusable control focused. `keyboard_gray.png` shows the on-screen keyboard
//! under a focused text field. Regenerate with
//! `KAIA_UPDATE_GOLDEN=1 cargo test -p kaia-ui --test golden_controls`.

mod common;

use std::path::Path;

use common::*;
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::{Point, Rect};
use kaia_input::Key;
use kaia_ui::components::{Column, CrossAlign, Expanded, Label, Padding, ProgressBar, Row, Spacer};
use kaia_ui::controls::{
    Button, ButtonStyle, Checkbox, IconButton, OnScreenKeyboard, RadioGroup, Slider, Stepper, TextField,
    Toggle, ENGLISH,
};
use kaia_ui::theme::Theme;
use kaia_ui::{Harness, Insets, Tone, WidgetPod};

fn section(t: &Theme, text: &str) -> WidgetPod {
    WidgetPod::new(Label::new(text, t.font.label.clone(), Tone::InkSecondary))
}

fn controls_screen(t: &Theme) -> Padding {
    let buttons = Row::new(vec![
        WidgetPod::new(
            Button::new(ButtonStyle::Primary)
                .text("Open")
                .icon("book.opened")
                .on_action("open"),
        ),
        WidgetPod::new(
            Button::new(ButtonStyle::Secondary)
                .text("Cancel")
                .on_action("cancel"),
        )
        .with_key("secondary")
        .focusable(true),
        WidgetPod::new(Button::new(ButtonStyle::Text).text("More")),
        WidgetPod::new(Button::new(ButtonStyle::Primary).text("Save").enabled(false)),
    ])
    .gap(t.space.md)
    .cross_align(CrossAlign::Center);

    let icons = Row::new(vec![
        WidgetPod::new(IconButton::new("appbar.search")),
        WidgetPod::new(IconButton::new("appbar.menu")),
        WidgetPod::new(IconButton::new("home")),
        WidgetPod::new(IconButton::new("edit")),
        WidgetPod::new(IconButton::new("plus").enabled(false)),
    ])
    .gap(t.space.md)
    .cross_align(CrossAlign::Center);

    let switches = Row::new(vec![
        WidgetPod::new(Toggle::new(false)),
        WidgetPod::new(Toggle::new(true)),
        WidgetPod::new(Checkbox::new(true).label("Notify")),
        WidgetPod::new(Checkbox::new(false).label("Sync")),
    ])
    .gap(t.space.lg)
    .cross_align(CrossAlign::Center);

    let choice = RadioGroup::new(
        vec!["Serif".to_string(), "Sans".to_string(), "Mono".to_string()],
        Some(1),
    );

    let values = Row::new(vec![
        WidgetPod::new(Stepper::new(12.0, 8.0, 36.0, 2.0).format(|v| format!("{v:.0} pt"))),
        WidgetPod::new(Expanded::new(WidgetPod::new(ProgressBar::new(0.42)), 1)),
    ])
    .gap(t.space.lg)
    .cross_align(CrossAlign::Center);

    let slider = Slider::new(4.0, 0.0, 10.0, 1.0).format(|v| format!("{v:.0}"));

    let text = WidgetPod::new(
        TextField::new()
            .with_text("Gethen")
            .placeholder("Search")
            .max_length(40),
    )
    .focusable(true);

    let page = Column::new(vec![
        section(t, "Buttons"),
        WidgetPod::new(buttons),
        section(t, "Icon buttons"),
        WidgetPod::new(icons),
        section(t, "Switches"),
        WidgetPod::new(switches),
        section(t, "Choice"),
        WidgetPod::new(choice),
        section(t, "Value"),
        WidgetPod::new(values),
        WidgetPod::new(slider),
        section(t, "Text"),
        text,
        WidgetPod::new(Spacer::new(1)),
    ])
    .gap(t.space.md);

    Padding::new(WidgetPod::new(page), Insets::all(t.space.lg))
}

fn golden(name: &str) -> std::path::PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden")).join(name)
}

#[test]
fn controls_screen_matches_golden() {
    let t = theme(false);
    let mut h = Harness::new(controls_screen(&t), SCREEN, &t, &assets());
    h.frame();
    // Page-turn moves focus to the first focusable control, the secondary button.
    h.key(Key::RPgFwd);
    let pm = h.frame().clone();
    assert_matches_golden(&pm, &golden("controls_gray.png"), 0);
}

fn keyboard_screen(t: &Theme) -> Column {
    let field = TextField::new().with_text("Kaia").placeholder("Title");
    Column::new(vec![
        WidgetPod::new(Padding::new(
            WidgetPod::new(field).focusable(true),
            Insets::all(t.space.lg),
        ))
        .with_key("field-pad"),
        WidgetPod::new(Spacer::new(1)),
        WidgetPod::new(OnScreenKeyboard::new(&ENGLISH)),
    ])
}

#[test]
fn keyboard_screen_matches_golden() {
    let t = theme(false);
    let mut h = Harness::new(keyboard_screen(&t), SCREEN, &t, &assets());
    h.frame();
    let field: Rect = h.find_rect("field-pad").expect("the field is laid out");
    // Tap the field so it takes focus and shows the caret and ring.
    h.tap(Point::new(
        field.x + field.w as i32 / 2,
        field.y + field.h as i32 / 2,
    ));
    let pm = h.frame().clone();
    assert_matches_golden(&pm, &golden("keyboard_gray.png"), 0);
}
