//! Conversion between Lua values and `kaia_events::Value`.
//!
//! - nil, booleans, numbers and strings keep their type. Lua numbers and Kaia
//!   numbers are both f64, so NaN, the infinities and -0.0 pass unchanged.
//! - A table converts with all its keys, array part and hash part alike. Number
//!   keys with an integer value become `Key::Int`, other numbers `Key::Num`.
//!   Booleans and strings keep their type.
//! - Tables nest to at most [`MAX_DEPTH`] levels. A table that contains itself
//!   is an error. A table that appears twice without a cycle is copied twice.
//! - Strings must be valid UTF-8, because `Value::Str` holds a Rust `String`.
//! - Functions, threads, userdata, light userdata and LuaJIT cdata are not
//!   Kaia values. They are an error, with the Lua type name. `Value` has no
//!   variant for an opaque handle, so no marker is produced.
//! - An integer key outside +-2^53 is an error when it goes to Lua, because a
//!   Lua number cannot hold it exactly.

use std::ffi::c_void;

use kaia_events::value::{Key, Table, Value};
use mlua::Lua;

use crate::error::lua_error;
use crate::Error;

/// The deepest table nesting that converts. The top-level table is level 1.
pub const MAX_DEPTH: usize = 256;

const MAX_EXACT_INTEGER: i64 = 1 << 53;

/// Converts a Lua value into a Kaia value.
pub(crate) fn from_lua(value: &mlua::Value) -> Result<Value, Error> {
    read(value, 0, &mut Vec::new())
}

/// Converts a Kaia value into a Lua value. Tables are new Lua tables.
pub(crate) fn to_lua(lua: &Lua, value: &Value) -> Result<mlua::Value, Error> {
    write(lua, value, 0)
}

fn read(value: &mlua::Value, depth: usize, ancestors: &mut Vec<*const c_void>) -> Result<Value, Error> {
    Ok(match value {
        mlua::Value::Nil => Value::Nil,
        mlua::Value::Boolean(b) => Value::Bool(*b),
        mlua::Value::Integer(i) => Value::Number(*i as f64),
        mlua::Value::Number(n) => Value::Number(*n),
        mlua::Value::String(s) => Value::Str(text(s)?),
        mlua::Value::Table(t) => read_table(t, depth, ancestors)?,
        other => return Err(unsupported(other)),
    })
}

fn read_table(t: &mlua::Table, depth: usize, ancestors: &mut Vec<*const c_void>) -> Result<Value, Error> {
    if depth >= MAX_DEPTH {
        return Err(Error::Convert(format!(
            "tables nest deeper than {MAX_DEPTH} levels"
        )));
    }
    // The identity is compared only, never dereferenced.
    let id = t.to_pointer();
    if ancestors.contains(&id) {
        return Err(Error::Convert("a table contains itself".to_owned()));
    }
    ancestors.push(id);
    let mut out = Table::new();
    for pair in t.pairs::<mlua::Value, mlua::Value>() {
        let (k, v) = pair.map_err(lua_error)?;
        let key = read_key(&k)?;
        let val = read(&v, depth + 1, ancestors)?;
        out.insert(key, val);
    }
    ancestors.pop();
    Ok(Value::Table(out))
}

fn read_key(key: &mlua::Value) -> Result<Key, Error> {
    match key {
        mlua::Value::Boolean(b) => Ok(Key::Bool(*b)),
        mlua::Value::Integer(i) => {
            // LuaJIT uses 32-bit integers on the Kobo and 64-bit integers on desktop.
            #[allow(clippy::useless_conversion)]
            let integer = i64::from(*i);
            Ok(Key::Int(integer))
        }
        mlua::Value::Number(n) => {
            Key::num(*n).ok_or_else(|| Error::Convert("NaN is not a table key".to_owned()))
        }
        mlua::Value::String(s) => Ok(Key::Str(text(s)?)),
        other => Err(Error::Convert(format!(
            "table keys of Lua type {} do not convert to Kaia values",
            kind(other)
        ))),
    }
}

fn write(lua: &Lua, value: &Value, depth: usize) -> Result<mlua::Value, Error> {
    Ok(match value {
        Value::Nil => mlua::Value::Nil,
        Value::Bool(b) => mlua::Value::Boolean(*b),
        Value::Number(n) => mlua::Value::Number(*n),
        Value::Str(s) => mlua::Value::String(lua.create_string(s).map_err(lua_error)?),
        Value::Table(entries) => {
            if depth >= MAX_DEPTH {
                return Err(Error::Convert(format!(
                    "tables nest deeper than {MAX_DEPTH} levels"
                )));
            }
            let table = lua.create_table().map_err(lua_error)?;
            for (k, v) in entries {
                let key = write_key(lua, k)?;
                let val = write(lua, v, depth + 1)?;
                table.raw_set(key, val).map_err(lua_error)?;
            }
            mlua::Value::Table(table)
        }
    })
}

fn write_key(lua: &Lua, key: &Key) -> Result<mlua::Value, Error> {
    Ok(match key {
        Key::Int(i) => {
            if !(-MAX_EXACT_INTEGER..=MAX_EXACT_INTEGER).contains(i) {
                return Err(Error::Convert(format!(
                    "integer key {i} is outside +-2^53 and cannot be a Lua number"
                )));
            }
            mlua::Value::Number(*i as f64)
        }
        Key::Num(n) => mlua::Value::Number(n.get()),
        Key::Str(s) => mlua::Value::String(lua.create_string(s).map_err(lua_error)?),
        Key::Bool(b) => mlua::Value::Boolean(*b),
    })
}

fn text(s: &mlua::String) -> Result<String, Error> {
    let borrowed = s
        .to_str()
        .map_err(|_| Error::Convert("a string is not valid UTF-8".to_owned()))?;
    Ok(borrowed.to_owned())
}

fn unsupported(value: &mlua::Value) -> Error {
    Error::Convert(format!(
        "Lua {} values do not convert to Kaia values",
        kind(value)
    ))
}

fn kind(value: &mlua::Value) -> &'static str {
    match value {
        mlua::Value::Function(_) => "function",
        mlua::Value::Thread(_) => "thread",
        mlua::Value::UserData(_) => "userdata",
        mlua::Value::LightUserData(_) => "light userdata",
        mlua::Value::Other(_) => "cdata",
        mlua::Value::Error(_) => "error",
        _ => "unknown",
    }
}
