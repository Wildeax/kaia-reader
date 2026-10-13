//! The Kaia app core that later maps onto KOReader's Lua modules.
//!
//! See `docs/kaia/REWRITE_PLAN.md`, "Compatibility seams". `Value` and
//! `Event` are shared types; change them only with the orchestrator's
//! agreement.
//!
//! Modules and the KOReader code they mirror:
//! - [`handler`]: `EventListener` and `WidgetContainer` (`onEventName` methods).
//! - [`window`]: `UIManager`'s window stack, `show`, `close`, `sendEvent`, `broadcastEvent`.
//! - [`dirty`]: `UIManager:setDirty`, `_refresh`, `_repaint`, refresh modes.
//! - [`scheduler`]: the `UIManager` task queue: `scheduleIn`, `nextTick`, `tickAfterNext`, `unschedule`.
//! - [`ctx`]: the context that handlers and tasks receive.
//! - [`settings`] and [`json`]: `LuaSettings` and the JSON file format.
//! - [`menu`]: `MenuSorter` and `reader_menu_order.lua`.
//! - [`actions`]: `Dispatcher` settings and execution.
//! - [`gestures`]: gesture-to-action bindings.

pub mod actions;
pub mod ctx;
pub mod dirty;
pub mod geom;
pub mod gestures;
pub mod handler;
pub mod json;
pub mod menu;
pub mod scheduler;
pub mod settings;
pub mod value;
pub mod window;

pub use actions::{Action, ActionCategory, ActionRegistry, Section};
pub use ctx::Ctx;
pub use dirty::{DirtyQueue, Refresh, RefreshMode};
pub use geom::Rect;
pub use gestures::{GestureBinding, GestureBindings};
pub use handler::{Container, EventHandler, Handlers};
pub use menu::{MenuBuild, MenuItem, MenuKind, MenuOrder, MenuRegistry};
pub use scheduler::{Clock, ManualClock, Scheduler, SystemClock, Task, TaskId};
pub use settings::{OpenStatus, Settings};
pub use value::{Key, Table, Value};
pub use window::{ShowOpts, Window, WindowId, WindowStack};

/// A named event with arguments, like KOReader's `Event:new(name, ...)`.
/// Handlers are looked up by name, so KOReader's `onPageUpdate` handler
/// matches the event named `PageUpdate`.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub name: String,
    pub args: Vec<Value>,
}

impl Event {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            args: Vec::new(),
        }
    }

    pub fn with_args(name: impl Into<String>, args: Vec<Value>) -> Self {
        Self {
            name: name.into(),
            args,
        }
    }

    /// The KOReader handler method name: "on" followed by the event name.
    pub fn handler_name(&self) -> String {
        format!("on{}", self.name)
    }
}
