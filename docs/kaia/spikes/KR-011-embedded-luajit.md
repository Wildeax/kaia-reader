# KR-011: Embedded LuaJIT host feasibility spike

Date: 2026-10-13
Status: measured on the desktop emulator (macOS 26.3, arm64). Not run on a Kobo or on any ARM Linux device.
Backlog: `docs/kaia/INITIAL_BACKLOG.md`, KR-011. Context: `docs/kaia/ENGINEERING_PLAN.md` section 4, `docs/kaia/adr/0002-one-coordinator-and-luajit-runtime.md`.
Code: `rust/experiments/kaia-host-spike/` (standalone Cargo project with its own `[workspace]`, not a member of `rust/`). No file under `base/`, `frontend/` or `plugins/` was changed. The product behavior is unchanged.

## Summary

- A Rust binary built with `mlua` 0.10.5 (feature `luajit`, no `vendored`) links KOReader's own LuaJIT, the `libs/libluajit.dylib` that the emulator's native modules bind to. The same binary runs the module checks in the plan: `setupkoenv`, `ffi/util`, `luasettings`, `docsettings`, device and CanvasContext setup, `kaia/rustlib` with `libkaia.dylib` (ABI 1), `kaia/store` (open, put, list, close), and the files of `plugins/hello.koplugin`.
- The same driver file, run under the emulator's own `./luajit`, gives the same results. The differences come from the host: an exported host symbol, a Rust callback, and the count of LuaJIT images in the process.
- Time to the ready point is about the same: median 0.22 s wall time for the host and 0.21 s for `./luajit`, with 10 runs each and one 0.38 s outlier in the host set. Peak RSS at ready is 126.9 MiB (host, dylib link) and 126.7 MiB (`./luajit`).
- Lua errors reach Rust as `mlua::Error::RuntimeError` with the Lua traceback. A Rust callback that panics becomes a Lua error, and the process continues. Closing the Lua state runs cdata finalizers. A Rust `Table` handle does not keep the state open: using it after the Lua handle is dropped panics with exit code 101.
- The baseline emulator already has two LuaJIT images in one process. The `./luajit` executable has its own static copy, and the first native module (`libs/libkoreader-lfs.so`) loads `libs/libluajit.dylib` as a second image. The ADR 0002 rule of one LuaJIT runtime is therefore not met by the current desktop build. The host can meet it with the dylib link, but not with the static archive.
- Recommendation: keep the Lua host. The spike found no startup or memory gain from a Rust host, and it adds a second entry point and a new unsafe-facing dependency. Details are in "Recommendation".

## Versions and artifacts

| Item | Value | Where it comes from |
|---|---|---|
| KOReader baseline | tag `v2026.07.1`, commit `9192014d8` | `CLAUDE.md` |
| LuaJIT upstream | commit `3c4f9fe2052b8d08a917ac0d5f38563f0297b5a3` | `base/thirdparty/luajit/CMakeLists.txt` (`external_project`) |
| LuaJIT version string | `2.1.1783773675` (`jit.version`) | `base/build/.../thirdparty/luajit/source/src/luajit_relver.txt` |
| KOReader LuaJIT patches | `koreader-luajit-makefile-tweaks.patch`, `koreader-luajit-enable-table_pack.patch` | `PATCH_FILES` in the CMake file. The `verbose-build` patch is in the directory but not in the list. |
| Emulator build flags | `CC=ccache clang`, `CFLAGS=-Og -g -pipe -fno-omit-frame-pointer ... -march=native -fPIC`, `LDFLAGS=-Wl,-rpath,@executable_path/libs,-rpath,@executable_path/../koreader/libs`, `TARGET_DYLIBPATH=@rpath/libluajit.dylib` | `base/build/.../thirdparty/luajit/log/install` |
| Macros | No `LUAJIT_ENABLE_LUA52COMPAT`. `table.pack` and `table.unpack` come from the patch. GC64 is on (`ffi.abi("gc64")` is true on arm64). | Makefile, runtime check |
| Rust | rustc and cargo 1.99.0 (the toolchain pinned in `rust/rust-toolchain.toml`) | `rustc --version` |
| mlua, mlua-sys | 0.10.5, 0.6.8 | `rust/experiments/kaia-host-spike/Cargo.lock` |
| libkaia | `rust/target/release/libkaia.dylib`, built by `tools/kaia/build-libkaia.sh host`, ABI 1 | script output, `kaia.rustlib` check |

LuaJIT artifacts in the desktop build:

| File (under `base/build/arm64-apple-darwin25.3.0-debug/`) | Bytes | md5 | Role |
|---|---|---|---|
| `libs/libluajit.dylib` | 642,352 | `85ac5b241aeb7bd54d285bb468edd0ab` | Install name `@rpath/libluajit.dylib`. The native modules `libs/libkoreader-lfs.so`, `libs/libkoreader-xtext.so` and `common/rapidjson.so` link to it. 148 global `lua*` symbols. |
| `staging/lib/libluajit.dylib` | 779,472 | `5484179017046724543a5e51c5752f3f` | Same version and 148 exported symbols, different bytes. The spike did not find out why (a strip or install step is the likely cause). Not used. |
| `staging/lib/libluajit-5.1.a` | 2,697,880 | `39aa3ef9e5867a0e7b99c07b1392d4ae` | Static archive. Identical to `thirdparty/luajit/source/src/libluajit.a`. Used by the static variant. |
| `luajit` (emulator executable, symlinked as `koreader/luajit`) | 641,944 | `d9a140501b2aad489946803a34093d1b` | LuaJIT linked into the executable. Links only `libSystem`. 150 global symbols in total. |

Full md5 for the staging dylib: `5484179017046724543a5e51c5752f3f`.

## Setup and commands

Build and run order (all from the repository root):

```sh
tools/kaia/build-libkaia.sh host                      # builds rust/ (already up to date), copies libkaia.dylib into the emulator libs/
rust/experiments/kaia-host-spike/build.sh dylib       # host linked to libs/libluajit.dylib
rust/experiments/kaia-host-spike/build.sh static      # host linked to staging/lib/libluajit-5.1.a, target-static/
rust/experiments/kaia-host-spike/run.sh host          # driver under the Rust host
rust/experiments/kaia-host-spike/run.sh luajit        # same driver under the emulator's ./luajit
rust/experiments/kaia-host-spike/measure.sh 10        # timing and RSS, 10 runs per variant
```

How the host is built:

- `build.sh` sets `LUA_LIB` to the LuaJIT directory and `LUA_LIB_NAME` to `luajit` (or `luajit-5.1` with `LUA_LINK=static`). mlua-sys 0.6.8 reads these in `build/find_normal.rs` and skips pkg-config when `LUA_LIB_NAME` is set. The `vendored` feature is not enabled, so mlua builds no second Lua.
- `build.rs` adds an `LC_RPATH` with the directory of `libluajit.dylib`, because the dylib's install name is `@rpath/libluajit.dylib`. The path is absolute and comes from `build.sh`. A product build would need install-time handling (not measured).
- The host calls `Lua::unsafe_new()`, which loads the `debug` and `ffi` libraries that KOReader needs. It registers a `spike` table (`clock`, `pid`, `add`, `fail`, `panic`, `reenter`) and then runs `lua/driver.lua` as chunk `@driver.lua`.
- `run.sh` changes to the emulator's `koreader/` directory, sets `KAIA_SPIKE_TMP` to a new scratch directory and `KAIA_LIBKAIA` to the built library, and deletes the scratch directory on exit. Both runtimes use the same environment.
- `lua/driver.lua` runs the same checks in both runtimes. It mirrors `reader.lua` lines 37 to 43 (`G_defaults`, `G_reader_settings`) and lines 163 to 167 (`device`, `CanvasContext:init`). `G_reader_settings` points to a scratch file, so the emulator's `settings.reader.lua` is not touched.

Lint and format results (run in `rust/experiments/kaia-host-spike/` and at the repository root):

```text
$ cargo fmt --check && echo "fmt clean"
fmt clean
$ cargo clippy --release --all-targets -- -D warnings
    Checking kaia-host-spike v0.0.0 (.../rust/experiments/kaia-host-spike)
    Finished `release` profile [optimized] target(s) in 1.41s
$ luacheck rust/experiments/kaia-host-spike/lua/driver.lua
Checking rust/experiments/kaia-host-spike/lua/driver.lua OK

Total: 0 warnings / 0 errors in 1 file
```

## Results

Final run, host (dylib link) and emulator `./luajit`, same driver. Full output is in the run logs; the lines below are copied from them.

| Plan item | Host (Rust, dylib) | `./luajit` | Notes |
|---|---|---|---|
| Runtime | PASS. LuaJIT 2.1.1783773675, arm64, GC64, `table.pack` present | PASS, same values | |
| Module search path | NOTE. `package.path` starts with `common/?.lua;frontend/?.lua;plugins/exporter.koplugin/?.lua`. `package.cpath` starts with `common/?.so;common/?.dll;/usr/lib/lua/?.so`, then LuaJIT's default `./?.so`. `require("libs/libkoreader-lfs")` resolves to `./libs/libkoreader-lfs.so`. | same | Resolution is relative to the working directory, so the host must run from `koreader/`. |
| `setupkoenv` | PASS. `ffi/loadlib` loaded, `ffi.load` overridden | PASS | `ffi/loadlib` prints the monolibtic probe and `lib_search_path`, and on macOS it runs `brew --prefix` through `io.popen`. |
| `ffi/util` | PASS | PASS | `copyFile` returns nil on success and an error string on failure. |
| `ffi.C` sees host executable symbols | PASS. `kaia_spike_host_add(2,3) = 5` | NOTE. Not visible (expected) | On macOS the executable exports its global symbols without a flag. `dyld_info -exports` lists `_kaia_spike_host_add`. |
| `ffi.C` sees a library loaded later, local load | NOTE. `ffi.load("libs/libkoreader-lfs.so")` works. `ffi.C.luaopen_lfs` is not visible. | same | `ffi.load` without `global` uses RTLD_LOCAL. Access through the returned handle (`lib.luaopen_lfs`) is the way to reach it. |
| `ffi.C` sees a library loaded later, global load | NOTE. `ffi.load("libs/libkoreader-xtext.so", true)`, then `ffi.C.luaopen_xtext` is visible | same | |
| `require("libs/libkoreader-lfs")` | PASS. LuaFileSystem 1.9.0. One `libluajit` image in the process. | PASS. One image after the load, zero before | See "Two LuaJIT images". |
| `luasettings` | PASS. Write, flush, reopen, read back, nested tables. | PASS | |
| `docsettings` | PASS. Sidecar `leaves.sdr/metadata.epub.lua` next to a scratch copy of `test/leaves.epub`. Round trip OK. | PASS | Sample EPUB is `test/leaves.epub`. The driver copies it to a scratch directory first. |
| `ui/size` | PASS. `Size.line.thin = 1` after the device bootstrap | PASS | The plan wrote `frontend/ui/size`. That is not a valid module name under `package.path`. The name is `ui/size`. |
| `device` | NOTE. `model=Emulator`, `isEmulator=true`, `isDesktop=false`. Loading it starts SDL3 with the cocoa driver. | same | SDL3 is loaded from `/opt/homebrew/lib/libSDL3.0.dylib`, which is outside the repository. |
| `kaia/rustlib` | PASS. `abi=1`, expected 1. Loaded from `KAIA_LIBKAIA`. | PASS | |
| `kaia/store` | NOTE. Open, `listAll`, `put`, `close` work. A call after close returns code -2 (BAD_HANDLE). | same | `put` needs `doc` as a table `{partial_md5, file_size, title}`. A string gives code -7 (VALIDATION). `Store.defaultDir()` returns `./kaia`, which is relative to the working directory. |
| `kaia.quote_locate` | NOTE. Returns offset 10 for the sample text. | same | Not checked against the expected result. |
| `plugins/hello.koplugin/_meta.lua` | PASS. `fullname = "Hello"` | PASS | |
| `plugins/hello.koplugin/main.lua` as shipped | PASS. Returns `{disabled = true}` | PASS | The guard `if true then return ... end` comes before every `require`, so the shipped file needs none of the modules below. |
| Same file with the guard removed | NOTE. `dispatcher`, `ui/widget/infomessage`, `ui/uimanager`, `ui/widget/container/widgetcontainer` and `gettext` all load after the bootstrap. The chunk returns a table. | same | Before the device and CanvasContext bootstrap was added, `dispatcher` failed in `frontend/fontlist.lua:93` (`CanvasContext:isKindle` is nil). |
| Lua error to Rust | PASS. `error('deep failure')` arrives as `runtime error: ... deep failure | stack traceback: ...` with the frames `inner`, `outer` and main chunk. | Not run on `./luajit` (host-only test). | |
| Syntax error to Rust | PASS. `syntax error: [string "syntax_error"]:1: unexpected symbol near '<eof>'` | Not run | |
| Non-string error value | NOTE. `error({code = 7})` arrives as `runtime error: table: 0x...` | PASS in Lua (`pcall` keeps the table) | The table identity is lost once the error crosses into Rust. |
| Missing module | PASS. Message lists every searched path. | PASS | |
| Rust error through Lua `pcall` | PASS. Lua can catch it, or rethrow it, and Rust gets the message with `[C]: in function 'spike.fail'` in the traceback. | Not run | |
| Lua error inside a Rust callback | PASS. `spike.reenter(function() error(...) end)` returns an error to Rust, traceback includes `[C]: in function 'reenter'`. | Not run | |
| Rust callback calls back into Lua | PASS. `spike.reenter(f)` returns 5. | SKIP (needs the host) | Synchronous reentry only. |
| Rust panic inside a callback | PASS. Lua `pcall` gets `false, "spike deliberate panic inside a Rust callback"`. The process exits with 0. The panic message is printed to stderr. | Not run | Tested in `SPIKE_MODE=panic` (separate run). |
| Cdata finalizers | PASS. 1000 of 1000 finalizers run after two `collectgarbage()` calls. | PASS | |
| Table churn and memory return | PASS. See the GC table below. | PASS | The Lua heap ends about 0.7 MB above its start in both. RSS does not return to the start level in the host (see "Risks"). |
| Global environment after bootstrap | NOTE. Globals added: `G_defaults`, `G_reader_settings`, `json`, `lfs`, `lpeg`. | same | `lfs`, `json` and `lpeg` appear as globals after their modules load. The spike did not trace which module sets each one. |
| Userdata lifetime, Lua state closed | PASS. A cdata finalizer attached to a held `LuaSettings` runs when the Lua handle is dropped. | PASS. The finalizer runs at process exit. | |
| Userdata lifetime, Table handle kept after drop | Exit code 101. `Lua instance is destroyed` panic at `mlua-0.10.5/src/table.rs:542`. | Not applicable | mlua does not keep the state alive for handles. The use-after-drop fails loudly and does not corrupt memory. |

Summary lines from the final runs:

```text
SUMMARY host=rust-host pass=20 fail=0 note=9 skip=0
SUMMARY host=emulator-luajit pass=18 fail=0 note=10 skip=1
```

## Numbers

Timing with `measure.sh 10` (10 runs per row). Wall time comes from `/usr/bin/time -l`. The host ready clock is the time from the start of `main` to the READY line, reported by the host. RSS is the `maximum resident set size` from `/usr/bin/time -l`.

| Variant | Wall time to ready, min to max (median) | Host ready clock, median | Peak RSS at ready, median | RSS of an empty script |
|---|---|---|---|---|
| Host, dylib link | 0.21 to 0.38 s (0.22) | 0.208 s | 126.9 MiB | 2.1 MiB |
| Host, static archive | 0.21 to 0.29 s (0.22) | 0.210 s | 127.0 MiB | 2.1 MiB |
| Emulator `./luajit` | 0.21 to 0.23 s (0.21) | n/a | 126.7 MiB | 1.6 MiB |

The empty script takes under 10 ms on both (`/usr/bin/time` shows `0.00 real`). Lua state creation in the host took 0.058 to 0.089 ms over five runs.

"Ready" means `setupkoenv`, the reader globals, and the device and CanvasContext bootstrap have run. Most of the time is spent there, in SDL, fonts and KOReader modules. The spike did not split that time further. The Lua state itself is cheap.

Table churn (`gc.table_churn`): five rounds of 200,000 small tables, each round followed by `collectgarbage("collect")`. Three runs per runtime. Values are KB as printed by the driver (`ps -o rss=`), and `lua_kb` is `collectgarbage("count")`.

| Runtime | Lua heap before, KB | Lua heap after, KB | RSS before, KB | RSS peak, KB | RSS after, KB |
|---|---|---|---|---|---|
| Host, run 1 | 5180 | 5949 | 140640 | 254976 | 252912 |
| Host, run 2 | 5186 | 5956 | 140800 | 255120 | 253056 |
| Host, run 3 | 5192 | 5961 | 140880 | 258560 | 256496 |
| `./luajit`, run 1 | 5348 | 5949 | 139808 | 253312 | 139920 |
| `./luajit`, run 2 | 5348 | 5949 | 139856 | 256688 | 146112 |
| `./luajit`, run 3 | 5348 | 5948 | 139760 | 256592 | 143200 |

The Lua heap ends at 5.9 MB in both runtimes, about 0.7 MB above the start. RSS does not. After the collection, the host keeps about 253 to 256 MB, and `./luajit` returns to about 140 to 146 MB. The cause is not isolated (see "Risks").

## Problems and failure modes

1. Module name in the plan. `require("frontend/ui/size")` does not resolve, because `package.path` adds `frontend/` as a prefix. The name is `require("ui/size")`.
2. `util.copyFile` returns nil on success. Checking its return value with `assert` fails the check. The driver checks for an error string.
3. The store's `doc` field is a table (`DocumentId`), not a string. A string gives validation error -7.
4. Bootstrap order. `require("dispatcher")` fails in `fontlist.lua:93` when `document/canvascontext` has not been initialized. The first host run had loaded `device` but had not called `CanvasContext:init`. `reader.lua` does this at lines 163 to 167. A host has to copy that order, or `CanvasContext` will be nil where modules expect it.
5. The hello plugin does nothing when loaded as shipped. Its `main.lua` returns before its `require` calls. The `WidgetContainer` and `UIManager` requires only run when the guard is removed, and then they load after the bootstrap.
6. Non-string Lua errors lose their value. `error({code = 7})` arrives in Rust as the text `table: 0x...`. Code that needs structured errors must pass a string, or use a Lua-side convention.
7. Use after drop. Dropping the `Lua` handle closes the state at once, even while a `Table` handle exists. The next use of that `Table` panics with `Lua instance is destroyed` and the process exits with code 101. The state is not corrupted, but a product host must make sure no handle outlives its state.
8. Local `ffi.load` hides symbols. `ffi.C` cannot see symbols of a library loaded with `ffi.load(path)`. Use `ffi.load(path, true)`, or call through the returned handle. The KOReader loader uses the local form, so this matters for any `ffi.C` lookup of native module symbols.
9. The device needs SDL3 from Homebrew. `require("device")` loads `/opt/homebrew/lib/libSDL3.0.dylib` and starts SDL with the cocoa video driver. This is the same on the emulator. The spike did not inspect the window. Any host on macOS needs SDL3 installed, which the product build does not state.
10. Relative paths. `require("libs/...")`, `setupkoenv`, `Store.defaultDir()` (`./kaia`) and the module search all depend on the working directory. The host must `chdir` to the KOReader directory, as `run.sh` does.
11. `libkaia.dylib` install name. The library built by `tools/kaia/build-libkaia.sh host` has the install name `/Users/.../rust/target/release/deps/libkaia.dylib`. That does not stop `ffi.load` by path, but it would break any binary that links the library by install name. Not a spike result, but a packaging note.
12. Panics in Rust callbacks. mlua catches them and returns them to Lua as an error. The test used one callback in one state. Panics that cross the `libkaia` C ABI were not tested.

## Two LuaJIT images

`DYLD_PRINT_LIBRARIES=1` on the emulator's `./luajit` after `require("libs/libkoreader-lfs")` shows:

```text
dyld[...]: .../base/build/arm64-apple-darwin25.3.0-debug/luajit
dyld[...]: .../base/build/arm64-apple-darwin25.3.0-debug/libs/libkoreader-lfs.so
dyld[...]: .../base/build/arm64-apple-darwin25.3.0-debug/libs/libluajit.dylib
```

The executable and the dylib both contain LuaJIT. Both expose `lua_*` symbols. The count of `libluajit` images in the process was 0 before the lfs load and 1 after it, in the `./luajit` run.

The two variants of the host:

- dylib link: 1 `libluajit` image at start (the one the host links). The lfs load adds no second copy. This is the only variant with one runtime.
- static archive: 0 images named `libluajit` at start, because the LuaJIT code is inside the host binary. The lfs load adds `libs/libluajit.dylib`, so the process holds two copies.

The spike ran the lfs `attributes` call and the `docsettings` and `luasettings` paths in the static variant, and they worked. Two copies of LuaJIT with shared state is a risk to design out, not a failure seen in these tests.

## Risks

- One runtime. ADR 0002 requires one LuaJIT runtime. The baseline emulator violates that already (see above), and the spike did not check the Kobo build. Any Rust host that uses the static archive adds a second copy. The dylib link avoids that on the desktop only.
- Callbacks and reentrancy. Synchronous Rust-to-Lua calls work, and a Lua error inside one propagates with its traceback. Not measured: Lua callbacks run later from a timer or a worker thread, Lua yields across Rust frames, and nested callbacks over many frames. The one-coordinator rule in ADR 0002 says Lua runs on one thread. The spike used one thread. It did not test whether mlua's `Lua` type rejects use from another thread, because the `send` feature was left off and no compile check was written.
- Allocator. mlua creates the state with `lua_newstate(ALLOCATOR, ...)` (`mlua-0.10.5/src/state/raw.rs`), so the Lua heap uses Rust's global allocator. The emulator's `luaL_newstate` uses LuaJIT's `LJ_ALLOCF_INTERNAL` allocator (`lib_aux.c`, `lj_state.c`). The table churn shows the RSS difference after collection (host 253 to 256 MB, `./luajit` 140 to 146 MB). That the allocator causes it is a hypothesis. Not measured: the same run with the allocator swapped.
- Memory limits. mlua's allocator can enforce a memory limit. Not measured.
- Out of memory, native crashes and aborts. Not tested. ADR 0002 sends them to the recovery path. The spike did not exercise that path.
- Error values. Structured errors cross into Rust as text. Callers need a convention.
- Handle lifetime. Use after drop panics. Code that holds `Table` or `Function` handles across a state drop needs a review rule.
- Process ownership. The host runs the bootstrap. It did not start `UIManager`, the reader's event loop, input, display or suspend. The one-coordinator question is therefore not tested. A Rust-owned main loop would compete with the Lua one, and this spike does not show how to avoid that.
- Environment dependencies. SDL3 from Homebrew, and absolute rpath and paths to the build directory, were used. None of these is in the product plan.
- Platform. Everything here is macOS arm64. The ELF and `armv7-unknown-linux-gnueabihf` behavior (`dlsym` visibility without `-rdynamic`, RTLD flags, the loader) was not measured.

## Recommendation

Keep the Lua host. `reader.lua` stays the entry point.

The spike shows that mlua can load KOReader's LuaJIT and run its modules, with clear errors and no crashes in the tested paths. It shows no benefit yet. Startup and peak RSS match `./luajit`. The Rust host would need its own copy of the `reader.lua` bootstrap, its own rpath or install-time loader setup, and a review of every `unsafe` path. The static variant would add a second LuaJIT image, which the baseline already has on the desktop. The one-coordinator rule is untested, and an unmanaged Rust loop would break it.

Do not start a Rust host replacement on the strength of this spike. If a later milestone needs a Rust-owned process loop, take it to the KR-016 go or no-go with device numbers: ready time, RSS, the LuaJIT image count on the device build, and a test of the coordinator rule with UIManager running. Until then, `libkaia` stays a library that Lua calls through the ABI in `docs/kaia/specs/ABI.md`.

The `mlua` crate is still a possible tool for host-side tests of `libkaia`, which can run without the device. This was not measured here.

## Not measured

- Kobo hardware, ARM Linux, the `armv7-unknown-linux-gnueabihf` build, glibc symbol versions, and the Kobo LuaJIT build.
- Full reader startup: `UIManager`, the event loop, input, display, suspend, and any plugin run past its `require` calls.
- Lua callbacks from other threads, timers, yields across Rust frames, and `Send`/`Sync` behavior of `mlua::Lua` (no compile test).
- Allocator swap (the RSS cause is a hypothesis), memory limits, out-of-memory, native crashes and aborts.
- Performance of Lua code beyond startup and the table churn loop.
- Rust panics crossing the `libkaia` C ABI. Only one callback panic was tested, in a separate run.
- The reason the two LuaJIT dylibs differ in bytes.
- Whether the emulator window opened in the SDL device check. Only the log line was recorded.

## Files

- `rust/experiments/kaia-host-spike/Cargo.toml`, `Cargo.lock`, `build.rs`, `src/main.rs`: the host.
- `rust/experiments/kaia-host-spike/lua/driver.lua`: the checks (run in both runtimes).
- `rust/experiments/kaia-host-spike/build.sh`, `run.sh`, `measure.sh`: build, run and timing.
- `rust/experiments/kaia-host-spike/.gitignore`: ignores `target/` and `target-static/`.
- `docs/kaia/spikes/KR-011-embedded-luajit.md`: this file.
