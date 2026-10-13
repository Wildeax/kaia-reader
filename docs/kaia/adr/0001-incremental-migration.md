# ADR-001: Incremental application migration

Status: Superseded by ADR-008

Date: 2026-10-13

## Context

Kaia Reader is a fork of KOReader. The baseline is tag v2026.07.1, with a release commit that begins with `9192014`. The full commit, submodules and build inputs get recorded in Phase 0, before development starts. [S1]

The KOReader frontend is Lua. Plugins call frontend objects directly. The example plugin imports UI widgets, calls UIManager and registers an application menu item. [S2, S3, S5] Plugins also reach document objects, device services, settings and native libraries. A new frontend that keeps only a Lua interpreter would break these plugins. [S5]

The plan rejects a translation of the whole repository. It also rejects removing working Lua only to reach a full rewrite.

## Decision

Kaia migrates the application layer in steps. Each step moves one part to Rust after that part passes its gate. The Lua frontend, the LuaJIT runtime, the document engines and the Kobo integration stay in place until a replacement passes the compatibility, document-correctness, data-durability, hardware and resource-budget gates.

Kaia keeps the original source paths: `frontend/`, `plugins/`, `base/` and `platform/`. The Rust workspace sits beside them. Kaia does not rename directories or reformat upstream files, so upstream merges stay small.

Every migration step answers the checklist in `docs/kaia/CONTRIBUTING.md`.

## Consequences

- The codebase stays mixed-language for a long time.
- Upstream fixes merge with less work because the paths stay the same.
- A failed step has a fallback. If the FFI or host prototype fails, the Lua host stays, and the redesigned reader still ships with Rust services.
- The timeline depends on measured progress. The plan estimates 6 to 12 months for a polished public beta, with two experienced full-time engineers and part-time design and QA. The estimate is not a forecast.
- Broad compatibility and host migration may take substantially longer. The host migration gets its own estimate after Phases 1 to 3.

## Evidence needed to accept

- The Phase 0 exit gate (KR-004) needs a fresh clean environment that builds the pinned, unmodified baseline. Reading, configuration, suspend and exit work on the device.
- The Phase 1 exit gate (KR-008 to KR-011) needs the Rust service, its error behavior, rendering ownership and representative plugins to work on hardware.
- KR-016 records a go or no-go decision. The decision cites measured device results, complexity, plugin compatibility and upstream merge burden.

## Sources

- S1: KOReader latest release and baseline. https://github.com/koreader/koreader/releases/latest ; https://github.com/koreader/koreader/releases/tag/v2026.07.1
- S2: Frontend development guide. https://github.com/koreader/koreader/blob/master/doc/Development_guide.md
- S3: Tagged example plugin. https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/plugins/hello.koplugin/main.lua
- S5: Plugin loader and application integration. https://github.com/koreader/koreader/blob/master/frontend/pluginloader.lua ; https://github.com/koreader/koreader/blob/master/frontend/apps/reader/readerui.lua
