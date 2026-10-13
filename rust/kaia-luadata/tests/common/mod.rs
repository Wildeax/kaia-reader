//! Shared generators and comparison for the integration tests.
#![allow(dead_code)]

use kaia_events::{Key, Value};
use proptest::prelude::*;

/// Structural equality that compares numbers by bits, so `-0.0` and `0.0`
/// differ and any two NaNs are equal.
pub fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Number(x), Value::Number(y)) => (x.is_nan() && y.is_nan()) || x.to_bits() == y.to_bits(),
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Table(x), Value::Table(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y.iter())
                    .all(|((ka, va), (kb, vb))| ka == kb && same(va, vb))
        }
        _ => false,
    }
}

pub fn arb_string() -> impl Strategy<Value = String> {
    let special = prop::sample::select(vec![
        '"',
        '\'',
        '\\',
        '\n',
        '\r',
        '\t',
        '\0',
        '\u{7f}',
        ']',
        '[',
        '=',
        '-',
        '\u{1}',
        '\u{b}',
        '\u{c}',
        '1',
        '9',
        'x',
        ' ',
        '\u{80}',
        '\u{feff}',
        '\u{1f642}',
    ]);
    prop::collection::vec(prop_oneof![3 => special, 2 => any::<char>()], 0..24)
        .prop_map(|chars| chars.into_iter().collect())
}

pub fn arb_key() -> impl Strategy<Value = Key> {
    prop_oneof![
        3 => (-(1i64 << 53)..=(1i64 << 53)).prop_map(Key::Int),
        3 => any::<f64>().prop_filter_map("NaN is not a key", Key::num),
        3 => arb_string().prop_map(Key::Str),
        1 => any::<bool>().prop_map(Key::Bool),
    ]
}

fn arb_leaf() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<bool>().prop_map(Value::Bool),
        any::<f64>().prop_map(Value::Number),
        arb_string().prop_map(Value::Str),
    ]
}

/// Any value. Tables never hold nil, but a top-level value may be nil.
pub fn arb_value() -> impl Strategy<Value = Value> {
    let non_nil = arb_leaf().prop_recursive(4, 48, 6, |inner| {
        prop::collection::btree_map(arb_key(), inner, 0..6).prop_map(Value::Table)
    });
    prop_oneof![
        1 => Just(Value::Nil),
        9 => non_nil,
    ]
}

/// A fresh directory under the system temp dir, unique per test run.
pub fn scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("kaia-luadata-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}
