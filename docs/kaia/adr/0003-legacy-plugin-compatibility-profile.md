# ADR-003: Explicit, versioned legacy plugin compatibility profile

Status: Proposed

Date: 2026-10-13

## Context

Running the existing Lua environment does not show that plugins work. KOReader's example plugin calls widgets, UIManager and the menu system directly. [S2, S3, S5] Plugins also use document methods, device services, settings and native FFI. [S24] Searching for `require()` finds only part of this. Dynamic Lua behavior needs runtime tracing as well.

The plan rejects the claim that all Lua executes, so all plugins work. Kaia can make a narrower claim that a test suite supports.

Community work exists for stylus input and stickers. [S16, S17] Kaia must audit that code and its pinned versions before it relies on it.

## Decision

Kaia publishes a versioned compatibility profile. The profile uses five classes.

- Applicable bundled plugins are release-blocking tests at the pinned upstream revision. Any exception is explained, including disabled, debug-only and platform-inapplicable plugins.
- Selected community plugins are listed with their repository, exact version or commit, tested workflows and known limitations.
- Plugins that use shared widgets keep their API and semantics. They adopt the Kaia appearance where the widgets allow it.
- Plugins that draw custom UI or depend deeply on internals keep their legacy code paths until an adapter exists and passes tests.
- Arbitrary user patches and UI replacement plugins get best-effort support. Known conflicts get detected. Kaia does not claim support for them.

Each plugin gets an inventory of module imports, global access, UI class extension, menu registration, event ordering, timers, document methods, native FFI and file access. The inventory combines static scans and runtime traces.

Compatibility tests run real workflows. The workflows are: enable, configure, open a book, trigger commands, show dialogs, change settings, save, suspend, resume, close and reopen. Each result is compared with stock KOReader.

Kaia adds a versioned Kaia Lua API for declarative menus, commands, settings, dialogs and annotation overlays. The legacy interface stays. Plugin authors adopt the new API when they choose.

## Consequences

- Kaia makes narrow compatibility claims. Each claim has a test behind it.
- Each supported plugin version adds maintenance and test work.
- Plugins that keep custom drawing may look older than the rest of the reader. Functional compatibility and visual consistency are tested as separate dimensions.
- Kaia does not provide a plugin marketplace in version 1. It does not download plugins silently, and it does not run code from book content automatically.
- The compatibility inventory lives in `docs/kaia/inventory/plugins.md`.

## Evidence needed to accept

- KR-005 requires every inventory item to have an owner, a baseline workflow, the expected data effects and an initial compatibility class.
- KR-012 requires representative plugins to have repeatable workflow tests. Undocumented direct access must appear in the inventory with a classification.
- Each behavior that enters the profile has a minimal fixture.
- Phase 3 exit: applicable bundled plugins and the selected community workflows meet the profile.

## Sources

- S2: Frontend development guide. https://github.com/koreader/koreader/blob/master/doc/Development_guide.md
- S3: Tagged example plugin. https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/plugins/hello.koplugin/main.lua
- S5: Plugin loader and application integration. https://github.com/koreader/koreader/blob/master/frontend/pluginloader.lua ; https://github.com/koreader/koreader/blob/master/frontend/apps/reader/readerui.lua
- S16: Community stylus plugin. https://github.com/mysticknits/pencil.koplugin
- S17: Community sticker plugin. https://github.com/toastiez/pagenotes.koplugin
- S6: Tagged Kobo backend. https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/frontend/device/kobo/device.lua
- S19: LuaJIT FFI. https://luajit.org/ext_ffi.html
- S24: Example native and system access. https://github.com/koreader/koreader/blob/master/plugins/SSH.koplugin/main.lua
