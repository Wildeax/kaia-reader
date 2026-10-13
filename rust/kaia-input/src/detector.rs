//! Gesture recognizer. A port of KOReader's `frontend/device/gesturedetector.lua`.
//!
//! Each touch slot is a contact with a small state machine: tap, pan, hold or void.
//! Slots 0 and 1 are buddies, so two fingers can form a two-finger gesture. The hold
//! and double tap timers live on the contact. They fire from [`GestureDetector::poll`],
//! or before the next event whose time has passed their deadline.
//!
//! [`GestureDetector::feed_frame`] updates all slots before processing them in
//! their order of appearance in the input frame.
//!
//! Function names in comments refer to KOReader's `gesturedetector.lua`.

use std::collections::BTreeMap;

use kaia_gfx::Point;

use crate::event::{Phase, TouchEvent};
use crate::gesture::{Axis, Direction, Gesture, HoldPanDirection, Turn};

/// Recognizer settings. The defaults match KOReader's `GestureDetector` and `Input`.
///
/// KOReader measures distances in dp (density-independent pixels, 160 dpi is 1 dp per
/// pixel) and converts them with `screen:scaleByDPI`. This struct does the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GestureConfig {
    /// Screen density in dpi. KOReader's fallback is 160.
    pub dpi: u32,
    /// Taps closer than this window, and within the bounce distance, are dropped. KOReader default 0.
    pub tap_interval_ms: u64,
    /// Whether taps wait for a possible second tap. KOReader's `Input` disables this by default.
    pub double_tap_enabled: bool,
    /// Lift-to-lift window for a double tap (KOReader `DOUBLE_TAP_INTERVAL_MS`).
    pub double_tap_interval_ms: u64,
    /// Window from the first down to the lift of a two-finger tap (`TWO_FINGER_TAP_DURATION_MS`).
    pub two_finger_tap_duration_ms: u64,
    /// Stillness needed before a contact becomes a hold (`HOLD_INTERVAL_MS`).
    pub hold_interval_ms: u64,
    /// Window from the first down to the lift of a swipe (`SWIPE_INTERVAL_MS`).
    pub swipe_interval_ms: u64,
    /// Two-finger tap region in dp (`scaleByDPI(20)`).
    pub two_finger_tap_region_dp: u32,
    /// Double tap and bounce distance in dp (`scaleByDPI(50)`). Multiswipe uses the same value.
    pub double_tap_distance_dp: u32,
    /// Movement that turns a contact into a pan, in dp (`scaleByDPI(35)`).
    pub pan_threshold_dp: u32,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            dpi: 160,
            tap_interval_ms: 0,
            double_tap_enabled: false,
            double_tap_interval_ms: 300,
            two_finger_tap_duration_ms: 300,
            hold_interval_ms: 500,
            swipe_interval_ms: 900,
            two_finger_tap_region_dp: 20,
            double_tap_distance_dp: 50,
            pan_threshold_dp: 35,
        }
    }
}

/// KOReader's `screen:scaleByDPI`: `ceil(dp * dpi / 160)` pixels.
pub fn dp_to_px(dp: u32, dpi: u32) -> i64 {
    (i64::from(dp) * i64::from(dpi) + 159) / 160
}

/// The latest sample for a slot. `lifted` is true for an Up sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sample {
    pos: Point,
    time_us: u64,
    lifted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Tap,
    Pan,
    Hold,
    Void,
}

/// KOReader's `mt_gesture`: what a contact belongs to when it has a buddy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mt {
    Tap,
    Swipe,
    Pan,
    PanRelease,
    Rotate,
    Hold,
    HoldPan,
    HoldRelease,
    HoldPanRelease,
}

#[derive(Debug, Clone)]
struct PendingTap {
    deadline_us: u64,
    gesture: Gesture,
}

#[derive(Debug, Clone)]
struct Contact {
    state: State,
    /// The contact has gone down and not yet been lifted for good (KOReader `down`).
    down: bool,
    /// The first sample of the contact, copied at the first down.
    initial: Sample,
    current: Sample,
    pending_hold: Option<u64>,
    pending_double_tap: Option<PendingTap>,
    mt: Option<Mt>,
    mt_immobile: bool,
    /// Accumulated multiswipe chain: the direction and the point where it was last updated.
    multiswipe: Vec<(Direction, Point)>,
    multiswipe_straight: bool,
}

impl Contact {
    /// KOReader `newContact` followed by `initialState`: a new contact starts in tap state.
    fn new(sample: Sample) -> Self {
        Self {
            state: State::Tap,
            down: true,
            initial: sample,
            current: sample,
            pending_hold: None,
            pending_double_tap: None,
            mt: None,
            mt_immobile: true,
            multiswipe: Vec::new(),
            multiswipe_straight: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Timer {
    Hold,
    DoubleTap,
}

/// The gesture state machine for all touch slots.
#[derive(Debug, Clone)]
pub struct GestureDetector {
    config: GestureConfig,
    tap_interval_override_ms: Option<u64>,
    pan_px: i64,
    two_finger_region_px: i64,
    /// Used for the double tap distance, the bounce distance and the multiswipe threshold.
    double_tap_px: i64,
    contacts: BTreeMap<u8, Contact>,
    /// The last tap per slot. KOReader keeps this outside the contacts, so it survives lifts.
    previous_tap: BTreeMap<u8, (Point, u64)>,
}

impl Default for GestureDetector {
    fn default() -> Self {
        Self::new(GestureConfig::default())
    }
}

impl GestureDetector {
    pub fn new(config: GestureConfig) -> Self {
        Self {
            pan_px: dp_to_px(config.pan_threshold_dp, config.dpi),
            two_finger_region_px: dp_to_px(config.two_finger_tap_region_dp, config.dpi),
            double_tap_px: dp_to_px(config.double_tap_distance_dp, config.dpi),
            config,
            tap_interval_override_ms: None,
            contacts: BTreeMap::new(),
            previous_tap: BTreeMap::new(),
        }
    }

    pub fn config(&self) -> &GestureConfig {
        &self.config
    }

    /// Changes whether a tap waits for a possible second tap.
    pub fn set_disable_double_tap(&mut self, disabled: bool) {
        self.config.double_tap_enabled = !disabled;
    }

    /// Overrides the tap bounce interval in milliseconds. None restores the configured interval.
    pub fn set_tap_interval_override(&mut self, interval_ms: Option<u64>) {
        self.tap_interval_override_ms = interval_ms;
    }

    /// Feeds one touch sample. Timers that expired before the sample's time fire first.
    pub fn feed(&mut self, ev: TouchEvent) -> Vec<Gesture> {
        self.feed_frame(&[ev])
    }

    /// Feeds one input frame. The last sample for each slot wins, while slots are
    /// processed in the order they first appeared in the frame.
    pub fn feed_frame(&mut self, events: &[TouchEvent]) -> Vec<Gesture> {
        let Some(last) = events.last() else {
            return Vec::new();
        };
        let mut out = self.fire_due(last.time_us);
        let mut slots = Vec::new();
        let mut samples = BTreeMap::new();
        for ev in events {
            if !samples.contains_key(&ev.slot) {
                slots.push(ev.slot);
            }
            samples.insert(
                ev.slot,
                Sample {
                    pos: ev.pos,
                    time_us: ev.time_us,
                    lifted: ev.phase == Phase::Up,
                },
            );
        }
        for (&slot, &sample) in &samples {
            if let Some(contact) = self.contacts.get_mut(&slot) {
                contact.current = sample;
            }
        }
        for slot in slots {
            out.extend(self.process(slot, samples[&slot]));
        }
        out
    }

    /// Fires the timers whose deadline is at or before `now_us`.
    pub fn poll(&mut self, now_us: u64) -> Vec<Gesture> {
        self.fire_due(now_us)
    }

    /// The earliest pending timer deadline, so the caller knows when to call `poll`.
    pub fn next_deadline(&self) -> Option<u64> {
        self.contacts
            .values()
            .flat_map(|c| {
                c.pending_hold
                    .into_iter()
                    .chain(c.pending_double_tap.as_ref().map(|p| p.deadline_us))
            })
            .min()
    }

    /// Forgets all active contacts and their timers. KOReader's `dropContacts`.
    pub fn reset(&mut self) {
        self.contacts.clear();
    }

    fn process(&mut self, slot: u8, sample: Sample) -> Option<Gesture> {
        if let Some(c) = self.contacts.get_mut(&slot) {
            c.current = sample;
            let state = c.state;
            return match state {
                State::Tap => self.tap_state(slot, false),
                State::Pan => self.pan_state(slot, false),
                State::Hold => self.hold_state(slot, false),
                State::Void => self.void_state(slot),
            };
        }
        if sample.lifted {
            return None;
        }
        self.contacts.insert(slot, Contact::new(sample));
        self.tap_state(slot, true)
    }

    fn fire_due(&mut self, now_us: u64) -> Vec<Gesture> {
        let mut out = Vec::new();
        while let Some((_, slot, timer)) = self.next_due(now_us) {
            match timer {
                Timer::Hold => {
                    if let Some(c) = self.contacts.get_mut(&slot) {
                        c.pending_hold = None;
                    }
                    // Only a contact that is still down in tap state becomes a hold.
                    if self
                        .contacts
                        .get(&slot)
                        .is_some_and(|c| c.state == State::Tap && c.down)
                    {
                        self.set_state(slot, State::Hold);
                        out.extend(self.hold_state(slot, true));
                    }
                }
                Timer::DoubleTap => {
                    let fired = self.contacts.get_mut(&slot).and_then(|c| {
                        let pending = c.pending_double_tap.take()?;
                        (c.state == State::Tap).then_some(pending.gesture)
                    });
                    if let Some(gesture) = fired {
                        self.contacts.remove(&slot);
                        out.push(gesture);
                    }
                }
            }
        }
        out
    }

    fn next_due(&self, now_us: u64) -> Option<(u64, u8, Timer)> {
        self.contacts
            .iter()
            .flat_map(|(&slot, c)| {
                let hold = c
                    .pending_hold
                    .filter(|&d| d <= now_us)
                    .map(|d| (d, slot, Timer::Hold));
                let tap = c
                    .pending_double_tap
                    .as_ref()
                    .filter(|p| p.deadline_us <= now_us)
                    .map(|p| (p.deadline_us, slot, Timer::DoubleTap));
                hold.into_iter().chain(tap)
            })
            .min()
    }

    // ----- helpers ---------------------------------------------------------------------

    fn contact(&self, slot: u8) -> Option<Contact> {
        self.contacts.get(&slot).cloned()
    }

    fn set_state(&mut self, slot: u8, state: State) {
        if let Some(c) = self.contacts.get_mut(&slot) {
            c.state = state;
        }
    }

    fn set_mt(&mut self, slot: u8, mt: Option<Mt>) {
        if let Some(c) = self.contacts.get_mut(&slot) {
            c.mt = mt;
        }
    }

    fn set_lifted(&mut self, slot: u8, lifted: bool) {
        if let Some(c) = self.contacts.get_mut(&slot) {
            c.current.lifted = lifted;
        }
    }

    fn drop_contact(&mut self, slot: u8) {
        self.contacts.remove(&slot);
    }

    /// KOReader `GestureDetector:newContact` pairs slots 0 and 1 only.
    fn buddy(&self, slot: u8) -> Option<u8> {
        let other = match slot {
            0 => 1,
            1 => 0,
            _ => return None,
        };
        self.contacts.contains_key(&other).then_some(other)
    }

    fn is_pan_threshold_crossed(&self, c: &Contact) -> bool {
        let (dx, dy) = axis_diff(c.current.pos, c.initial.pos);
        dx >= self.pan_px || dy >= self.pan_px
    }

    /// KOReader `isSwipe`: moved, and lifted within the swipe window.
    fn is_swipe(&self, c: &Contact) -> bool {
        let in_window = matches!(
            elapsed(c.initial.time_us, c.current.time_us),
            Some(d) if d < ms(self.config.swipe_interval_ms)
        );
        in_window && c.current.pos != c.initial.pos
    }

    /// KOReader `isTwoFingerTap`.
    fn is_two_finger_tap(&self, a: &Contact, b: &Contact) -> bool {
        let window = ms(self.config.two_finger_tap_duration_ms);
        let in_window =
            |c: &Contact| matches!(elapsed(c.initial.time_us, c.current.time_us), Some(d) if d < window);
        let region = self.two_finger_region_px;
        in_window(a)
            && in_window(b)
            && within(a.current.pos, a.initial.pos, region)
            && within(b.current.pos, b.initial.pos, region)
    }

    /// KOReader `isTapBounce`. `prev` is the earlier tap, `cur` the later one.
    fn is_tap_bounce(&self, prev: (Point, u64), cur: (Point, u64), interval_us: u64) -> bool {
        matches!(elapsed(prev.1, cur.1), Some(d) if d < interval_us)
            && within(cur.0, prev.0, self.double_tap_px)
    }

    /// KOReader `isDoubleTap`.
    fn is_double_tap(&self, prev: (Point, u64), cur: (Point, u64)) -> bool {
        matches!(elapsed(prev.1, cur.1), Some(d) if d < ms(self.config.double_tap_interval_ms))
            && within(cur.0, prev.0, self.double_tap_px)
    }

    // ----- tap state -------------------------------------------------------------------

    /// KOReader `Contact:tapState`. Handles lifts with buddies and the first move.
    fn tap_state(&mut self, slot: u8, new_tap: bool) -> Option<Gesture> {
        let c = self.contact(slot)?;
        if c.current.lifted {
            if c.down {
                if let Some(b) = self.buddy(slot) {
                    let buddy = self.contact(b)?;
                    if self.is_two_finger_tap(&c, &buddy) {
                        self.set_mt(slot, Some(Mt::Tap));
                        self.set_mt(b, Some(Mt::Tap));
                        self.set_state(b, State::Void);
                        self.drop_contact(slot);
                        let (p0, p1) = (c.current.pos, buddy.current.pos);
                        return Some(Gesture::TwoFingerTap {
                            pos: midpoint(p0, p1),
                            span: distance(p0, p1),
                            time_us: c.current.time_us,
                        });
                    }
                    // The lift fails the two-finger constraints: send a single tap now.
                    self.set_state(b, State::Void);
                    self.drop_contact(slot);
                    return Some(Gesture::Tap {
                        pos: c.current.pos,
                        time_us: c.current.time_us,
                    });
                }
            }
            if c.down || c.pending_double_tap.is_some() {
                return self.handle_double_tap(slot);
            }
            self.drop_contact(slot);
            return None;
        }
        if c.pending_double_tap.is_some() && !c.down {
            if let Some(c) = self.contacts.get_mut(&slot) {
                c.down = true;
            }
        }
        self.handle_non_tap(slot, new_tap)
    }

    /// KOReader `Contact:handleDoubleTap`. Emits a tap now, or waits for a second tap.
    fn handle_double_tap(&mut self, slot: u8) -> Option<Gesture> {
        let c = self.contact(slot)?;
        // A second lift without a new down is a hover, so it is ignored.
        if c.pending_double_tap.is_some() && !c.down {
            return None;
        }
        let cur = (c.current.pos, c.current.time_us);
        let prev = self.previous_tap.get(&slot).copied();
        let bounce_us = ms(self
            .tap_interval_override_ms
            .unwrap_or(self.config.tap_interval_ms));
        if bounce_us != 0 {
            if let Some(p) = prev {
                if self.is_tap_bounce(p, cur, bounce_us) {
                    self.drop_contact(slot);
                    return None;
                }
            }
        }
        let tap = Gesture::Tap {
            pos: c.current.pos,
            time_us: c.current.time_us,
        };
        if self.config.double_tap_enabled && c.pending_double_tap.is_some() {
            if let Some(p) = prev {
                if self.is_double_tap(p, cur) {
                    self.drop_contact(slot);
                    return Some(Gesture::DoubleTap {
                        pos: c.current.pos,
                        time_us: c.current.time_us,
                    });
                }
            }
        }
        self.previous_tap.insert(slot, cur);
        if !self.config.double_tap_enabled {
            self.drop_contact(slot);
            return Some(tap);
        }
        if c.pending_double_tap.is_none() {
            let deadline_us = c.current.time_us + ms(self.config.double_tap_interval_ms);
            if let Some(c) = self.contacts.get_mut(&slot) {
                c.pending_double_tap = Some(PendingTap {
                    deadline_us,
                    gesture: tap,
                });
            }
        }
        if let Some(c) = self.contacts.get_mut(&slot) {
            c.down = false;
        }
        None
    }

    /// KOReader `Contact:handleNonTap`. The first down emits `touch` and arms the hold timer.
    fn handle_non_tap(&mut self, slot: u8, new_tap: bool) -> Option<Gesture> {
        let c = self.contact(slot)?;
        if new_tap && c.pending_hold.is_none() {
            let deadline = c.current.time_us + ms(self.config.hold_interval_ms);
            if let Some(c) = self.contacts.get_mut(&slot) {
                c.pending_hold = Some(deadline);
            }
            return Some(Gesture::Touch {
                pos: c.current.pos,
                time_us: c.current.time_us,
            });
        }
        if self.is_pan_threshold_crossed(&c) {
            self.set_state(slot, State::Pan);
            return self.pan_state(slot, false);
        }
        None
    }

    // ----- pan state -------------------------------------------------------------------

    /// KOReader `Contact:panState`. `keep` stops the contact from being dropped on lift.
    fn pan_state(&mut self, slot: u8, keep: bool) -> Option<Gesture> {
        let c = self.contact(slot)?;
        if !c.current.lifted {
            return self.handle_pan(slot);
        }
        if self.is_swipe(&c) {
            if !c.down {
                return None;
            }
            let Some(b) = self.buddy(slot) else {
                return self.handle_swipe(slot);
            };
            let bc = self.contact(b)?;
            self.set_mt(slot, Some(Mt::Swipe));
            let buddy_mt = if bc.state != State::Pan && bc.mt_immobile {
                Mt::Rotate
            } else {
                Mt::Swipe
            };
            self.set_mt(b, Some(buddy_mt));
            self.set_state(b, State::Void);
            let ges = self.handle_two_finger_pan(slot, b);
            let ges = if buddy_mt == Mt::Swipe {
                match ges {
                    Some(Gesture::TwoFingerPan {
                        pos,
                        start_pos,
                        direction,
                        distance,
                        time_us,
                    }) => Some(Gesture::TwoFingerSwipe {
                        pos: start_pos,
                        end_pos: pos,
                        direction,
                        distance,
                        time_us,
                    }),
                    Some(Gesture::InwardPan {
                        pos,
                        start_pos,
                        axis,
                        distance,
                        span,
                        start_span,
                        time_us,
                    }) => Some(Gesture::Pinch {
                        pos,
                        start_pos,
                        axis,
                        distance,
                        span,
                        start_span,
                        time_us,
                    }),
                    Some(Gesture::OutwardPan {
                        pos,
                        end_pos,
                        axis,
                        distance,
                        span,
                        start_span,
                        time_us,
                    }) => Some(Gesture::Spread {
                        pos,
                        end_pos,
                        axis,
                        distance,
                        span,
                        start_span,
                        time_us,
                    }),
                    _ => None,
                }
            } else {
                match ges {
                    Some(rotate @ Gesture::Rotate { .. }) => Some(rotate),
                    _ => None,
                }
            };
            if !keep {
                self.drop_contact(slot);
            }
            return ges;
        }
        if c.down {
            return self.handle_pan_release(slot, keep);
        }
        self.drop_contact(slot);
        None
    }

    /// KOReader `Contact:handlePan`. Emits pans and records the multiswipe chain.
    fn handle_pan(&mut self, slot: u8) -> Option<Gesture> {
        let c = self.contact(slot)?;
        if !c.down {
            return None;
        }
        if let Some(b) = self.buddy(slot) {
            let bc = self.contact(b)?;
            self.set_mt(slot, Some(Mt::Pan));
            let rotate = bc.state != State::Pan && bc.mt_immobile && c.state == State::Pan;
            self.set_mt(b, Some(if rotate { Mt::Rotate } else { Mt::Pan }));
            self.set_state(b, State::Void);
            return self.handle_two_finger_pan(slot, b);
        }

        let (direction, distance) = get_path(c.initial.pos, c.current.pos, false, false);
        let pan = Gesture::Pan {
            pos: c.current.pos,
            start_pos: c.initial.pos,
            direction,
            distance,
            time_us: c.current.time_us,
        };

        // Multiswipe: measure from the last recorded point, with 4 or 8 directions.
        let count = c.multiswipe.len();
        let prev_dir = c.multiswipe.last().map(|e| e.0);
        let origin = c.multiswipe.last().map_or(c.initial.pos, |e| e.1);
        let straight = if count == 0 {
            matches!(
                direction,
                Some(Direction::North | Direction::South | Direction::East | Direction::West)
            )
        } else {
            c.multiswipe_straight
        };
        let (ms_dir, ms_dist) = get_path(origin, c.current.pos, true, !straight);
        if let Some(c) = self.contacts.get_mut(&slot) {
            c.multiswipe_straight = straight;
        }
        if let (Some(dir), true) = (ms_dir, ms_dist > self.double_tap_px as f64) {
            if let Some(c) = self.contacts.get_mut(&slot) {
                if Some(dir) != prev_dir {
                    c.multiswipe.push((dir, c.current.pos));
                } else if let Some(last) = c.multiswipe.last_mut() {
                    *last = (dir, c.current.pos);
                }
            }
        }
        Some(pan)
    }

    /// KOReader `Contact:handleTwoFingerPan`. `t` is the triggering slot, `r` its buddy.
    fn handle_two_finger_pan(&mut self, t: u8, r: u8) -> Option<Gesture> {
        let tc = self.contact(t)?;
        let rc = self.contact(r)?;
        let (tdir, tdis) = get_path(tc.initial.pos, tc.current.pos, false, false);
        let tstart = tc.initial.pos;
        let tend = tc.current.pos;
        let rstart = rc.initial.pos;

        if tc.current.lifted && rc.mt == Some(Mt::Rotate) {
            let angle = get_rotate(rstart, tstart, tend);
            let turn = if angle >= 0.0 {
                Turn::Clockwise
            } else {
                Turn::CounterClockwise
            };
            return Some(Gesture::Rotate {
                pos: rstart,
                angle,
                turn,
                time_us: tc.current.time_us,
            });
        }

        let (rdir, rdis) = get_path(rc.initial.pos, rc.current.pos, false, false);
        let rend = rc.current.pos;
        let start_mid = midpoint(tstart, rstart);
        let end_mid = midpoint(tend, rend);
        let avg = distance(start_mid, end_mid);
        let start_span = distance(tstart, rstart);
        let end_span = distance(tend, rend);
        let time_us = tc.current.time_us;

        if tdir != rdir {
            let axis = Axis::of(tdir);
            let total = tdis + rdis;
            return Some(if start_span > end_span {
                Gesture::InwardPan {
                    pos: end_mid,
                    start_pos: start_mid,
                    axis,
                    distance: total,
                    span: end_span,
                    start_span,
                    time_us,
                }
            } else {
                Gesture::OutwardPan {
                    pos: start_mid,
                    end_pos: end_mid,
                    axis,
                    distance: total,
                    span: end_span,
                    start_span,
                    time_us,
                }
            });
        }
        if tc.state == State::Hold {
            self.set_mt(t, Some(Mt::HoldPan));
            self.set_mt(r, Some(Mt::HoldPan));
            return Some(Gesture::TwoFingerHoldPan {
                pos: end_mid,
                start_pos: start_mid,
                direction: tdir,
                distance: avg,
                time_us,
            });
        }
        Some(Gesture::TwoFingerPan {
            pos: end_mid,
            start_pos: start_mid,
            direction: tdir,
            distance: avg,
            time_us,
        })
    }

    /// KOReader `Contact:handleSwipe`. Emits a swipe, or a multiswipe with two or more turns.
    fn handle_swipe(&mut self, slot: u8) -> Option<Gesture> {
        let c = self.contact(slot)?;
        let (direction, distance) = get_path(c.initial.pos, c.current.pos, false, false);
        self.drop_contact(slot);
        if c.multiswipe.len() > 1 {
            return Some(Gesture::Multiswipe {
                pos: c.initial.pos,
                end_pos: c.current.pos,
                direction,
                distance,
                directions: c.multiswipe.iter().map(|e| e.0).collect(),
                time_us: c.current.time_us,
            });
        }
        Some(Gesture::Swipe {
            pos: c.initial.pos,
            end_pos: c.current.pos,
            direction,
            distance,
            time_us: c.current.time_us,
        })
    }

    /// KOReader `Contact:handlePanRelease`.
    fn handle_pan_release(&mut self, slot: u8, keep: bool) -> Option<Gesture> {
        let c = self.contact(slot)?;
        let release = c.current.pos;
        if c.down {
            if let Some(b) = self.buddy(slot) {
                let bc = self.contact(b)?;
                self.set_mt(slot, Some(Mt::PanRelease));
                self.set_mt(b, Some(Mt::PanRelease));
                self.set_state(b, State::Void);
                if !keep {
                    self.drop_contact(slot);
                }
                return Some(Gesture::TwoFingerPanRelease {
                    pos: midpoint(release, bc.current.pos),
                    time_us: c.current.time_us,
                });
            }
            self.drop_contact(slot);
            return Some(Gesture::PanRelease {
                pos: release,
                time_us: c.current.time_us,
            });
        }
        self.drop_contact(slot);
        None
    }

    // ----- hold state ------------------------------------------------------------------

    /// KOReader `Contact:holdState`. `new_hold` is true when the hold timer fired.
    fn hold_state(&mut self, slot: u8, new_hold: bool) -> Option<Gesture> {
        let c = self.contact(slot)?;
        if new_hold && !c.current.lifted {
            if c.down {
                if let Some(b) = self.buddy(slot) {
                    let bc = self.contact(b)?;
                    self.set_mt(slot, Some(Mt::Hold));
                    self.set_mt(b, Some(Mt::Hold));
                    self.set_state(b, State::Void);
                    return Some(Gesture::TwoFingerHold {
                        pos: midpoint(c.current.pos, bc.current.pos),
                        span: distance(c.current.pos, bc.current.pos),
                        time_us: c.current.time_us,
                    });
                }
                return Some(Gesture::Hold {
                    pos: c.current.pos,
                    time_us: c.current.time_us,
                });
            }
            return None;
        }

        if c.current.lifted {
            if c.down {
                if let Some(b) = self.buddy(slot) {
                    let bc = self.contact(b)?;
                    if c.mt == Some(Mt::Rotate) && bc.mt == Some(Mt::Pan) {
                        // The hold finger lifted first, as the pivot of a rotate.
                        let ges = self
                            .handle_two_finger_pan(b, slot)
                            .filter(|g| matches!(g, Gesture::Rotate { .. }));
                        self.set_state(b, State::Void);
                        self.drop_contact(slot);
                        return ges;
                    }
                    let mt = if matches!(c.mt, Some(Mt::HoldPan | Mt::Pan)) {
                        Mt::HoldPanRelease
                    } else {
                        Mt::HoldRelease
                    };
                    self.set_mt(slot, Some(mt));
                    self.set_mt(b, Some(mt));
                    self.set_state(b, State::Void);
                    self.drop_contact(slot);
                    let pos = midpoint(c.current.pos, bc.current.pos);
                    let span = distance(c.current.pos, bc.current.pos);
                    let time_us = c.current.time_us;
                    return Some(if mt == Mt::HoldPanRelease {
                        Gesture::TwoFingerHoldPanRelease { pos, span, time_us }
                    } else {
                        Gesture::TwoFingerHoldRelease { pos, span, time_us }
                    });
                }
                self.drop_contact(slot);
                return Some(Gesture::HoldRelease {
                    pos: c.current.pos,
                    time_us: c.current.time_us,
                });
            }
            self.drop_contact(slot);
            return None;
        }

        if self.is_pan_threshold_crossed(&c) {
            // Stay in hold state and report the move as a hold pan.
            return self.handle_pan(slot).map(to_hold_pan);
        }
        None
    }

    // ----- void state ------------------------------------------------------------------

    /// KOReader `Contact:voidState`. A buddy that was absorbed into another gesture.
    fn void_state(&mut self, slot: u8) -> Option<Gesture> {
        let c = self.contact(slot)?;
        if !c.current.lifted {
            if c.mt_immobile && self.is_pan_threshold_crossed(&c) {
                if let Some(c) = self.contacts.get_mut(&slot) {
                    c.mt_immobile = false;
                }
                if let Some(b) = self.buddy(slot) {
                    let buddy_mt = self.contact(b).and_then(|bc| bc.mt);
                    if buddy_mt == Some(Mt::Hold) && c.mt == Some(Mt::Hold) {
                        self.set_mt(slot, Some(Mt::Pan));
                        self.set_mt(b, Some(Mt::Rotate));
                    }
                }
            }
            return None;
        }

        if c.down {
            if let Some(b) = self.buddy(slot) {
                let bc = self.contact(b)?;
                if bc.down {
                    if let Some(mt) = c.mt {
                        return match mt {
                            Mt::Tap => self.tap_state(slot, false),
                            Mt::Swipe | Mt::Pan | Mt::PanRelease => self.pan_state(slot, false),
                            Mt::Rotate => {
                                // The pivot lifted first. Lift the buddy for one call so that
                                // pan_state can detect the rotate, then restore its sample.
                                let ges = if bc.current.lifted {
                                    self.pan_state(b, false)
                                } else {
                                    self.set_lifted(b, true);
                                    let ges = self.pan_state(b, true);
                                    self.set_lifted(b, false);
                                    self.set_state(b, State::Void);
                                    ges
                                };
                                self.drop_contact(slot);
                                ges
                            }
                            Mt::Hold | Mt::HoldPan | Mt::HoldRelease | Mt::HoldPanRelease => {
                                self.hold_state(slot, false)
                            }
                        };
                    }
                }
            }
        }
        self.drop_contact(slot);
        None
    }
}

/// Converts a pan from hold state into a `hold_pan`, as KOReader renames the gesture.
fn to_hold_pan(g: Gesture) -> Gesture {
    match g {
        Gesture::Pan {
            pos,
            start_pos,
            direction,
            distance,
            time_us,
        }
        | Gesture::TwoFingerPan {
            pos,
            start_pos,
            direction,
            distance,
            time_us,
        } => Gesture::HoldPan {
            pos,
            start_pos: Some(start_pos),
            end_pos: None,
            direction: direction.map(HoldPanDirection::Compass),
            distance,
            span: None,
            start_span: None,
            time_us,
        },
        Gesture::InwardPan {
            pos,
            start_pos,
            axis,
            distance,
            span,
            start_span,
            time_us,
        } => Gesture::HoldPan {
            pos,
            start_pos: Some(start_pos),
            end_pos: None,
            direction: axis.map(HoldPanDirection::Axis),
            distance,
            span: Some(span),
            start_span: Some(start_span),
            time_us,
        },
        Gesture::OutwardPan {
            pos,
            end_pos,
            axis,
            distance,
            span,
            start_span,
            time_us,
        } => Gesture::HoldPan {
            pos,
            start_pos: None,
            end_pos: Some(end_pos),
            direction: axis.map(HoldPanDirection::Axis),
            distance,
            span: Some(span),
            start_span: Some(start_span),
            time_us,
        },
        other => other,
    }
}

/// KOReader `Contact:getPath`. Returns the direction and the distance from `from` to `to`.
///
/// With `simple`, only the four cardinal directions are returned unless `diagonal` is set.
fn get_path(from: Point, to: Point, simple: bool, diagonal: bool) -> (Option<Direction>, f64) {
    let dx = i64::from(to.x) - i64::from(from.x);
    let dy = i64::from(to.y) - i64::from(from.y);
    let dist = ((dx * dx + dy * dy) as f64).sqrt();
    if dx == 0 && dy == 0 {
        return (None, dist);
    }
    let north = dy < 0;
    let east = dx >= 0;
    let (adx, ady) = (dx.abs() as f64, dy.abs() as f64);
    let is_diagonal = (!simple && ady > 0.577 * adx && ady < 1.732 * adx) || (simple && diagonal);
    let direction = if is_diagonal {
        Direction::from_parts(north, east)
    } else if adx > ady {
        if east {
            Direction::East
        } else {
            Direction::West
        }
    } else if north {
        Direction::North
    } else {
        Direction::South
    };
    (Some(direction), dist)
}

/// KOReader `getRotate`: the signed angle in degrees, normalized to [-180, 180].
fn get_rotate(orig: Point, start: Point, end: Point) -> f64 {
    let angle_of = |p: Point| ((p.y - orig.y) as f64).atan2((p.x - orig.x) as f64);
    let mut rad = angle_of(end) - angle_of(start);
    if rad < -std::f64::consts::PI {
        rad += 2.0 * std::f64::consts::PI;
    } else if rad > std::f64::consts::PI {
        rad -= 2.0 * std::f64::consts::PI;
    }
    rad * 180.0 / std::f64::consts::PI
}

/// Microseconds for a millisecond setting.
fn ms(value: u64) -> u64 {
    value.saturating_mul(1000)
}

/// The time from `from` to `to`. Returns None when time went backwards, which KOReader
/// treats as an infinite delay.
fn elapsed(from: u64, to: u64) -> Option<u64> {
    to.checked_sub(from)
}

fn axis_diff(a: Point, b: Point) -> (i64, i64) {
    (
        (i64::from(a.x) - i64::from(b.x)).abs(),
        (i64::from(a.y) - i64::from(b.y)).abs(),
    )
}

/// True when both axis differences are strictly below `radius` (KOReader uses `<`).
fn within(a: Point, b: Point, radius: i64) -> bool {
    let (dx, dy) = axis_diff(a, b);
    dx < radius && dy < radius
}

/// Midpoint, rounded with KOReader's `floor(x + 0.5)` rule.
fn midpoint(a: Point, b: Point) -> Point {
    Point::new(
        ((i64::from(a.x) + i64::from(b.x) + 1).div_euclid(2)) as i32,
        ((i64::from(a.y) + i64::from(b.y) + 1).div_euclid(2)) as i32,
    )
}

fn distance(a: Point, b: Point) -> f64 {
    let dx = f64::from(a.x) - f64::from(b.x);
    let dy = f64::from(a.y) - f64::from(b.y);
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_thresholds_match_koreader_at_160_dpi() {
        let det = GestureDetector::default();
        assert_eq!(det.pan_px, 35);
        assert_eq!(det.two_finger_region_px, 20);
        assert_eq!(det.double_tap_px, 50);
    }

    #[test]
    fn dp_scales_like_koreader_scale_by_dpi() {
        // math.ceil(35 * 300 / 160) = ceil(65.625) = 66
        assert_eq!(dp_to_px(35, 300), 66);
        assert_eq!(dp_to_px(35, 160), 35);
        assert_eq!(dp_to_px(20, 320), 40);
        assert_eq!(dp_to_px(1, 160), 1);
    }

    #[test]
    fn midpoint_rounds_half_up() {
        assert_eq!(
            midpoint(Point::new(100, -2), Point::new(201, -1)),
            Point::new(151, -1)
        );
    }

    #[test]
    fn runtime_tap_settings_update() {
        let mut detector = GestureDetector::default();
        detector.set_disable_double_tap(false);
        assert!(detector.config().double_tap_enabled);
        detector.set_disable_double_tap(true);
        assert!(!detector.config().double_tap_enabled);
        detector.set_tap_interval_override(Some(25));
        assert_eq!(detector.tap_interval_override_ms, Some(25));
        detector.set_tap_interval_override(None);
        assert_eq!(detector.tap_interval_override_ms, None);
    }

    #[test]
    fn cardinal_and_diagonal_paths() {
        let o = Point::new(100, 100);
        assert_eq!(
            get_path(o, Point::new(200, 100), false, false).0,
            Some(Direction::East)
        );
        assert_eq!(
            get_path(o, Point::new(100, 0), false, false).0,
            Some(Direction::North)
        );
        assert_eq!(
            get_path(o, Point::new(170, 170), false, false).0,
            Some(Direction::SouthEast)
        );
        // dy = 10 is below 0.577 * dx = 57.7, so a shallow move stays horizontal.
        assert_eq!(
            get_path(o, Point::new(200, 110), false, false).0,
            Some(Direction::East)
        );
        assert_eq!(get_path(o, o, false, false).0, None);
    }

    #[test]
    fn rotate_normalizes_across_the_branch_cut() {
        // Pivot at the origin, one finger from (-1, 0) to (1, 0): half a turn, normalized.
        let angle = get_rotate(Point::new(0, 0), Point::new(-10, 0), Point::new(10, 0));
        assert!((angle.abs() - 180.0).abs() < 1e-9);
        let quarter = get_rotate(Point::new(0, 0), Point::new(10, 0), Point::new(0, 10));
        assert!((quarter - 90.0).abs() < 1e-9);
    }

    #[test]
    fn hold_pan_keeps_inward_fields() {
        let inward = Gesture::InwardPan {
            pos: Point::new(1, 1),
            start_pos: Point::new(0, 0),
            axis: Some(Axis::Horizontal),
            distance: 5.0,
            span: 1.0,
            start_span: 2.0,
            time_us: 7,
        };
        match to_hold_pan(inward) {
            Gesture::HoldPan {
                direction,
                start_pos,
                span,
                start_span,
                ..
            } => {
                assert_eq!(direction, Some(HoldPanDirection::Axis(Axis::Horizontal)));
                assert_eq!(start_pos, Some(Point::new(0, 0)));
                assert_eq!(span, Some(1.0));
                assert_eq!(start_span, Some(2.0));
            }
            other => panic!("expected hold_pan, got {}", other.name()),
        }
    }
}
