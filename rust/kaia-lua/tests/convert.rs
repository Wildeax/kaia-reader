//! Value conversion between Lua and `kaia_events::Value`.

mod common;

use common::{host, serial, TempDir};
use kaia_events::value::{Key, Table, Value};
use kaia_lua::{Error, MAX_DEPTH};

fn convert_error(err: Error) -> String {
    match err {
        Error::Convert(message) => message,
        other => panic!("expected a conversion error, got {other}"),
    }
}

#[test]
fn scalars_round_trip() {
    let _guard = serial();
    let data = TempDir::new("scalars");
    let host = host(&data);
    for value in [
        Value::Nil,
        Value::Bool(true),
        Value::Bool(false),
        Value::Number(42.5),
        Value::Number(-0.0),
        Value::Str("héllo wörld".to_owned()),
        Value::Str(String::new()),
        Value::Str("with\0nul".to_owned()),
    ] {
        host.set_global("probe", &value).unwrap();
        assert_eq!(host.get_global("probe").unwrap(), value);
    }
}

#[test]
fn nan_and_infinities_pass_through() {
    let _guard = serial();
    let data = TempDir::new("special");
    let host = host(&data);

    host.set_global("probe", &Value::Number(f64::NAN)).unwrap();
    host.exec("nan.lua", "assert(probe ~= probe)").unwrap();
    match host.get_global("probe").unwrap() {
        Value::Number(n) => assert!(n.is_nan()),
        other => panic!("expected a number, got {other:?}"),
    }

    host.set_global("probe", &Value::Number(f64::INFINITY)).unwrap();
    host.exec("inf.lua", "assert(probe == math.huge)").unwrap();
    assert_eq!(host.get_global("probe").unwrap(), Value::Number(f64::INFINITY));

    host.set_global("probe", &Value::Number(f64::NEG_INFINITY))
        .unwrap();
    assert_eq!(
        host.get_global("probe").unwrap(),
        Value::Number(f64::NEG_INFINITY)
    );

    // Lua-made values come back the same way.
    let nan = host.eval("nan_src.lua", "return 0 / 0").unwrap();
    assert!(matches!(nan, Value::Number(n) if n.is_nan()));
    assert_eq!(
        host.eval("inf_src.lua", "return -1 / 0").unwrap(),
        Value::Number(f64::NEG_INFINITY)
    );
}

#[test]
fn array_and_hash_parts_convert_with_their_keys() {
    let _guard = serial();
    let data = TempDir::new("keys");
    let host = host(&data);
    let value = host
        .eval(
            "keys.lua",
            "return { 10, 20, x = 'y', [100] = 3, [1.5] = 4, [true] = 5, [-7] = 6 }",
        )
        .unwrap();
    let table = value.as_table().expect("a table");
    assert_eq!(table.get(&Key::Int(1)), Some(&Value::Number(10.0)));
    assert_eq!(table.get(&Key::Int(2)), Some(&Value::Number(20.0)));
    assert_eq!(
        table.get(&Key::Str("x".to_owned())),
        Some(&Value::Str("y".to_owned()))
    );
    assert_eq!(table.get(&Key::Int(100)), Some(&Value::Number(3.0)));
    assert_eq!(table.get(&Key::Int(-7)), Some(&Value::Number(6.0)));
    assert_eq!(table.get(&Key::num(1.5).unwrap()), Some(&Value::Number(4.0)));
    assert_eq!(table.get(&Key::Bool(true)), Some(&Value::Number(5.0)));
    assert_eq!(table.len(), 7);
    assert_eq!(value.array_len(), 2);
}

#[test]
fn integer_valued_float_keys_are_integer_keys() {
    let _guard = serial();
    let data = TempDir::new("float-keys");
    let host = host(&data);
    let value = host
        .eval("float_keys.lua", "return { [2.0] = 'a', [0.5] = 'b' }")
        .unwrap();
    let table = value.as_table().unwrap();
    assert_eq!(table.get(&Key::Int(2)), Some(&Value::Str("a".to_owned())));
    assert_eq!(table.len(), 2);
    assert_eq!(
        table.get(&Key::num(0.5).unwrap()),
        Some(&Value::Str("b".to_owned()))
    );
    assert!(table.keys().any(|k| matches!(k, Key::Num(_))));
}

#[test]
fn infinite_keys_round_trip() {
    let _guard = serial();
    let data = TempDir::new("inf-keys");
    let host = host(&data);
    let mut table = Table::new();
    table.insert(Key::num(f64::INFINITY).unwrap(), Value::Str("up".to_owned()));
    host.set_global("probe", &Value::Table(table.clone())).unwrap();
    host.exec("inf_keys.lua", "assert(probe[math.huge] == 'up')")
        .unwrap();
    assert_eq!(host.get_global("probe").unwrap(), Value::Table(table));
}

#[test]
fn nested_tables_round_trip() {
    let _guard = serial();
    let data = TempDir::new("nested");
    let host = host(&data);
    let mut inner = Table::new();
    inner.insert(Key::Int(1), Value::Bool(true));
    inner.insert(Key::Str("name".to_owned()), Value::Str("leaf".to_owned()));
    let mut outer = Table::new();
    outer.insert(
        Key::Str("list".to_owned()),
        Value::array([Value::Number(1.0), Value::Table(inner)]),
    );
    outer.insert(Key::Int(9), Value::Number(9.0));
    let value = Value::Table(outer);
    host.set_global("probe", &value).unwrap();
    host.exec(
        "nested.lua",
        "assert(probe.list[2].name == 'leaf' and probe[9] == 9 and probe.list[2][1] == true)",
    )
    .unwrap();
    assert_eq!(host.get_global("probe").unwrap(), value);
}

#[test]
fn shared_tables_are_copied_not_rejected() {
    let _guard = serial();
    let data = TempDir::new("shared");
    let host = host(&data);
    let value = host
        .eval("shared.lua", "local s = { 1, 2 }; return { a = s, b = s }")
        .unwrap();
    let table = value.as_table().unwrap();
    assert_eq!(table.get(&Key::Str("a".into())), table.get(&Key::Str("b".into())));
}

fn nested_lua(levels: usize) -> String {
    // Builds `levels` tables, each one inside the previous one at key "n".
    format!("local t = {{}}; local cur = t; for _ = 2, {levels} do cur.n = {{}}; cur = cur.n end; return t")
}

fn depth_of(value: &Value) -> usize {
    let mut depth = 0;
    let mut current = value;
    while let Some(table) = current.as_table() {
        depth += 1;
        match table.get(&Key::Str("n".into())) {
            Some(next) => current = next,
            None => break,
        }
    }
    depth
}

#[test]
fn deep_nesting_converts_up_to_the_limit() {
    let _guard = serial();
    let data = TempDir::new("deep-ok");
    let host = host(&data);
    let value = host.eval("deep_ok.lua", &nested_lua(MAX_DEPTH)).unwrap();
    assert_eq!(depth_of(&value), MAX_DEPTH);
    host.set_global("probe", &value).unwrap();
    assert_eq!(host.get_global("probe").unwrap(), value);
}

#[test]
fn deep_nesting_past_the_limit_is_an_error() {
    let _guard = serial();
    let data = TempDir::new("deep-over");
    let host = host(&data);
    let message = convert_error(
        host.eval("deep_over.lua", &nested_lua(MAX_DEPTH + 1))
            .unwrap_err(),
    );
    assert!(message.contains("256"), "{message}");

    // The same limit applies to Kaia values that come from Rust.
    let mut value = Value::Nil;
    for _ in 0..(MAX_DEPTH + 1) {
        let mut table = Table::new();
        table.insert(Key::Str("n".into()), value);
        value = Value::Table(table);
    }
    let message = convert_error(host.set_global("probe", &value).unwrap_err());
    assert!(message.contains("256"), "{message}");
}

#[test]
fn self_containing_table_is_an_error() {
    let _guard = serial();
    let data = TempDir::new("cycle");
    let host = host(&data);
    let message = convert_error(
        host.eval("cycle.lua", "local t = {}; t.self = t; return t")
            .unwrap_err(),
    );
    assert!(message.contains("contains itself"), "{message}");
}

#[test]
fn functions_threads_and_userdata_are_errors() {
    let _guard = serial();
    let data = TempDir::new("opaque");
    let host = host(&data);
    let cases = [
        ("return function() end", "function"),
        ("return { callback = print }", "function"),
        ("return coroutine.create(function() end)", "thread"),
        ("return io.stdout", "userdata"),
        ("return ffi.new('int[1]')", "cdata"),
        ("return { [print] = 1 }", "function"),
    ];
    for (source, kind) in cases {
        let message = convert_error(host.eval("opaque.lua", source).unwrap_err());
        assert!(message.contains(kind), "{source}: {message}");
    }
}

#[test]
fn invalid_utf8_string_is_an_error() {
    let _guard = serial();
    let data = TempDir::new("utf8");
    let host = host(&data);
    let message = convert_error(host.eval("utf8.lua", "return string.char(255, 254)").unwrap_err());
    assert!(message.contains("UTF-8"), "{message}");
    // Such a string is fine inside Lua. Only the conversion refuses it.
    host.exec("utf8_inside.lua", "assert(#string.char(255) == 1)")
        .unwrap();
}

#[test]
fn integer_keys_outside_the_exact_range_are_errors() {
    let _guard = serial();
    let data = TempDir::new("big-key");
    let host = host(&data);
    let edge = 1_i64 << 53;
    let mut table = Table::new();
    table.insert(Key::Int(edge), Value::Bool(true));
    host.set_global("probe", &Value::Table(table)).unwrap();
    host.exec("edge.lua", "assert(probe[9007199254740992] == true)")
        .unwrap();

    let mut too_big = Table::new();
    too_big.insert(Key::Int(edge + 1), Value::Bool(true));
    let message = convert_error(host.set_global("probe", &Value::Table(too_big)).unwrap_err());
    assert!(message.contains("2^53"), "{message}");
}

#[test]
fn numbers_use_double_precision() {
    let _guard = serial();
    let data = TempDir::new("precision");
    let host = host(&data);
    // 2^60 is exact in a double.
    let big = Value::Number((1_u64 << 60) as f64);
    host.set_global("probe", &big).unwrap();
    assert_eq!(host.get_global("probe").unwrap(), big);
    assert_eq!(host.eval("int.lua", "return 7").unwrap().as_i64(), Some(7));
}
