//! Gesture bindings: gesture names to the actions they run, as Dispatcher
//! stores them in KOReader's gesture settings.
//!
//! Gesture names are free text, for example `"tap_left_bottom_corner"` or
//! `"swipe_north"`. The bindings do not check them, so names the app does not
//! produce are kept as they are.

use std::collections::BTreeMap;

use crate::Value;

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
        let removed = bindings.len() != before;
        if bindings.is_empty() {
            self.by_gesture.remove(gesture);
        }
        removed
    }

    /// The bindings for a gesture, in binding order. Empty when nothing is bound.
    pub fn actions(&self, gesture: &str) -> &[GestureBinding] {
        self.by_gesture.get(gesture).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The gesture names that have at least one binding, in sorted order.
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
    fn unbind_removes_empty_gestures() {
        let mut g = GestureBindings::new();
        g.bind("swipe_north", "toc", vec![]);
        assert!(g.unbind("swipe_north", "toc"));
        assert!(!g.unbind("swipe_north", "toc"));
        assert!(g.is_empty());
        assert!(g.actions("swipe_north").is_empty());
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
