//! KR-011 feasibility spike: runs KOReader's LuaJIT inside a Rust process.
//!
//! Usage: kaia-host-spike <driver.lua>
//!
//! Run it with the emulator's koreader/ directory as the working directory
//! (see run.sh). Environment:
//!   SPIKE_MODE=panic   only run the panic-in-callback check, then exit
//!   SPIKE_MODE=hold    run the driver, then drop the Lua handle while a
//!                      Table handle to a LuaSettings object is still held
//!   SPIKE_HOST_TESTS=0 skip the Rust-side error tests
//!
//! Not product code. Results: docs/kaia/spikes/KR-011-embedded-luajit.md.

use std::env;
use std::fs;
use std::hint::black_box;
use std::os::raw::c_int;
use std::panic;
use std::process::ExitCode;
use std::sync::OnceLock;
use std::time::Instant;

use mlua::{Function, Lua, Table, Value};

static START: OnceLock<Instant> = OnceLock::new();

fn elapsed_secs() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// C-linkage symbol. LuaJIT's ffi.C finds it only when the executable exports
/// its dynamic symbols (cargo feature `export-host-symbols`).
#[no_mangle]
pub extern "C" fn kaia_spike_host_add(a: c_int, b: c_int) -> c_int {
    a.wrapping_add(b)
}

fn register_host(lua: &Lua) -> mlua::Result<()> {
    let spike = lua.create_table()?;
    spike.set("host", "rust")?;
    spike.set("clock", lua.create_function(|_, ()| Ok(elapsed_secs()))?)?;
    spike.set("pid", lua.create_function(|_, ()| Ok(std::process::id()))?)?;
    spike.set("add", lua.create_function(|_, (a, b): (i64, i64)| Ok(a + b))?)?;
    spike.set(
        "fail",
        lua.create_function(|_, msg: String| -> mlua::Result<()> { Err(mlua::Error::RuntimeError(msg)) })?,
    )?;
    spike.set(
        "panic",
        lua.create_function(|_, ()| -> mlua::Result<()> {
            panic!("spike deliberate panic inside a Rust callback")
        })?,
    )?;
    spike.set(
        "reenter",
        lua.create_function(|_, f: Function| f.call::<Value>(()))?,
    )?;
    lua.globals().set("spike", spike)?;
    Ok(())
}

fn first_lines(text: &str, count: usize) -> String {
    text.lines().take(count).collect::<Vec<_>>().join(" | ")
}

/// Errors that Lua raises, syntax errors and Rust errors, as seen from Rust.
fn host_error_tests(lua: &Lua) {
    let cases: [(&str, &str); 8] = [
        (
            "error_with_traceback",
            "local function inner() error('deep failure') end\n\
             local function outer() inner() end\n\
             outer()",
        ),
        ("syntax_error", "local x = "),
        ("error_with_table_value", "error({ code = 7 })"),
        ("missing_module", "require('no.such.module')"),
        (
            "rust_error_rethrown_by_lua",
            "local ok, e = pcall(spike.fail, 'from rust'); error(e, 0)",
        ),
        (
            "rust_error_caught_by_lua",
            "local ok, e = pcall(spike.fail, 'from rust'); assert(not ok); return e",
        ),
        (
            "lua_error_inside_reentry",
            "spike.reenter(function() error('raised inside the callback') end)",
        ),
        (
            "reentry_rust_calls_lua",
            "local r = spike.reenter(function() return spike.add(2, 3) end); \
             assert(r == 5, 'reentry result ' .. tostring(r))",
        ),
    ];
    for (name, source) in cases {
        match lua.load(source).set_name(name).exec() {
            Ok(()) => println!("HOST-CHECK {name:<28} ok"),
            Err(err) => println!("HOST-CHECK {name:<28} err: {}", first_lines(&err.to_string(), 8)),
        }
    }
}

fn panic_test(lua: &Lua) {
    let outcome = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        lua.load(
            "local ok, e = pcall(spike.panic); \
             return tostring(ok) .. ' ' .. tostring(e)",
        )
        .eval::<String>()
    }));
    match outcome {
        Ok(Ok(text)) => println!("HOST-CHECK panic_in_callback        lua returned: {text}"),
        Ok(Err(err)) => println!(
            "HOST-CHECK panic_in_callback        mlua error: {}",
            first_lines(&err.to_string(), 4)
        ),
        Err(_) => println!("HOST-CHECK panic_in_callback        panic unwound out of lua call"),
    }
}

fn main() -> ExitCode {
    black_box(kaia_spike_host_add as *const () as usize);
    elapsed_secs();

    let args: Vec<String> = env::args().collect();
    let Some(driver_path) = args.get(1) else {
        eprintln!("usage: kaia-host-spike <driver.lua>");
        return ExitCode::from(2);
    };
    let mode = env::var("SPIKE_MODE").unwrap_or_default();

    let created = Instant::now();
    // SAFETY: the spike runs one Lua state on the main thread and does not hand
    // it to other threads. unsafe_new loads the debug and ffi libraries, which
    // KOReader needs. This matches the emulator's own luajit main().
    let lua = unsafe { Lua::unsafe_new() };
    let lua_new_ms = created.elapsed().as_secs_f64() * 1000.0;
    if let Err(err) = register_host(&lua) {
        eprintln!("HOST register failed: {err}");
        return ExitCode::from(1);
    }
    println!(
        "HOST-TIMING lua_new_ms={lua_new_ms:.3} host_clock_at_lua_ready={:.3}",
        elapsed_secs()
    );

    if mode == "panic" {
        panic_test(&lua);
        return ExitCode::SUCCESS;
    }
    if env::var("SPIKE_HOST_TESTS").as_deref() != Ok("0") {
        host_error_tests(&lua);
    }

    let source = match fs::read_to_string(driver_path) {
        Ok(text) => text,
        Err(err) => {
            eprintln!("cannot read driver {driver_path}: {err}");
            return ExitCode::from(2);
        }
    };
    let driver_started = Instant::now();
    let result = lua.load(&source).set_name("@driver.lua").exec();
    println!(
        "HOST-TIMING driver_ms={:.3} host_clock_at_exit={:.3}",
        driver_started.elapsed().as_secs_f64() * 1000.0,
        elapsed_secs()
    );
    let mut code = ExitCode::SUCCESS;
    if let Err(err) = &result {
        println!("HOST driver raised: {}", first_lines(&err.to_string(), 12));
        code = ExitCode::from(1);
    }

    if mode == "hold" && result.is_ok() {
        // The driver left KAIA_SPIKE_HELD (a LuaSettings object with a cdata
        // finalizer) in the globals. Keep one Table handle to it, drop the Lua
        // handle, then use the handle and drop it. Each step prints a marker.
        match lua.globals().get::<Table>("KAIA_SPIKE_HELD") {
            Ok(held) => {
                drop(lua);
                println!("HOST dropped Lua handle while Table handle is held");
                match held.get::<String>("file") {
                    Ok(file) => println!("HOST held table still readable, file={file}"),
                    Err(err) => println!("HOST held table unreadable: {err}"),
                }
                drop(held);
                println!("HOST dropped Table handle (last reference)");
                return code;
            }
            Err(err) => println!("HOST hold: no KAIA_SPIKE_HELD: {err}"),
        }
    }

    drop(lua);
    println!("HOST Lua state dropped");
    code
}
