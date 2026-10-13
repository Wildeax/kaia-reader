# Kaia Reader status

Date: 2026-10-13. Base: KOReader v2026.07.1 (`9192014d8`).

This page lists each backlog item, what exists in the repository, how it was checked, and what still needs a Kobo Libra Colour. "Done on desktop" means the code exists and passes its tests in the emulator or under armv7 emulation. No item has run on a physical device yet, so no item meets a gate that the plan ties to hardware.

## Summary

| Area | State |
|---|---|
| Fork, identity, separate data | Done. Kaia has its own data folder, install folder, launcher, and no KOReader update feed. |
| Rust library through LuaJIT FFI | Done on desktop and under armv7 emulation with the Kobo build of LuaJIT. |
| Annotation storage and stickers | Done on desktop. Image stickers and text notes in the margin of EPUBs, page stickers in PDFs, repair of orphaned stickers from Notes, fault tests, export and import. |
| Kaia surfaces | Home, Library, Notes and Settings (existing menus) share one nav bar; Extensions lists every plugin with its compatibility status. Portrait and landscape layouts checked in screenshots. |
| Moving from KOReader | One-time import of KOReader settings, history, book data and plugin data. KOReader's own files are never changed. |
| Extension API | Versioned Lua API v1 for commands, menu items, settings and messages, with an example plugin. |
| Safe mode and recovery | Done on desktop. Device recovery procedure written, not run. |
| Kobo packages | Built for firmware 4.x (`kobo`) and 5.x (`kobov5`). 40 package checks pass. Not installed on a device. |
| CI | Rust, Lua (including the emulator build and full front suite), shell and docs workflows on GitHub, all passing. |
| Device gates (power, latency, ghosting, soak, 500 suspend cycles) | Not started. They need hardware. |

## Backlog items

| Item | State | Evidence | Needs a device for |
|---|---|---|---|
| KR-001 identity | Done | `frontend/kaia/identity.lua`, data dir `kaia`, OTA menu hidden, `platform/kobo/kaia/kaia.sh`, package installs to `.adds/kaia` | Confirming both apps run side by side |
| KR-002 hardware profile | Tooling done | `tools/kaia/device/collect-profile.sh` with a 38-check leak test | Collecting the real profile |
| KR-003 backup and recovery | Tooling and safe mode done | `backup.sh`, `restore.sh`, `frontend/kaia/safemode.lua`, emulator check that a broken plugin is skipped in safe mode | The injected startup failure test on the device |
| KR-004 rebuild unmodified KOReader | Done for desktop and Kobo | Desktop build, all upstream front specs pass; `tools/kaia/build-kobo.sh` builds `kobo` and `kobov5` in the pinned toolchain image | Reading, suspend and exit on the device |
| KR-005 inventory | Done | `docs/kaia/inventory/plugins.md`, `functions.md` | Applicability of four conditional plugins |
| KR-006 measurements | Method, workload and desktop baseline done | `tools/kaia/bench/`, `docs/kaia/device/BENCHMARKS.md` | Every number that the plan's gates use |
| KR-007 licenses | Inventory done, 84 items need review | `docs/kaia/inventory/LICENSES.md`, `RELEASE_SOURCE_CHECKLIST.md` | Nothing; needs a human license review |
| KR-008 first Rust service | Done on desktop and armv7 emulation | `rust/kaia-ffi`, `frontend/kaia/rustlib.lua`, `tools/kaia/arm-smoke.sh` | Repeated calls on the device |
| KR-009 error and lifetime tests | Done | 10,000-cycle allocation test, stale handles, panics, size limits, header parity | Device stress run |
| KR-010 hybrid UI proof | Done on desktop | `spec/unit/kaia_hybrid_ui_spec.lua`, `docs/kaia/specs/KR-010-hybrid-ui.md` | E Ink refresh and ghosting |
| KR-011 embedded LuaJIT spike | Done; recommends keeping the Lua host | `docs/kaia/spikes/KR-011-embedded-luajit.md` | Device numbers before revisiting |
| KR-012 compatibility tracing | Done | `frontend/kaia/trace.lua`, `docs/kaia/inventory/TRACING.md` | Traces of real use |
| KR-013 design system | Done | `frontend/kaia/theme.lua`, `docs/kaia/specs/DESIGN.md`, restyled `ui/size.lua` | Readability on the panel |
| KR-014 daily-reading slice | Done on desktop | Home, Library, Notes, Extensions, return to Home after reading, import from KOReader | Daily use on the device |
| KR-015 annotation storage | Done | `docs/kaia/specs/STORAGE.md`, `rust/kaia-storage` with the seven fault tests | Power-cut and full-disk tests on real storage |
| KR-016 architecture review | Prepared | `docs/kaia/adr/` (all proposed), this page | The go/no-go decision needs device evidence |

## Found and fixed along the way

- With the sidecar location set outside the book folder, upstream `DocSettings:flush` deleted the `.sdr` folder next to the book after reading it. On a device that also runs stock KOReader that would delete KOReader's reading data. Kaia now never deletes or moves files in the book folder when it writes elsewhere, and prefers its own copy on open (`spec/unit/kaia_sidecar_coexistence_spec.lua`).
- Rust std needs `getauxval`, which the Kobo 4.x sysroot (glibc 2.15) lacks. `libkaia.so` provides a fallback and the build fails if it needs a newer glibc than the sysroot.
- `kodev release` runs `make po`, which moves the `l10n` submodule to upstream master. The Kaia build script skips it.

## Known gaps

- Stickers in PDFs are placed where the user selects text and can cover it; there is no margin placement for fixed pages.
- In a narrow margin an image sticker is drawn faded over the end of its line, while a text note becomes a marker.
- New UI strings are English only; they go through gettext but have no translations.
- The NickelMenu entry syntax and the KFMon path are unverified on a device.
- The Library title bar is KOReader's, so its header is not aligned with the Kaia headers.
- The package has no Kaia icon or artwork yet.
- Kaia has no update channel. Updates mean installing a new package by hand.

## How to try it

Desktop: see `docs/kaia/CONTRIBUTING.md` (`source tools/kaia/env.sh`, `./kodev build`, `tools/kaia/build-libkaia.sh host`, `./kodev run`).

Kobo: see `docs/kaia/INSTALL_KOBO.md`. Back up the device first with `tools/kaia/device/backup.sh`.
