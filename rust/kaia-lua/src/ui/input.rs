//! Kaia input and Kaia events as KOReader event objects.
//!
//! A gesture becomes the `ges` table of `frontend/device/gesturedetector.lua`.
//! The shim `kaia/events.lua` adds the Geom and time objects, because only Lua
//! can build them. A key becomes `Event:new("KeyPress", Key)` with the device
//! key object of `frontend/device/key.lua`.

use kaia_events::value::Value;
use kaia_gfx::Point;
use kaia_input::{Axis, Direction, Gesture, Key};
use mlua::{Function, Lua, MultiValue, Table, Value as LuaValue};

use crate::convert;

/// The shim module that builds KOReader event objects.
const EVENTS_MODULE: &str = "kaia/events";

fn runtime(message: impl std::fmt::Display) -> mlua::Error {
    mlua::Error::RuntimeError(message.to_string())
}

/// Calls a function of `kaia/events`.
fn events_function(lua: &Lua, name: &str) -> mlua::Result<Function> {
    let require: Function = lua.globals().get("require")?;
    let module: Table = require.call(EVENTS_MODULE)?;
    module.get(name)
}

fn point_table(lua: &Lua, p: Point) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("x", p.x)?;
    t.set("y", p.y)?;
    Ok(t)
}

/// The fields a gesture can carry, in KOReader's names and value formats. A
/// field the gesture does not have stays `None`, and the table has no entry for it.
///
/// `direction` has three meanings, as in `frontend/device/gesturedetector.lua`:
/// a compass name for pans and swipes, the axis ("horizontal", "vertical" or
/// "diagonal") for pinch, spread and the two-finger span pans, and "cw" or "ccw"
/// for rotate.
#[derive(Default)]
struct Fields {
    start_pos: Option<Point>,
    end_pos: Option<Point>,
    /// Current position minus start position, for the pan gestures.
    relative: Option<Point>,
    direction: Option<&'static str>,
    distance: Option<f64>,
    span: Option<f64>,
    start_span: Option<f64>,
    angle: Option<f64>,
    /// Space-separated direction names, as `Contact:handleSwipe` builds them.
    multiswipe_directions: Option<String>,
}

fn direction_name(d: &Option<Direction>) -> Option<&'static str> {
    d.map(Direction::name)
}

fn axis_name(a: &Option<Axis>) -> Option<&'static str> {
    a.map(Axis::name)
}

/// Current minus start, the `relative` field of the pan gestures.
fn delta(pos: Point, start: Point) -> Point {
    Point::new(pos.x - start.x, pos.y - start.y)
}

fn fields(g: &Gesture) -> Fields {
    let mut f = Fields::default();
    match g {
        Gesture::HoldPan {
            pos,
            start_pos,
            direction,
            distance,
            ..
        } => {
            f.start_pos = *start_pos;
            f.relative = start_pos.map(|s| delta(*pos, s));
            f.direction = direction_name(direction);
            f.distance = Some(*distance);
        }
        Gesture::Pan {
            pos,
            start_pos,
            direction,
            distance,
            ..
        }
        | Gesture::TwoFingerHoldPan {
            pos,
            start_pos,
            direction,
            distance,
            ..
        }
        | Gesture::TwoFingerPan {
            pos,
            start_pos,
            direction,
            distance,
            ..
        } => {
            f.start_pos = Some(*start_pos);
            f.relative = Some(delta(*pos, *start_pos));
            f.direction = direction_name(direction);
            f.distance = Some(*distance);
        }
        Gesture::Swipe {
            end_pos,
            direction,
            distance,
            ..
        }
        | Gesture::TwoFingerSwipe {
            end_pos,
            direction,
            distance,
            ..
        } => {
            f.end_pos = Some(*end_pos);
            f.direction = direction_name(direction);
            f.distance = Some(*distance);
        }
        Gesture::Multiswipe {
            end_pos,
            direction,
            distance,
            directions,
            ..
        } => {
            f.end_pos = Some(*end_pos);
            f.direction = direction_name(direction);
            f.distance = Some(*distance);
            let names: Vec<&str> = directions.iter().map(|d| d.name()).collect();
            f.multiswipe_directions = Some(names.join(" "));
        }
        Gesture::TwoFingerTap { span, .. }
        | Gesture::TwoFingerHold { span, .. }
        | Gesture::TwoFingerHoldRelease { span, .. }
        | Gesture::TwoFingerHoldPanRelease { span, .. } => {
            f.span = Some(*span);
        }
        Gesture::InwardPan {
            start_pos,
            axis,
            distance,
            span,
            start_span,
            ..
        }
        | Gesture::Pinch {
            start_pos,
            axis,
            distance,
            span,
            start_span,
            ..
        } => {
            f.start_pos = Some(*start_pos);
            f.direction = axis_name(axis);
            f.distance = Some(*distance);
            f.span = Some(*span);
            f.start_span = Some(*start_span);
        }
        Gesture::OutwardPan {
            end_pos,
            axis,
            distance,
            span,
            start_span,
            ..
        }
        | Gesture::Spread {
            end_pos,
            axis,
            distance,
            span,
            start_span,
            ..
        } => {
            f.end_pos = Some(*end_pos);
            f.direction = axis_name(axis);
            f.distance = Some(*distance);
            f.span = Some(*span);
            f.start_span = Some(*start_span);
        }
        Gesture::Rotate { angle, turn, .. } => {
            f.angle = Some(*angle);
            f.direction = Some(turn.name());
        }
        _ => {}
    }
    f
}

/// The `ges` table for a gesture. `pos` is the gesture position. `time_us` is
/// the event time in microseconds, which the shim turns into a `time` value.
pub(crate) fn gesture_table(lua: &Lua, g: &Gesture) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("ges", g.name())?;
    t.set("pos", point_table(lua, g.pos())?)?;
    t.set("time_us", g.time_us() as f64)?;
    let f = fields(g);
    if let Some(p) = f.start_pos {
        t.set("start_pos", point_table(lua, p)?)?;
    }
    if let Some(p) = f.end_pos {
        t.set("end_pos", point_table(lua, p)?)?;
    }
    if let Some(p) = f.relative {
        t.set("relative", point_table(lua, p)?)?;
    }
    if let Some(d) = f.direction {
        t.set("direction", d)?;
    }
    if let Some(d) = f.distance {
        t.set("distance", d)?;
    }
    if let Some(s) = f.span {
        t.set("span", s)?;
    }
    if let Some(s) = f.start_span {
        t.set("start_span", s)?;
    }
    if let Some(a) = f.angle {
        t.set("angle", a)?;
    }
    if let Some(dirs) = f.multiswipe_directions {
        t.set("multiswipe_directions", dirs)?;
    }
    Ok(t)
}

/// `Event:new("Gesture", ges)`.
pub(crate) fn gesture_event(lua: &Lua, g: &Gesture) -> mlua::Result<LuaValue> {
    let make = events_function(lua, "gesture")?;
    let table = gesture_table(lua, g)?;
    make.call(table)
}

/// `Event:new("KeyPress", key)` or `KeyRelease`.
pub(crate) fn key_event(lua: &Lua, key: Key, pressed: bool) -> mlua::Result<LuaValue> {
    let make = events_function(lua, "key")?;
    make.call((key.name(), pressed))
}

/// `Event:new(name, ...)` with the Kaia values as arguments.
pub(crate) fn named_event(lua: &Lua, name: &str, args: &[Value]) -> mlua::Result<LuaValue> {
    let make = events_function(lua, "named")?;
    let mut values = Vec::with_capacity(args.len() + 1);
    values.push(LuaValue::String(lua.create_string(name)?));
    for arg in args {
        values.push(convert::to_lua(lua, arg).map_err(runtime)?);
    }
    make.call(MultiValue::from_vec(values))
}

/// `widget:handleEvent(event)`. Returns true when the handler used the event.
pub(crate) fn handle(widget: &Table, event: LuaValue) -> mlua::Result<bool> {
    let method: Function = widget.get("handleEvent")?;
    let answer: Option<bool> = method.call((widget.clone(), event))?;
    Ok(answer == Some(true))
}
