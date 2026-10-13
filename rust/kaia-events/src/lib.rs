//! The Kaia app core that later maps onto KOReader's Lua modules.
//!
//! See `docs/kaia/REWRITE_PLAN.md`, "Compatibility seams". `Value` and
//! `Event` are shared types; change them only with the orchestrator's
//! agreement.

pub mod value;

pub use value::{Key, Table, Value};

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
