# ADR-002: One coordinator, one LuaJIT runtime and an initial Rust C ABI

Status: Proposed

Date: 2026-10-13

## Context

The reader needs one owner for input dispatch, one owner for display submission and one owner for suspend and resume, at every migration stage. A Rust UI loop that runs next to the Lua display and power loop would compete with it.

Lua plugin code runs on one designated application thread. Background work uses bounded jobs with cancellation. Workers return owned results. They never call Lua objects directly. The plan does not add a multithreaded asynchronous runtime.

LuaJIT documents direct calls to C functions, and Rust documents C-compatible FFI. [S19, S20] A standalone executable that works on the Kobo does not show that a library works inside the existing process. A library built against musl must not load into the GNU libc process.

## Decision

Kaia runs one reader process. The existing KOReader coordinator keeps input, UI events, display and suspend.

Rust code ships as a native library, `libkaia`. LuaJIT loads it with `ffi.load`. The first boundary is the C ABI in `docs/kaia/specs/ABI.md`, version 1. The header is `rust/kaia-ffi/include/kaia.h`. Only fixed-width scalars, byte buffers and explicit structs cross the boundary. Lua calls Rust and receives a result. Rust does not call Lua in version 1.

Kaia uses the exact approved LuaJIT configuration. It does not add a second Lua runtime. Lua 5.4, Lua 5.5 and Luau do not replace LuaJIT.

Each exported Rust function contains its panics. No unwinding crosses the C ABI.

The candidate build target is `armv7-unknown-linux-gnueabihf`. It must be validated against the device before use. [S10, S23]

## Consequences

- Lua sends work to Rust and builds the UI from the result. Callbacks from Rust into Lua wait for later work, with explicit lifetime and reentrancy tests.
- If the Rust library is missing, or its ABI version is not 1, the Lua wrapper returns nil. Code that needs Rust stays off, and KOReader behaves like the baseline.
- Out-of-memory errors, native crashes and aborts are not ordinary errors. They go to the recovery path described in ADR-007.
- The Lua host does not change until the embedding spike publishes its results. The `mlua` crate is a candidate for the later host phase. It is not part of the first integration. [S21]

## Evidence needed to accept

- KR-008 needs repeated calls and allocation and free cycles that pass on the device, with no leaked allocations and no uncontrolled ABI exceptions. Disabling the component restores baseline behavior.
- KR-009 needs tests for bad inputs, large buffers, stale handles, cancellation and panic conversion. Normal errors keep the process and user state. Reviewers approve each unsafe boundary. The required tests in `docs/kaia/specs/ABI.md` pass.
- KR-011 publishes the results and failure modes of the embedded LuaJIT spike.
- The device profile from KR-002 confirms the CPU, float ABI, dynamic loader and libc requirements that the library is built for.

## Sources

- S10: KOReader toolchains. https://github.com/koreader/koxtoolchain ; https://github.com/NiLuJe/koxtoolchain
- S19: LuaJIT FFI. https://luajit.org/ext_ffi.html
- S20: Rust FFI and unwinding. https://doc.rust-lang.org/nomicon/ffi.html
- S21: mlua configuration. https://github.com/mlua-rs/mlua
- S22: LuaJIT compatibility. https://luajit.org/extensions.html
- S23: Rust supported targets. https://doc.rust-lang.org/stable/rustc/platform-support.html
