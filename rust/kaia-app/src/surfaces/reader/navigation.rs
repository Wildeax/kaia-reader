//! Page entry, tap zones, turn gestures, brightness swipes and quick flip timers.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use kaia_events::{Ctx, Settings, TaskId, Value};
use kaia_ui::widget::PaintHint;

use super::commands::{self, GestureCommand};
use super::page::{PageView, PAGE_KEY};
use super::state::{schedule_if_pending, Core, Turn};
use crate::surface::{Outcome, SurfaceEnv};
use crate::surfaces::settings::keys;

/// The tap layout from `reading.tap_zones`. Each layout splits the page into zones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TapLayout {
    /// The left third goes back, the middle third opens the menu, and the right third goes forward.
    Default,
    /// A strip the width of the left third goes back. A box at the bottom centre opens the menu.
    /// The rest goes forward.
    ForwardRight,
    /// The mirror of `ForwardRight`: a strip on the right goes back.
    ForwardLeft,
    /// The top third goes back, the middle third opens the menu, and the bottom third goes forward.
    Vertical,
}

/// Where a tap on the page goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TapZone {
    Back,
    Menu,
    Forward,
}

/// Which touches turn a page, from `reading.turn_gesture`. Taps that open the menu
/// and follow links work in every mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnGesture {
    TapSwipe,
    Swipe,
    Tap,
}

impl TurnGesture {
    /// Whether a tap in a back or forward zone turns the page.
    pub(super) fn taps_turn(self) -> bool {
        self != TurnGesture::Swipe
    }

    /// Whether a swipe turns the page.
    pub(super) fn swipes_turn(self) -> bool {
        self != TurnGesture::Tap
    }
}

/// The layout the setting names. The old presets map to the layout with the nearest
/// page turn zones. `left_handed` maps to `ForwardLeft`, and `page_only` maps to
/// `ForwardRight`. An unknown or unset value is the default layout.
pub(super) fn tap_layout(settings: &Settings) -> TapLayout {
    match settings.read(keys::READING_TAP_ZONES).as_str() {
        Some("forward_right" | "page_only") => TapLayout::ForwardRight,
        Some("forward_left" | "left_handed") => TapLayout::ForwardLeft,
        Some("vertical") => TapLayout::Vertical,
        _ => TapLayout::Default,
    }
}

/// The turn gesture the setting names. An unknown or unset value is `tap_swipe`.
pub(super) fn turn_gesture(settings: &Settings) -> TurnGesture {
    match settings.read(keys::READING_TURN_GESTURE).as_str() {
        Some("swipe") => TurnGesture::Swipe,
        Some("tap") => TurnGesture::Tap,
        _ => TurnGesture::TapSwipe,
    }
}

/// The zone of a tap at `(x, y)` on a page of `w` by `h` pixels. The bottom box of the
/// two forward layouts is the bottom quarter of the centre third.
pub(super) fn tap_zone(layout: TapLayout, x: f64, y: f64, w: f64, h: f64) -> TapZone {
    let (left, right) = (w / 3.0, 2.0 * w / 3.0);
    let centre_column = x >= left && x < right;
    let bottom_box = centre_column && y >= 3.0 * h / 4.0;
    match layout {
        TapLayout::Default if x < left => TapZone::Back,
        TapLayout::Default if centre_column => TapZone::Menu,
        TapLayout::Default => TapZone::Forward,
        TapLayout::ForwardRight if x < left => TapZone::Back,
        TapLayout::ForwardRight if bottom_box => TapZone::Menu,
        TapLayout::ForwardRight => TapZone::Forward,
        TapLayout::ForwardLeft if x >= right => TapZone::Back,
        TapLayout::ForwardLeft if bottom_box => TapZone::Menu,
        TapLayout::ForwardLeft => TapZone::Forward,
        TapLayout::Vertical if y < h / 3.0 => TapZone::Back,
        TapLayout::Vertical if y < 2.0 * h / 3.0 => TapZone::Menu,
        TapLayout::Vertical => TapZone::Forward,
    }
}

#[derive(Default)]
pub(super) struct Gestures {
    light_start: Option<u8>,
    light_write: Option<Duration>,
    light_hide: Option<TaskId>,
    flip_zone: Option<u8>,
    flip_timer: Option<TaskId>,
    fixed_page: bool,
}

/// Run before the document command queue, including while a frame is rendering.
pub(super) fn command(
    rc: &Rc<RefCell<Core>>,
    id: &str,
    args: &[Value],
    env: &mut SurfaceEnv<'_>,
) -> Option<Outcome> {
    let Some(command) = commands::gesture(id, args) else {
        if matches!(id, "reader:tap" | "reader:overlay" | "reader:hold") || id.starts_with("reader:panel:") {
            stop_flip(&mut rc.borrow_mut(), env.ctx);
        }
        return None;
    };
    match command {
        GestureCommand::Brightness { delta, release } => {
            let mut c = rc.borrow_mut();
            stop_flip(&mut c, env.ctx);
            let now = env.ctx.scheduler.now();
            let start = *c
                .gestures
                .light_start
                .get_or_insert_with(|| env.services.platform.frontlight.intensity());
            let header = kaia_gfx::Rect::new(0, 0, c.viewport.w, env.theme().touch_target.min(c.viewport.h));
            let level = (f64::from(start) + delta).round().clamp(0.0, 100.0) as u8;
            if release
                || c.gestures
                    .light_write
                    .is_none_or(|last| now.saturating_sub(last) >= Duration::from_millis(50))
            {
                crate::surfaces::settings::quick::command(
                    "settings:quick:frontlight",
                    &[Value::Number(f64::from(level))],
                    env,
                );
                c.gestures.light_write = Some(now);
            }
            if let Some(task) = c.gestures.light_hide.take() {
                env.ctx.scheduler.unschedule(task);
            }
            if let Some(window) = &c.page_window {
                window.update::<PageView, _>(env.ctx, PAGE_KEY, |view, cx| {
                    if view.brightness != Some(level) {
                        view.brightness = Some(level);
                        cx.request_paint_rect(header, PaintHint::Ui);
                    }
                });
            }
            if release {
                c.gestures.light_start = None;
                c.gestures.light_write = None;
                let weak = Rc::downgrade(rc);
                c.gestures.light_hide = Some(env.ctx.scheduler.schedule_in(
                    2.0,
                    Box::new(move |ctx| {
                        if let Some(rc) = weak.upgrade() {
                            let mut c = rc.borrow_mut();
                            c.gestures.light_hide = None;
                            if let Some(window) = &c.page_window {
                                window.update::<PageView, _>(ctx, PAGE_KEY, |view, cx| {
                                    view.brightness = None;
                                    cx.request_paint_rect(header, PaintHint::Ui);
                                });
                            }
                        }
                    }),
                ));
            }
        }
        GestureCommand::Flip(zone) => {
            let mut c = rc.borrow_mut();
            if c.closed || c.overlay.is_some() || c.panel.is_some() {
                return Some(Outcome::Handled);
            }
            if c.gestures.flip_zone == Some(zone) {
                return Some(Outcome::Handled);
            }
            if let Some(task) = c.gestures.flip_timer.take() {
                env.ctx.scheduler.unschedule(task);
            }
            let changing_zone = c.gestures.flip_zone.replace(zone).is_some();
            if let Some(window) = &c.page_window {
                window.update::<PageView, _>(env.ctx, PAGE_KEY, |view, cx| {
                    view.flipping = true;
                    cx.request_paint_with(PaintHint::Fast);
                });
            }
            if changing_zone {
                if let Some(interval) = flip_interval(zone, c.gestures.fixed_page) {
                    schedule_flip(rc, &mut c, env.ctx, interval);
                }
            } else {
                drop(c);
                flip_step(rc, env.ctx);
            }
        }
        GestureCommand::FlipStop => stop_flip(&mut rc.borrow_mut(), env.ctx),
    }
    Some(Outcome::Handled)
}

fn flip_interval(zone: u8, fixed_page: bool) -> Option<Duration> {
    match zone {
        0 | 4 => Some(Duration::from_millis(if fixed_page { 150 } else { 300 })),
        1 | 3 => Some(Duration::from_millis(600)),
        _ => None,
    }
}

fn flip_step(rc: &Rc<RefCell<Core>>, ctx: &mut Ctx) {
    let mut c = rc.borrow_mut();
    c.gestures.flip_timer = None;
    let Some(zone) = c.gestures.flip_zone else { return };
    if c.closed || c.overlay.is_some() || c.panel.is_some() {
        stop_flip(&mut c, ctx);
        return;
    }
    if let Some(session) = c.session.as_ref() {
        c.gestures.fixed_page = !session.is_reflowable();
    }
    let Some(interval) = flip_interval(zone, c.gestures.fixed_page) else {
        return;
    };
    // A missed deadline never becomes a queued turn or a burst of catch-up work.
    if c.job.is_none()
        && !c.layout_pending
        && c.queued.is_empty()
        && c.events.is_empty()
        && c.turn(ctx, zone > 2) == Turn::Moved
    {
        c.set_pending_hint(PaintHint::Fast);
    }
    schedule_flip(rc, &mut c, ctx, interval);
    drop(c);
    schedule_if_pending(rc, ctx);
}

fn schedule_flip(rc: &Rc<RefCell<Core>>, c: &mut Core, ctx: &mut Ctx, interval: Duration) {
    let weak = Rc::downgrade(rc);
    c.gestures.flip_timer = Some(ctx.scheduler.schedule_in(
        interval.as_secs_f64(),
        Box::new(move |ctx| {
            if let Some(rc) = weak.upgrade() {
                flip_step(&rc, ctx);
            }
        }),
    ));
}

fn stop_flip(c: &mut Core, ctx: &mut Ctx) {
    if c.gestures.flip_zone.take().is_none() {
        return;
    }
    if let Some(task) = c.gestures.flip_timer.take() {
        ctx.scheduler.unschedule(task);
    }
    c.pages_since_full = 0;
    c.set_pending_hint(PaintHint::Full);
    let pending = c.job.is_some();
    if let Some(window) = &c.page_window {
        window.update::<PageView, _>(ctx, PAGE_KEY, |view, cx| {
            view.flipping = false;
            // An in-flight frame supplies the clearing refresh when it arrives.
            if !pending {
                cx.request_paint_with(PaintHint::Full);
            }
        });
    }
}

pub(super) fn interrupt(rc: &Rc<RefCell<Core>>, event: &str, ctx: &mut Ctx) {
    if !matches!(event, "Suspend" | "ScreenResized" | "Close") {
        return;
    }
    let mut c = rc.borrow_mut();
    stop_flip(&mut c, ctx);
    if let Some(task) = c.gestures.light_hide.take() {
        ctx.scheduler.unschedule(task);
    }
    c.gestures.light_start = None;
    c.gestures.light_write = None;
    if let Some(window) = &c.page_window {
        window.update::<PageView, _>(ctx, PAGE_KEY, |view, cx| {
            view.brightness = None;
            view.cancel_gestures();
            if let Some(ink) = &mut view.ink {
                ink.cancel();
                cx.request_paint();
            }
        });
    }
}

pub(super) fn entered_page(text: &str, percent: bool, count: usize) -> Option<u32> {
    if count == 0 {
        return None;
    }
    let value: f64 = text.trim().trim_end_matches('%').trim().parse().ok()?;
    if !value.is_finite() {
        return None;
    }
    if percent {
        if !(0.0..=100.0).contains(&value) {
            return None;
        }
        Some(((value / 100.0 * count as f64).round() as i64 - 1).clamp(0, count as i64 - 1) as u32)
    } else if value >= 1.0 && value <= count as f64 && value.fract() == 0.0 {
        Some(value as u32 - 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kaia_events::{Clock, ManualClock};

    #[test]
    fn quick_flip_intervals_match_the_five_zones() {
        for (fixed, expected) in [
            (false, [Some(300), Some(600), None, Some(600), Some(300)]),
            (true, [Some(150), Some(600), None, Some(600), Some(150)]),
        ] {
            for (zone, ms) in expected.into_iter().enumerate() {
                assert_eq!(flip_interval(zone as u8, fixed), ms.map(Duration::from_millis));
            }
        }
    }

    #[test]
    fn a_busy_worker_skips_flip_steps_and_release_cancels_the_timer() {
        let clock = ManualClock::new();
        let mut ctx = Ctx::new(Box::new(clock.clone()), kaia_events::Rect::new(0, 0, 100, 100));
        let core = Rc::new(RefCell::new(Core::new(1)));
        let (_send, receive) = std::sync::mpsc::channel();
        core.borrow_mut().job = Some(receive);
        core.borrow_mut().gestures.flip_zone = Some(4);
        flip_step(&core, &mut ctx);
        for _ in 0..10 {
            clock.advance(Duration::from_millis(300));
            ctx.run_due(clock.now());
            assert!(core.borrow().queued.is_empty());
            assert!(core.borrow().job.is_some());
        }
        stop_flip(&mut core.borrow_mut(), &mut ctx);
        assert!(core.borrow().gestures.flip_zone.is_none());
        assert!(core.borrow().gestures.flip_timer.is_none());
        clock.advance(Duration::from_secs(10));
        ctx.run_due(clock.now());
        assert!(core.borrow().queued.is_empty());
    }

    #[test]
    fn center_zone_does_not_schedule_turns() {
        let clock = ManualClock::new();
        let mut ctx = Ctx::new(Box::new(clock), kaia_events::Rect::new(0, 0, 100, 100));
        let core = Rc::new(RefCell::new(Core::new(1)));
        core.borrow_mut().gestures.flip_zone = Some(2);
        flip_step(&core, &mut ctx);
        assert!(core.borrow().gestures.flip_timer.is_none());
        assert!(core.borrow().job.is_none());
    }

    #[test]
    fn validates_page_and_percent_boundaries() {
        for (input, expected) in [
            ("1", Some(0)),
            ("10", Some(9)),
            ("0", None),
            ("11", None),
            ("1.5", None),
        ] {
            assert_eq!(entered_page(input, false, 10), expected);
        }
        for (input, expected) in [
            ("0", Some(0)),
            ("50%", Some(4)),
            ("100", Some(9)),
            ("-1", None),
            ("101", None),
            ("NaN", None),
            ("inf", None),
            ("", None),
        ] {
            assert_eq!(entered_page(input, true, 10), expected);
        }
        assert_eq!(entered_page("1", false, 0), None);
    }
}
