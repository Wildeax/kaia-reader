//! The JSON form of settings. This mapping is part of the file format, so
//! changing it breaks saved files.
//!
//! Values:
//! - `nil` is never stored. Encoding skips nil entries inside tables.
//! - `true` and `false` are JSON booleans.
//! - An integer with absolute value below 2^53 is a JSON integer. Other finite
//!   numbers are JSON floats. Negative zero is written as `-0.0`, so the sign
//!   survives.
//! - `inf`, `-inf` and `nan` are JSON strings `"~n:inf"`, `"~n:-inf"`, `"~n:NaN"`.
//! - A string that starts with `~` is written with one extra `~` in front. So
//!   `"~x"` becomes `"~~x"`. A JSON string that starts with `~` but not `~~` or
//!   `~n:` is an error.
//! - A table whose keys are exactly `1..=n` is a JSON array. Every other table is
//!   a JSON object.
//!
//! Object keys. Array positions are implicit, so they have no key:
//! - A string key is written as the string, with the same `~` rule as values.
//! - An integer key is `"~i:<decimal>"`, for example `"~i:-3"`.
//! - A non-integer number key is `"~n:<shortest round-trip form>"`, for example `"~n:0.5"`.
//! - A boolean key is `"~b:true"` or `"~b:false"`.
//!
//! The root of a settings file is a table: a JSON object or array.
//! Object keys are sorted by key order, so the output is stable.

use serde_json::{Map, Number, Value as Json};
use std::fmt;

use crate::value::{Key, Table, Value};

/// 2^53: integers up to this size are exact in an f64 and are written as JSON integers.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_992.0;

/// A JSON document that does not follow the mapping above.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError(String);

impl DecodeError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "settings JSON: {}", self.0)
    }
}

impl std::error::Error for DecodeError {}

/// Encodes a key as a JSON object key.
pub fn encode_key(key: &Key) -> String {
    match key {
        Key::Str(s) if s.starts_with('~') => format!("~{s}"),
        Key::Str(s) => s.clone(),
        Key::Int(i) => format!("~i:{i}"),
        Key::Num(n) => format!("~n:{}", n.get()),
        Key::Bool(b) => format!("~b:{b}"),
    }
}

/// Decodes a JSON object key.
pub fn decode_key(text: &str) -> Result<Key, DecodeError> {
    if text.starts_with("~~") {
        return Ok(Key::Str(text[1..].to_owned()));
    }
    if let Some(rest) = text.strip_prefix("~i:") {
        let i = rest
            .parse::<i64>()
            .map_err(|_| DecodeError::new(format!("bad integer key {text:?}")))?;
        return Ok(Key::Int(i));
    }
    if let Some(rest) = text.strip_prefix("~n:") {
        let n = rest
            .parse::<f64>()
            .map_err(|_| DecodeError::new(format!("bad number key {text:?}")))?;
        return Key::num(n).ok_or_else(|| DecodeError::new(format!("NaN key {text:?}")));
    }
    if let Some(rest) = text.strip_prefix("~b:") {
        return match rest {
            "true" => Ok(Key::Bool(true)),
            "false" => Ok(Key::Bool(false)),
            _ => Err(DecodeError::new(format!("bad boolean key {text:?}"))),
        };
    }
    if text.starts_with('~') {
        return Err(DecodeError::new(format!("unknown key prefix in {text:?}")));
    }
    Ok(Key::Str(text.to_owned()))
}

/// Encodes a value. Nil becomes JSON `null`, but nil never appears inside a
/// table, because the encoder skips those entries.
pub fn encode_value(value: &Value) -> Json {
    match value {
        Value::Nil => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Number(n) => encode_number(*n),
        Value::Str(s) if s.starts_with('~') => Json::String(format!("~{s}")),
        Value::Str(s) => Json::String(s.clone()),
        Value::Table(table) => encode_table(table),
    }
}

fn encode_number(n: f64) -> Json {
    if !n.is_finite() {
        return Json::String(format!("~n:{n}"));
    }
    let negative_zero = n == 0.0 && n.is_sign_negative();
    if n.fract() == 0.0 && n.abs() < MAX_SAFE_INTEGER && !negative_zero {
        return Json::Number(Number::from(n as i64));
    }
    match Number::from_f64(n) {
        Some(number) => Json::Number(number),
        None => Json::String(format!("~n:{n}")),
    }
}

/// Decodes a value. JSON `null` decodes to nil.
pub fn decode_value(json: &Json) -> Result<Value, DecodeError> {
    match json {
        Json::Null => Ok(Value::Nil),
        Json::Bool(b) => Ok(Value::Bool(*b)),
        Json::Number(number) => number
            .as_f64()
            .map(Value::Number)
            .ok_or_else(|| DecodeError::new(format!("number out of range: {number}"))),
        Json::String(s) => decode_string(s),
        Json::Array(items) => decode_array(items).map(Value::Table),
        Json::Object(map) => decode_object(map).map(Value::Table),
    }
}

fn decode_string(s: &str) -> Result<Value, DecodeError> {
    if s.starts_with("~~") {
        return Ok(Value::Str(s[1..].to_owned()));
    }
    if let Some(rest) = s.strip_prefix("~n:") {
        let n = rest
            .parse::<f64>()
            .map_err(|_| DecodeError::new(format!("bad number {s:?}")))?;
        return Ok(Value::Number(n));
    }
    if s.starts_with('~') {
        return Err(DecodeError::new(format!("unknown string prefix in {s:?}")));
    }
    Ok(Value::Str(s.to_owned()))
}

/// Encodes a table, as an array when its keys are exactly `1..=n`, otherwise as an object.
pub fn encode_table(table: &Table) -> Json {
    let entries: Vec<(&Key, &Value)> = table.iter().filter(|(_, value)| !value.is_nil()).collect();
    let is_sequence = entries
        .iter()
        .enumerate()
        .all(|(index, (key, _))| **key == Key::Int(index as i64 + 1));
    if is_sequence {
        Json::Array(entries.iter().map(|(_, value)| encode_value(value)).collect())
    } else {
        let mut map = Map::new();
        for (key, value) in entries {
            map.insert(encode_key(key), encode_value(value));
        }
        Json::Object(map)
    }
}

/// Decodes a JSON array or object into a table.
pub fn decode_table(json: &Json) -> Result<Table, DecodeError> {
    match json {
        Json::Array(items) => decode_array(items),
        Json::Object(map) => decode_object(map),
        _ => Err(DecodeError::new("the root must be an object or an array")),
    }
}

fn decode_array(items: &[Json]) -> Result<Table, DecodeError> {
    let mut table = Table::new();
    for (index, item) in items.iter().enumerate() {
        let value = decode_value(item)?;
        if !value.is_nil() {
            table.insert(Key::Int(index as i64 + 1), value);
        }
    }
    Ok(table)
}

fn decode_object(map: &Map<String, Json>) -> Result<Table, DecodeError> {
    let mut table = Table::new();
    for (text, item) in map {
        let key = decode_key(text)?;
        let value = decode_value(item)?;
        if value.is_nil() {
            continue;
        }
        if table.insert(key, value).is_some() {
            return Err(DecodeError::new(format!("duplicate key {text:?}")));
        }
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(value: &Value) -> Value {
        let text = serde_json::to_string(&encode_value(value)).unwrap();
        let json: Json = serde_json::from_str(&text).unwrap();
        decode_value(&json).unwrap()
    }

    fn table_round_trip(table: &Table) -> Table {
        let text = serde_json::to_string_pretty(&encode_table(table)).unwrap();
        let json: Json = serde_json::from_str(&text).unwrap();
        decode_table(&json).unwrap()
    }

    #[test]
    fn scalars_round_trip() {
        for value in [
            Value::Bool(true),
            Value::Bool(false),
            Value::Number(0.0),
            Value::Number(-0.0),
            Value::Number(42.0),
            Value::Number(-7.0),
            Value::Number(0.1),
            Value::Number(1e300),
            Value::Number(-1.5e-300),
            Value::Number(9_007_199_254_740_993.0),
            Value::Number(f64::INFINITY),
            Value::Number(f64::NEG_INFINITY),
            Value::Str(String::new()),
            Value::Str("plain".to_owned()),
            Value::Str("~tilde".to_owned()),
            Value::Str("~".to_owned()),
            Value::Str("~n:inf".to_owned()),
        ] {
            assert_eq!(round_trip(&value), value, "value {value:?}");
        }
    }

    #[test]
    fn nan_round_trips_as_nan() {
        match round_trip(&Value::Number(f64::NAN)) {
            Value::Number(n) => assert!(n.is_nan()),
            other => panic!("expected a number, got {other:?}"),
        }
    }

    #[test]
    fn integers_are_json_integers_and_floats_stay_floats() {
        assert_eq!(
            serde_json::to_string(&encode_value(&Value::Number(3.0))).unwrap(),
            "3"
        );
        assert_eq!(
            serde_json::to_string(&encode_value(&Value::Number(3.5))).unwrap(),
            "3.5"
        );
        assert_eq!(
            serde_json::to_string(&encode_value(&Value::Number(-0.0))).unwrap(),
            "-0.0"
        );
    }

    #[test]
    fn strings_with_tilde_are_escaped() {
        assert_eq!(encode_value(&Value::from("~x")), Json::String("~~x".to_owned()));
        assert_eq!(encode_value(&Value::from("x~")), Json::String("x~".to_owned()));
    }

    #[test]
    fn bad_prefixes_are_rejected() {
        assert!(decode_value(&Json::String("~oops".to_owned())).is_err());
        assert!(decode_key("~z:1").is_err());
        assert!(decode_key("~i:x").is_err());
        assert!(decode_key("~n:NaN").is_err());
    }

    #[test]
    fn sequences_become_arrays_and_others_objects() {
        let array = Value::array([Value::from(1i64), Value::from("two")]);
        assert_eq!(encode_value(&array), serde_json::json!([1, "two"]));

        let mut holey = Table::new();
        holey.insert(Key::Int(1), Value::from(true));
        holey.insert(Key::Int(3), Value::from(true));
        match encode_table(&holey) {
            Json::Object(map) => assert!(map.contains_key("~i:1") && map.contains_key("~i:3")),
            other => panic!("expected an object, got {other}"),
        }
    }

    #[test]
    fn keys_of_every_kind_round_trip() {
        let mut table = Table::new();
        table.insert(Key::from("plain"), Value::from(1i64));
        table.insert(Key::from("~tilde"), Value::from(2i64));
        table.insert(Key::from("~i:5"), Value::from(3i64));
        table.insert(Key::Int(5), Value::from(4i64));
        table.insert(Key::Int(-1), Value::from(5i64));
        table.insert(Key::num(0.5).unwrap(), Value::from(6i64));
        table.insert(Key::num(f64::INFINITY).unwrap(), Value::from(7i64));
        table.insert(Key::Bool(true), Value::from(8i64));
        table.insert(Key::Bool(false), Value::from(9i64));
        assert_eq!(table_round_trip(&table), table);
    }

    #[test]
    fn nested_tables_round_trip() {
        let inner = Value::array([Value::from("a"), Value::Table(Table::new())]);
        let mut table = Table::new();
        table.insert(Key::from("list"), inner);
        table.insert(Key::from("empty"), Value::Table(Table::new()));
        assert_eq!(table_round_trip(&table), table);
    }

    #[test]
    fn nil_entries_are_dropped_on_encode() {
        let mut table = Table::new();
        table.insert(Key::from("keep"), Value::from(true));
        table.insert(Key::from("drop"), Value::Nil);
        let decoded = table_round_trip(&table);
        assert_eq!(decoded.len(), 1);
        assert!(decoded.contains_key(&Key::from("keep")));
    }

    #[test]
    fn duplicate_keys_are_an_error() {
        // "~i:01" and "~i:1" are different JSON keys that decode to the same integer key.
        let json: Json = serde_json::from_str(r#"{"~i:1": true, "~i:01": false}"#).unwrap();
        assert!(decode_table(&json).is_err());
    }

    #[test]
    fn root_must_be_a_table() {
        assert!(decode_table(&Json::Bool(true)).is_err());
        assert!(decode_table(&serde_json::json!({})).unwrap().is_empty());
    }

    #[test]
    fn number_keys_keep_integer_and_float_identity() {
        assert_eq!(encode_key(&Key::Int(2)), "~i:2");
        assert_eq!(encode_key(&Key::num(2.25).unwrap()), "~n:2.25");
    }
}
