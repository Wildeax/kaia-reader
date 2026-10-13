//! KOReader widgets in Kaia windows (stage L2 and L3). Each test runs Lua
//! code on a `LuaUi`, paints the frame, sends input and checks the result.
//! The rendered frames are golden PNG files in `tests/golden`.

mod common;

use common::{golden_path, lua_ui, serial, TempDir};
use kaia_events::{Key as TableKey, Value};
use kaia_gfx::golden::assert_matches_golden;
use kaia_gfx::Point;
use kaia_input::{Gesture, Key};
use kaia_lua::LuaUi;

/// Lua helper that finds the button with the given text in a widget tree.
/// Widgets keep their children in the array part.
const FIND_BUTTON: &str = r#"
function FIND_BUTTON(w, text)
    if type(w) ~= "table" then return nil end
    if w.text == text and w.callback and w.dimen then return w end
    for _, child in ipairs(w) do
        local found = FIND_BUTTON(child, text)
        if found then return found end
    end
    return nil
end
"#;

/// Runs a Lua chunk. A Lua error fails the test.
fn run(ui: &mut LuaUi, name: &str, code: &str) {
    ui.exec(name, code).unwrap_or_else(|e| panic!("{name}: {e}"));
}

/// Evaluates a Lua expression that returns a table `{x = .., y = ..}` and
/// returns the point.
fn point_of(ui: &LuaUi, expr: &str) -> Point {
    let value = ui
        .host()
        .eval("point.lua", &format!("return {expr}"))
        .unwrap_or_else(|e| panic!("point: {e}"));
    let table = value.as_table().expect("a table with x and y");
    let coord = |key: &str| {
        table
            .get(&TableKey::Str(key.to_owned()))
            .and_then(Value::as_f64)
            .unwrap_or_else(|| panic!("{key} is not a number")) as i32
    };
    Point::new(coord("x"), coord("y"))
}

/// The centre of a widget's `dimen`, from the last paint.
fn centre_of(ui: &LuaUi, widget_expr: &str) -> Point {
    point_of(
        ui,
        &format!(
            "{{ x = {w}.dimen.x + math.floor({w}.dimen.w / 2), y = {w}.dimen.y + math.floor({w}.dimen.h / 2) }}",
            w = widget_expr
        ),
    )
}

fn tap(ui: &mut LuaUi, at: Point) -> bool {
    ui.gesture(&Gesture::Tap { pos: at, time_us: 0 })
}

fn lua_number(ui: &LuaUi, expr: &str) -> f64 {
    ui.host()
        .eval("number.lua", &format!("return {expr}"))
        .unwrap_or_else(|e| panic!("{expr}: {e}"))
        .as_f64()
        .unwrap_or_else(|| panic!("{expr} is not a number"))
}

fn lua_bool(ui: &LuaUi, expr: &str) -> bool {
    ui.host()
        .eval("bool.lua", &format!("return {expr}"))
        .unwrap_or_else(|e| panic!("{expr}: {e}"))
        .truthy()
}

fn assert_no_lua_errors(ui: &LuaUi) {
    let errors = ui.take_errors();
    assert!(errors.is_empty(), "Lua errors: {errors:#?}");
}

#[test]
fn info_message_shows_and_closes_after_its_timeout() {
    let _guard = serial();
    let data = TempDir::new("info");
    let (mut ui, clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "info.lua",
        r#"
        local InfoMessage = require("ui/widget/infomessage")
        local UIManager = require("ui/uimanager")
        INFO = InfoMessage:new{ text = "Notes saved.", timeout = 2 }
        UIManager:show(INFO)
        "#,
    );
    let refreshes = ui.frame();
    assert!(!refreshes.is_empty(), "showing a widget refreshes the panel");
    assert_no_lua_errors(&ui);
    assert_eq!(ui.window_count(), 1);
    assert_matches_golden(ui.screen(), &golden_path("info_message.png"), 0);

    clock.advance(std::time::Duration::from_millis(1900));
    ui.run_due();
    assert_eq!(ui.window_count(), 1, "the message stays before its timeout");

    clock.advance(std::time::Duration::from_millis(200));
    ui.run_due();
    assert_eq!(ui.window_count(), 0, "the message closes at its timeout");
    assert!(!lua_bool(&ui, "require('ui/uimanager'):isWidgetShown(INFO)"));
    let refreshes = ui.frame();
    assert!(
        !refreshes.is_empty(),
        "closing refreshes the area the message covered"
    );
    assert_no_lua_errors(&ui);
}

#[test]
fn confirm_box_ok_tap_runs_the_callback_and_closes() {
    let _guard = serial();
    let data = TempDir::new("confirm");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "confirm.lua",
        &format!(
            "{FIND_BUTTON}{}",
            r#"
        local ConfirmBox = require("ui/widget/confirmbox")
        local UIManager = require("ui/uimanager")
        OK_CALLS = 0
        BOX = ConfirmBox:new{
            text = "Delete this note?",
            ok_text = "Delete",
            ok_callback = function() OK_CALLS = OK_CALLS + 1 end,
        }
        UIManager:show(BOX)
        "#
        ),
    );
    ui.frame();
    run(&mut ui, "find.lua", r#"OK_BUTTON = FIND_BUTTON(BOX, "Delete")"#);
    assert_no_lua_errors(&ui);
    assert_eq!(ui.window_count(), 1);
    assert_matches_golden(ui.screen(), &golden_path("confirm_box.png"), 0);

    // The OK button's dimen is set by the paint.
    let ok_at = centre_of(&ui, "OK_BUTTON");
    assert!(tap(&mut ui, ok_at), "the OK button uses the tap");
    assert_no_lua_errors(&ui);
    assert_eq!(lua_number(&ui, "OK_CALLS"), 1.0);
    assert_eq!(ui.window_count(), 0, "OK closes the box");
}

#[test]
fn button_dialog_shows_three_buttons() {
    let _guard = serial();
    let data = TempDir::new("buttons");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "buttons.lua",
        &format!(
            "{FIND_BUTTON}{}",
            r#"
        local ButtonDialog = require("ui/widget/buttondialog")
        local UIManager = require("ui/uimanager")
        PICKED = nil
        DIALOG = ButtonDialog:new{
            title = "Open the book with",
            buttons = {
                {
                    { text = "Reader", callback = function() PICKED = "reader" end },
                    { text = "Text view", callback = function()
                        PICKED = "text"
                        UIManager:close(DIALOG)
                    end },
                    { text = "Cancel", callback = function() PICKED = "cancel" end },
                },
            },
        }
        UIManager:show(DIALOG)
        "#
        ),
    );
    ui.frame();
    run(
        &mut ui,
        "find.lua",
        r#"TEXT_BUTTON = FIND_BUTTON(DIALOG, "Text view")"#,
    );
    assert_no_lua_errors(&ui);
    assert_eq!(ui.window_count(), 1);
    assert_eq!(lua_number(&ui, "#DIALOG.buttons[1]"), 3.0);
    assert_matches_golden(ui.screen(), &golden_path("button_dialog.png"), 0);

    let text_at = centre_of(&ui, "TEXT_BUTTON");
    assert!(tap(&mut ui, text_at));
    assert_no_lua_errors(&ui);
    assert_eq!(
        ui.host().eval("p.lua", "return PICKED").unwrap(),
        Value::Str("text".to_owned())
    );
    assert_eq!(ui.window_count(), 0, "the callback closes the dialog");
}

#[test]
fn menu_pages_thirty_items_with_its_own_page_buttons() {
    let _guard = serial();
    let data = TempDir::new("menu");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "menu.lua",
        r#"
        local Menu = require("ui/widget/menu")
        local UIManager = require("ui/uimanager")
        local items = {}
        for i = 1, 30 do
            items[i] = { text = "Chapter " .. i, mandatory = "p. " .. (i * 7) }
        end
        MENU = Menu:new{ title = "Contents", item_table = items, width = 1100, height = 1500 }
        UIManager:show(MENU)
        "#,
    );
    ui.frame();
    assert_no_lua_errors(&ui);
    assert_eq!(ui.window_count(), 1);
    assert!(
        lua_number(&ui, "MENU.page_num") > 1.0,
        "30 items take more than one page"
    );
    assert_eq!(lua_number(&ui, "MENU.page"), 1.0);
    assert_matches_golden(ui.screen(), &golden_path("menu_page_1.png"), 0);

    let next_at = centre_of(&ui, "MENU.page_info_right_chev");
    assert!(tap(&mut ui, next_at), "the next page button uses the tap");
    ui.frame();
    assert_no_lua_errors(&ui);
    assert_eq!(lua_number(&ui, "MENU.page"), 2.0);
    assert_matches_golden(ui.screen(), &golden_path("menu_page_2.png"), 0);
}

#[test]
fn input_dialog_receives_text_through_key_events() {
    let _guard = serial();
    let data = TempDir::new("input");
    let (mut ui, _clock) = lua_ui(&data, false);
    run(
        &mut ui,
        "input.lua",
        r#"
        local InputDialog = require("ui/widget/inputdialog")
        local UIManager = require("ui/uimanager")
        DIALOG = InputDialog:new{
            title = "Note title",
            input = "",
            buttons = {
                {
                    { text = "Cancel", id = "close", callback = function() end },
                    { text = "Save", is_enter_default = true, callback = function() end },
                },
            },
        }
        UIManager:show(DIALOG)
        "#,
    );
    ui.frame();
    assert_no_lua_errors(&ui);
    assert_eq!(ui.window_count(), 1);
    for key in [Key::Char('h'), Key::Char('i')] {
        ui.key(key, true);
        ui.key(key, false);
    }
    ui.frame();
    assert_no_lua_errors(&ui);
    let text = ui
        .host()
        .eval("text.lua", "return DIALOG:getInputText()")
        .expect("getInputText");
    assert_eq!(text, Value::Str("hi".to_owned()));
    assert_matches_golden(ui.screen(), &golden_path("input_dialog.png"), 0);
}

#[test]
fn info_message_on_a_colour_panel() {
    let _guard = serial();
    let data = TempDir::new("colour-info");
    let (mut ui, _clock) = lua_ui(&data, true);
    run(
        &mut ui,
        "colour_info.lua",
        r#"
        local InfoMessage = require("ui/widget/infomessage")
        local UIManager = require("ui/uimanager")
        UIManager:show(InfoMessage:new{ text = "Saved to the library.", timeout = 30 })
        "#,
    );
    ui.frame();
    assert_no_lua_errors(&ui);
    assert_matches_golden(ui.screen(), &golden_path("info_message_colour.png"), 0);
}
