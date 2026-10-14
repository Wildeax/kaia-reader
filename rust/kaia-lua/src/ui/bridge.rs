//! The queue between the Lua UI shims and `LuaUi`, and the `KAIA_UI` table.
//!
//! A Lua call to `UIManager:show`, `close`, `setDirty`, `scheduleIn`,
//! `sendEvent` and the others records a [`Request`]. `LuaUi` applies the
//! requests after the Lua call returns. So the window stack, the scheduler and
//! the Rust state are never borrowed while Lua runs, and a handler can ask for
//! any change without a re-entrant borrow.
//!
//! `show` and `close` also change the [`Mirror`] at once. Queries such as
//! `isWidgetShown` therefore answer right after the call, as KOReader's do. The
//! window itself appears when `LuaUi` applies the request. The mirror and the
//! stack use the same insertion rule, so they agree once the requests are applied.
//!
//! Widgets are kept in the Lua registry through `RegistryKey`. A widget is
//! identified by the address of its table, which stays fixed while the key
//! holds the table. The address is compared, never read.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use kaia_events::Rect;
use kaia_events::{Refresh, RefreshMode, TaskId};
use mlua::{Lua, RegistryKey, Table, Value as LuaValue};

/// The window flags that `UIManager:show` reads from a widget, as the shim sends them.
/// `invisible` is not a flag here. Queries read the widget's field when they run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Flags {
    pub(crate) modal: bool,
    pub(crate) toast: bool,
    pub(crate) covers_fullscreen: bool,
    pub(crate) always_active: bool,
}

/// A change that a Lua call asked for. `LuaUi` applies it later, in order.
pub(crate) enum Request {
    Show {
        widget: Rc<RegistryKey>,
        ptr: usize,
        x: i32,
        y: i32,
        flags: Flags,
        refresh: Option<Refresh>,
    },
    Close {
        widget: Rc<RegistryKey>,
        ptr: usize,
        refresh: Option<Refresh>,
    },
    /// `all` marks every window. Otherwise `ptr` names the widget, and the
    /// window is marked only when it is on the stack and visible.
    SetDirty {
        ptr: Option<usize>,
        all: bool,
        invisible: bool,
        refresh: Option<Refresh>,
    },
    Schedule {
        seconds: f64,
        action: Rc<RegistryKey>,
        ptr: usize,
    },
    Unschedule {
        ptr: usize,
    },
    Send {
        event: Rc<RegistryKey>,
        broadcast: bool,
    },
}

/// A window on the stack, bottom first. Placement reads every window, Rust's
/// too, so `lua` is `None` for a window that Rust owns. Lua reads the windows
/// that have a `lua` entry, through `isWidgetShown`, `getTopmostVisibleWidget`
/// and `getNthTopWidget`.
pub(crate) struct Mirror {
    pub(crate) modal: bool,
    pub(crate) toast: bool,
    pub(crate) lua: Option<LuaWindow>,
}

/// The Lua side of a [`Mirror`] entry: the widget's address and its registry key.
pub(crate) struct LuaWindow {
    pub(crate) ptr: usize,
    pub(crate) widget: Rc<RegistryKey>,
}

/// A Lua task that the scheduler holds. `token` removes the record when the task runs.
pub(crate) struct TaskRecord {
    pub(crate) token: u64,
    pub(crate) ptr: usize,
    pub(crate) id: TaskId,
}

/// Shared state between the Lua callbacks and `LuaUi`.
pub(crate) struct Bridge {
    queue: RefCell<VecDeque<Request>>,
    errors: RefCell<Vec<String>>,
    /// The window stack, bottom first. `show` and `close` update it at call
    /// time. `LuaUi` replaces it with the real stack after each drain.
    pub(crate) mirror: RefCell<Vec<Mirror>>,
    pub(crate) tasks: RefCell<Vec<TaskRecord>>,
    next_token: Cell<u64>,
    repaint: Cell<bool>,
    /// Refresh functions from `setDirty(widget, function)`. They run after the next paint.
    after_paint: RefCell<Vec<Rc<RegistryKey>>>,
}

impl Bridge {
    pub(crate) fn new() -> Self {
        Self {
            queue: RefCell::new(VecDeque::new()),
            errors: RefCell::new(Vec::new()),
            mirror: RefCell::new(Vec::new()),
            tasks: RefCell::new(Vec::new()),
            next_token: Cell::new(0),
            repaint: Cell::new(false),
            after_paint: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn push(&self, request: Request) {
        self.queue.borrow_mut().push_back(request);
    }

    pub(crate) fn pop(&self) -> Option<Request> {
        self.queue.borrow_mut().pop_front()
    }

    pub(crate) fn record_error(&self, message: impl Into<String>) {
        self.errors.borrow_mut().push(message.into());
    }

    pub(crate) fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.errors.borrow_mut())
    }

    pub(crate) fn next_token(&self) -> u64 {
        let token = self.next_token.get();
        self.next_token.set(token + 1);
        token
    }

    /// Removes the record of a task that has run.
    pub(crate) fn forget_task(&self, token: u64) {
        self.tasks.borrow_mut().retain(|record| record.token != token);
    }

    pub(crate) fn request_repaint(&self) {
        self.repaint.set(true);
    }

    pub(crate) fn take_repaint(&self) -> bool {
        self.repaint.replace(false)
    }

    pub(crate) fn push_after_paint(&self, function: Rc<RegistryKey>) {
        self.after_paint.borrow_mut().push(function);
    }

    pub(crate) fn take_after_paint(&self) -> Vec<Rc<RegistryKey>> {
        std::mem::take(&mut *self.after_paint.borrow_mut())
    }

    /// `UIManager:show` on the mirror. The window goes where `UIManager:show`
    /// puts it.
    pub(crate) fn mirror_show(&self, window: LuaWindow, flags: Flags) {
        let mut mirror = self.mirror.borrow_mut();
        let at = insert_position(&mirror, flags);
        mirror.insert(
            at,
            Mirror {
                modal: flags.modal,
                toast: flags.toast,
                lua: Some(window),
            },
        );
    }

    /// `UIManager:close` on the mirror. Every window of the widget leaves.
    pub(crate) fn mirror_close(&self, ptr: usize) {
        self.mirror
            .borrow_mut()
            .retain(|m| !m.lua.as_ref().is_some_and(|w| w.ptr == ptr));
    }

    /// The Lua windows, top first. The keys are copies, so a caller can run
    /// Lua while it holds them.
    pub(crate) fn lua_windows_top_down(&self) -> Vec<Rc<RegistryKey>> {
        self.mirror
            .borrow()
            .iter()
            .rev()
            .filter_map(|m| m.lua.as_ref().map(|w| Rc::clone(&w.widget)))
            .collect()
    }
}

/// The index where `UIManager:show` inserts a window, counted from the bottom.
/// The scan runs from the top. For each window:
/// - if it is a toast, a new toast goes directly above it;
/// - if it is not a toast, a new window goes directly above it when the new
///   window is modal or the window is not modal.
///
/// With no match, the window goes to the bottom. `WindowStack::insert_position`
/// in kaia-events applies the same rule to the real stack.
fn insert_position(mirror: &[Mirror], flags: Flags) -> usize {
    for (index, top) in mirror.iter().enumerate().rev() {
        if top.toast {
            if flags.toast {
                return index + 1;
            }
        } else if flags.modal || !top.modal {
            return index + 1;
        }
    }
    0
}

/// Keeps a widget or a function in the registry and returns its address.
fn keep(lua: &Lua, value: LuaValue) -> mlua::Result<(Rc<RegistryKey>, usize)> {
    match value {
        LuaValue::Table(_) | LuaValue::Function(_) => {}
        other => {
            return Err(mlua::Error::RuntimeError(format!(
                "expected a widget or a function, got {}",
                other.type_name()
            )))
        }
    }
    let ptr = value.to_pointer() as usize;
    let key = lua.create_registry_value(value)?;
    Ok((Rc::new(key), ptr))
}

fn flags_from(table: Option<Table>) -> mlua::Result<Flags> {
    let Some(t) = table else {
        return Ok(Flags::default());
    };
    let flag = |name: &str| -> mlua::Result<bool> { Ok(t.get::<Option<bool>>(name)?.unwrap_or(false)) };
    Ok(Flags {
        modal: flag("modal")?,
        toast: flag("toast")?,
        covers_fullscreen: flag("covers_fullscreen")?,
        always_active: flag("always_active")?,
    })
}

/// Reads a refresh request from the shim: `{mode = "ui", region = {x, y, w, h}, dither = bool}`.
/// A `deferred` function is queued on the bridge and returns no refresh now.
fn refresh_from(lua: &Lua, bridge: &Bridge, spec: Option<Table>) -> mlua::Result<Option<Refresh>> {
    let Some(t) = spec else {
        return Ok(None);
    };
    if let Some(function) = t.get::<Option<mlua::Function>>("deferred")? {
        bridge.push_after_paint(Rc::new(lua.create_registry_value(function)?));
        return Ok(None);
    }
    let name: String = t.get("mode")?;
    let mode = RefreshMode::from_name(&name)
        .ok_or_else(|| mlua::Error::RuntimeError(format!("unknown refresh mode {name:?}")))?;
    let region = match t.get::<Option<Table>>("region")? {
        Some(r) => Some(rect_from(&r)?),
        None => None,
    };
    let dither = t.get::<Option<bool>>("dither")?.unwrap_or(false);
    Ok(Some(Refresh { mode, region, dither }))
}

pub(crate) fn rect_from(t: &Table) -> mlua::Result<Rect> {
    let x: f64 = t.get("x")?;
    let y: f64 = t.get("y")?;
    let w: f64 = t.get("w")?;
    let h: f64 = t.get("h")?;
    Ok(Rect::new(
        x.round() as i32,
        y.round() as i32,
        w.round() as i32,
        h.round() as i32,
    ))
}

/// The `invisible` field of a widget, read when a query runs. KOReader's
/// `getTopmostVisibleWidget` reads it the same way, so a field set after `show` counts.
fn is_invisible(widget: &Table) -> mlua::Result<bool> {
    let flag: LuaValue = widget.get("invisible")?;
    Ok(!matches!(flag, LuaValue::Nil | LuaValue::Boolean(false)))
}

/// Creates the global `KAIA_UI`, the Rust side of `ui/uimanager`.
pub(crate) fn install(lua: &Lua, bridge: &Rc<Bridge>) -> mlua::Result<()> {
    let ui = lua.create_table()?;

    let b = Rc::clone(bridge);
    ui.set(
        "show",
        lua.create_function(
            move |lua, (widget, x, y, flags, refresh): (LuaValue, i32, i32, Option<Table>, Option<Table>)| {
                let (widget, ptr) = keep(lua, widget)?;
                // Everything that can fail runs before the mirror changes, so the
                // mirror and the queue always change together.
                let flags = flags_from(flags)?;
                let refresh = refresh_from(lua, &b, refresh)?;
                b.mirror_show(
                    LuaWindow {
                        ptr,
                        widget: Rc::clone(&widget),
                    },
                    flags,
                );
                b.push(Request::Show {
                    widget,
                    ptr,
                    x,
                    y,
                    flags,
                    refresh,
                });
                Ok(())
            },
        )?,
    )?;

    let b = Rc::clone(bridge);
    ui.set(
        "close",
        lua.create_function(move |lua, (widget, refresh): (LuaValue, Option<Table>)| {
            let (widget, ptr) = keep(lua, widget)?;
            let refresh = refresh_from(lua, &b, refresh)?;
            b.mirror_close(ptr);
            b.push(Request::Close { widget, ptr, refresh });
            Ok(())
        })?,
    )?;

    let b = Rc::clone(bridge);
    ui.set(
        "set_dirty",
        lua.create_function(
            move |lua, (widget, all, invisible, refresh): (LuaValue, bool, bool, Option<Table>)| {
                let ptr = match widget {
                    LuaValue::Nil => None,
                    other => Some(keep(lua, other)?.1),
                };
                b.push(Request::SetDirty {
                    ptr,
                    all,
                    invisible,
                    refresh: refresh_from(lua, &b, refresh)?,
                });
                Ok(())
            },
        )?,
    )?;

    let b = Rc::clone(bridge);
    ui.set(
        "schedule",
        lua.create_function(move |lua, (seconds, action): (f64, LuaValue)| {
            let (action, ptr) = keep(lua, action)?;
            b.push(Request::Schedule { seconds, action, ptr });
            Ok(())
        })?,
    )?;

    let b = Rc::clone(bridge);
    ui.set(
        "unschedule",
        lua.create_function(move |_, action: LuaValue| {
            let ptr = action.to_pointer() as usize;
            b.push(Request::Unschedule { ptr });
            Ok(())
        })?,
    )?;

    let b = Rc::clone(bridge);
    ui.set(
        "send",
        lua.create_function(move |lua, (event, broadcast): (LuaValue, bool)| {
            let event = Rc::new(lua.create_registry_value(event)?);
            b.push(Request::Send { event, broadcast });
            Ok(())
        })?,
    )?;

    let b = Rc::clone(bridge);
    ui.set(
        "is_shown",
        lua.create_function(move |_, widget: LuaValue| {
            let ptr = widget.to_pointer() as usize;
            Ok(b.mirror
                .borrow()
                .iter()
                .any(|m| m.lua.as_ref().is_some_and(|w| w.ptr == ptr)))
        })?,
    )?;

    let b = Rc::clone(bridge);
    ui.set(
        "topmost_visible",
        lua.create_function(move |lua, ()| {
            // The keys are copies, so no borrow of the mirror is held while
            // `invisible` is read. That read can run Lua.
            for key in b.lua_windows_top_down() {
                let table: Table = lua.registry_value(&key)?;
                if !is_invisible(&table)? {
                    return Ok(LuaValue::Table(table));
                }
            }
            Ok(LuaValue::Nil)
        })?,
    )?;

    let b = Rc::clone(bridge);
    ui.set(
        "nth_top",
        lua.create_function(move |lua, n: i64| {
            let index = n.checked_sub(1).and_then(|i| usize::try_from(i).ok());
            let key = index.and_then(|i| b.lua_windows_top_down().into_iter().nth(i));
            match key {
                Some(key) => lua.registry_value::<LuaValue>(&key),
                None => Ok(LuaValue::Nil),
            }
        })?,
    )?;

    let b = Rc::clone(bridge);
    ui.set(
        "force_repaint",
        lua.create_function(move |_, ()| {
            b.request_repaint();
            Ok(())
        })?,
    )?;

    lua.globals().set("KAIA_UI", ui)
}
