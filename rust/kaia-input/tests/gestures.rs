//! Gesture replay tests. Each test names the KOReader function whose behavior it checks.
//! Thresholds use the 160 dpi default, so 1 dp equals 1 pixel.

use kaia_gfx::Point;
use kaia_input::{
    replay, replay_with, Axis, DesktopPointer, Direction, Gesture, GestureConfig, GestureDetector,
    MouseButton, TouchEvent, Turn,
};

fn names(gestures: &[Gesture]) -> Vec<&'static str> {
    gestures.iter().map(Gesture::name).collect()
}

fn double_tap_on() -> GestureConfig {
    GestureConfig {
        double_tap_enabled: true,
        ..GestureConfig::default()
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

fn script(lines: &[&str]) -> String {
    lines.join("\n")
}

/// Tap: `GestureDetector:tapState` and `handleDoubleTap` with double tap off (the Input default).
#[test]
fn tap_emits_touch_then_tap_on_lift() {
    let g = replay(&script(&["down 0 500 800 0", "up 0 500 800 120"]));
    assert_eq!(names(&g), ["touch", "tap"]);
    assert_eq!(
        g[1],
        Gesture::Tap {
            pos: Point::new(500, 800),
            time_us: 120_000
        }
    );
}

/// Double tap off: each tap is emitted at its lift (`handleDoubleTap`, `disable_double_tap`).
#[test]
fn double_tap_off_emits_two_taps() {
    let g = replay(&script(&[
        "down 0 500 800 0",
        "up 0 500 800 100",
        "down 0 500 800 200",
        "up 0 500 800 250",
    ]));
    assert_eq!(names(&g), ["touch", "tap", "touch", "tap"]);
}

/// Double tap on: a second lift inside the window is a double tap (`isDoubleTap`, `handleDoubleTap`).
#[test]
fn double_tap_on_second_lift_inside_window() {
    let g = replay_with(
        double_tap_on(),
        &script(&[
            "down 0 500 800 0",
            "up 0 500 800 100",
            "down 0 500 800 200",
            "up 0 500 800 250",
        ]),
    );
    assert_eq!(names(&g), ["touch", "double_tap"]);
    assert_eq!(g[1].time_us(), 250_000);
}

/// `isDoubleTap` compares with `<`, so a second lift exactly 300 ms after the first is no
/// double tap. The pending tap then fires from the timer in `handleDoubleTap`.
#[test]
fn double_tap_window_is_strict_at_300ms() {
    let inside = replay_with(
        double_tap_on(),
        &script(&[
            "down 0 500 800 0",
            "up 0 500 800 100",
            "down 0 500 800 350",
            "up 0 500 800 399",
        ]),
    );
    assert_eq!(names(&inside), ["touch", "double_tap"]);

    let at_edge = replay_with(
        double_tap_on(),
        &script(&[
            "down 0 500 800 0",
            "up 0 500 800 100",
            "down 0 500 800 350",
            "up 0 500 800 400",
        ]),
    );
    assert_eq!(names(&at_edge), ["touch", "tap"]);
    assert_eq!(
        at_edge[1].time_us(),
        100_000,
        "the single tap keeps its first lift time"
    );
}

/// Timer in `handleDoubleTap`: a lone tap is emitted at lift + 300 ms, not before.
#[test]
fn double_tap_timer_emits_single_tap_at_300ms() {
    let mut det = GestureDetector::new(double_tap_on());
    let touch = |phase, t| TouchEvent {
        slot: 0,
        phase,
        pos: Point::new(500, 800),
        time_us: t,
    };
    assert_eq!(det.feed(touch(kaia_input::Phase::Down, 0)).len(), 1);
    assert!(det.feed(touch(kaia_input::Phase::Up, 100_000)).is_empty());
    assert_eq!(det.next_deadline(), Some(400_000));
    assert!(det.poll(399_999).is_empty());
    let fired = det.poll(400_000);
    assert_eq!(names(&fired), ["tap"]);
    assert_eq!(fired[0].time_us(), 100_000);
}

/// Hold: `handleNonTap` arms the hold timer at 500 ms, and `holdState` emits `hold`.
/// `hold_release` comes from the lift in `holdState`.
#[test]
fn hold_fires_at_500ms_and_releases_on_lift() {
    let mut det = GestureDetector::default();
    let first = det.feed(TouchEvent {
        slot: 0,
        phase: kaia_input::Phase::Down,
        pos: Point::new(500, 800),
        time_us: 0,
    });
    assert_eq!(names(&first), ["touch"]);
    assert!(det.poll(499_999).is_empty());
    let held = det.poll(500_000);
    assert_eq!(names(&held), ["hold"]);
    // KOReader sets a hold's time from the last event on the contact (the down), not the timer.
    assert_eq!(held[0].time_us(), 0);
    let released = det.feed(TouchEvent {
        slot: 0,
        phase: kaia_input::Phase::Up,
        pos: Point::new(500, 800),
        time_us: 700_000,
    });
    assert_eq!(names(&released), ["hold_release"]);
    assert_eq!(released[0].time_us(), 700_000);
}

/// `holdState` with a move past the pan threshold emits `hold_pan` (`handlePan` renamed).
/// Its lift emits `hold_release`, not `hold_pan_release`, for one finger.
#[test]
fn hold_pan_then_hold_release_for_one_finger() {
    let g = replay(&script(&[
        "down 0 500 800 0",
        "poll 500",
        "move 0 500 840 600",
        "up 0 500 840 700",
    ]));
    assert_eq!(names(&g), ["touch", "hold", "hold_pan", "hold_release"]);
    match &g[2] {
        Gesture::HoldPan {
            pos,
            start_pos,
            direction,
            distance,
            ..
        } => {
            assert_eq!(*pos, Point::new(500, 840));
            assert_eq!(*start_pos, Some(Point::new(500, 800)));
            assert_eq!(*direction, Some(Direction::South));
            assert!(close(*distance, 40.0));
        }
        other => panic!("expected hold_pan, got {}", other.name()),
    }
}

/// `Contact:handleSwipe` and `getPath`: a fast single-finger move in each of the 8 directions.
#[test]
fn swipe_in_eight_directions() {
    let cases = [
        (100, 0, Direction::East),
        (70, 70, Direction::SouthEast),
        (0, 100, Direction::South),
        (-70, 70, Direction::SouthWest),
        (-100, 0, Direction::West),
        (-70, -70, Direction::NorthWest),
        (0, -100, Direction::North),
        (70, -70, Direction::NorthEast),
    ];
    for (dx, dy, expected) in cases {
        let (x, y) = (500 + dx, 800 + dy);
        let g = replay(&script(&[
            "down 0 500 800 0",
            &format!("move 0 {x} {y} 100"),
            &format!("up 0 {x} {y} 200"),
        ]));
        assert_eq!(names(&g), ["touch", "pan", "swipe"], "for {expected:?}");
        match &g[2] {
            Gesture::Swipe {
                pos,
                end_pos,
                direction,
                distance,
                ..
            } => {
                assert_eq!(*pos, Point::new(500, 800));
                assert_eq!(*end_pos, Point::new(x, y));
                assert_eq!(*direction, Some(expected));
                assert!(close(*distance, ((dx * dx + dy * dy) as f64).sqrt()));
            }
            other => panic!("expected swipe, got {}", other.name()),
        }
    }
}

/// A move of 34 px is below `PAN_THRESHOLD` (35), so the contact stays a tap (`handleNonTap`).
#[test]
fn short_movement_below_threshold_is_still_a_tap() {
    let g = replay(&script(&[
        "down 0 500 800 0",
        "move 0 534 800 50",
        "up 0 534 800 120",
    ]));
    assert_eq!(names(&g), ["touch", "tap"]);
    assert_eq!(g[1].pos(), Point::new(534, 800));
}

/// `handleNonTap` uses `>=`, so a move of exactly 35 px is a pan. Its lift is a swipe.
#[test]
fn movement_at_threshold_starts_a_pan() {
    let g = replay(&script(&[
        "down 0 500 800 0",
        "move 0 535 800 50",
        "up 0 535 800 120",
    ]));
    assert_eq!(names(&g), ["touch", "pan", "swipe"]);
}

/// `isSwipe` uses `<` on the 900 ms window. Lift at 899 ms is a swipe. Lift at 900 ms is a
/// `pan_release` (`panState`, `handlePanRelease`).
#[test]
fn swipe_window_is_strict_at_900ms() {
    let swipe = replay(&script(&[
        "down 0 500 800 0",
        "move 0 600 800 100",
        "up 0 600 800 899",
    ]));
    assert_eq!(names(&swipe), ["touch", "pan", "swipe"]);

    let release = replay(&script(&[
        "down 0 500 800 0",
        "move 0 600 800 100",
        "up 0 600 800 900",
    ]));
    assert_eq!(names(&release), ["touch", "pan", "pan_release"]);
}

/// `handlePan` emits `pan` on each move, and `handlePanRelease` emits `pan_release`.
#[test]
fn pan_and_pan_release() {
    let g = replay(&script(&[
        "down 0 500 800 0",
        "move 0 600 800 100",
        "move 0 650 800 200",
        "up 0 650 800 2000",
    ]));
    assert_eq!(names(&g), ["touch", "pan", "pan", "pan_release"]);
    match &g[2] {
        Gesture::Pan {
            start_pos,
            distance,
            direction,
            ..
        } => {
            assert_eq!(*start_pos, Point::new(500, 800));
            assert!(close(*distance, 150.0));
            assert_eq!(*direction, Some(Direction::East));
        }
        other => panic!("expected pan, got {}", other.name()),
    }
}

/// `Contact:isTwoFingerTap` on the first lift with the buddy still down (`tapState`).
#[test]
fn two_finger_tap() {
    let g = replay(&script(&[
        "down 0 500 800 0",
        "down 1 520 800 50",
        "up 0 500 800 150",
        "up 1 520 800 200",
    ]));
    assert_eq!(names(&g), ["touch", "touch", "two_finger_tap"]);
    match g[2] {
        Gesture::TwoFingerTap { pos, span, time_us } => {
            assert_eq!(pos, Point::new(510, 800));
            assert!(close(span, 20.0));
            assert_eq!(time_us, 150_000);
        }
        ref other => panic!("expected two_finger_tap, got {}", other.name()),
    }
}

/// `isTwoFingerTap` uses `<` on `TWO_FINGER_TAP_DURATION_MS` (300). At 300 ms the first lift
/// falls back to a single tap (`tapState`, the "failed constraints" branch).
#[test]
fn two_finger_tap_duration_is_strict_at_300ms() {
    let g = replay(&script(&[
        "down 0 500 800 0",
        "down 1 520 800 50",
        "up 0 500 800 300",
    ]));
    assert_eq!(names(&g), ["touch", "touch", "tap"]);
}

/// `isTwoFingerTap` checks each finger's own travel against `TWO_FINGER_TAP_REGION`
/// (20 px, strict). 19 px of travel still counts as a two-finger tap. 20 px does not.
#[test]
fn two_finger_tap_region_is_strict_at_20px() {
    let inside = replay(&script(&[
        "down 0 500 800 0",
        "down 1 520 800 50",
        "move 0 519 800 100",
        "up 0 519 800 150",
    ]));
    assert_eq!(names(&inside), ["touch", "touch", "two_finger_tap"]);

    let outside = replay(&script(&[
        "down 0 500 800 0",
        "down 1 520 800 50",
        "move 0 520 800 100",
        "up 0 520 800 150",
    ]));
    assert_eq!(names(&outside), ["touch", "touch", "tap"]);
}

/// Pinch: `handleTwoFingerPan` emits `inward_pan` while moving, and `panState` turns it into a
/// `pinch` on the swipe-like lift.
#[test]
fn pinch_fingers_moving_together() {
    let g = replay(&script(&[
        "down 0 400 800 0",
        "down 1 600 800 0",
        "move 0 450 800 100",
        "move 1 550 800 200",
        "up 0 450 800 300",
    ]));
    assert_eq!(names(&g), ["touch", "touch", "inward_pan", "pinch"]);
    match &g[3] {
        Gesture::Pinch {
            pos,
            start_pos,
            axis,
            distance,
            span,
            start_span,
            ..
        } => {
            assert_eq!(*pos, Point::new(500, 800));
            assert_eq!(*start_pos, Point::new(500, 800));
            assert_eq!(*axis, Some(Axis::Horizontal));
            assert!(close(*distance, 100.0));
            assert!(close(*span, 100.0));
            assert!(close(*start_span, 200.0));
        }
        other => panic!("expected pinch, got {}", other.name()),
    }
}

/// Spread: the mirror of the pinch. `handleTwoFingerPan` emits `outward_pan`, and `panState`
/// turns it into a `spread` on lift.
#[test]
fn spread_fingers_moving_apart() {
    let g = replay(&script(&[
        "down 0 450 800 0",
        "down 1 550 800 0",
        "move 0 350 800 100",
        "move 1 650 800 200",
        "up 0 350 800 300",
    ]));
    assert_eq!(names(&g), ["touch", "touch", "outward_pan", "spread"]);
    match &g[3] {
        Gesture::Spread {
            pos,
            end_pos,
            axis,
            distance,
            span,
            start_span,
            ..
        } => {
            assert_eq!(*pos, Point::new(500, 800));
            assert_eq!(*end_pos, Point::new(500, 800));
            assert_eq!(*axis, Some(Axis::Horizontal));
            assert!(close(*distance, 200.0));
            assert!(close(*span, 300.0));
            assert!(close(*start_span, 100.0));
        }
        other => panic!("expected spread, got {}", other.name()),
    }
}

/// Rotate: `handleTwoFingerPan` with a pivot finger that stays down while the other moves
/// round it. `getRotate` gives 90 degrees clockwise.
#[test]
fn rotate_around_a_still_finger() {
    let g = replay(&script(&[
        "down 0 500 800 0",
        "down 1 600 800 0",
        "move 1 500 900 100",
        "up 1 500 900 200",
    ]));
    assert_eq!(names(&g), ["touch", "touch", "outward_pan", "rotate"]);
    match &g[3] {
        Gesture::Rotate { pos, angle, turn, .. } => {
            assert_eq!(*pos, Point::new(500, 800));
            assert!(close(*angle, 90.0), "angle was {angle}");
            assert_eq!(*turn, Turn::Clockwise);
        }
        other => panic!("expected rotate, got {}", other.name()),
    }
}

/// Two-finger swipe: both fingers move the same way. The first finger's lift gives
/// `two_finger_swipe` (`panState` swipe branch).
#[test]
fn two_finger_swipe_same_direction() {
    let g = replay(&script(&[
        "down 0 500 800 0",
        "down 1 600 800 0",
        "move 0 600 800 100",
        "move 1 700 800 110",
        "up 0 600 800 200",
    ]));
    assert_eq!(names(&g), ["touch", "touch", "inward_pan", "two_finger_swipe"]);
    match &g[3] {
        Gesture::TwoFingerSwipe {
            pos,
            end_pos,
            direction,
            distance,
            ..
        } => {
            assert_eq!(*pos, Point::new(550, 800));
            assert_eq!(*end_pos, Point::new(650, 800));
            assert_eq!(*direction, Some(Direction::East));
            assert!(close(*distance, 100.0));
        }
        other => panic!("expected two_finger_swipe, got {}", other.name()),
    }
}

/// Multiswipe: `handlePan` records a turn after 50 px, and `handleSwipe` reports the chain.
#[test]
fn multiswipe_east_then_south() {
    let g = replay(&script(&[
        "down 0 500 500 0",
        "move 0 600 500 100",
        "move 0 600 600 200",
        "up 0 600 600 300",
    ]));
    assert_eq!(names(&g), ["touch", "pan", "pan", "multiswipe"]);
    match &g[3] {
        Gesture::Multiswipe {
            direction,
            directions,
            ..
        } => {
            assert_eq!(*direction, Some(Direction::SouthEast));
            assert_eq!(*directions, vec![Direction::East, Direction::South]);
        }
        other => panic!("expected multiswipe, got {}", other.name()),
    }
}

/// `handlePan` uses `msd_distance > MULTISWIPE_THRESHOLD` (50 px, strict). A first leg of
/// 51 px counts as a turn. A first leg of 50 px does not, so the lift is a plain swipe.
#[test]
fn multiswipe_threshold_is_strict_at_50px() {
    let counted = replay(&script(&[
        "down 0 500 500 0",
        "move 0 551 500 100",
        "move 0 551 600 200",
        "up 0 551 600 300",
    ]));
    assert_eq!(names(&counted), ["touch", "pan", "pan", "multiswipe"]);

    let ignored = replay(&script(&[
        "down 0 500 500 0",
        "move 0 550 500 100",
        "move 0 550 600 200",
        "up 0 550 600 300",
    ]));
    assert_eq!(names(&ignored), ["touch", "pan", "pan", "swipe"]);
    match &ignored[3] {
        Gesture::Swipe { direction, .. } => assert_eq!(*direction, Some(Direction::South)),
        other => panic!("expected swipe, got {}", other.name()),
    }
}

/// `handleDoubleTap` drops a second lift within `TAP_INTERVAL_MS` at the same spot (`isTapBounce`).
#[test]
fn tap_bounce_drops_a_fast_second_tap() {
    let config = GestureConfig {
        tap_interval_ms: 50,
        ..GestureConfig::default()
    };
    let g = replay_with(
        config,
        &script(&[
            "down 0 500 800 0",
            "up 0 500 800 10",
            "down 0 500 800 20",
            "up 0 500 800 30",
        ]),
    );
    assert_eq!(names(&g), ["touch", "tap", "touch"]);
}

/// Desktop mouse: a right-button drag spreads two fingers around the anchor (`outward_pan`,
/// `spread`). Slot 0 lifts before slot 1, as the two-finger logic expects.
#[test]
fn desktop_right_button_drag_spreads() {
    let mut pointer = DesktopPointer::new(Point::new(500, 800));
    let mut events = Vec::new();
    events.extend(pointer.press(Point::new(450, 800), 0, MouseButton::Right, false));
    events.extend(pointer.move_to(Point::new(400, 800), 100_000));
    events.extend(pointer.release(Point::new(400, 800), 200_000));

    let mut det = GestureDetector::default();
    let mut out = Vec::new();
    for ev in events {
        out.extend(det.feed(ev));
    }
    assert_eq!(names(&out), ["touch", "touch", "outward_pan", "spread"]);
}

#[test]
fn next_deadline_tracks_the_hold_timer() {
    let mut det = GestureDetector::default();
    assert_eq!(det.next_deadline(), None);
    det.feed(TouchEvent {
        slot: 0,
        phase: kaia_input::Phase::Down,
        pos: Point::new(1, 1),
        time_us: 1_000,
    });
    assert_eq!(det.next_deadline(), Some(501_000));
    det.reset();
    assert_eq!(det.next_deadline(), None);
}

/// Two-finger hold: `holdState` with a buddy. The hold timer fires for the first finger, and
/// the lift of that finger gives `two_finger_hold_release`. The buddy's lift is then void.
#[test]
fn two_finger_hold_then_release() {
    let g = replay(&script(&[
        "down 0 500 800 0",
        "down 1 600 800 0",
        "poll 500",
        "up 0 500 800 600",
        "up 1 600 800 650",
    ]));
    assert_eq!(
        names(&g),
        ["touch", "touch", "two_finger_hold", "two_finger_hold_release"]
    );
    match &g[2] {
        Gesture::TwoFingerHold { pos, span, .. } => {
            assert_eq!(*pos, Point::new(550, 800));
            assert!(close(*span, 100.0));
        }
        other => panic!("expected two_finger_hold, got {}", other.name()),
    }
}
