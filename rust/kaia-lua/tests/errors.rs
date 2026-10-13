//! Errors between Rust and Lua: tracebacks, Rust callbacks, panics.

mod common;

use common::{host, serial, TempDir};
use kaia_events::value::Value;
use kaia_lua::Error;

fn lua_error(err: Error) -> String {
    match err {
        Error::Lua(message) => message,
        other => panic!("expected a Lua error, got {other}"),
    }
}

#[test]
fn runtime_error_carries_the_traceback() {
    let _guard = serial();
    let data = TempDir::new("traceback");
    let host = host(&data);
    let source = "local function inner() error('deep failure') end\n\
                  local function outer() inner() end\n\
                  outer()";
    let message = lua_error(host.exec("traceback.lua", source).unwrap_err());
    assert!(message.contains("deep failure"), "{message}");
    assert!(message.contains("stack traceback"), "{message}");
    assert!(message.contains("inner"), "{message}");
    assert!(message.contains("outer"), "{message}");
    assert!(message.contains("traceback.lua"), "{message}");
}

#[test]
fn syntax_error_names_the_chunk() {
    let _guard = serial();
    let data = TempDir::new("syntax");
    let host = host(&data);
    let message = lua_error(host.exec("broken.lua", "local x = ").unwrap_err());
    assert!(message.contains("broken.lua"), "{message}");
    assert!(message.contains("unexpected"), "{message}");
    let message = lua_error(host.eval("broken_eval.lua", "return (").unwrap_err());
    assert!(message.contains("broken_eval.lua"), "{message}");
}

#[test]
fn non_string_error_value_is_reported_as_text() {
    let _guard = serial();
    let data = TempDir::new("table-error");
    let host = host(&data);
    let message = lua_error(host.exec("table_error.lua", "error({ code = 7 })").unwrap_err());
    assert!(message.contains("table"), "{message}");
}

#[test]
fn host_keeps_working_after_an_error() {
    let _guard = serial();
    let data = TempDir::new("recover");
    let host = host(&data);
    assert!(host.exec("fail.lua", "error('first')").is_err());
    assert!(host.eval("fail2.lua", "return nil.x").is_err());
    assert_eq!(host.eval("ok.lua", "return 1 + 1").unwrap(), Value::Number(2.0));
}

#[test]
fn missing_module_is_a_lua_error() {
    let _guard = serial();
    let data = TempDir::new("missing-module");
    let host = host(&data);
    let message = lua_error(host.require("no.such.module").unwrap_err());
    assert!(message.contains("no.such.module"), "{message}");
}

#[test]
fn rust_error_is_catchable_by_lua() {
    let _guard = serial();
    let data = TempDir::new("rust-error");
    let host = host(&data);
    host.register("fail", |_args| Err("refused by Rust".to_owned()))
        .unwrap();
    host.exec("catch.lua", "ok, msg = pcall(fail)").unwrap();
    assert_eq!(host.get_global("ok").unwrap(), Value::Bool(false));
    let msg = host.get_global("msg").unwrap();
    assert!(msg.as_str().unwrap().contains("refused by Rust"), "{msg:?}");
    // Uncaught, the same error reaches Rust as a Lua error.
    let message = lua_error(host.exec("uncaught.lua", "fail()").unwrap_err());
    assert!(message.contains("refused by Rust"), "{message}");
}

#[test]
fn rust_callback_panic_becomes_a_lua_error() {
    let _guard = serial();
    let data = TempDir::new("panic");
    let host = host(&data);
    host.register("explode", |_args| panic!("deliberate test panic"))
        .unwrap();
    // Caught by Lua: the panic never reaches a Rust frame.
    let text = host
        .eval(
            "panic_caught.lua",
            "local ok, e = pcall(explode); return tostring(ok) .. ' ' .. e",
        )
        .unwrap();
    let text = text.as_str().unwrap().to_owned();
    assert!(text.starts_with("false "), "{text}");
    assert!(text.contains("deliberate test panic"), "{text}");
    // Uncaught: exec returns the error, and the caller is not unwound.
    let message = lua_error(host.exec("panic_uncaught.lua", "explode()").unwrap_err());
    assert!(message.contains("panicked"), "{message}");
    assert!(message.contains("deliberate test panic"), "{message}");
    // The host still runs chunks afterwards.
    assert_eq!(
        host.eval("after.lua", "return 'alive'").unwrap().as_str(),
        Some("alive")
    );
}

#[test]
fn callbacks_receive_and_return_kaia_values() {
    let _guard = serial();
    let data = TempDir::new("callback-values");
    let host = host(&data);
    host.register("total", |args| {
        let mut sum = 0.0;
        for arg in args {
            match arg {
                Value::Number(n) => sum += n,
                Value::Table(t) => {
                    for v in t.values() {
                        if let Value::Number(n) = v {
                            sum += n;
                        }
                    }
                }
                other => return Err(format!("unexpected argument {other:?}")),
            }
        }
        Ok(Value::Number(sum))
    })
    .unwrap();
    assert_eq!(
        host.eval("total.lua", "return total(1, 2, {10, 20})").unwrap(),
        Value::Number(33.0)
    );
    let message = lua_error(host.exec("bad_arg.lua", "total('text')").unwrap_err());
    assert!(message.contains("unexpected argument"), "{message}");
    // A function argument cannot convert, and Lua sees that as an error.
    let message = lua_error(host.exec("fn_arg.lua", "total(print)").unwrap_err());
    assert!(message.contains("function"), "{message}");
}

#[test]
fn call_global_runs_lua_functions() {
    let _guard = serial();
    let data = TempDir::new("call-global");
    let host = host(&data);
    host.exec("greet.lua", "function greet(name) return 'hi ' .. name end")
        .unwrap();
    let out = host
        .call_global("greet", &[Value::Str("kaia".to_owned())])
        .unwrap();
    assert_eq!(out, Value::Str("hi kaia".to_owned()));

    host.exec("boom.lua", "function boom() error('inside boom') end")
        .unwrap();
    let message = lua_error(host.call_global("boom", &[]).unwrap_err());
    assert!(message.contains("inside boom"), "{message}");
    assert!(message.contains("stack traceback"), "{message}");

    let message = lua_error(host.call_global("no_such_function", &[]).unwrap_err());
    assert!(!message.is_empty());
}

#[test]
fn set_and_get_global_round_trip_through_lua() {
    let _guard = serial();
    let data = TempDir::new("globals-io");
    let host = host(&data);
    host.set_global("answer", &Value::Number(42.0)).unwrap();
    host.exec("double.lua", "answer = answer * 2").unwrap();
    assert_eq!(host.get_global("answer").unwrap(), Value::Number(84.0));
    assert_eq!(host.get_global("never_set").unwrap(), Value::Nil);
}
