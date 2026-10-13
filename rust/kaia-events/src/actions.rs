//! Dispatcher actions, mirroring `frontend/dispatcher.lua`: `settingsList`,
//! `registerAction`, `removeAction`, and the event that `Dispatcher:execute`
//! sends for each category.

use std::collections::HashMap;

use crate::{Event, Key, Table, Value};

/// The active reader mode used by `Dispatcher:isActionEnabled`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionContext {
    FileManager,
    Paging,
    Rolling,
}

/// An event from `Dispatcher:execute`. Batched update events are broadcast.
#[derive(Debug, Clone, PartialEq)]
pub struct DispatchEvent {
    pub event: Event,
    pub broadcast: bool,
}

/// The option name and value mapping used for `ConfigChange`.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigurableAction {
    pub name: String,
    pub values: Vec<Value>,
}

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
    /// Updates a configurable option (KOptions).
    Configurable,
    /// Sends a key press and release.
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
    /// KOReader's separate `configurable` field, which can accompany a category.
    pub configurable: Option<ConfigurableAction>,
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
            configurable: None,
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

    pub fn with_configurable(mut self, name: impl Into<String>, values: Vec<Value>) -> Self {
        self.configurable = Some(ConfigurableAction {
            name: name.into(),
            values,
        });
        self
    }

    pub fn is_enabled_in(&self, context: ActionContext) -> bool {
        if !self.enabled {
            return false;
        }
        !match context {
            ActionContext::FileManager => self
                .sections
                .iter()
                .any(|section| matches!(section, Section::Reader | Section::Rolling | Section::Paging)),
            ActionContext::Paging => self.sections.contains(&Section::Rolling),
            ActionContext::Rolling => self.sections.contains(&Section::Paging),
        }
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
    /// Returns the first event for source compatibility. Use `events_for` to
    /// get all events, including `ConfigChange` and key release.
    pub fn event_for(&self, id: &str, value: Option<Value>) -> Option<Event> {
        self.events_for(id, value, ActionContext::Paging, None, None)
            .into_iter()
            .next()
    }

    /// Events for one enabled action in KOReader execution order.
    pub fn events_for(
        &self,
        id: &str,
        value: Option<Value>,
        context: ActionContext,
        gesture: Option<Value>,
        exec_props: Option<Value>,
    ) -> Vec<Event> {
        let Some(action) = self.get(id) else {
            return Vec::new();
        };
        if !action.is_enabled_in(context) {
            return Vec::new();
        }
        let mut events = Vec::new();
        if let Some(configurable) = &action.configurable {
            let mut config_value = value.clone().unwrap_or(Value::Nil);
            if !matches!(config_value, Value::Number(_)) {
                if let Some(index) = action
                    .values
                    .iter()
                    .position(|candidate| candidate == &config_value)
                {
                    if let Some(mapped) = configurable.values.get(index) {
                        config_value = mapped.clone();
                    }
                }
            }
            events.push(Event::with_args(
                "ConfigChange",
                vec![Value::from(configurable.name.clone()), config_value],
            ));
        }
        let args = match action.category {
            ActionCategory::None => {
                if let Some(arg) = &action.arg {
                    vec![arg.clone(), exec_props.unwrap_or(Value::Nil)]
                } else {
                    Vec::new()
                }
            }
            ActionCategory::String | ActionCategory::AbsoluteNumber => {
                let value = value.unwrap_or(Value::Nil);
                match &action.arg {
                    Some(arg) => vec![Value::array([arg.clone(), value])],
                    None => vec![value],
                }
            }
            ActionCategory::Arg => gesture.or_else(|| action.arg.clone()).into_iter().collect(),
            ActionCategory::IncrementalNumber => {
                let step = value.filter(|value| value != &Value::Number(0.0));
                vec![step.or(gesture).unwrap_or(Value::Number(0.0))]
            }
            ActionCategory::Configurable => return events,
            ActionCategory::Key => {
                let Some(key_name) = action.arg.clone() else {
                    return events;
                };
                let mut key = Table::new();
                key.insert(Key::from("key"), key_name.clone());
                key.insert(Key::from("modifiers"), Value::Table(Table::new()));
                if let Some(name) = key_name.as_str() {
                    key.insert(Key::from(name), Value::Bool(true));
                }
                let key = Value::Table(key);
                events.push(Event::with_args("KeyPress", vec![key.clone()]));
                events.push(Event::with_args("KeyRelease", vec![key]));
                return events;
            }
        };
        events.push(Event::with_args(action.event.clone(), args));
        events
    }

    /// Executes a KOReader action table. The `settings` sub-table controls
    /// ordering, one-by-one execution, and batched update events.
    pub fn execute(
        &self,
        settings: &mut Table,
        context: ActionContext,
        gesture: Option<Value>,
        exec_props: Option<Value>,
    ) -> Vec<DispatchEvent> {
        let meta_key = Key::from("settings");
        let count = settings.keys().filter(|key| **key != meta_key).count();
        let order: Option<Vec<String>> = settings
            .get(&meta_key)
            .and_then(Value::as_table)
            .and_then(|meta| meta.get(&Key::from("order")))
            .and_then(Value::as_table)
            .map(|order| {
                order
                    .values()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            });
        let has_order = order.is_some();
        let mut ids: Vec<String> = order.unwrap_or_else(|| {
            settings
                .keys()
                .filter_map(|key| match key {
                    Key::Str(id) if id != "settings" => Some(id.clone()),
                    _ => None,
                })
                .collect()
        });
        let mut one_by_one = false;
        if let Some(Value::Table(meta)) = settings.get_mut(&meta_key) {
            one_by_one = meta
                .get(&Key::from("execute_one_by_one"))
                .is_some_and(Value::truthy);
            if has_order {
                if let Some(index) = meta.get(&Key::from("execute_one_by_one")).and_then(Value::as_i64) {
                    if !ids.is_empty() {
                        let index = index.max(1) as usize;
                        let index = if index > ids.len() { 1 } else { index };
                        let selected = ids[index - 1].clone();
                        meta.insert(
                            Key::from("execute_one_by_one"),
                            Value::from(if index == ids.len() { 1 } else { index + 1 } as i64),
                        );
                        ids = vec![selected];
                    }
                }
            }
        }
        let batched = count > 1 && !one_by_one;
        let mut result = Vec::new();
        if batched {
            result.push(DispatchEvent {
                event: Event::new("BatchedUpdate"),
                broadcast: true,
            });
        }
        for id in ids {
            let value = settings.get(&Key::from(id.as_str())).cloned();
            result.extend(
                self.events_for(&id, value, context, gesture.clone(), exec_props.clone())
                    .into_iter()
                    .map(|event| DispatchEvent {
                        event,
                        broadcast: false,
                    }),
            );
        }
        if batched {
            result.push(DispatchEvent {
                event: Event::new("BatchedUpdateDone"),
                broadcast: true,
            });
        }
        result
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
            Some(Event::with_args(
                "GoBackLink",
                vec![Value::Bool(true), Value::Nil]
            ))
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
        assert_eq!(
            r.event_for("set_night", None),
            Some(Event::with_args("SetNightMode", vec![Value::Nil]))
        );
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
            r.events_for(
                "arg",
                None,
                ActionContext::FileManager,
                Some(Value::from("gesture")),
                None
            ),
            vec![Event::with_args("GoToPageLink", vec![Value::from("gesture")])]
        );
        assert_eq!(
            r.event_for("inc", None),
            Some(Event::with_args("IncreaseFl", vec![Value::from(0.0)]))
        );
    }

    #[test]
    fn configurable_maps_display_value_and_key_emits_press_then_release() {
        let mut r = ActionRegistry::new();
        r.register(
            Action::new("cfg", "Cfg", ActionCategory::Configurable, "ConfigChange")
                .with_values(vec![Value::from("large")], vec![])
                .with_configurable("font_size", vec![Value::from(24i64)]),
        );
        r.register(Action::new("key", "Key", ActionCategory::Key, "KeyPress").with_arg("Back"));
        assert_eq!(
            r.events_for(
                "cfg",
                Some(Value::from("large")),
                ActionContext::Paging,
                None,
                None
            ),
            vec![Event::with_args(
                "ConfigChange",
                vec![Value::from("font_size"), Value::from(24i64)]
            )]
        );
        let key = r.events_for("key", None, ActionContext::FileManager, None, None);
        assert_eq!(key.len(), 2);
        assert_eq!(key[0].name, "KeyPress");
        assert_eq!(key[1].name, "KeyRelease");
        assert_eq!(key[0].args[0].get("key"), &Value::from("Back"));
        assert_eq!(key[0].args[0].get("Back"), &Value::Bool(true));
        assert_eq!(key[0].args, key[1].args);
    }

    #[test]
    fn context_filters_reader_and_paging_actions() {
        let mut r = ActionRegistry::new();
        r.register(
            Action::new("reader", "Reader", ActionCategory::None, "ReaderEvent").in_section(Section::Reader),
        );
        r.register(
            Action::new("paging", "Paging", ActionCategory::None, "PagingEvent").in_section(Section::Paging),
        );
        r.register(
            Action::new("rolling", "Rolling", ActionCategory::None, "RollingEvent")
                .in_section(Section::Rolling),
        );
        assert!(r
            .events_for("reader", None, ActionContext::FileManager, None, None)
            .is_empty());
        assert!(r
            .events_for("paging", None, ActionContext::FileManager, None, None)
            .is_empty());
        assert!(r
            .events_for("rolling", None, ActionContext::FileManager, None, None)
            .is_empty());
        assert!(r
            .events_for("rolling", None, ActionContext::Paging, None, None)
            .is_empty());
        assert!(r
            .events_for("paging", None, ActionContext::Rolling, None, None)
            .is_empty());
        assert_eq!(
            r.events_for("reader", None, ActionContext::Paging, None, None)
                .len(),
            1
        );
    }

    #[test]
    fn execute_brackets_multiple_actions_and_honors_order() {
        let mut r = ActionRegistry::new();
        r.register(Action::new("first", "First", ActionCategory::None, "First"));
        r.register(Action::new("second", "Second", ActionCategory::None, "Second"));
        let mut settings = Table::new();
        settings.insert(Key::from("first"), Value::Bool(true));
        settings.insert(Key::from("second"), Value::Bool(true));
        let mut meta = Table::new();
        meta.insert(
            Key::from("order"),
            Value::array([Value::from("second"), Value::from("first")]),
        );
        settings.insert(Key::from("settings"), Value::Table(meta));
        let out = r.execute(&mut settings, ActionContext::FileManager, None, None);
        assert_eq!(
            out.iter()
                .map(|item| item.event.name.as_str())
                .collect::<Vec<_>>(),
            vec!["BatchedUpdate", "Second", "First", "BatchedUpdateDone"]
        );
        assert!(out[0].broadcast && out[3].broadcast);
        assert!(!out[1].broadcast && !out[2].broadcast);
        let Value::Table(meta) = settings.get_mut(&Key::from("settings")).unwrap() else {
            panic!()
        };
        meta.insert(Key::from("execute_one_by_one"), Value::from(1i64));
        let out = r.execute(&mut settings, ActionContext::FileManager, None, None);
        assert_eq!(
            out.iter()
                .map(|item| item.event.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Second"]
        );
        assert_eq!(
            settings
                .get(&Key::from("settings"))
                .unwrap()
                .get("execute_one_by_one"),
            &Value::from(2i64)
        );
    }

    #[test]
    fn fixed_arg_exec_props_and_zero_step_follow_dispatcher() {
        let mut r = ActionRegistry::new();
        r.register(Action::new("direct", "Direct", ActionCategory::None, "Direct").with_arg("fixed"));
        r.register(Action::new(
            "step",
            "Step",
            ActionCategory::IncrementalNumber,
            "Step",
        ));
        let props = Value::Table(Table::new());
        assert_eq!(
            r.events_for(
                "direct",
                None,
                ActionContext::FileManager,
                None,
                Some(props.clone())
            ),
            vec![Event::with_args("Direct", vec![Value::from("fixed"), props])]
        );
        assert_eq!(
            r.events_for(
                "step",
                Some(Value::from(0i64)),
                ActionContext::FileManager,
                Some(Value::from("gesture")),
                None
            ),
            vec![Event::with_args("Step", vec![Value::from("gesture")])]
        );
    }

    #[test]
    fn one_by_one_without_order_does_not_choose_one_action() {
        let mut r = ActionRegistry::new();
        r.register(Action::new("a", "A", ActionCategory::None, "A"));
        r.register(Action::new("b", "B", ActionCategory::None, "B"));
        let mut settings = Table::new();
        settings.insert(Key::from("a"), Value::Bool(true));
        settings.insert(Key::from("b"), Value::Bool(true));
        let mut meta = Table::new();
        meta.insert(Key::from("execute_one_by_one"), Value::from(1i64));
        settings.insert(Key::from("settings"), Value::Table(meta));
        let out = r.execute(&mut settings, ActionContext::FileManager, None, None);
        assert_eq!(
            out.iter()
                .map(|item| item.event.name.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "B"]
        );
    }
}
