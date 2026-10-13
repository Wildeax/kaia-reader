# Kaia Reader

Kaia Reader is a fork of KOReader (baseline tag `v2026.07.1`, commit `9192014d8`) for the Kobo Libra Colour. The fork keeps KOReader's Lua frontend, LuaJIT runtime, document engines and Kobo integration, and adds a Rust library (`libkaia`) called through LuaJIT FFI. Read `docs/kaia/ENGINEERING_PLAN.md` and `docs/kaia/INITIAL_BACKLOG.md` before changing architecture.

## Layout

- `frontend/`, `plugins/`, `base/`, `platform/`: upstream KOReader. Change them only when a Kaia task needs it, and keep each change small so upstream merges stay easy. No mass reformatting or renames.
- `frontend/kaia/`: Kaia Lua modules (FFI binding, theme, home, notes, tracing).
- `plugins/kaia*.koplugin/`: Kaia plugins.
- `rust/`: Cargo workspace (`kaia-core`, `kaia-storage`, `kaia-ffi`, `kaia-compat`).
- `tools/kaia/`: Kaia scripts (inventories, packaging, device helpers).
- `docs/kaia/`: plan, specs, ADRs, inventories, device procedures.
- `spec/unit/kaia_*_spec.lua`: Kaia Lua tests (busted, run by `./kodev test front kaia_<name>`).

## Contracts

- C ABI: `docs/kaia/specs/ABI.md`. Header: `rust/kaia-ffi/include/kaia.h`.
- Storage: `docs/kaia/specs/STORAGE.md`.
- One owner per data class. Never write KOReader sidecars or settings from Rust.

## Build and test

```sh
source tools/kaia/env.sh             # PATH for Homebrew GNU tools and cargo on macOS
./kodev build                        # desktop emulator, output in koreader-emulator-*/koreader
./kodev run                          # start the emulator
./kodev test front kaia_store        # one Lua spec
cd rust && cargo test --workspace    # Rust tests
cd rust && cargo clippy --workspace --all-targets -- -D warnings
luacheck frontend/kaia plugins/kaia*.koplugin
```

## Writing style

All prose, comments, commit messages, docs and UI strings follow the unslop rules in `~/.claude/skills/unslop/SKILL.md`. In short: no em dashes, no AI vocabulary (delve, crucial, robust, seamless, leverage, showcase, enhance), sentence case headings, no decorative emoji, plain words, active voice, concrete facts instead of feelings.

UI strings go through gettext: `local _ = require("gettext")`.

## Rules for worker agents

- Edit only the paths your task assigns to you. If you need a change elsewhere, stop and report it.
- Do not run `git commit`, `git push`, `git checkout`, `git stash` or `git reset`. The orchestrator commits.
- Run the tests and linters for what you changed and paste the real output in your report. Do not claim a test passed without running it.
- Do not install system packages. Cargo crates from crates.io are allowed in `rust/`.
- Do not add network calls, telemetry or downloads to the product.
- Report: files changed, commands run with results, anything unfinished or uncertain.
