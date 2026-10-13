//! Golden PNG tests for a sample screen built from the core components.
//!
//! The screen follows the Home card in DESIGN.md: a header, a card with an
//! icon, a title, a paragraph and a progress bar with the percent.
//! Regenerate with `KAIA_UPDATE_GOLDEN=1 cargo test -p kaia-ui --test golden`.

mod common;

use std::path::Path;

use common::*;
use kaia_gfx::golden::assert_matches_golden;
use kaia_ui::components::{
    Column, CrossAlign, Divider, Expanded, Frame, Icon, Label, Padding, Paragraph, ProgressBar, Row, Spacer,
};
use kaia_ui::theme::Theme;
use kaia_ui::{Insets, Tone, WidgetPod};

fn home_screen(t: &Theme) -> Padding {
    let dp = |v| t.scale.dp(v);

    let header = Row::new(vec![
        WidgetPod::new(Label::new("Kaia", t.font.title.clone(), Tone::Ink)),
        WidgetPod::new(Spacer::new(1)),
        WidgetPod::new(Label::new("84%", t.font.caption.clone(), Tone::InkSecondary)),
        WidgetPod::new(Icon::new("appbar.menu", dp(40), Tone::Ink)),
    ])
    .gap(t.space.md)
    .cross_align(CrossAlign::Center);

    let cover = Frame::new(WidgetPod::new(Icon::new(
        "book.opened",
        dp(120),
        Tone::InkSecondary,
    )))
    .border(Tone::Divider, t.line.divider)
    .radius(t.radius.button)
    .padding(Insets::all(t.space.md));

    let details = Column::new(vec![
        WidgetPod::new(Label::new(
            "The Left Hand of Darkness",
            t.font.book_title.clone(),
            Tone::Ink,
        )),
        WidgetPod::new(Label::new(
            "Ursula K. Le Guin",
            t.font.caption.clone(),
            Tone::InkSecondary,
        )),
        WidgetPod::new(
            Paragraph::new(
                "Genly Ai is sent to Gethen, a winter world where the people have no fixed sex.",
                t.font.body.clone(),
                Tone::Ink,
            )
            .max_lines(3),
        ),
        WidgetPod::new(ProgressBar::new(0.42)),
        WidgetPod::new(Label::new(
            "Chapter 7",
            t.font.caption.clone(),
            Tone::InkSecondary,
        )),
    ])
    .gap(t.space.sm)
    .shrink();

    let card = Frame::new(WidgetPod::new(
        Row::new(vec![
            WidgetPod::new(cover),
            WidgetPod::new(Expanded::new(WidgetPod::new(details), 1)),
        ])
        .gap(t.space.lg)
        .cross_align(CrossAlign::Start),
    ))
    .fill(Tone::SurfaceAlt)
    .radius(t.radius.card)
    .padding(Insets::all(t.space.lg));

    let page = Column::new(vec![
        WidgetPod::new(header),
        WidgetPod::new(Divider::new(Tone::Divider)),
        WidgetPod::new(Label::new(
            "Continue reading",
            t.font.label.clone(),
            Tone::InkSecondary,
        )),
        WidgetPod::new(card),
        WidgetPod::new(Spacer::new(1)),
        WidgetPod::new(Label::new(
            "Library: 128 books",
            t.font.caption.clone(),
            Tone::InkSecondary,
        )),
    ])
    .gap(t.space.md);

    Padding::new(WidgetPod::new(page), Insets::all(t.space.lg))
}

fn golden(name: &str) -> std::path::PathBuf {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden")).join(name)
}

#[test]
fn home_screen_grayscale_matches_golden() {
    let t = theme(false);
    let pm = kaia_ui::render(home_screen(&t), SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("home_gray.png"), 0);
}

#[test]
fn home_screen_color_matches_golden() {
    let t = theme(true);
    let pm = kaia_ui::render(home_screen(&t), SCREEN, &t, &assets());
    assert_matches_golden(&pm, &golden("home_color.png"), 0);
}
