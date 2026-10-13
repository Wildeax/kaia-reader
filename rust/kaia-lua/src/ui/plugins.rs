//! Reader and library callbacks, plugin state, and menu registration.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use kaia_events::{Action, ActionCategory, ActionRegistry, MenuItem, MenuRegistry, Section, Value};
use mlua::{Function, Lua, Table, Value as LuaValue};

use crate::convert;
use crate::{Error, LibraryHost, ReaderHost};

use super::LuaUi;

/// One plugin's state in the current Lua session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStatus {
    pub name: String,
    pub loaded: bool,
    pub error: Option<String>,
}

/// Which bundled plugins to start. An empty list starts all discovered plugins.
#[derive(Debug, Clone, Default)]
pub struct PluginOptions {
    pub names: Vec<String>,
    pub safe_mode: bool,
    pub force_hello: bool,
    pub extra_paths: Vec<PathBuf>,
}

pub(super) fn install_reader(lua: &Lua, host: Rc<RefCell<dyn ReaderHost>>) -> mlua::Result<()> {
    let api = lua.create_table()?;
    let h = Rc::clone(&host);
    api.set("file", lua.create_function(move |_, ()| Ok(h.borrow().file()))?)?;
    let h = Rc::clone(&host);
    api.set(
        "page_count",
        lua.create_function(move |_, ()| Ok(h.borrow().page_count()))?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "current_page",
        lua.create_function(move |_, ()| Ok(h.borrow().current_page()))?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "go_to_page",
        lua.create_function(move |_, page: u32| {
            h.borrow_mut().go_to_page(page).map_err(mlua::Error::RuntimeError)
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "page_text",
        lua.create_function(move |_, page: u32| {
            h.borrow_mut().page_text(page).map_err(mlua::Error::RuntimeError)
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "properties",
        lua.create_function(move |lua, ()| {
            convert::to_lua(lua, &h.borrow().properties())
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "annotations",
        lua.create_function(move |lua, ()| {
            convert::to_lua(lua, &Value::array(h.borrow_mut().annotations()))
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "toc",
        lua.create_function(move |lua, ()| {
            convert::to_lua(lua, &Value::array(h.borrow().toc()))
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "get_setting",
        lua.create_function(move |lua, key: String| {
            convert::to_lua(lua, &h.borrow().get_setting(&key))
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "set_setting",
        lua.create_function(move |_, (key, value): (String, LuaValue)| {
            let value = convert::from_lua(&value).map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            h.borrow_mut()
                .set_setting(&key, value)
                .map_err(mlua::Error::RuntimeError)
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "text_from_positions",
        lua.create_function(move |_, (a, b): (LuaValue, LuaValue)| {
            let a = convert::from_lua(&a).map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            let b = convert::from_lua(&b).map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            h.borrow_mut()
                .text_from_positions(&a, &b)
                .map_err(mlua::Error::RuntimeError)
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "boxes_from_positions",
        lua.create_function(move |lua, (a, b): (LuaValue, LuaValue)| {
            let a = convert::from_lua(&a).map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            let b = convert::from_lua(&b).map_err(|e| mlua::Error::RuntimeError(e.to_string()))?;
            let boxes = h
                .borrow_mut()
                .boxes_from_positions(&a, &b)
                .map_err(mlua::Error::RuntimeError)?;
            convert::to_lua(lua, &Value::array(boxes)).map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "xpointer_for_page",
        lua.create_function(move |_, page: u32| {
            h.borrow()
                .xpointer_for_page(page)
                .map_err(mlua::Error::RuntimeError)
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "page_for_xpointer",
        lua.create_function(move |_, xp: String| {
            h.borrow()
                .page_for_xpointer(&xp)
                .map_err(mlua::Error::RuntimeError)
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "page_margins",
        lua.create_function(move |lua, page: u32| {
            let value = h.borrow().page_margins(page).map_err(mlua::Error::RuntimeError)?;
            convert::to_lua(lua, &value).map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })?,
    )?;
    let h = Rc::clone(&host);
    api.set(
        "native_page_dimensions",
        lua.create_function(move |lua, page: u32| {
            let value = h
                .borrow()
                .native_page_dimensions(page)
                .map_err(mlua::Error::RuntimeError)?;
            convert::to_lua(lua, &value).map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })?,
    )?;
    lua.globals().set("KAIA_READER", api)
}

pub(super) fn install_library(lua: &Lua, host: Rc<RefCell<dyn LibraryHost>>) -> mlua::Result<()> {
    let api = lua.create_table()?;
    let h = Rc::clone(&host);
    api.set(
        "current_folder",
        lua.create_function(move |_, ()| Ok(h.borrow().current_folder()))?,
    )?;
    let h = Rc::clone(&host);
    api.set("files", lua.create_function(move |_, ()| Ok(h.borrow().files()))?)?;
    api.set(
        "open_book",
        lua.create_function(move |_, path: String| {
            host.borrow_mut()
                .open_book(&path)
                .map_err(mlua::Error::RuntimeError)
        })?,
    )?;
    lua.globals().set("KAIA_LIBRARY", api)
}

pub(super) fn collect(ui: &mut LuaUi) -> Result<(), Error> {
    let globals = ui.lua.globals();
    let runtime: Table = globals.get("KAIA_PLUGIN").map_err(crate::error::lua_error)?;
    let menus: Table = runtime.get("menus").map_err(crate::error::lua_error)?;
    let mut registry = MenuRegistry::new();
    for pair in menus.pairs::<String, Table>() {
        let (id, item) = pair.map_err(crate::error::lua_error)?;
        register_item(&mut registry, &id, &item).map_err(crate::error::lua_error)?;
    }
    ui.menu_registry = registry;
    let actions: Table = runtime.get("actions").map_err(crate::error::lua_error)?;
    let mut action_registry = ActionRegistry::new();
    for pair in actions.pairs::<String, Table>() {
        let (id, item) = pair.map_err(crate::error::lua_error)?;
        let category: String = item.get("category").map_err(crate::error::lua_error)?;
        let category = match category.as_str() {
            "arg" => ActionCategory::Arg,
            "string" => ActionCategory::String,
            "absolutenumber" => ActionCategory::AbsoluteNumber,
            "incrementalnumber" => ActionCategory::IncrementalNumber,
            "configurable" => ActionCategory::Configurable,
            "key" => ActionCategory::Key,
            _ => ActionCategory::None,
        };
        let title: String = item.get("title").unwrap_or_else(|_| id.clone());
        let event: String = item.get("event").unwrap_or_default();
        let mut action = Action::new(id, title, category, event);
        for (key, section) in [
            ("general", Section::General),
            ("device", Section::Device),
            ("screen", Section::Screen),
            ("filemanager", Section::FileManager),
            ("reader", Section::Reader),
            ("rolling", Section::Rolling),
            ("paging", Section::Paging),
        ] {
            if item.get::<bool>(key).unwrap_or(false) {
                action = action.in_section(section);
            }
        }
        if let Ok(value) = item.get::<LuaValue>("arg") {
            if !matches!(value, LuaValue::Nil) {
                action.arg = Some(convert::from_lua(&value)?);
            }
        }
        if let Ok(args) = item.get::<Table>("args") {
            for value in args.sequence_values::<LuaValue>() {
                action
                    .values
                    .push(convert::from_lua(&value.map_err(crate::error::lua_error)?)?);
            }
        }
        if let Ok(toggle) = item.get::<Table>("toggle") {
            for title in toggle.sequence_values::<String>() {
                action.toggle.push(title.map_err(crate::error::lua_error)?);
            }
        }
        action.min = item.get("min").ok();
        action.max = item.get("max").ok();
        action.default = item.get("default").ok();
        action.separator = item.get("separator").unwrap_or(false);
        action_registry.register(action);
    }
    ui.action_registry = action_registry;
    Ok(())
}

fn register_item(registry: &mut MenuRegistry, id: &str, item: &Table) -> mlua::Result<()> {
    let text: String = match item.get("text")? {
        LuaValue::String(s) => s.to_str()?.to_string(),
        _ => id.to_owned(),
    };
    let mut entry = MenuItem::callback(id, text, id);
    if let Some(hint) = item.get::<Option<String>>("sorting_hint")? {
        entry = entry.with_sorting_hint(hint);
    }
    registry.register(entry);
    Ok(())
}

pub(super) fn statuses(lua: &Lua) -> Vec<PluginStatus> {
    let Ok(runtime) = lua.globals().get::<Table>("KAIA_PLUGIN") else {
        return Vec::new();
    };
    let Ok(status) = runtime.get::<Table>("status") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, state) in status.pairs::<String, Table>().flatten() {
        out.push(PluginStatus {
            name,
            loaded: state.get("loaded").unwrap_or(false),
            error: state.get("error").ok(),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub(super) fn call(lua: &Lua, method: &str, args: impl mlua::IntoLuaMulti) -> mlua::Result<LuaValue> {
    let runtime: Table = lua.globals().get("KAIA_PLUGIN")?;
    let function: Function = runtime.get(method)?;
    function.call(args)
}
