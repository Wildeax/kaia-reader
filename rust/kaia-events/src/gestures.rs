//! Gesture bindings: gesture names to the actions they run, as Dispatcher
//! stores them in KOReader's gesture settings.
//!
//! Gesture names are free text, for example `"tap_left_bottom_corner"` or
//! `"swipe_north"`. The bindings do not check them, so names the app does not
//! produce are kept as they are.

use std::collections::BTreeMap;

use crate::{Key, Table, Value};

/// One action bound to a gesture, with the arguments to pass.
#[derive(Debug, Clone, PartialEq)]
pub struct GestureBinding {
    pub action: String,
    pub args: Vec<Value>,
}

/// Maps gesture names to their bindings, in the order they were bound.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GestureBindings {
    by_gesture: BTreeMap<String, Vec<GestureBinding>>,
}

impl GestureBindings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `action` to `gesture` with `args`. Binding the same action again
    /// replaces its arguments and keeps its place.
    pub fn bind(&mut self, gesture: &str, action: &str, args: Vec<Value>) {
        let bindings = self.by_gesture.entry(gesture.to_owned()).or_default();
        match bindings.iter_mut().find(|binding| binding.action == action) {
            Some(existing) => existing.args = args,
            None => bindings.push(GestureBinding {
                action: action.to_owned(),
                args,
            }),
        }
    }

    /// Removes one action from a gesture. Returns false when it was not bound.
    pub fn unbind(&mut self, gesture: &str, action: &str) -> bool {
        let Some(bindings) = self.by_gesture.get_mut(gesture) else {
            return false;
        };
        let before = bindings.len();
        bindings.retain(|binding| binding.action != action);
        bindings.len() != before
    }

    /// An empty binding is KOReader's "Nothing" action. A missing binding
    /// lets the gesture pass through.
    pub fn set_nothing(&mut self, gesture: &str) {
        self.by_gesture.insert(gesture.to_owned(), Vec::new());
    }

    pub fn contains(&self, gesture: &str) -> bool {
        self.by_gesture.contains_key(gesture)
    }

    /// Restores pass-through behavior for a gesture.
    pub fn remove_gesture(&mut self, gesture: &str) -> bool {
        self.by_gesture.remove(gesture).is_some()
    }

    /// The bindings for a gesture, in binding order. Empty when nothing is bound.
    pub fn actions(&self, gesture: &str) -> &[GestureBinding] {
        self.by_gesture.get(gesture).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The stored gesture names, including "Nothing", in sorted order.
    pub fn gestures(&self) -> impl Iterator<Item = &str> {
        self.by_gesture.keys().map(String::as_str)
    }

    /// The number of bound gestures.
    pub fn len(&self) -> usize {
        self.by_gesture.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_gesture.is_empty()
    }
}

/// KOReader's `gestures.lua` data. The two gesture sections stay separate.
/// Each gesture is a table of action names to values, with an optional
/// `settings` table. Keeping the original tables also preserves fields that
/// the plugin layer does not interpret yet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KoreaderGestures {
    data: Table,
}

impl KoreaderGestures {
    pub const FILE_MANAGER: &'static str = "gesture_fm";
    pub const READER: &'static str = "gesture_reader";

    /// Reads the root table of KOReader's gesture data.
    pub fn from_table(data: Table) -> Self {
        Self { data }
    }

    /// Reads a value returned by `kaia_luadata::parse`.
    pub fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::Table(data) => Some(Self { data }),
            _ => None,
        }
    }

    /// Returns the root table.
    pub fn to_table(&self) -> Table {
        self.data.clone()
    }

    /// Returns a value for `kaia_luadata::to_string`.
    pub fn to_value(&self) -> Value {
        Value::Table(self.to_table())
    }

    pub fn section(&self, section: &str) -> Option<&Table> {
        self.data.get(&Key::from(section))?.as_table()
    }

    pub fn gesture(&self, section: &str, name: &str) -> Option<&Table> {
        self.section(section)?.get(&Key::from(name))?.as_table()
    }

    /// A present empty table swallows the gesture. `None` passes it through.
    pub fn set_nothing(&mut self, section: &str, name: &str) {
        self.section_mut(section)
            .insert(Key::from(name), Value::Table(Table::new()));
    }

    pub fn remove_gesture(&mut self, section: &str, name: &str) -> bool {
        self.section_mut(section).remove(&Key::from(name)).is_some()
    }

    pub fn set_action(&mut self, section: &str, name: &str, action: &str, value: Value) {
        let gesture = self.gesture_mut(section, name);
        if value.is_nil() {
            gesture.remove(&Key::from(action));
        } else {
            gesture.insert(Key::from(action), value);
        }
    }

    pub fn settings(&self, section: &str, name: &str) -> Option<&Table> {
        self.gesture(section, name)?
            .get(&Key::from("settings"))?
            .as_table()
    }

    pub fn set_settings(&mut self, section: &str, name: &str, settings: Table) {
        self.gesture_mut(section, name)
            .insert(Key::from("settings"), Value::Table(settings));
    }

    fn section_mut(&mut self, section: &str) -> &mut Table {
        self.data
            .entry(Key::from(section))
            .or_insert_with(|| Value::Table(Table::new()));
        match self.data.get_mut(&Key::from(section)).unwrap() {
            Value::Table(table) => table,
            value => {
                *value = Value::Table(Table::new());
                match value {
                    Value::Table(table) => table,
                    _ => unreachable!(),
                }
            }
        }
    }

    fn gesture_mut(&mut self, section: &str, name: &str) -> &mut Table {
        let value = self
            .section_mut(section)
            .entry(Key::from(name))
            .or_insert_with(|| Value::Table(Table::new()));
        if !matches!(value, Value::Table(_)) {
            *value = Value::Table(Table::new());
        }
        match value {
            Value::Table(table) => table,
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bindings_keep_order_and_replace_args() {
        let mut g = GestureBindings::new();
        g.bind("tap_left_bottom_corner", "page_jmp", vec![Value::from(-1i64)]);
        g.bind("tap_left_bottom_corner", "toc", vec![]);
        g.bind("tap_left_bottom_corner", "page_jmp", vec![Value::from(-2i64)]);
        let actions = g.actions("tap_left_bottom_corner");
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].action, "page_jmp");
        assert_eq!(actions[0].args, vec![Value::from(-2i64)]);
        assert_eq!(actions[1].action, "toc");
    }

    #[test]
    fn unbind_keeps_nothing_distinct_from_missing() {
        let mut g = GestureBindings::new();
        g.bind("swipe_north", "toc", vec![]);
        assert!(g.unbind("swipe_north", "toc"));
        assert!(!g.unbind("swipe_north", "toc"));
        assert!(g.contains("swipe_north"));
        assert_eq!(g.len(), 1);
        assert!(g.actions("swipe_north").is_empty());
        assert!(g.remove_gesture("swipe_north"));
        assert!(!g.contains("swipe_north"));
    }

    #[test]
    fn koreader_sections_round_trip_actions_and_settings() {
        let mut data = KoreaderGestures::default();
        data.set_nothing(KoreaderGestures::FILE_MANAGER, "hold");
        data.set_action(KoreaderGestures::READER, "swipe", "page_jmp", Value::from(-1i64));
        let mut settings = Table::new();
        settings.insert(Key::from("always_active"), Value::Bool(true));
        data.set_settings(KoreaderGestures::READER, "swipe", settings.clone());
        let table = data.to_table();
        let restored = KoreaderGestures::from_table(table.clone());
        assert_eq!(restored.to_table(), table);
        assert_eq!(
            KoreaderGestures::from_value(data.to_value()),
            Some(restored.clone())
        );
        assert_eq!(
            restored.gesture(KoreaderGestures::FILE_MANAGER, "hold"),
            Some(&Table::new())
        );
        assert_eq!(
            restored.settings(KoreaderGestures::READER, "swipe"),
            Some(&settings)
        );
        assert_eq!(restored.gesture(KoreaderGestures::FILE_MANAGER, "absent"), None);
    }

    #[test]
    fn gestures_are_listed_sorted() {
        let mut g = GestureBindings::new();
        g.bind("swipe_north", "a", vec![]);
        g.bind("hold", "b", vec![]);
        assert_eq!(g.gestures().collect::<Vec<_>>(), vec!["hold", "swipe_north"]);
        assert_eq!(g.len(), 2);
    }
}
