//! The Lua host: one LuaJIT state with KOReader's module paths and startup.

use std::ffi::c_void;
use std::fs;
use std::marker::PhantomData;
use std::panic::{self, AssertUnwindSafe};

use kaia_events::Value;
use mlua::{Lua, MultiValue};

use crate::convert;
use crate::error::lua_error;
use crate::{Error, LuaPaths};

/// Startup for the host. See `bootstrap.lua`.
const BOOTSTRAP: &str = include_str!("bootstrap.lua");

/// Wraps a registered Rust function so that a failed call raises a plain
/// string error. The argument is the Rust function.
const CALLBACK_WRAPPER: &str = "local raw = ...
return function(...)
    local ok, result = raw(...)
    if not ok then
        error(result, 0)
    end
    return result
end";

/// One LuaJIT state, set up the way KOReader's `setupkoenv.lua` and the
/// non-UI part of `reader.lua` set it up.
///
/// The host runs on the thread that created it. It is not `Send`. A Rust
/// callback registered with [`LuaHost::register`] runs on that thread too.
///
/// ```compile_fail
/// fn assert_send<T: Send>() {}
/// assert_send::<kaia_lua::LuaHost>();
/// ```
pub struct LuaHost {
    lua: Lua,
    paths: LuaPaths,
    // The mlua state is not Send. This marker keeps the host !Send if mlua's
    // feature set changes.
    _not_send: PhantomData<*const c_void>,
}

impl LuaHost {
    /// Creates a host. This sets the search paths, loads `ffi/loadlib`, and sets
    /// the globals `KAIA_PATHS`, `G_defaults` and `G_reader_settings`. It does
    /// not run `reader.lua` and loads no UI module.
    pub fn new(paths: LuaPaths) -> Result<Self, Error> {
        paths.check()?;
        fs::create_dir_all(&paths.data_dir)
            .map_err(|e| Error::Init(format!("cannot create {}: {e}", paths.data_dir.display())))?;

        // SAFETY: `unsafe_new` loads the `debug` and `ffi` libraries. KOReader's
        // modules need both (`ffi.load`, `debug.traceback`), and KOReader's own
        // luajit executable loads them too. The state is created here, used
        // only on this thread (the host is !Send) and closed when the host is
        // dropped. No table or function handle outlives the host.
        let lua = unsafe { Lua::unsafe_new() };
        let host = LuaHost {
            lua,
            paths,
            _not_send: PhantomData,
        };
        host.bootstrap()?;
        Ok(host)
    }

    /// The directories this host was created with.
    pub fn paths(&self) -> &LuaPaths {
        &self.paths
    }

    fn bootstrap(&self) -> Result<(), Error> {
        let shim = path_text(&self.paths.shim_dir)?;
        let koreader = path_text(&self.paths.koreader_dir)?;
        let data = path_text(&self.paths.data_dir)?;
        self.lua
            .load(BOOTSTRAP)
            .set_name("@kaia/bootstrap.lua")
            .call::<mlua::Value>((shim, koreader, data))
            .map(|_| ())
            .map_err(|e| Error::Init(e.to_string()))
    }

    /// Runs a chunk. `name` is the chunk name in error messages, for example
    /// `"test.lua"`. A runtime error or a syntax error comes back as
    /// [`Error::Lua`], with the Lua traceback in the message.
    pub fn exec(&self, name: &str, source: &str) -> Result<(), Error> {
        self.lua
            .load(source)
            .set_name(chunk_name(name))
            .exec()
            .map_err(lua_error)
    }

    /// Runs a chunk and converts the first value it returns. See `convert` for
    /// the rules.
    pub fn eval(&self, name: &str, source: &str) -> Result<Value, Error> {
        let value: mlua::Value = self
            .lua
            .load(source)
            .set_name(chunk_name(name))
            .eval()
            .map_err(lua_error)?;
        convert::from_lua(&value)
    }

    /// Loads a module with Lua's `require`. The module runs once per host, as in Lua.
    pub fn require(&self, module: &str) -> Result<(), Error> {
        let require: mlua::Function = self.lua.globals().get("require").map_err(lua_error)?;
        require
            .call::<mlua::Value>(module.to_owned())
            .map(|_| ())
            .map_err(lua_error)
    }

    /// Reads a global. Missing globals are `Value::Nil`.
    pub fn get_global(&self, name: &str) -> Result<Value, Error> {
        let value: mlua::Value = self.lua.globals().get(name).map_err(lua_error)?;
        convert::from_lua(&value)
    }

    /// Sets a global to a copy of `value`.
    pub fn set_global(&self, name: &str, value: &Value) -> Result<(), Error> {
        let lua_value = convert::to_lua(&self.lua, value)?;
        self.lua.globals().set(name, lua_value).map_err(lua_error)
    }

    /// Calls a global function with copies of `args` and converts the result.
    pub fn call_global(&self, name: &str, args: &[Value]) -> Result<Value, Error> {
        let function: mlua::Function = self.lua.globals().get(name).map_err(lua_error)?;
        let lua_args = args
            .iter()
            .map(|v| convert::to_lua(&self.lua, v))
            .collect::<Result<Vec<_>, _>>()?;
        let result: mlua::Value = function.call(MultiValue::from_vec(lua_args)).map_err(lua_error)?;
        convert::from_lua(&result)
    }

    /// Registers a global Rust function. Lua calls it with its arguments as
    /// Kaia values (see `convert` for what they can be), and it returns one
    /// Kaia value. An `Err` raises a Lua error with that text. A panic inside
    /// `f` raises a Lua error too, and no panic reaches a Lua frame.
    ///
    /// The Lua error is a plain string, so `pcall` returns a string that Lua
    /// code can concatenate, as KOReader's code does.
    pub fn register<F>(&self, name: &str, f: F) -> Result<(), Error>
    where
        F: Fn(&[Value]) -> Result<Value, String> + 'static,
    {
        let label = name.to_owned();
        // Returns (true, value) or (false, message). The wrapper turns the
        // second form into a Lua error, so mlua never wraps the message in an
        // error object.
        let raw = self
            .lua
            .create_function(move |lua, args: MultiValue| {
                let outcome = call_callback(&label, &f, &args)
                    .and_then(|value| convert::to_lua(lua, &value).map_err(|e| e.to_string()));
                match outcome {
                    Ok(value) => Ok((true, value)),
                    Err(message) => Ok((false, mlua::Value::String(lua.create_string(&message)?))),
                }
            })
            .map_err(lua_error)?;
        let wrapper: mlua::Function = self
            .lua
            .load(CALLBACK_WRAPPER)
            .set_name("@kaia/callback.lua")
            .call(raw)
            .map_err(lua_error)?;
        self.lua.globals().set(name, wrapper).map_err(lua_error)
    }
}

/// Runs a registered Rust function. Arguments that do not convert, and a
/// panic, are errors. The panic payload is turned into text here.
fn call_callback<F>(label: &str, f: &F, args: &MultiValue) -> Result<Value, String>
where
    F: Fn(&[Value]) -> Result<Value, String>,
{
    let values = args
        .iter()
        .map(convert::from_lua)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    match panic::catch_unwind(AssertUnwindSafe(|| f(&values))) {
        Ok(result) => result,
        Err(payload) => Err(format!(
            "Rust function {label} panicked: {}",
            panic_text(payload.as_ref())
        )),
    }
}

fn chunk_name(name: &str) -> String {
    format!("@{name}")
}

fn path_text(path: &std::path::Path) -> Result<String, Error> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Init(format!("path is not UTF-8: {}", path.display())))
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_owned()
    }
}
