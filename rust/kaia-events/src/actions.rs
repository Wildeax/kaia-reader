//! Dispatcher actions, mirroring `frontend/dispatcher.lua`: `settingsList`,
//! `registerAction`, `removeAction`, and the event that `Dispatcher:execute`
//! sends for each category.

use std::collections::HashMap;

use crate::{Event, Value};

/// Where an action appears in menus. KOReader's section flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    General,
    Device,
    Screen,
    FileManager,
    Reader,
    Rolling,
    Paging,
}

impl Section {
    /// The flag name KOReader uses in `settingsList` (`general = true`, `filemanager = true`).
    pub fn name(self) -> &'static str {
        match self {
            Section::General => "general",
            Section::Device => "device",
            Section::Screen => "screen",
            Section::FileManager => "filemanager",
            Section::Reader => "reader",
            Section::Rolling => "rolling",
            Section::Paging => "paging",
        }
    }
}

/// How an action takes its value. KOReader's `category`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionCategory {
    /// A direct event call. The optional `arg` is sent with it.
    None,
    /// An event that takes a gesture or an argument.
    Arg,
    /// An event that takes one of the allowed `values`.
    String,
    /// An event that sets a number.
    AbsoluteNumber,
    /// An event that changes a number by the given step.
    IncrementalNumber,
    /// Updates a configurable option (KOptions). Not modelled here: `event_for` returns None.
    Configurable,
    /// Sends a key press. Not modelled here: `event_for` returns None.
    Key,
}

impl ActionCategory {
    pub fn name(self) -> &'static str {
        match self {
            ActionCategory::None => "none",
            ActionCategory::Arg => "arg",
            ActionCategory::String => "string",
            ActionCategory::AbsoluteNumber => "absolutenumber",
            ActionCategory::IncrementalNumber => "incrementalnumber",
            ActionCategory::Configurable => "configurable",
            ActionCategory::Key => "key",
        }
    }
}

/// One dispatchable action. Fields follow the KOReader `settingsList` entries.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    pub id: String,
    pub title: String,
    pub category: ActionCategory,
    /// The event name the action sends.
    pub event: String,
    /// The fixed argument (KOReader's `arg`).
    pub arg: Option<Value>,
    /// Allowed values for `String` actions (KOReader's `args`).
    pub values: Vec<Value>,
    /// Display names for `values` (KOReader's `toggle`).
    pub toggle: Vec<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub default: Option<f64>,
    pub sections: Vec<Section>,
    /// Draw a separator after this action in its menu.
    pub separator: bool,
    /// The `condition` of the entry, evaluated by the caller at start-up.
    pub enabled: bool,
}

impl Action {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        category: ActionCategory,
        event: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            category,
            event: event.into(),
            arg: None,
            values: Vec::new(),
            toggle: Vec::new(),
            min: None,
            max: None,
            default: None,
            sections: Vec::new(),
            separator: false,
            enabled: true,
        }
    }

    pub fn with_arg(mut self, arg: impl Into<Value>) -> Self {
        self.arg = Some(arg.into());
        self
    }

    pub fn with_values(mut self, values: Vec<Value>, toggle: Vec<String>) -> Self {
        self.values = values;
        self.toggle = toggle;
        self
    }

    pub fn with_range(mut self, min: f64, max: f64, default: Option<f64>) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self.default = default;
        self
    }

    pub fn in_section(mut self, section: Section) -> Self {
        if !self.sections.contains(&section) {
            self.sections.push(section);
        }
        self
    }

    pub fn with_separator(mut self) -> Self {
        self.separator = true;
        self
    }

    pub fn is_in(&self, section: Section) -> bool {
        self.sections.contains(&section)
    }
}

/// The actions, in registration order. Registration order is the order menus show them in.
#[derive(Debug, Clone, Default)]
pub struct ActionRegistry {
    order: Vec<String>,
    actions: HashMap<String, Action>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// `registerAction`: adds the action unless the id is taken. Returns true
    /// when it was added. KOReader keeps the first registration, and so does this.
    pub fn register(&mut self, action: Action) -> bool {
        if self.actions.contains_key(&action.id) {
            return false;
        }
        self.order.push(action.id.clone());
        self.actions.insert(action.id.clone(), action);
        true
    }

    /// `removeAction`. Returns false when the id is not registered.
    pub fn remove(&mut self, id: &str) -> bool {
        if self.actions.remove(id).is_none() {
            return false;
        }
        self.order.retain(|known| known != id);
        true
    }

    pub fn get(&self, id: &str) -> Option<&Action> {
        self.actions.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Action> {
        self.actions.get_mut(id)
    }

    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// All actions in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &Action> {
        self.order.iter().filter_map(|id| self.actions.get(id))
    }

    /// The actions for one menu section, in registration order.
    pub fn in_section(&self, section: Section) -> Vec<&Action> {
        self.iter().filter(|action| action.is_in(section)).collect()
    }

    /// The event that `Dispatcher:execute` sends for an action.
    ///
    /// `value` is the stored setting for `String` and `AbsoluteNumber`
    /// actions, and the step or gesture value for `Arg` and
    /// `IncrementalNumber` actions. The rules, from `Dispatcher:execute`:
    /// - `None`: the fixed arg, or no argument.
    /// - `String` and `AbsoluteNumber`: one argument. It is `value`, or the
    ///   table `{arg, value}` when there is a fixed arg.
    /// - `Arg`: `value`, or else the fixed arg.
    /// - `IncrementalNumber`: `value`, or 0.
    ///
    /// Returns None when the action is unknown, when a value is needed and
    /// missing, or for `Configurable` and `Key` actions.
    pub fn event_for(&self, id: &str, value: Option<Value>) -> Option<Event> {
        let action = self.get(id)?;
        let args = match action.category {
            ActionCategory::None => action.arg.iter().cloned().collect(),
            ActionCategory::String | ActionCategory::AbsoluteNumber => {
                let value = value?;
                match &action.arg {
                    Some(arg) => vec![Value::array([arg.clone(), value])],
                    None => vec![value],
                }
            }
            ActionCategory::Arg => value.or_else(|| action.arg.clone()).into_iter().collect(),
            ActionCategory::IncrementalNumber => vec![value.unwrap_or(Value::Number(0.0))],
            ActionCategory::Configurable | ActionCategory::Key => return None,
        };
        Some(Event::with_args(action.event.clone(), args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> ActionRegistry {
        let mut r = ActionRegistry::new();
        r.register(
            Action::new("history", "History", ActionCategory::None, "ShowHist").in_section(Section::General),
        );
        r.register(
            Action::new(
                "toggle_status_bar",
                "Toggle status bar",
                ActionCategory::None,
                "ToggleFooterMode",
            )
            .in_section(Section::Reader),
        );
        r.register(
            Action::new(
                "page_jmp",
                "Turn pages",
                ActionCategory::AbsoluteNumber,
                "GotoViewRel",
            )
            .with_range(-100.0, 100.0, None)
            .in_section(Section::Reader),
        );
        r.register(
            Action::new(
                "set_night",
                "Set night mode",
                ActionCategory::String,
                "SetNightMode",
            )
            .with_values(
                vec![Value::Bool(true), Value::Bool(false)],
                vec!["on".into(), "off".into()],
            )
            .in_section(Section::Screen),
        );
        r.register(Action::new("exit", "Exit", ActionCategory::None, "Exit").with_separator());
        r
    }

    #[test]
    fn register_keeps_the_first_action() {
        let mut r = registry();
        assert!(!r.register(Action::new("history", "Other", ActionCategory::None, "Other")));
        assert_eq!(r.get("history").unwrap().event, "ShowHist");
        assert_eq!(r.len(), 5);
    }

    #[test]
    fn remove_drops_the_action_and_its_place_in_order() {
        let mut r = registry();
        assert!(r.remove("page_jmp"));
        assert!(!r.remove("page_jmp"));
        let ids: Vec<&str> = r.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["history", "toggle_status_bar", "set_night", "exit"]);
    }

    #[test]
    fn sections_filter_in_registration_order() {
        let r = registry();
        let reader: Vec<&str> = r
            .in_section(Section::Reader)
            .iter()
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(reader, vec!["toggle_status_bar", "page_jmp"]);
        assert!(r.get("exit").unwrap().sections.is_empty());
        assert_eq!(Section::FileManager.name(), "filemanager");
    }

    #[test]
    fn none_category_sends_its_fixed_arg_only() {
        let mut r = ActionRegistry::new();
        r.register(Action::new("back", "Back", ActionCategory::None, "Back"));
        r.register(Action::new("prev", "Previous", ActionCategory::None, "GoBackLink").with_arg(true));
        assert_eq!(r.event_for("back", None), Some(Event::new("Back")));
        assert_eq!(
            r.event_for("prev", None),
            Some(Event::with_args("GoBackLink", vec![Value::Bool(true)]))
        );
        assert_eq!(r.event_for("missing", None), None);
    }

    #[test]
    fn string_and_number_categories_send_the_value() {
        let r = registry();
        assert_eq!(
            r.event_for("set_night", Some(Value::Bool(true))),
            Some(Event::with_args("SetNightMode", vec![Value::Bool(true)]))
        );
        assert_eq!(r.event_for("set_night", None), None);
        assert_eq!(
            r.event_for("page_jmp", Some(Value::from(5i64))),
            Some(Event::with_args("GotoViewRel", vec![Value::from(5i64)]))
        );
    }

    #[test]
    fn fixed_arg_with_a_value_sends_a_pair_table() {
        let mut r = ActionRegistry::new();
        r.register(Action::new("set_flag", "Set flag", ActionCategory::String, "SetFlag").with_arg("left"));
        let event = r.event_for("set_flag", Some(Value::Bool(false))).unwrap();
        assert_eq!(
            event.args,
            vec![Value::array([Value::from("left"), Value::Bool(false)])]
        );
    }

    #[test]
    fn arg_and_incremental_categories() {
        let mut r = ActionRegistry::new();
        r.register(Action::new("arg", "Follow", ActionCategory::Arg, "GoToPageLink").with_arg(1i64));
        r.register(Action::new(
            "inc",
            "Increase",
            ActionCategory::IncrementalNumber,
            "IncreaseFl",
        ));
        assert_eq!(
            r.event_for("arg", None),
            Some(Event::with_args("GoToPageLink", vec![Value::from(1i64)]))
        );
        assert_eq!(
            r.event_for("arg", Some(Value::from("gesture"))),
            Some(Event::with_args("GoToPageLink", vec![Value::from("gesture")]))
        );
        assert_eq!(
            r.event_for("inc", None),
            Some(Event::with_args("IncreaseFl", vec![Value::from(0.0)]))
        );
    }

    #[test]
    fn configurable_and_key_are_not_modelled() {
        let mut r = ActionRegistry::new();
        r.register(Action::new(
            "cfg",
            "Cfg",
            ActionCategory::Configurable,
            "ConfigChange",
        ));
        r.register(Action::new("key", "Key", ActionCategory::Key, "KeyPress"));
        assert_eq!(r.event_for("cfg", Some(Value::Nil)), None);
        assert_eq!(r.event_for("key", None), None);
    }
}
