# ADR-008: Rewrite the app in Rust, add Lua plugin compatibility afterwards

Status: Accepted

Date: 2026-10-13

Supersedes: ADR-001 (incremental application migration). Amends ADR-002 (one coordinator: the coordinator becomes the Rust main loop).

## Context

The project owner's goal is a Rust e-reader with a redesigned UI that keeps KOReader's reading quality and later runs KOReader plugins. ADR-001 chose an incremental migration inside the KOReader Lua app. That path keeps KOReader's UI structure for a long time, which works against the redesign.

KOReader plugins call KOReader's internal modules directly (UIManager, widgets, ReaderUI, DocSettings, Dispatcher), so plugin compatibility means providing those modules, not only a Lua interpreter. The KR-011 spike showed that a Rust host can embed KOReader's exact LuaJIT build.

## Decision

Build a new Rust app first, with no Lua. Design its core with the eight compatibility seams in `docs/kaia/REWRITE_PLAN.md` from the start. Reimplement the most used bundled features natively. Then add an embedded LuaJIT layer that emulates KOReader's modules for plugins. Keep the document engines, FBInk and LuaJIT as C and C++ libraries from KOReader's pinned build.

## Consequences

- The Lua app remains the shipped product until the Rust app passes the device gates, and the Lua app is the behavior reference.
- Plugin compatibility arrives in phase R7, not at the first Rust release.
- The work done for the Lua app that is not UI (Rust storage, stickers model, Kobo build pipeline, CI, safe mode design, inventories, design tokens) carries over.

## Evidence needed

- R0 to R3 exit checks in the rewrite plan, then the device gates from ENGINEERING_PLAN.md section 8.
- For R7: the bundled plugin workflows from `docs/kaia/inventory/plugins.md` pass under the Lua layer.
