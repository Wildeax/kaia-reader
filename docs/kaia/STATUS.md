# Kaia Reader status

Date: 2026-10-15. Base: KOReader v2026.07.1 (`9192014d8`). Kaia code checked at `485aa0ec2`, with the Rust changes in the working tree on 2026-10-14. Rows changed on 2026-10-15 were checked against `65ee54cee`. The first device run is dated 2026-10-14.

This page covers both apps. For each item it gives the state, what exists in the repository, how it was checked, and what still needs a Kobo Libra Colour. The 4.x package ran on a Libra Colour on 2026-10-14. [device/FIRST_DEVICE_RUN.md](device/FIRST_DEVICE_RUN.md) lists the checks that ran and the ones that did not. Apart from those checks, nothing has run on a device, and no device gate in `docs/kaia/ENGINEERING_PLAN.md` section 8 is recorded as passed.

## What ships today

The Lua app is the shipped product and the default. The Kobo package starts it from the NickelMenu entry "Kaia Reader". The same package also carries the Rust app, which starts from a second entry, "Kaia Reader Rust". The Rust app becomes the default only after its device gates pass (phase R8).

The 4.x package ran on a Libra Colour on 2026-10-14, for the checks in `device/FIRST_DEVICE_RUN.md`. No 5.x package run is recorded. Every other check on this page ran on a desktop, on the GitHub CI runners, in the emulator, or under armv7 emulation.

## Rust app

The rewrite follows ADR-008 and `docs/kaia/REWRITE_PLAN.md`. The work began with the plan commit `ea8a62533`. That commit and the 88 commits after it make 89 commits up to `485aa0ec2`. The workspace in `rust/` has 27 crates.

Test counts are grep counts of `#[test]` in each crate, taken on 2026-10-14. The total is 1,916. The count covers source attributes, not the tests a run executes. It includes 3 ignored tests and tests that need a feature, such as `cre`. This page did not run the workspace test build.

"Done on desktop" means the phase's exit checks have code and tests on desktop. "Partly done" means at least one exit check is missing. R3 is built for armv7. Its first run on a Libra Colour is in `device/FIRST_DEVICE_RUN.md`.

| Phase | State | Evidence | Needs a device for |
|---|---|---|---|
| R0 Foundations | Done on desktop | `kaia-events` (96 tests), `kaia-gfx` (62), `kaia-input` (42), `kaia-ui` (189), `kaia-display` (123, desktop and headless backends), `kaia-luadata` (52). `--headless --screenshot` writes a PNG, and `--script` replays input. | The panel and touch, which R3 covers |
| R1 Reader with MuPDF | Partly done. The page-turn gate has a partial desktop measurement, and it fails for EPUB. | `kaia-doc` (64 tests, MuPDF and CREngine backends). The corpus tests in `tests/corpus.rs` open the repository's EPUB and PDF fixtures, and the Gutenberg EPUBs in `~/kaia-test-library` when that folder exists. `kaia-book` (93 tests) covers page turns, the table of contents, saved progress, font size and margins, and footnote links with a back stack (`tests/reader_links.rs`). A partial desktop release run at `61a72323e` fails the check for EPUB, with p50 page turns of 250 to 320 ms (`device/BENCHMARKS.md`, Rust app, desktop release). Commit `bd977b9c7` changes the ink anchor path after that run and has no rerun. `tools/kaia/bench` measures the Lua app. | Page-turn time on the panel. The 100 ms desktop release check needs a rerun at HEAD. |
| R2 Shell | Done on desktop | `kaia-app` (413 tests) has Home, Library, Reader, Notes and Settings. Extensions is a Settings section. `kaia-library` (127 tests) covers scanning, the SQLite index, covers, collections and search. `kaia-ui` has the on-screen keyboard and dialogs. Golden tests use 47 PNGs in `rust/kaia-app/tests/golden`. Review 2 found 22 issues in this layer. | Layout and touch on the panel beyond the screens in the first device run (`device/FIRST_DEVICE_RUN.md`). The golden images are desktop renders. |
| R3 Kobo | Built for armv7. Ran on a Libra Colour on 2026-10-14 for the checks in `device/FIRST_DEVICE_RUN.md` | `kaia-display` has the Kobo backend and evdev input (`kaia-display/src/kobo`). `kaia-fbink-sys` (4 tests) binds FBInk. `kaia-platform` (121 tests) covers battery, frontlight and warmth, suspend, Wi-Fi, USB handover and paths. The launcher is `platform/kobo/kaia/kaia-rust.sh`, with the NickelMenu entry `nickelmenu-kaia-rust`. `tools/kaia/build-rust-kobo.sh` builds the binary, and `tools/kaia/check-rust-package.sh` checks the package. The device procedure is `docs/kaia/device/RUST_APP_TEST.md`. The armv7 smoke script `tools/kaia/rust-app-smoke.sh` needs Docker. CI does not run it, and this page did not run it. | Sleep and wake, Wi-Fi and standby, and each other step in `RUST_APP_TEST.md` that `FIRST_DEVICE_RUN.md` does not record |
| R4 Annotations | Partly done. Device checks are open. | `kaia-book` saves highlights, bookmarks and notes in KOReader's sidecar format, with `kaia-luadata` and `kaia-id`. In-book search and the dictionary work (`kaia-dict`, 47 tests). Notes export is in `surfaces/notes/export.rs`. Reflow tests are in `kaia-book/tests/mupdf_reflow.rs` and `cre_order.rs`. `kaia-core` (48 tests) holds the annotation and sticker types. `kaia-storage` (45 tests, 8 in `tests/faults.rs`) stores stickers and the word list. Dictionary order, a per-book dictionary choice and the word list are built. The Rust app draws and edits stickers in `surfaces/reader/stickers.rs` and `sticker_actions.rs`, and the Lua app reaches the same store through `kaia-ffi`. | Reopen and reflow on real books, crash and power-cut behavior on real storage, and export onto the FAT data volume |
| R5 CREngine | Partly done. Automated desktop comparison passes for the two existing EPUB fixtures. | `tools/kaia/cre_compare.py` compares the emulator Lua wrapper with kaia-doc at matched fonts and layout settings. The local macOS arm64 run on 2026-10-14 matched page counts, text, anchors and pixels for two books under three profiles across 42 sampled pages. `kaia-lua.yml` runs the comparison with a 15-minute limit and uploads reports and diff images. See `CI.md` for settings, thresholds and sampling limits. No layout mapping mismatch was found in this corpus. | Broader licensed corpus coverage under B-12/KR-007, Linux CI execution, and the on-device B-75 comparison |
| R6 Native features | Partly done | The app uses `kaia-stats` (47 tests, KOReader's statistics database) and `kaia-sync` (58 tests, KOReader's sync protocol). `kaia-opds` (59 tests) is a client, and the catalogs surface (`surfaces/catalogs`) lists, searches and downloads from OPDS catalogs. `kaia-calibre` (41 tests) is a dependency of `kaia-app`, and the Calibre view searches a downloaded metadata file. `docs/kaia/specs/STORAGE.md` covers the statistics database. `specs/SYNC.md`, `specs/OPDS.md` and `specs/CALIBRE.md` cover sync, OPDS and Calibre, each with a table of its contracts and the tests that cover them. | Sync against a real server, an OPDS download, and a Calibre connection from the Kobo |
| R7 Lua layer | Partly done. The L5 workflows are incomplete. | `kaia-lua` (118 tests). The embedded LuaJIT runs KOReader's pure modules, KOReader widgets run in Kaia windows, and plugins load as reader modules. The bundled inventory has 37 entries. Two work, 22 work with limits, 9 are blocked and 4 do not apply (`docs/kaia/inventory/PLUGIN_STATUS.md`). The community list has 9 pinned plugins. Three work with limits and 6 are blocked (`docs/kaia/inventory/COMMUNITY_PLUGINS.md`). The spec is `docs/kaia/specs/LUA_COMPAT.md`. | The workflows in both inventories, plus power-off, wakeup, USB keyboard and the physical Kobo plugin paths |
| R8 Migration and release | Started. The device gates have not run. | The Rust app imports KOReader's reading history, collections and statistics from Settings (`settings:data:import`), and Kobo reading data from `KoboReader.sqlite` (Settings, Data). It has no import for data from the Lua app. It is not the default. `RUST_APP_TEST.md` says not to make it the default before the hardware checks pass. | Every device gate in `docs/kaia/ENGINEERING_PLAN.md` section 8 |

### Review findings

Review 1 (`docs/kaia/reviews/2026-10-13-review-1.md`) covers the Rust crates. It lists 86 findings: 5 critical, 21 high, 32 medium, 27 low and 1 process item. 86 are fixed, and no row is marked open. UI-L is fixed except for the headless whole-frame refresh. K-H4 is fixed for the Lua plugin layer, where `show` and `close` answer at once. The Rust `Ctx` still queues them, by design.

Review 2 (`docs/kaia/reviews/2026-10-13-review-2.md`) covers `kaia-app`. It lists 22 findings: 8 high, 13 medium and 1 low. Its status table marks all 22 fixed. Its fix table names regression tests for 14 of them. Tests in `rust/kaia-app/tests/review2_refresh.rs` and `review2_latency.rs` cover R2-H6, H7, H8, M8 and M10. R2-M9, M11 and M12 have code changes but no test with a matching name.

Review 3 (`docs/kaia/reviews/2026-10-14-review-3.md`) reads commits `332f37ace` through `a5c0aa4b6`. It lists 7 findings: 5 high and 2 medium. Its table marks all 7 fixed, and each fix names a regression test. The test names exist in the tree: `rust/kaia-lua/tests/docsettings_guard.rs`, `rust/kaia-app/tests/plugin_library.rs`, `rust/kaia-library/tests/move_and_index.rs`, `rust/kaia-calibre/src/lib.rs`, `rust/kaia-calibre/src/policy.rs`, `rust/kaia-app/tests/calibre_policy.rs`, `rust/kaia-app/tests/review3_plugins.rs`, `rust/kaia-app/tests/review3_shutdown.rs` and `rust/kaia-app/src/surfaces/reader/plugin_doc.rs`. This check confirmed the names. It did not run those tests. The review used no physical Kobo.

### Nickel parity

The Nickel parity plan (`docs/kaia/nickel/PARITY.md`, section 4) lists the stock features that the Rust app should match. Tier 1 is built, with one partial item. T1-7 has a page button wake setting, but it stays off because no platform reports support for it yet. The other Tier 1 items are built, including the highlight shade (T1-12) and library search suggestions (T1-17).

Tier 2 is built except auto rotation (T2-2) and text sharpness (part of T2-8). Auto rotation waits on the orientation sensor flag (Q9). Sharpness is blocked by Q26. Text weight is built as a base weight in the Font sheet. Reading positions are not imported (T2-3, Q33). The WebDAV annotation export (T2-4) is implemented for B-120, and `docs/kaia/specs/WEBDAV.md` says a real server and a Kobo still need checks. Dictionary downloads (T2-6) are built and off by default.

Tier 3 is built for large print (T3-3), the experimental page (T3-5, with no flags yet), refresh and waveform choices (T3-6), header and footer items with margin marks and the dog-ear (T3-8), the author view (T3-9), storage by source (T3-10), finished counts (T3-11), airplane mode (T3-12), the time zone picker and network time (T3-13), text and CBR files (T3-15), hyphenation for 49 languages (T3-7) and Calibre browse and search (T3-17). Settings has reset and erase (T3-18), with the warning text and recovery steps still open in PARITY. Notes export writes Markdown, JSON and PDF (T3-16). Input methods (T3-4), Bluetooth (T3-14) and ambient light (T3-19) are not built. Ambient light is not needed for stock parity.

Stylus ink (T3-1) is built in the Rust reader, and its device checks are open. Read aloud (T3-2) waits on Q28, because the firmware has no offline speech engine.

The first device run (2026-10-14) read the raw page key codes as 193 (back) and 194 (forward) on gpio-keys, which bears on T1-3 and Q3. It also read the frontlight level and warmth, but it did not record which warmth node the write reached. The other open checks are the light key code and flag (T1-1, Q9), the page button wake nodes (T1-7, Q9 and Q12), the page-turn repeat timing (T1-8, Q1 and Q2), the warmth node (T2-1, Q10), and the Kobo import against a real `KoboReader.sqlite` (Q34 and Q48). The first device run did not record those.

Reading positions wait on Q33, which asks what the position fields mean. The WebDAV export waits on a decision about the target and format. Dictionary downloads wait on a decision about the source, the checksum and an opt-in switch. The network rule applies to both. CBR comics wait on the decoder decision.

## Lua app

### Summary

| Area | State |
|---|---|
| Fork, identity, separate data | Done. Kaia has its own data folder, install folder, launcher, and no KOReader update feed. |
| Rust library through LuaJIT FFI | Done on desktop and under armv7 emulation with the Kobo build of LuaJIT. |
| Annotation storage and stickers | Done on desktop. Image stickers and text notes in the margin of EPUBs, page stickers in PDFs, repair of orphaned stickers from Notes, fault tests, export and import. |
| Kaia surfaces | Home, Library, Notes and Settings (existing menus) share one nav bar. Extensions lists every plugin with its compatibility status. Portrait and landscape layouts checked in screenshots. |
| Moving from KOReader | One-time import of KOReader settings, history, book data and plugin data. KOReader's own files are never changed. |
| Extension API | Versioned Lua API v1 for commands, menu items, settings and messages, with an example plugin. |
| Safe mode and recovery | Done on desktop in both apps (`frontend/kaia/safemode.lua`, `rust/kaia-app/src/safe_mode.rs`). The device recovery procedure is written and has not run on a device. |
| Kobo packages | Built for firmware 4.x (`kobo`) and 5.x (`kobov5`). `spec/kobo-package/test-package.sh` passed 38 of 38 checks on 2026-10-13, using the packages in `dist/`. The 4.x package ran on a Libra Colour on 2026-10-14. No 5.x run is recorded. |
| CI | On `89bdcb717`, the kaia-docs, kaia-shell and kaia-rust workflows passed. The kaia-lua workflow failed in its emulator job, on the golden test `settings_root_and_reading_golden`. The same test failed on `332f37ace`. See the Rust app known gaps. |
| Device gates (power, latency, ghosting, soak, 500 suspend cycles) | Not started. They need hardware. |

### Backlog items

| Item | State | Evidence | Needs a device for |
|---|---|---|---|
| KR-001 identity | Done | `frontend/kaia/identity.lua`, data dir `kaia`, OTA menu hidden, `platform/kobo/kaia/kaia.sh`, package installs to `.adds/kaia` | Confirming both apps run side by side |
| KR-002 hardware profile | Tooling done | `tools/kaia/device/collect-profile.sh` with a 38-check leak test | Collecting the real profile |
| KR-003 backup and recovery | Tooling and safe mode done | `backup.sh`, `restore.sh`, `frontend/kaia/safemode.lua`, emulator check that a broken plugin is skipped in safe mode | The injected startup failure test on the device |
| KR-004 rebuild unmodified KOReader | Done for desktop and Kobo | Desktop build, all upstream front specs pass; `tools/kaia/build-kobo.sh` builds `kobo` and `kobov5` in the pinned toolchain image | Suspend on the device. Reading and exit to Nickel were checked for the Lua app on 2026-10-14 (`device/FIRST_DEVICE_RUN.md`) |
| KR-005 inventory | Done | `docs/kaia/inventory/plugins.md`, `functions.md` | Applicability of four conditional plugins |
| KR-006 measurements | Method, workload, desktop baseline done, and a partial Rust desktop page-turn run that fails at `61a72323e` | `tools/kaia/bench/`, `docs/kaia/device/BENCHMARKS.md` | Every number that the plan's gates use |
| KR-007 licenses | Inventory done, 85 items need review (`LICENSES.md`, Needs review list) | `docs/kaia/inventory/LICENSES.md`, `RELEASE_SOURCE_CHECKLIST.md` | Nothing; needs a human license review |
| KR-008 first Rust service | Done on desktop and armv7 emulation | `rust/kaia-ffi`, `frontend/kaia/rustlib.lua`, `tools/kaia/arm-smoke.sh` | Repeated calls on the device |
| KR-009 error and lifetime tests | Done | 10,000-cycle allocation test in `rust/kaia-ffi/tests/alloc.rs`, stale handles, panics, size limits, header parity | Device stress run |
| KR-010 hybrid UI proof | Done on desktop | `spec/unit/kaia_hybrid_ui_spec.lua`, `docs/kaia/specs/KR-010-hybrid-ui.md` | E Ink refresh and ghosting |
| KR-011 embedded LuaJIT spike | Done; recommends keeping the Lua host | `docs/kaia/spikes/KR-011-embedded-luajit.md` | Device numbers before revisiting |
| KR-012 compatibility tracing | Done | `frontend/kaia/trace.lua`, `docs/kaia/inventory/TRACING.md` | Traces of real use |
| KR-013 design system | Done | `frontend/kaia/theme.lua`, `docs/kaia/specs/DESIGN.md`, restyled `ui/size.lua` | Readability on the panel |
| KR-014 daily-reading slice | Done on desktop | Home, Library, Notes, Extensions, return to Home after reading, import from KOReader | Daily use on the device |
| KR-015 annotation storage | Done | `docs/kaia/specs/STORAGE.md`, `rust/kaia-storage` with eight tests in `tests/faults.rs` | Power-cut and full-disk tests on real storage |
| KR-016 architecture review | Prepared | `docs/kaia/adr/` holds the records. ADR-001 is superseded by ADR-008, which is accepted. ADR-002 to ADR-007 are still proposed. This page. | The go/no-go decision needs device evidence |

## Found and fixed along the way

- With the sidecar location set outside the book folder, upstream `DocSettings:flush` deleted the `.sdr` folder next to the book after reading it. On a device that also runs stock KOReader that would delete KOReader's reading data. Kaia now never deletes or moves files in the book folder when it writes elsewhere, and prefers its own copy on open (`spec/unit/kaia_sidecar_coexistence_spec.lua`).
- Rust std needs `getauxval`, which the Kobo 4.x sysroot (glibc 2.15) lacks. `libkaia.so` provides a fallback and the build fails if it needs a newer glibc than the sysroot.
- `kodev release` runs `make po`, which moves the `l10n` submodule to upstream master. The Kaia build script skips it.
- The Rust index deleted every book under a library folder that was missing or unmounted, along with its collections and history (review 1, D-C1). The index keeps books when their folder is missing.
- A hold gesture on a Kobo fired only when the finger lifted, because the app mixed two clock sources (review 1, G-H1, fixed).
- The Rust app let a second instance overwrite the first instance's settings (review 2, R2-H2). An OS lock on `kaia.lock` now refuses the second instance.
- Opening a damaged `settings.json` in the Rust app overwrote the original file without a warning (review 2, R2-H1). The original bytes are now copied before any write.
- The Rust sync queue could drop an update that another writer queued during a drain (review 2, R2-H5). Reads and writes take a lock now, and an acknowledgement removes only the version it sent.

## Known gaps

### Lua app

- Stickers in PDFs are placed where the user selects text and can cover it. Fixed pages have no margin placement.
- In a narrow margin an image sticker is drawn faded over the end of its line, while a text note becomes a marker.
- New UI strings are English only. They go through gettext but have no translations.
- The NickelMenu entry syntax was checked on a device (NickelMenu v0.6.0, 2026-10-14). The KFMon path is unverified.
- The Library title bar is KOReader's, so its header is not aligned with the Kaia headers.
- The package has no Kaia icon or artwork yet.
- Kaia has no update channel. Updates mean installing a new package by hand.

### Rust app

#### Device and rollout

- The Rust app ran on a Libra Colour on 2026-10-14. `docs/kaia/device/FIRST_DEVICE_RUN.md` lists what ran and what did not. `docs/kaia/device/RUST_APP_TEST.md` is the procedure for the checks it did not cover.
- The armv7 smoke script `tools/kaia/rust-app-smoke.sh` runs by hand, with Docker. CI does not run it.
- The Rust page-turn check fails for EPUB in a partial desktop run at `61a72323e`. Commit `bd977b9c7` changes the ink anchor path after that run, so the current page-turn time is not measured.
- Review 2 lists these as untested: physical Kobo behavior, ARM release timing, a physically full volume, power-cut injection and a TLS handshake. The first device run (2026-10-14) covered part of the physical Kobo behavior, see `docs/kaia/device/FIRST_DEVICE_RUN.md`.
- Sensor-driven rotation is not wired. The app starts in portrait and tells the user that automatic rotation is unavailable.
- The golden images are desktop renders. `docs/kaia/specs/DESIGN.md` says the physical panel decides.
- The packages in `dist/` are local outputs from earlier builds. The Kaia package there is named for commit `00731437b`. Rebuild from the commit under test before a device run.

#### Review findings still open

- Review 1 item UI-L is open for headless output, which still refreshes the whole frame.
- A full refresh after an image page uses CREngine's image share per page (`BookSession::page_image_share`, threshold 0.5). MuPDF reflow reports no share yet.

#### Plugins

- Nine of 37 bundled entries are blocked: SSH, autodim, autosuspend, autowarmth, coverbrowser, externalkeyboard, keepalive, terminal and timesync. Each entry has its own reason in `PLUGIN_STATUS.md`.
- Coverbrowser needs display-mode and cover-grid adapters for the native library. Lua FileChooser patches cannot change the Rust library view.
- Terminal needs app-owned process creation and nonblocking PTY I/O.
- Six of nine community plugins are blocked. pencil needs stylus and eraser events and stroke storage. footcream needs document-wide search. anki needs dictionary buttons and selection context. highlightsync needs a host sync and storage adapter. readeck needs a server, and the tests exclude network access. foldermemory needs library display hooks.
- `docsettingtweak` needs directory defaults before the first layout. DocSettingsLoad arrives after BookSession opens.
- Power-off, wakeup, USB keyboard and physical Kobo plugin workflows are untested.
- Text selection covers only the active selection. Dictionary review buttons and Japanese selection expansion need more work.

#### Wiring and data

- OPDS catalogs and Calibre wireless open from Network and sync (`surfaces/catalogs`). The Calibre view searches and browses the metadata file that the device downloads (T3-17).
- `docs/kaia/specs/STORAGE.md` covers the statistics database.
- `docs/kaia/specs/SYNC.md` covers progress sync with KOReader's sync protocol. Its Tests table lists the contracts with no test.
- `docs/kaia/specs/OPDS.md` covers OPDS catalog browsing and downloads. Its Tests table lists the contracts with no test.
- `docs/kaia/specs/CALIBRE.md` covers the Calibre wireless connection and the metadata view. Its Tests table lists the contracts with no test.
- The Rust app has no import for Kaia Lua-app data. It imports KOReader data and Kobo reading data.
- Kaia's own UI strings have no translations yet: `l10n-kaia` holds only the template. Strings that KOReader also uses come from KOReader's catalogs, so those show in the chosen language.

#### Build and CI

- The `settings_reading.png` failure on `332f37ace` and `89bdcb717` came from the Dictionary folder row printing the host-specific temporary path. Commit `7bfb863a7` shortened that row to the last two path components. The golden passes on Linux in kaia-lua run `38611826467`, with the existing tolerance of 14. That run fails later in the annotation tests.

## How to try it

### Desktop, Lua app

See `docs/kaia/CONTRIBUTING.md`. The short form is `source tools/kaia/env.sh`, `./kodev build`, `tools/kaia/build-libkaia.sh host`, then `./kodev run`.

### Desktop, Rust app

From `rust/`, after `source tools/kaia/env.sh`:

```sh
cargo run -p kaia-app -- --help
```

The help text lists the flags that `rust/kaia-app/src/main.rs` reads: `--headless`, `--safe-mode`, `--screenshot PNG`, `--data-dir DIR`, `--books DIR`, `--open BOOK` and `--script FILE`. `--script` replays input lines such as `key RPgFwd`.

The default `lua` feature links KOReader's LuaJIT. Run `./kodev build` once, so that `base/build/*/` contains LuaJIT and `rust/kaia-lua/env.sh` finds it. Otherwise set `KAIA_LUAJIT_LIB` and `KAIA_LUAJIT_INCLUDE`.

A headless run needs no window:

```sh
cargo run -p kaia-app -- --headless --data-dir /tmp/kaia-data --books path/to/books --screenshot /tmp/kaia-home.png
```

On 2026-10-13 this command ran against the six-book test library in `~/kaia-test-library`. It wrote a 1264 by 1680 PNG and exited with status 0. Without `--headless` the app opens a desktop window. This page does not record a window run.

### Kobo

See `docs/kaia/INSTALL_KOBO.md` for the package, the two NickelMenu entries and the firmware check. Read `docs/kaia/device/RUST_APP_TEST.md` before a Rust device run, and `docs/kaia/device/FIRST_DEVICE_RUN.md` for what the first run checked. Back up the device first with `tools/kaia/device/backup.sh`, as `docs/kaia/device/BACKUP_AND_RECOVERY.md` describes.
