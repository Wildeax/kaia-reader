//! Dirty tracking: what a frame repaints and which refresh rectangles it queues.

mod common;

use common::*;
use kaia_events::{Event, Rect as ERect, Refresh, RefreshMode};
use kaia_gfx::{Color, Pixmap, Rect, Size};
use kaia_input::Key;
use kaia_ui::components::Column;
use kaia_ui::widget::{EventCx, Handled, LayoutCx, PaintCx, PaintHint, UiEvent, Widget};
use kaia_ui::{Constraints, Harness, Tone, WidgetPod};

const SCREEN: Size = Size::new(300, 300);

/// A 200 x 40 box. Named event "toggle" flips its tone and repaints the whole
/// box. "partial" flips it and repaints a 20 x 10 part with the partial hint.
struct Toggler {
    on: bool,
}

impl Widget for Toggler {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(200, 40))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let color = if self.on {
            cx.color(Tone::Ink)
        } else {
            cx.color(Tone::SurfaceAlt)
        };
        let b = cx.bounds();
        cx.fill_rect(b, color);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        match ev {
            UiEvent::Named(e) if e.name == "toggle" => {
                self.on = !self.on;
                cx.request_paint();
                Handled::Yes
            }
            UiEvent::Named(e) if e.name == "partial" => {
                self.on = !self.on;
                cx.request_paint_rect(Rect::new(10, 5, 20, 10), PaintHint::Partial);
                Handled::Yes
            }
            _ => Handled::No,
        }
    }
}

/// Fills 500 x 500 from its origin when it paints, but its bounds are 50 x 50.
/// Named event "bleed" flips the fill and repaints the bounds only. The clip
/// must keep the extra pixels off the frame.
struct Bleeder {
    on: bool,
}

impl Widget for Bleeder {
    kaia_ui::impl_as_any!();
    fn layout(&mut self, _cx: &mut LayoutCx, c: Constraints) -> Size {
        c.constrain(Size::new(50, 50))
    }

    fn paint(&mut self, cx: &mut PaintCx) {
        let color = if self.on {
            Color::gray(0x10)
        } else {
            Color::gray(0xF0)
        };
        let b = cx.bounds();
        cx.fill_rect(Rect::new(b.x, b.y, 500, 500), color);
    }

    fn event(&mut self, cx: &mut EventCx, ev: &UiEvent) -> Handled {
        if let UiEvent::Named(e) = ev {
            if e.name == "bleed" {
                self.on = !self.on;
                cx.request_paint();
                return Handled::Yes;
            }
        }
        Handled::No
    }
}

/// Bounding box of the pixels that differ between two frames.
fn changed_bounds(before: &Pixmap, after: &Pixmap) -> Option<Rect> {
    let (w, h) = (before.width() as i32, before.height() as i32);
    let mut out: Option<Rect> = None;
    for y in 0..h {
        for x in 0..w {
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

fn e_rect(x: i32, y: i32, w: i32, h: i32) -> ERect {
    ERect::new(x, y, w, h)
}

fn refresh(mode: RefreshMode, region: ERect) -> Refresh {
    Refresh {
        mode,
        region: Some(region),
        dither: false,
    }
}

/// The bleeder at y = 0 (50 high) and the toggler at y = 100, in a 300 x 300 window.
fn scene() -> Harness {
    let column = Column::new(vec![
        WidgetPod::new(Bleeder { on: false }),
        WidgetPod::new(Toggler { on: false }),
    ])
    .gap(50)
    .shrink();
    let mut h = Harness::new(column, SCREEN, &theme(false), &assets());
    h.frame();
    h
}

#[test]
fn the_first_frame_refreshes_the_whole_screen() {
    let mut h = Harness::new(Toggler { on: false }, SCREEN, &theme(false), &assets());
    h.frame();
    assert_eq!(
        h.refresh_requests(),
        &[refresh(RefreshMode::Ui, e_rect(0, 0, 300, 300))]
    );
}

#[test]
fn a_frame_with_nothing_dirty_queues_no_refresh() {
    let mut h = scene();
    h.frame();
    assert!(h.refresh_requests().is_empty(), "{:?}", h.refresh_requests());
}

#[test]
fn a_repaint_request_queues_the_widget_rectangle_and_changes_only_that_area() {
    let mut h = scene();
    let before = h.screen().clone();
    assert!(h.send(Event::new("toggle")));
    let after = h.frame().clone();

    assert_eq!(
        h.refresh_requests(),
        &[refresh(RefreshMode::Ui, e_rect(0, 100, 200, 40))]
    );
    assert_eq!(changed_bounds(&before, &after), Some(Rect::new(0, 100, 200, 40)));
}

#[test]
fn a_partial_repaint_request_queues_only_that_part() {
    let mut h = scene();
    h.send(Event::new("partial"));
    h.frame();
    assert_eq!(
        h.refresh_requests(),
        &[refresh(RefreshMode::Partial, e_rect(10, 105, 20, 10))]
    );
}

#[test]
fn pixels_outside_the_clip_are_never_written() {
    let mut h = scene();
    let before = h.screen().clone();
    // The bleeder paints 500 x 500, but only its 50 x 50 bounds are repainted.
    h.send(Event::new("bleed"));
    let after = h.frame().clone();
    assert_eq!(changed_bounds(&before, &after), Some(Rect::new(0, 0, 50, 50)));
    assert_eq!(
        h.refresh_requests(),
        &[refresh(RefreshMode::Ui, e_rect(0, 0, 50, 50))]
    );
}

#[test]
fn two_dirty_widgets_queue_two_refreshes_in_tree_order() {
    let mut h = scene();
    h.send(Event::new("bleed"));
    h.send(Event::new("toggle"));
    h.frame();
    assert_eq!(
        h.refresh_requests(),
        &[
            refresh(RefreshMode::Ui, e_rect(0, 0, 50, 50)),
            refresh(RefreshMode::Ui, e_rect(0, 100, 200, 40)),
        ]
    );
}

#[test]
fn moving_focus_repaints_the_old_and_the_new_widget() {
    let a = WidgetPod::new(Toggler { on: false }).focusable(true);
    let b = WidgetPod::new(Toggler { on: false }).focusable(true);
    let column = Column::new(vec![a, b]).shrink();
    let mut h = Harness::new(column, SCREEN, &theme(false), &assets());
    h.frame();

    h.key(Key::LPgFwd);
    h.frame();
    assert_eq!(
        h.refresh_requests(),
        &[refresh(RefreshMode::Ui, e_rect(0, 0, 200, 40))]
    );

    // The two boxes share an edge at y = 40. DirtyQueue counts shared edges as
    // overlap, as KOReader does, so the two refreshes merge into one.
    h.key(Key::RPgFwd);
    h.frame();
    assert_eq!(
        h.refresh_requests(),
        &[refresh(RefreshMode::Ui, e_rect(0, 0, 200, 80))]
    );
}

#[test]
fn hints_map_to_kaia_events_refresh_modes() {
    assert_eq!(PaintHint::Ui.mode().name(), "ui");
    assert_eq!(PaintHint::Partial.mode().name(), "partial");
    assert_eq!(PaintHint::Fast.mode().name(), "fast");
    assert_eq!(PaintHint::Full.mode().name(), "full");
    assert_eq!(PaintHint::Flash.mode().name(), "flashui");
}

#[test]
fn render_paints_the_tree_into_a_fresh_pixmap_in_the_panel_format() {
    let gray = theme(false);
    let pm = kaia_ui::render(Toggler { on: true }, SCREEN, &gray, &assets());
    assert_eq!(pm.size(), SCREEN);
    assert_eq!(pm.format(), kaia_gfx::PixelFormat::Gray8);
    // The toggler's bounds are the whole window here, so every pixel is ink.
    assert_eq!(pm.get(10, 10), Some(gray.color(Tone::Ink)));

    let color = theme(true);
    let pm = kaia_ui::render(Toggler { on: false }, SCREEN, &color, &assets());
    assert_eq!(pm.format(), kaia_gfx::PixelFormat::Rgb24);
    assert_eq!(pm.get(10, 10), Some(color.color(Tone::SurfaceAlt)));
}
