//! The UI bridge in isolation: event order, widget identity, scheduling, errors,
//! input fields, refresh modes, the shared frame buffer and the screen scale.
//! The widgets here are small Lua tables, so each check tests one bridge rule.

mod common;

use std::time::Duration;

use common::{lua_ui, serial, TempDir};
use kaia_events::{Rect, Refresh, RefreshMode, Value};
use kaia_gfx::{Color, Point, Size};
use kaia_input::{Direction, Gesture, Key};
use kaia_lua::LuaUi;

/// A widget that logs the events it gets to `LOG`. `consume` makes its
/// handler return true.
const FAKE_WIDGET: &str = r#"
LOG = {}
function fake(name, consume)
    return {
        name = name,
        paintTo = function() end,
        handleEvent = function(self, ev)
            LOG[#LOG + 1] = name .. ":" .. ev.handler
            return consume == true
        end,
    }
end
"#;

fn run(ui: &mut LuaUi, name: &str, code: &str) {
    ui.exec(name, code).unwrap_or_else(|e| panic!("{name}: {e}"));
}

fn eval_string(ui: &LuaUi, expr: &str) -> String {
    match ui
        .host()
        .eval("eval.lua", &format!("return {expr}"))
        .unwrap_or_else(|e| panic!("{expr}: {e}"))
    {
        Value::Str(s) => s,
        other => panic!("{expr} is not a string: {other:?}"),
    }
}

fn eval_number(ui: &LuaUi, expr: &str) -> f64 {
    ui.host()
        .eval("eval.lua", &format!("return {expr}"))
        .unwrap_or_else(|e| panic!("{expr}: {e}"))
        .as_f64()
        .unwrap_or_else(|| panic!("{expr} is not a number"))
}

fn eval_bool(ui: &LuaUi, expr: &str) -> bool {
    ui.host()
        .eval("eval.lua", &format!("return {expr}"))
        .unwrap_or_else(|e| panic!("{expr}: {e}"))
        .truthy()
}

#[test]
fn send_reaches_only_the_top_window_and_broadcast_reaches_all_top_down() {
    let _guard = serial();
    let data = TempDir::new("order");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(&mut ui, "fake.lua", FAKE_WIDGET);
    run(
        &mut ui,
        "order.lua",
        r#"
        local UIManager = require("ui/uimanager")
        A = fake("A")
        B = fake("B")
        UIManager:show(A)
        UIManager:show(B)
        LOG = {}
        "#,
    );
    assert_eq!(ui.window_count(), 2);
    run(&mut ui, "reset.lua", "LOG = {}");
    run(
        &mut ui,
        "send.lua",
        r#"
        local UIManager = require("ui/uimanager")
        local Event = require("ui/event")
        UIManager:sendEvent(Event:new("Ping"))
        "#,
    );
    assert_eq!(eval_string(&ui, "table.concat(LOG, ',')"), "B:onPing");

    run(&mut ui, "reset.lua", "LOG = {}");
    run(
        &mut ui,
        "broadcast.lua",
        r#"
        local UIManager = require("ui/uimanager")
        local Event = require("ui/event")
        UIManager:broadcastEvent(Event:new("Ping"))
        "#,
    );
    assert_eq!(eval_string(&ui, "table.concat(LOG, ',')"), "B:onPing,A:onPing");
    assert!(ui.take_errors().is_empty());
}

#[test]
fn close_removes_the_window_of_that_table_only() {
    let _guard = serial();
    let data = TempDir::new("identity");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(&mut ui, "fake.lua", FAKE_WIDGET);
    run(
        &mut ui,
        "identity.lua",
        r#"
        local UIManager = require("ui/uimanager")
        FIRST = fake("first")
        SECOND = fake("second")
        UIManager:show(FIRST)
        UIManager:show(SECOND)
        UIManager:close(FIRST)
        "#,
    );
    ui.frame();
    assert_eq!(ui.window_count(), 1);
    assert!(!eval_bool(&ui, "require('ui/uimanager'):isWidgetShown(FIRST)"));
    assert!(eval_bool(&ui, "require('ui/uimanager'):isWidgetShown(SECOND)"));
    assert!(eval_bool(
        &ui,
        "require('ui/uimanager'):getTopmostVisibleWidget() == SECOND"
    ));

    // Closing a widget that was never shown still sends its close events, as in KOReader.
    run(
        &mut ui,
        "close_unshown.lua",
        r#"
        local UIManager = require("ui/uimanager")
        LOG = {}
        local never = fake("never")
        UIManager:close(never)
        "#,
    );
    assert_eq!(
        eval_string(&ui, "table.concat(LOG, ',')"),
        "never:onFlushSettings,never:onCloseWidget"
    );
    assert!(ui.take_errors().is_empty());
}

#[test]
fn scheduled_actions_run_when_due_and_unschedule_cancels_them() {
    let _guard = serial();
    let data = TempDir::new("schedule");
    let (mut ui, clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "schedule.lua",
        r#"
        local UIManager = require("ui/uimanager")
        RAN = {}
        local function later() RAN[#RAN + 1] = "later" end
        local function cancelled() RAN[#RAN + 1] = "cancelled" end
        local function tick() RAN[#RAN + 1] = "tick" end
        UIManager:scheduleIn(5, later)
        UIManager:scheduleIn(5, cancelled)
        UIManager:unschedule(cancelled)
        UIManager:nextTick(tick)
        "#,
    );
    ui.run_due();
    assert_eq!(eval_string(&ui, "table.concat(RAN, ',')"), "tick");
    clock.advance(Duration::from_millis(4999));
    ui.run_due();
    assert_eq!(eval_string(&ui, "table.concat(RAN, ',')"), "tick");
    clock.advance(Duration::from_millis(1));
    ui.run_due();
    assert_eq!(eval_string(&ui, "table.concat(RAN, ',')"), "tick,later");
    assert!(ui.take_errors().is_empty());
}

#[test]
fn a_failing_handler_is_recorded_and_the_ui_keeps_running() {
    let _guard = serial();
    let data = TempDir::new("errors");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "bad.lua",
        r#"
        local UIManager = require("ui/uimanager")
        BAD = { paintTo = function() end, handleEvent = function() error("boom from a handler") end }
        UIManager:show(BAD)
        "#,
    );
    // The Show event reaches the handler too, and it fails there. The gesture
    // check starts after it.
    assert_eq!(ui.take_errors().len(), 1);
    let g = Gesture::Tap {
        pos: Point::new(10, 10),
        time_us: 0,
    };
    assert!(!ui.gesture(&g), "a failed handler does not use the event");
    let errors = ui.take_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("boom from a handler"), "{errors:?}");

    ui.frame();
    assert_eq!(ui.window_count(), 1);
}

#[test]
fn gestures_and_keys_carry_koreader_fields() {
    let _guard = serial();
    let data = TempDir::new("fields");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "catch.lua",
        r#"
        local UIManager = require("ui/uimanager")
        SEEN = nil
        CATCH = {
            paintTo = function() end,
            handleEvent = function(self, ev)
                SEEN = ev
                return true
            end,
        }
        UIManager:show(CATCH)
        "#,
    );
    let swipe = Gesture::Swipe {
        pos: Point::new(900, 700),
        end_pos: Point::new(300, 720),
        direction: Some(Direction::West),
        distance: 600.0,
        time_us: 250_000,
    };
    assert!(ui.gesture(&swipe));
    assert_eq!(eval_string(&ui, "SEEN.handler"), "onGesture");
    assert_eq!(eval_string(&ui, "SEEN.args[1].ges"), "swipe");
    assert_eq!(eval_string(&ui, "SEEN.args[1].direction"), "west");
    assert_eq!(eval_number(&ui, "SEEN.args[1].pos.x"), 900.0);
    assert_eq!(eval_number(&ui, "SEEN.args[1].end_pos.x"), 300.0);
    assert_eq!(eval_number(&ui, "SEEN.args[1].distance"), 600.0);
    assert_eq!(
        eval_number(&ui, "SEEN.args[1].time"),
        eval_number(&ui, "require('ui/time').us(250000)"),
        "time is a KOReader time value of the gesture"
    );

    assert!(ui.key(Key::LPgFwd, true));
    assert_eq!(eval_string(&ui, "SEEN.handler"), "onKeyPress");
    assert_eq!(eval_string(&ui, "SEEN.args[1].key"), "LPgFwd");
    assert!(eval_bool(&ui, "SEEN.args[1]:match({'LPgFwd'})"));
    assert!(ui.take_errors().is_empty());
}

#[test]
fn set_dirty_queues_the_named_refresh_with_its_region() {
    let _guard = serial();
    let data = TempDir::new("refresh");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "dirty.lua",
        r#"
        local UIManager = require("ui/uimanager")
        local Geom = require("ui/geometry")
        DIRTY = { paintTo = function() end, handleEvent = function() return false end }
        UIManager:show(DIRTY)
        "#,
    );
    ui.frame();
    run(
        &mut ui,
        "fast.lua",
        r#"
        local UIManager = require("ui/uimanager")
        local Geom = require("ui/geometry")
        UIManager:setDirty(nil, "fast", Geom:new{ x = 10, y = 20, w = 300, h = 40 })
        "#,
    );
    let refreshes = ui.frame();
    assert_eq!(
        refreshes,
        vec![Refresh::region(RefreshMode::Fast, Rect::new(10, 20, 300, 40))]
    );

    // A refresh function runs after the paint, so its region is the one it returns then.
    run(
        &mut ui,
        "deferred.lua",
        r#"
        local UIManager = require("ui/uimanager")
        local Geom = require("ui/geometry")
        UIManager:setDirty(DIRTY, function()
            return "flashui", Geom:new{ x = 1, y = 2, w = 3, h = 4 }
        end)
        "#,
    );
    let refreshes = ui.frame();
    assert_eq!(
        refreshes,
        vec![Refresh::region(RefreshMode::FlashUi, Rect::new(1, 2, 3, 4))]
    );
    assert!(ui.take_errors().is_empty());
}

#[test]
fn widget_repaint_draws_into_the_frame_now() {
    let _guard = serial();
    let data = TempDir::new("frame");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "repaint.lua",
        r#"
        local UIManager = require("ui/uimanager")
        local Blitbuffer = require("ffi/blitbuffer")
        PAINTER = {
            paintTo = function(self, bb, x, y)
                bb:paintRect(x, y, 10, 10, Blitbuffer.COLOR_BLACK)
            end,
        }
        UIManager:widgetRepaint(PAINTER, 100, 200)
        "#,
    );
    assert_eq!(ui.screen().get(104, 204), Some(Color::BLACK));
    assert_eq!(ui.screen().get(90, 190), Some(Color::WHITE));
}

#[test]
fn scale_by_size_matches_koreader_framebuffer() {
    let _guard = serial();
    let data = TempDir::new("scale");
    // Each case: screen size, DPI override (0 for none), and the values to check.
    let cases: [(Size, u32); 3] = [
        (Size::new(1264, 1680), 300),
        (Size::new(1264, 1680), 0),
        (Size::new(600, 800), 0),
    ];
    for (size, dpi) in cases {
        let clock = kaia_events::ManualClock::new();
        let config = kaia_lua::UiConfig {
            size,
            dpi: (dpi > 0).then_some(dpi),
            color: false,
        };
        let ui = kaia_lua::LuaUi::new(common::host(&data), config, common::ui_assets(), Box::new(clock))
            .expect("LuaUi::new");
        let expr = format!(
            r#"(function()
                local Blitbuffer = require("ffi/blitbuffer")
                local fb = require("ffi/framebuffer"):new{{ bb = Blitbuffer.new({w}, {h}, Blitbuffer.TYPE_BB8) }}
                if {dpi} > 0 then fb:setDPI({dpi}) end
                local Screen = require("device").screen
                local out = {{}}
                for _, px in ipairs({{ 1, 7, 10, 22, 50, 106, 300 }}) do
                    local expected = fb:scaleBySize(px)
                    local ours = Screen:scaleBySize(px)
                    out[#out + 1] = expected == ours and "ok" or ("px " .. px .. ": " .. expected .. " vs " .. ours)
                end
                return table.concat(out, ";")
            end)()"#,
            w = size.w,
            h = size.h,
            dpi = dpi,
        );
        assert_eq!(
            eval_string(&ui, &expr),
            "ok;ok;ok;ok;ok;ok;ok",
            "size {size:?}, dpi {dpi}"
        );
    }
}

#[test]
fn a_toast_stays_above_a_modal_and_does_not_stop_the_event() {
    let _guard = serial();
    let data = TempDir::new("toast");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(&mut ui, "fake.lua", FAKE_WIDGET);
    // A toast shown first stays above a modal shown later, as in UIManager:show.
    run(
        &mut ui,
        "toast.lua",
        r#"
        local UIManager = require("ui/uimanager")
        MODAL = fake("modal", false)
        MODAL.modal = true
        TOAST = fake("toast", true)
        TOAST.toast = true
        UIManager:show(TOAST)
        UIManager:show(MODAL)
        "#,
    );
    assert_eq!(ui.window_count(), 2);
    run(&mut ui, "reset.lua", "LOG = {}");
    run(
        &mut ui,
        "ping.lua",
        r#"
        local UIManager = require("ui/uimanager")
        local Event = require("ui/event")
        UIManager:sendEvent(Event:new("Ping"))
        "#,
    );
    // The toast gets the event first, its answer does not stop it, and the modal gets it too.
    assert_eq!(
        eval_string(&ui, "table.concat(LOG, ',')"),
        "toast:onPing,modal:onPing"
    );
    assert!(ui.take_errors().is_empty());
}

#[test]
fn tick_after_next_runs_on_the_second_tick() {
    let _guard = serial();
    let data = TempDir::new("tick");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "tick.lua",
        r#"
        local UIManager = require("ui/uimanager")
        TICKS = {}
        UIManager:tickAfterNext(function() TICKS[#TICKS + 1] = "after" end)
        "#,
    );
    assert_eq!(eval_string(&ui, "table.concat(TICKS, ',')"), "");
    ui.run_due();
    assert_eq!(eval_string(&ui, "table.concat(TICKS, ',')"), "");
    ui.run_due();
    assert_eq!(eval_string(&ui, "table.concat(TICKS, ',')"), "after");
}

#[test]
fn colour_panel_paints_rgb_frames_through_32_bit_buffers() {
    let _guard = serial();
    let data = TempDir::new("colour");
    let (mut ui, _clock) = lua_ui(&data, true);
    run(
        &mut ui,
        "colour.lua",
        r#"
        local InfoMessage = require("ui/widget/infomessage")
        local UIManager = require("ui/uimanager")
        UIManager:show(InfoMessage:new{ text = "Colour panel", timeout = 30 })
        "#,
    );
    let refreshes = ui.frame();
    assert!(!refreshes.is_empty());
    assert_eq!(ui.screen().format(), kaia_gfx::PixelFormat::Rgb24);
    assert!(ui.take_errors().is_empty());
    assert!(
        eval_bool(&ui, "require('device').screen.bb:getType() == 5"),
        "RGB32 screen buffer"
    );
}

/// The `ges` fields that KOReader's gesture handlers read, one gesture at a time.
#[test]
fn gesture_tables_carry_the_fields_koreader_code_reads() {
    use kaia_input::{Axis, Turn};

    let _guard = serial();
    let data = TempDir::new("ges-fields");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "catch.lua",
        r#"
        local UIManager = require("ui/uimanager")
        SEEN = nil
        CATCH = {
            paintTo = function() end,
            handleEvent = function(self, ev)
                SEEN = ev.args[1]
                return true
            end,
        }
        UIManager:show(CATCH)
        "#,
    );
    let at = |x, y| Point::new(x, y);

    // Pinch, spread, inward and outward pans put the axis in `direction`.
    let pinch = Gesture::Pinch {
        pos: at(500, 800),
        start_pos: at(400, 800),
        axis: Some(Axis::Horizontal),
        distance: 120.0,
        span: 300.0,
        start_span: 180.0,
        time_us: 1,
    };
    assert!(ui.gesture(&pinch));
    assert_eq!(eval_string(&ui, "SEEN.ges"), "pinch");
    assert_eq!(eval_string(&ui, "SEEN.direction"), "horizontal");
    assert!(
        eval_bool(&ui, "SEEN.axis == nil"),
        "axis is not a field of KOReader's ges"
    );
    assert_eq!(eval_number(&ui, "SEEN.span"), 300.0);
    assert_eq!(eval_number(&ui, "SEEN.start_span"), 180.0);

    let spread = Gesture::Spread {
        pos: at(500, 800),
        end_pos: at(600, 800),
        axis: Some(Axis::Vertical),
        distance: 90.0,
        span: 260.0,
        start_span: 170.0,
        time_us: 2,
    };
    assert!(ui.gesture(&spread));
    assert_eq!(eval_string(&ui, "SEEN.ges"), "spread");
    assert_eq!(eval_string(&ui, "SEEN.direction"), "vertical");
    assert_eq!(eval_number(&ui, "SEEN.end_pos.x"), 600.0);

    let outward = Gesture::OutwardPan {
        pos: at(500, 800),
        end_pos: at(500, 700),
        axis: Some(Axis::Diagonal),
        distance: 40.0,
        span: 200.0,
        start_span: 150.0,
        time_us: 3,
    };
    assert!(ui.gesture(&outward));
    assert_eq!(eval_string(&ui, "SEEN.ges"), "outward_pan");
    assert_eq!(eval_string(&ui, "SEEN.direction"), "diagonal");

    // Rotate puts "cw" or "ccw" in `direction`, and KOReader has no `turn` field.
    let rotate = Gesture::Rotate {
        pos: at(300, 300),
        angle: -35.5,
        turn: Turn::CounterClockwise,
        time_us: 4,
    };
    assert!(ui.gesture(&rotate));
    assert_eq!(eval_string(&ui, "SEEN.ges"), "rotate");
    assert_eq!(eval_string(&ui, "SEEN.direction"), "ccw");
    assert_eq!(eval_number(&ui, "SEEN.angle"), -35.5);
    assert!(eval_bool(&ui, "SEEN.turn == nil"));

    // Pan gestures carry `relative`: current position minus the start.
    let pan = Gesture::Pan {
        pos: at(200, 900),
        start_pos: at(260, 850),
        direction: Some(Direction::SouthWest),
        distance: 78.1,
        time_us: 5,
    };
    assert!(ui.gesture(&pan));
    assert_eq!(eval_string(&ui, "SEEN.ges"), "pan");
    assert_eq!(eval_number(&ui, "SEEN.relative.x"), -60.0);
    assert_eq!(eval_number(&ui, "SEEN.relative.y"), 50.0);
    assert_eq!(eval_number(&ui, "SEEN.start_pos.x"), 260.0);

    let hold_pan = Gesture::HoldPan {
        pos: at(100, 100),
        start_pos: Some(at(90, 120)),
        direction: Some(Direction::North),
        distance: 20.0,
        time_us: 6,
    };
    assert!(ui.gesture(&hold_pan));
    assert_eq!(eval_string(&ui, "SEEN.ges"), "hold_pan");
    assert_eq!(eval_number(&ui, "SEEN.relative.x"), 10.0);
    assert_eq!(eval_number(&ui, "SEEN.relative.y"), -20.0);

    // Without a start position the field is absent, as KOReader's table has none.
    let hold_pan_unknown = Gesture::HoldPan {
        pos: at(100, 100),
        start_pos: None,
        direction: None,
        distance: 0.0,
        time_us: 7,
    };
    assert!(ui.gesture(&hold_pan_unknown));
    assert!(eval_bool(&ui, "SEEN.relative == nil"));

    let two_finger_pan = Gesture::TwoFingerPan {
        pos: at(700, 500),
        start_pos: at(650, 480),
        direction: Some(Direction::East),
        distance: 55.0,
        time_us: 8,
    };
    assert!(ui.gesture(&two_finger_pan));
    assert_eq!(eval_string(&ui, "SEEN.ges"), "two_finger_pan");
    assert_eq!(eval_number(&ui, "SEEN.relative.x"), 50.0);
    assert_eq!(eval_number(&ui, "SEEN.relative.y"), 20.0);

    let two_finger_hold_pan = Gesture::TwoFingerHoldPan {
        pos: at(700, 500),
        start_pos: at(700, 510),
        direction: Some(Direction::North),
        distance: 10.0,
        time_us: 9,
    };
    assert!(ui.gesture(&two_finger_hold_pan));
    assert_eq!(eval_string(&ui, "SEEN.ges"), "two_finger_hold_pan");
    assert_eq!(eval_number(&ui, "SEEN.relative.y"), -10.0);

    // Multiswipe: a space-separated string, which KOReader code calls :gsub and :gmatch on.
    let multiswipe = Gesture::Multiswipe {
        pos: at(100, 500),
        end_pos: at(300, 300),
        direction: Some(Direction::NorthEast),
        distance: 280.0,
        directions: vec![Direction::East, Direction::North, Direction::NorthEast],
        time_us: 10,
    };
    assert!(ui.gesture(&multiswipe));
    assert_eq!(eval_string(&ui, "SEEN.ges"), "multiswipe");
    assert_eq!(
        eval_string(&ui, "SEEN.multiswipe_directions"),
        "east north northeast"
    );
    assert_eq!(
        eval_string(&ui, "(SEEN.multiswipe_directions:gsub(' ', ','))"),
        "east,north,northeast"
    );
    assert!(ui.take_errors().is_empty());
}
