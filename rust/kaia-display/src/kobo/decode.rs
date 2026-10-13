//! Decodes Kobo evdev events into [`InputEvent`]s. Pure: the device code reads
//! `input_event` structs and passes them here as [`RawEvent`]s, so the decoder runs on
//! the host with event sequences that the tests build.
//!
//! # Touch
//!
//! The Libra Colour uses KOReader's multitouch protocol B path. The device sets no
//! snow or phoenix protocol flag (`frontend/device/kobo/device.lua`, `KoboMonza`), so
//! `Input:handleTouchEv` in `frontend/device/input.lua` handles its touch node.
//! Each slot holds a tracking id (`ABS_MT_TRACKING_ID`). A value of -1 lifts the
//! contact. Positions come from `ABS_MT_POSITION_X` and `ABS_MT_POSITION_Y`. KOReader
//! also accepts `ABS_X` and `ABS_Y` in the same slot, and so does this decoder.
//! Touch frames are emitted on `SYN_REPORT`, with slots in their order of appearance.
//!
//! Protocol A and the single-touch path are not implemented, because no Libra Colour
//! code path uses them.
//!
//! # Coordinates
//!
//! KOReader's Kobo hook (`Kobo:initEventAdjustHooks`) switches the axes and mirrors
//! one of them. KOReader's `device.lua` sets `touch_switch_xy` and `touch_mirrored_y`
//! for the Libra Colour. The raw ranges are read from the device (`EVIOCGABS`). KOReader
//! does not scale raw values. This decoder scales each axis from its raw range to the
//! screen. When a range equals the screen size minus one, scaling is the identity.
//!
//! # Keys
//!
//! Key codes come from KOReader's Kobo `event_map` (`frontend/device/kobo/device.lua`).
//! Only the codes that map to a [`Key`] are decoded. The light button (90) has no
//! [`Key`] variant and is dropped.

use kaia_gfx::{Point, Size};
use kaia_input::{Key, KeyEvent, Phase, TouchEvent};

use crate::input::InputEvent;
use crate::rotation::Rotation;

/// Event types and codes from `linux/input-event-codes.h`.
pub mod code {
    pub const EV_SYN: u16 = 0x00;
    pub const EV_KEY: u16 = 0x01;
    pub const EV_ABS: u16 = 0x03;
    pub const SYN_REPORT: u16 = 0x00;
    pub const ABS_X: u16 = 0x00;
    pub const ABS_Y: u16 = 0x01;
    pub const ABS_MT_SLOT: u16 = 0x2f;
    pub const ABS_MT_POSITION_X: u16 = 0x35;
    pub const ABS_MT_POSITION_Y: u16 = 0x36;
    pub const ABS_MT_TRACKING_ID: u16 = 0x39;
}

/// Key codes that KOReader's Kobo `event_map` names. The comments give KOReader's name.
pub mod key {
    /// KEY_H on the Elipsa and KEY_F1 on the other boards, both mapped to SleepCover.
    pub const SLEEP_COVER_A: u16 = 35;
    pub const SLEEP_COVER_B: u16 = 59;
    /// LightButton. No [`super::Key`] variant exists for it, so it is dropped.
    pub const LIGHT_BUTTON: u16 = 90;
    /// Home.
    pub const HOME: u16 = 102;
    /// Power.
    pub const POWER: u16 = 116;
    /// RPgBack, the left page-back button on the Kobo's bezel.
    pub const RPG_BACK: u16 = 193;
    /// RPgFwd, the page-forward button.
    pub const RPG_FWD: u16 = 194;
}

/// Values of `input_event.value` for `EV_KEY`.
const KEY_RELEASE: i32 = 0;
const KEY_PRESS: i32 = 1;

/// How many touch slots the decoder tracks. A slot number at or above this is ignored.
pub const MAX_SLOTS: usize = 10;

/// One `input_event`, with the time already in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawEvent {
    pub time_us: u64,
    pub kind: u16,
    pub code: u16,
    pub value: i32,
}

/// One decoded key or a complete touch frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedEvent {
    Key(KeyEvent),
    TouchFrame(Vec<TouchEvent>),
}

/// How raw touch coordinates become logical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calibration {
    /// The panel size at rotation 0, which is the portrait screen on the Kobo.
    pub panel: Size,
    /// Raw range of the X axis, inclusive. Read with `EVIOCGABS`.
    pub x_range: (i32, i32),
    /// Raw range of the Y axis, inclusive.
    pub y_range: (i32, i32),
    /// Swap the raw axes before mapping. KOReader's `touch_switch_xy`.
    pub switch_xy: bool,
    /// Mirror the screen X axis after the swap. KOReader's `touch_mirrored_x`.
    pub mirror_x: bool,
    /// Mirror the screen Y axis after the swap. KOReader's `touch_mirrored_y`.
    pub mirror_y: bool,
    /// The screen rotation. Touch positions come out in logical orientation.
    pub rotation: Rotation,
}

impl Calibration {
    /// The Libra Colour: X and Y switched, and Y mirrored (`KoboMonza` in
    /// `frontend/device/kobo/device.lua`). The ranges come from the device.
    pub fn libra_colour(x_range: (i32, i32), y_range: (i32, i32), rotation: Rotation) -> Self {
        Self {
            panel: Size::new(1264, 1680),
            x_range,
            y_range,
            switch_xy: true,
            mirror_x: false,
            mirror_y: true,
            rotation,
        }
    }

    /// Maps a raw sample to a logical pixel.
    pub fn map(&self, raw_x: i32, raw_y: i32) -> Point {
        let nx = normalize(raw_x, self.x_range);
        let ny = normalize(raw_y, self.y_range);
        let (mut sx, mut sy) = if self.switch_xy { (ny, nx) } else { (nx, ny) };
        if self.mirror_x {
            sx = 1.0 - sx;
        }
        if self.mirror_y {
            sy = 1.0 - sy;
        }
        let max_x = (self.panel.w.max(1) - 1) as f64;
        let max_y = (self.panel.h.max(1) - 1) as f64;
        let px = ((sx * max_x).round() as i32).clamp(0, self.panel.w.max(1) as i32 - 1);
        let py = ((sy * max_y).round() as i32).clamp(0, self.panel.h.max(1) as i32 - 1);
        self.rotation.panel_to_logical(Point::new(px, py), self.panel)
    }
}

/// Maps a raw value into `[0, 1]` over its range. A range with max not above min maps
/// to 0.
fn normalize(value: i32, (min, max): (i32, i32)) -> f64 {
    if max <= min {
        return 0.0;
    }
    ((value - min) as f64 / (max - min) as f64).clamp(0.0, 1.0)
}

/// What the decoder knows about one slot.
#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    /// Tracking id from the latest sample. None means no contact.
    id: Option<i32>,
    /// Tracking id that the last report sent. None means no contact was reported.
    reported: Option<i32>,
    raw_x: i32,
    raw_y: i32,
    /// True when a position changed since the last report.
    moved: bool,
}

/// Turns raw evdev events into touch and key events.
#[derive(Debug, Clone)]
pub struct EvdevDecoder {
    cal: Calibration,
    slots: [Slot; MAX_SLOTS],
    /// The slot that the next position and tracking events apply to. A value of
    /// [`MAX_SLOTS`] means an out-of-range slot, and those events are dropped.
    current: usize,
    frame_order: Vec<usize>,
}

impl EvdevDecoder {
    pub fn new(cal: Calibration) -> Self {
        Self {
            cal,
            slots: [Slot::default(); MAX_SLOTS],
            current: 0,
            frame_order: Vec::new(),
        }
    }

    /// Changes the rotation that later touch positions use.
    pub fn set_rotation(&mut self, rotation: Rotation) {
        self.cal.rotation = rotation;
    }

    /// Consumes a batch of raw events and returns the input events they produce, in
    /// arrival order. A touch frame is only final at its `SYN_REPORT`, so a partial
    /// frame at the end of the batch produces nothing yet.
    pub fn feed(&mut self, events: &[RawEvent]) -> Vec<InputEvent> {
        self.feed_frames(events)
            .into_iter()
            .flat_map(|event| match event {
                DecodedEvent::Key(key) => vec![InputEvent::Key(key)],
                DecodedEvent::TouchFrame(frame) => frame.into_iter().map(InputEvent::Touch).collect(),
            })
            .collect()
    }

    /// Returns a separate touch frame for each `SYN_REPORT`.
    pub fn feed_frames(&mut self, events: &[RawEvent]) -> Vec<DecodedEvent> {
        let mut out = Vec::new();
        for ev in events {
            match ev.kind {
                code::EV_ABS => self.abs(ev),
                code::EV_KEY => {
                    if let Some(key) = key_event(ev) {
                        out.push(DecodedEvent::Key(key));
                    }
                }
                code::EV_SYN if ev.code == code::SYN_REPORT => {
                    out.push(DecodedEvent::TouchFrame(self.report(ev.time_us)));
                }
                _ => {}
            }
        }
        out
    }

    fn abs(&mut self, ev: &RawEvent) {
        if ev.code == code::ABS_MT_SLOT {
            self.current = if (0..MAX_SLOTS as i32).contains(&ev.value) {
                ev.value as usize
            } else {
                MAX_SLOTS
            };
            return;
        }
        let Some(slot) = self.slots.get_mut(self.current) else {
            return;
        };
        if matches!(
            ev.code,
            code::ABS_MT_TRACKING_ID
                | code::ABS_MT_POSITION_X
                | code::ABS_MT_POSITION_Y
                | code::ABS_X
                | code::ABS_Y
        ) && !self.frame_order.contains(&self.current)
        {
            self.frame_order.push(self.current);
        }
        match ev.code {
            code::ABS_MT_TRACKING_ID => {
                slot.id = if ev.value < 0 { None } else { Some(ev.value) };
            }
            code::ABS_MT_POSITION_X | code::ABS_X => set_axis(&mut slot.raw_x, ev.value, &mut slot.moved),
            code::ABS_MT_POSITION_Y | code::ABS_Y => set_axis(&mut slot.raw_y, ev.value, &mut slot.moved),
            _ => {}
        }
    }

    /// Emits the changes of the frame that ended with `SYN_REPORT`.
    fn report(&mut self, time_us: u64) -> Vec<TouchEvent> {
        let cal = self.cal;
        let mut out = Vec::new();
        for index in self.frame_order.drain(..) {
            let slot = &mut self.slots[index];
            let pos = cal.map(slot.raw_x, slot.raw_y);
            let touch = |phase| TouchEvent {
                slot: index as u8,
                phase,
                pos,
                time_us,
            };
            match (slot.reported, slot.id) {
                (None, None) => {}
                (None, Some(id)) => {
                    out.push(touch(Phase::Down));
                    slot.reported = Some(id);
                }
                (Some(_), None) => {
                    out.push(touch(Phase::Up));
                    slot.reported = None;
                }
                (Some(old), Some(new)) if old != new => {
                    // A new contact in the same slot, without a lift in between.
                    out.push(touch(Phase::Up));
                    out.push(touch(Phase::Down));
                    slot.reported = Some(new);
                }
                (Some(_), Some(_)) => {
                    if slot.moved {
                        out.push(touch(Phase::Move));
                    }
                }
            }
            slot.moved = false;
        }
        out
    }
}

/// Stores a raw axis value and marks the slot as moved when the value changed.
fn set_axis(axis: &mut i32, value: i32, moved: &mut bool) {
    if *axis != value {
        *axis = value;
        *moved = true;
    }
}

/// Maps a `EV_KEY` event to a [`KeyEvent`]. Returns None for keys with no mapping,
/// for autorepeat (value 2), and for the light button.
fn key_event(ev: &RawEvent) -> Option<KeyEvent> {
    if ev.value != KEY_PRESS && ev.value != KEY_RELEASE {
        return None;
    }
    let pressed = ev.value == KEY_PRESS;
    let key = match ev.code {
        key::RPG_BACK => Key::RPgBack,
        key::RPG_FWD => Key::RPgFwd,
        key::POWER => Key::Power,
        key::HOME => Key::Home,
        // KOReader fires SleepCoverClosed on press and SleepCoverOpened on release.
        // Each lid event is reported once, with `pressed` set.
        key::SLEEP_COVER_A | key::SLEEP_COVER_B => {
            let key = if pressed {
                Key::SleepCoverClosed
            } else {
                Key::SleepCoverOpened
            };
            return Some(KeyEvent {
                key,
                pressed: true,
                time_us: ev.time_us,
            });
        }
        _ => return None,
    };
    Some(KeyEvent {
        key,
        pressed,
        time_us: ev.time_us,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Panel-sized identity calibration: the raw ranges equal the screen size minus one,
    /// and the axes are not switched or mirrored.
    fn identity() -> Calibration {
        Calibration {
            panel: Size::new(1264, 1680),
            x_range: (0, 1263),
            y_range: (0, 1679),
            switch_xy: false,
            mirror_x: false,
            mirror_y: false,
            rotation: Rotation::Deg0,
        }
    }

    /// Libra Colour with assumed ranges. The X axis runs along the long side of the
    /// panel, so its range is 0 to 1679, and Y runs 0 to 1263. These ranges are not
    /// measured on a device.
    fn libra() -> Calibration {
        Calibration::libra_colour((0, 1679), (0, 1263), Rotation::Deg0)
    }

    fn abs(code: u16, value: i32, t: u64) -> RawEvent {
        RawEvent {
            time_us: t,
            kind: code::EV_ABS,
            code,
            value,
        }
    }

    fn syn(t: u64) -> RawEvent {
        RawEvent {
            time_us: t,
            kind: code::EV_SYN,
            code: code::SYN_REPORT,
            value: 0,
        }
    }

    fn key(code: u16, value: i32, t: u64) -> RawEvent {
        RawEvent {
            time_us: t,
            kind: code::EV_KEY,
            code,
            value,
        }
    }

    fn touch(slot: u8, phase: Phase, x: i32, y: i32, t: u64) -> InputEvent {
        InputEvent::Touch(TouchEvent {
            slot,
            phase,
            pos: Point::new(x, y),
            time_us: t,
        })
    }

    #[test]
    fn frames_keep_slot_appearance_order_and_sync_boundaries() {
        let mut decoder = EvdevDecoder::new(identity());
        let frames = decoder.feed_frames(&[
            abs(code::ABS_MT_SLOT, 1, 0),
            abs(code::ABS_MT_TRACKING_ID, 11, 0),
            abs(code::ABS_MT_POSITION_X, 300, 0),
            abs(code::ABS_MT_SLOT, 0, 0),
            abs(code::ABS_MT_TRACKING_ID, 10, 0),
            abs(code::ABS_MT_POSITION_X, 100, 0),
            syn(1),
            abs(code::ABS_MT_POSITION_Y, 200, 2),
            syn(2),
        ]);
        assert_eq!(
            frames,
            vec![
                DecodedEvent::TouchFrame(vec![
                    TouchEvent {
                        slot: 1,
                        phase: Phase::Down,
                        pos: Point::new(300, 0),
                        time_us: 1
                    },
                    TouchEvent {
                        slot: 0,
                        phase: Phase::Down,
                        pos: Point::new(100, 0),
                        time_us: 1
                    },
                ]),
                DecodedEvent::TouchFrame(vec![TouchEvent {
                    slot: 0,
                    phase: Phase::Move,
                    pos: Point::new(100, 200),
                    time_us: 2
                },]),
            ]
        );
    }

    #[test]
    fn single_tap_is_down_then_up() {
        let mut d = EvdevDecoder::new(identity());
        let out = d.feed(&[
            abs(code::ABS_MT_SLOT, 0, 0),
            abs(code::ABS_MT_TRACKING_ID, 7, 0),
            abs(code::ABS_MT_POSITION_X, 100, 0),
            abs(code::ABS_MT_POSITION_Y, 200, 0),
            syn(1000),
            abs(code::ABS_MT_TRACKING_ID, -1, 0),
            syn(2000),
        ]);
        assert_eq!(
            out,
            vec![
                touch(0, Phase::Down, 100, 200, 1000),
                touch(0, Phase::Up, 100, 200, 2000),
            ]
        );
    }

    #[test]
    fn partial_frame_produces_nothing_until_sync() {
        let mut d = EvdevDecoder::new(identity());
        let out = d.feed(&[
            abs(code::ABS_MT_TRACKING_ID, 3, 0),
            abs(code::ABS_MT_POSITION_X, 10, 0),
            abs(code::ABS_MT_POSITION_Y, 20, 0),
        ]);
        assert!(out.is_empty());
        let out = d.feed(&[syn(5)]);
        assert_eq!(out, vec![touch(0, Phase::Down, 10, 20, 5)]);
    }

    #[test]
    fn libra_axes_are_switched_and_mirrored() {
        // Raw (100, 200): X becomes screen Y and is mirrored, Y becomes screen X.
        let mut d = EvdevDecoder::new(libra());
        let out = d.feed(&[
            abs(code::ABS_MT_TRACKING_ID, 1, 0),
            abs(code::ABS_MT_POSITION_X, 100, 0),
            abs(code::ABS_MT_POSITION_Y, 200, 0),
            syn(1),
        ]);
        assert_eq!(out, vec![touch(0, Phase::Down, 200, 1579, 1)]);
    }

    #[test]
    fn libra_raw_origin_maps_to_bottom_left() {
        let mut d = EvdevDecoder::new(libra());
        let out = d.feed(&[
            abs(code::ABS_MT_TRACKING_ID, 1, 0),
            abs(code::ABS_MT_POSITION_X, 0, 0),
            abs(code::ABS_MT_POSITION_Y, 0, 0),
            syn(1),
        ]);
        assert_eq!(out, vec![touch(0, Phase::Down, 0, 1679, 1)]);
    }

    #[test]
    fn raw_ranges_scale_to_the_screen() {
        // A 0..4095 range maps onto the screen. Raw X 2048 is about half of the range.
        let cal = Calibration {
            x_range: (0, 4095),
            y_range: (0, 4095),
            ..identity()
        };
        let mut d = EvdevDecoder::new(cal);
        let out = d.feed(&[
            abs(code::ABS_MT_TRACKING_ID, 1, 0),
            abs(code::ABS_MT_POSITION_X, 2048, 0),
            abs(code::ABS_MT_POSITION_Y, 4095, 0),
            syn(1),
        ]);
        // 2048 / 4095 * 1263 = 631.65, rounded to 632. Y at the top of the range maps to 1679.
        assert_eq!(out, vec![touch(0, Phase::Down, 632, 1679, 1)]);
    }

    #[test]
    fn two_finger_pinch_downs_moves_and_lifts_per_slot() {
        let mut d = EvdevDecoder::new(identity());
        let down = d.feed(&[
            abs(code::ABS_MT_SLOT, 0, 0),
            abs(code::ABS_MT_TRACKING_ID, 1, 0),
            abs(code::ABS_MT_POSITION_X, 100, 0),
            abs(code::ABS_MT_POSITION_Y, 100, 0),
            abs(code::ABS_MT_SLOT, 1, 0),
            abs(code::ABS_MT_TRACKING_ID, 2, 0),
            abs(code::ABS_MT_POSITION_X, 300, 0),
            abs(code::ABS_MT_POSITION_Y, 300, 0),
            syn(10),
        ]);
        assert_eq!(
            down,
            vec![
                touch(0, Phase::Down, 100, 100, 10),
                touch(1, Phase::Down, 300, 300, 10),
            ]
        );

        // Both fingers move apart.
        let moved = d.feed(&[
            abs(code::ABS_MT_SLOT, 0, 0),
            abs(code::ABS_MT_POSITION_X, 90, 0),
            abs(code::ABS_MT_SLOT, 1, 0),
            abs(code::ABS_MT_POSITION_X, 310, 0),
            syn(20),
        ]);
        assert_eq!(
            moved,
            vec![
                touch(0, Phase::Move, 90, 100, 20),
                touch(1, Phase::Move, 310, 300, 20),
            ]
        );

        // Only the first finger moves. The second one stays quiet.
        let one = d.feed(&[
            abs(code::ABS_MT_SLOT, 0, 0),
            abs(code::ABS_MT_POSITION_Y, 120, 0),
            syn(30),
        ]);
        assert_eq!(one, vec![touch(0, Phase::Move, 90, 120, 30)]);

        let lift = d.feed(&[
            abs(code::ABS_MT_SLOT, 0, 0),
            abs(code::ABS_MT_TRACKING_ID, -1, 0),
            abs(code::ABS_MT_SLOT, 1, 0),
            abs(code::ABS_MT_TRACKING_ID, -1, 0),
            syn(40),
        ]);
        assert_eq!(
            lift,
            vec![
                touch(0, Phase::Up, 90, 120, 40),
                touch(1, Phase::Up, 310, 300, 40),
            ]
        );
    }

    #[test]
    fn slot_switch_keeps_each_slots_position() {
        let mut d = EvdevDecoder::new(identity());
        d.feed(&[
            abs(code::ABS_MT_SLOT, 0, 0),
            abs(code::ABS_MT_TRACKING_ID, 5, 0),
            abs(code::ABS_MT_POSITION_X, 10, 0),
            abs(code::ABS_MT_POSITION_Y, 10, 0),
            syn(1),
        ]);
        // Slot 1 comes up while slot 0 is still down.
        let second = d.feed(&[
            abs(code::ABS_MT_SLOT, 1, 0),
            abs(code::ABS_MT_TRACKING_ID, 6, 0),
            abs(code::ABS_MT_POSITION_X, 500, 0),
            abs(code::ABS_MT_POSITION_Y, 500, 0),
            syn(2),
        ]);
        assert_eq!(second, vec![touch(1, Phase::Down, 500, 500, 2)]);

        // Slot 0 moves. The frame selects it first, and slot 1 keeps its position.
        let moved = d.feed(&[
            abs(code::ABS_MT_SLOT, 0, 0),
            abs(code::ABS_MT_POSITION_X, 12, 0),
            syn(3),
        ]);
        assert_eq!(moved, vec![touch(0, Phase::Move, 12, 10, 3)]);

        let lift = d.feed(&[
            abs(code::ABS_MT_SLOT, 1, 0),
            abs(code::ABS_MT_TRACKING_ID, -1, 0),
            syn(4),
        ]);
        assert_eq!(lift, vec![touch(1, Phase::Up, 500, 500, 4)]);
    }

    #[test]
    fn unchanged_position_emits_no_move() {
        let mut d = EvdevDecoder::new(identity());
        d.feed(&[
            abs(code::ABS_MT_TRACKING_ID, 1, 0),
            abs(code::ABS_MT_POSITION_X, 40, 0),
            abs(code::ABS_MT_POSITION_Y, 40, 0),
            syn(1),
        ]);
        let same = d.feed(&[abs(code::ABS_MT_POSITION_X, 40, 0), syn(2)]);
        assert!(same.is_empty());
        let frame_without_samples = d.feed(&[syn(3)]);
        assert!(frame_without_samples.is_empty());
    }

    #[test]
    fn new_contact_in_the_same_slot_is_up_then_down() {
        let mut d = EvdevDecoder::new(identity());
        d.feed(&[
            abs(code::ABS_MT_TRACKING_ID, 1, 0),
            abs(code::ABS_MT_POSITION_X, 5, 0),
            abs(code::ABS_MT_POSITION_Y, 5, 0),
            syn(1),
        ]);
        let out = d.feed(&[
            abs(code::ABS_MT_TRACKING_ID, 2, 0),
            abs(code::ABS_MT_POSITION_X, 60, 0),
            abs(code::ABS_MT_POSITION_Y, 70, 0),
            syn(2),
        ]);
        assert_eq!(
            out,
            vec![touch(0, Phase::Up, 60, 70, 2), touch(0, Phase::Down, 60, 70, 2),]
        );
    }

    #[test]
    fn out_of_range_slot_is_ignored() {
        let mut d = EvdevDecoder::new(identity());
        let out = d.feed(&[
            abs(code::ABS_MT_SLOT, MAX_SLOTS as i32 + 2, 0),
            abs(code::ABS_MT_TRACKING_ID, 3, 0),
            abs(code::ABS_MT_POSITION_X, 1, 0),
            abs(code::ABS_MT_POSITION_Y, 1, 0),
            syn(1),
        ]);
        assert!(out.is_empty());
    }

    #[test]
    fn rotation_moves_touch_into_logical_space() {
        // Deg90 is KOReader mode 1 (framebuffer.lua, line 51). Panel (100, 200) maps to
        // logical (1679 - 200, 100): x = H - 1 - y and y = x, with H = 1680.
        let cal = Calibration {
            rotation: Rotation::Deg90,
            ..identity()
        };
        let mut d = EvdevDecoder::new(cal);
        let out = d.feed(&[
            abs(code::ABS_MT_TRACKING_ID, 1, 0),
            abs(code::ABS_MT_POSITION_X, 100, 0),
            abs(code::ABS_MT_POSITION_Y, 200, 0),
            syn(1),
        ]);
        assert_eq!(out, vec![touch(0, Phase::Down, 1479, 100, 1)]);
    }

    #[test]
    fn page_turn_and_power_keys_use_koreader_names() {
        let mut d = EvdevDecoder::new(identity());
        let out = d.feed(&[
            key(key::RPG_FWD, KEY_PRESS, 1),
            key(key::RPG_FWD, KEY_RELEASE, 2),
            key(key::RPG_BACK, KEY_PRESS, 3),
            key(key::POWER, KEY_PRESS, 4),
            key(key::HOME, KEY_RELEASE, 5),
        ]);
        assert_eq!(
            out,
            vec![
                InputEvent::Key(KeyEvent {
                    key: Key::RPgFwd,
                    pressed: true,
                    time_us: 1
                }),
                InputEvent::Key(KeyEvent {
                    key: Key::RPgFwd,
                    pressed: false,
                    time_us: 2
                }),
                InputEvent::Key(KeyEvent {
                    key: Key::RPgBack,
                    pressed: true,
                    time_us: 3
                }),
                InputEvent::Key(KeyEvent {
                    key: Key::Power,
                    pressed: true,
                    time_us: 4
                }),
                InputEvent::Key(KeyEvent {
                    key: Key::Home,
                    pressed: false,
                    time_us: 5
                }),
            ]
        );
    }

    #[test]
    fn autorepeat_unknown_and_light_keys_are_dropped() {
        let mut d = EvdevDecoder::new(identity());
        let out = d.feed(&[
            key(key::RPG_FWD, 2, 1),
            key(10, KEY_PRESS, 2),
            key(key::LIGHT_BUTTON, KEY_RELEASE, 3),
        ]);
        assert!(out.is_empty());
    }

    #[test]
    fn sleep_cover_reports_closed_on_press_and_opened_on_release() {
        let mut d = EvdevDecoder::new(identity());
        let out = d.feed(&[
            key(key::SLEEP_COVER_B, KEY_PRESS, 1),
            key(key::SLEEP_COVER_A, KEY_RELEASE, 2),
        ]);
        assert_eq!(
            out,
            vec![
                InputEvent::Key(KeyEvent {
                    key: Key::SleepCoverClosed,
                    pressed: true,
                    time_us: 1
                }),
                InputEvent::Key(KeyEvent {
                    key: Key::SleepCoverOpened,
                    pressed: true,
                    time_us: 2
                }),
            ]
        );
    }

    #[test]
    fn touch_and_key_events_keep_arrival_order() {
        let mut d = EvdevDecoder::new(identity());
        let out = d.feed(&[
            key(key::POWER, KEY_PRESS, 1),
            abs(code::ABS_MT_TRACKING_ID, 1, 2),
            abs(code::ABS_MT_POSITION_X, 3, 2),
            abs(code::ABS_MT_POSITION_Y, 4, 2),
            syn(2),
            key(key::POWER, KEY_RELEASE, 3),
        ]);
        assert_eq!(out.len(), 3);
        assert!(matches!(
            out[0],
            InputEvent::Key(KeyEvent {
                key: Key::Power,
                pressed: true,
                ..
            })
        ));
        assert!(matches!(
            out[1],
            InputEvent::Touch(TouchEvent {
                phase: Phase::Down,
                ..
            })
        ));
        assert!(matches!(
            out[2],
            InputEvent::Key(KeyEvent {
                key: Key::Power,
                pressed: false,
                ..
            })
        ));
    }

    #[test]
    fn set_rotation_applies_to_later_samples() {
        let mut d = EvdevDecoder::new(identity());
        d.set_rotation(Rotation::Deg180);
        let out = d.feed(&[
            abs(code::ABS_MT_TRACKING_ID, 1, 0),
            abs(code::ABS_MT_POSITION_X, 0, 0),
            abs(code::ABS_MT_POSITION_Y, 0, 0),
            syn(1),
        ]);
        // Deg180 maps (0, 0) to the bottom right corner of the panel.
        assert_eq!(out, vec![touch(0, Phase::Down, 1263, 1679, 1)]);
    }
}
