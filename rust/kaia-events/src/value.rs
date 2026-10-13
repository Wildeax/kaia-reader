//! A dynamic value with Lua's data model: nil, booleans, numbers, strings
//! and tables. Settings, event arguments and plugin data use it, so the Lua
//! layer can convert values both ways without loss.

use std::collections::BTreeMap;

/// A table key. Lua allows any non-nil value as a key; Kaia data only uses
/// booleans, integers, other numbers and strings. Integer keys sort first,
/// then other numbers, then strings, then booleans, so serialized output is
/// stable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Key {
    Int(i64),
    /// A non-integer number stored as its IEEE 754 bits, ordered by value
    /// through [`Key::num`]. NaN is not allowed as a key.
    Num(OrderedF64),
    Str(String),
    Bool(bool),
}

/// An f64 with a total order, for table keys. Never NaN.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrderedF64(f64);

impl OrderedF64 {
    pub fn get(self) -> f64 {
        self.0
    }
}

impl Eq for OrderedF64 {}

impl PartialOrd for OrderedF64 {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedF64 {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl std::hash::Hash for OrderedF64 {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl Key {
    /// A number key: integers become `Int`, others `Num`. Returns None for NaN.
    pub fn num(n: f64) -> Option<Key> {
        if n.is_nan() {
            return None;
        }
        if n.fract() == 0.0 && n >= i64::MIN as f64 && n <= i64::MAX as f64 {
            Some(Key::Int(n as i64))
        } else {
            Some(Key::Num(OrderedF64(n)))
        }
    }
}

impl From<&str> for Key {
    fn from(s: &str) -> Self {
        Key::Str(s.to_owned())
    }
}

impl From<String> for Key {
    fn from(s: String) -> Self {
        Key::Str(s)
    }
}

impl From<i64> for Key {
    fn from(i: i64) -> Self {
        Key::Int(i)
    }
}

/// A Lua-style table: ordered keys to values. Arrays use keys 1..=n.
pub type Table = BTreeMap<Key, Value>;

/// A dynamic value.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Nil,
    Bool(bool),
    Number(f64),
    Str(String),
    Table(Table),
}

impl Value {
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    /// Lua truthiness: everything except nil and false is true.
    pub fn truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(n) if n.fract() == 0.0 => Some(*n as i64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_table(&self) -> Option<&Table> {
        match self {
            Value::Table(t) => Some(t),
            _ => None,
        }
    }

    /// Field lookup on a table value; Nil for anything else.
    pub fn get(&self, key: impl Into<Key>) -> &Value {
        static NIL: Value = Value::Nil;
        match self {
            Value::Table(t) => t.get(&key.into()).unwrap_or(&NIL),
            _ => &NIL,
        }
    }

    /// Builds an array table with keys 1..=n.
    pub fn array(items: impl IntoIterator<Item = Value>) -> Value {
        let mut t = Table::new();
        for (i, v) in items.into_iter().enumerate() {
            t.insert(Key::Int(i as i64 + 1), v);
        }
        Value::Table(t)
    }

    /// The length of the array part, like Lua's `#` for tables without holes.
    pub fn array_len(&self) -> usize {
        match self {
            Value::Table(t) => {
                let mut n = 0;
                while t.contains_key(&Key::Int(n as i64 + 1)) {
                    n += 1;
                }
                n
            }
            _ => 0,
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Number(n as f64)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::Str(s.to_owned())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::Str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_order_ints_numbers_strings_bools() {
        let mut keys = [
            Key::from("b"),
            Key::Bool(true),
            Key::num(1.5).unwrap(),
            Key::Int(2),
        ];
        keys.sort();
        assert_eq!(keys[0], Key::Int(2));
        assert_eq!(keys[1], Key::num(1.5).unwrap());
        assert_eq!(keys[2], Key::from("b"));
        assert_eq!(keys[3], Key::Bool(true));
    }

    #[test]
    fn integral_numbers_become_int_keys() {
        assert_eq!(Key::num(3.0), Some(Key::Int(3)));
        assert_eq!(Key::num(f64::NAN), None);
    }

    #[test]
    fn arrays_and_truthiness() {
        let a = Value::array([Value::from(1i64), Value::from("x")]);
        assert_eq!(a.array_len(), 2);
        assert_eq!(a.get(2i64), &Value::from("x"));
        assert!(!Value::Nil.truthy());
        assert!(Value::from(0i64).truthy());
    }
}
