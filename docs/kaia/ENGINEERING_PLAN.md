# Kaia Reader — Engineering and Migration Plan

Version: 0.2 · Research date: October 12, 2026  
Revision: Project naming update only; technical scope and research unchanged.  
Status: Proposed architecture; source-reviewed, not built or hardware-validated.  
Initial product target: Kobo Libra Colour running a specifically tested stock firmware.  
Selected project name: **Kaia Reader**. Name, organization, package and trademark availability are not reserved or legally cleared.

## Executive decision

Create a history-preserving fork of KOReader and migrate its application layer incrementally toward Rust. Keep the exact upstream LuaJIT runtime and the Lua environment required by existing plugins. Initially reuse KOReader's document engines, device integration, launcher and display-refresh machinery. Redesign the experience before attempting to replace its entire execution host.

The target is a polished, offline-first, extensible reader—not a percentage of Rust source code. A long-lived mixed-language result is acceptable. Removing working Lua solely to reach “100% rewritten” is not a release criterion.

The project's compatibility obligation is larger than executing Lua. Even KOReader's bundled example plugin imports UI widgets, uses UIManager and registers with an application menu. Existing plugins can also access document objects, device services, settings and native libraries. Replacing the frontend while retaining only a Lua interpreter would not preserve them. [S2–S5]

**Non-negotiable rule:** do not remove a legacy implementation until the replacement passes defined compatibility, document-correctness, data-durability, hardware and resource-budget gates.

## 1. Verified starting points and remaining uncertainty

| Topic | Source-grounded finding | Consequence |
|---|---|---|
| Upstream baseline | KOReader's latest-release endpoint resolved to v2026.07.1, release commit shorthand `9192014`, published August 1, 2026. [S1] | Start the baseline investigation at this tag; resolve and record full commits, submodules and build inputs before development. |
| Frontend | Upstream documents its frontend as Lua. Its example plugin calls concrete frontend objects rather than a language-independent plugin service. [S2–S3] | Retain frontend compatibility code and behavior, not just Lua syntax. |
| Native engines | koreader-base identifies MuPDF, CREngine, DjVuLibre and libk2pdfopt, with LuaJIT as runtime. [S4] | Keep document engines and their patches during application migration. |
| Libra Colour | The tagged Kobo device backend has the `KoboMonza` model, MTK handling, keys, frontlight, color screen and orientation support. [S6] | The first platform port already exists; wrapping it is safer than replacing it at inception. |
| Rust precedent | Plato's build uses Rust and its supported-device list includes Libra Colour. [S7–S8] | Rust application deployment has precedent; the proposed KOReader compatibility layer still needs proving. |
| Firmware sensitivity | NickelMenu currently says firmware 5.x is unsupported. KOReader has separate `kobo` and `kobov5` release packages/toolchain targets. [S9–S11] | Hardware support does not establish launcher or package support on every firmware. |
| Development | KOReader documents Linux/macOS emulation and a containerized development option. [S12] | Desktop development is useful, but it cannot certify E Ink behavior or power use. |
| License | KOReader and koreader-base distribute AGPLv3 licensing materials. [S4, S13] | Preserve upstream licensing and notices; audit every imported dependency and asset. |

No claims of universal plugin compatibility, matching Nickel battery life, production-quality stylus input, complete firmware replacement, or build reproducibility have been experimentally verified for Kaia. They remain test obligations.

### Related projects: use them appropriately

**ZenOS** describes itself as a customizable KOReader interface layer. It is useful for UX comparison and for studying integration techniques; it is not the required foundation for this fork. Avoid making Kaia dependent on another independently evolving UI layer. Any reused implementation needs attribution, license review and compatibility tests. [S14]

**Plato** is a valuable Rust/Kobo reference. It is not a substitute for KOReader's plugin environment. [S7–S8]

**kobo-core** documents Libra Colour and an ARMv7 musl build. Treat it as an optional implementation reference, not proof of power-management reliability or an automatic drop-in replacement for KOReader's device layer. [S15]

**pencil.koplugin** and **pagenotes.koplugin** are existing community work relevant to handwriting and stickers. Audit their code and pinned versions, reproduce their behavior, and consider cooperating with their authors. Their existence does not establish Kaia support, anchor robustness or production readiness. [S16–S17]

## 2. Product charter and scope

Kaia should feel like a complete reader, not a collection of patches. The default experience should offer a quiet home screen, a cover-based library, immediate return to the current book, predictable navigation, consolidated reading controls and annotations placed in the reading workflow.

Use Kobo's simplicity as a design reference, not its proprietary implementation or copied brand assets. Preserve advanced reading capabilities behind consistent, discoverable advanced controls. Do not equate “simpler” with deleting features.

### First supported release

Support one certified hardware/firmware combination: Kobo Libra Colour plus the firmware actually present on the development devices. Provide a Linux desktop development target. Keep other upstream platform code where practical without advertising untested support.

Preserve ordinary KOReader reading functionality and all applicable bundled plugins at the pinned baseline, subject to an explicit exception list. Establish a named, versioned list of supported third-party plugins. Demonstrate integrated stickers after their storage and anchoring are reliable.

### Explicit non-goals

Do not replace the kernel, bootloader or device firmware. Do not delete Nickel or depend on a Secure Boot bypass. Do not rewrite MuPDF, CREngine, text shaping, all image codecs or every native library. Do not promise Kobo Store services, DRM support, Kobo cloud annotation synchronization, or compatibility with arbitrary monkey patches.

KOReader's own quickstart states that DRM-protected books are not supported. Kaia's initial scope is readable, DRM-free content—not a clone of Kobo's commercial service ecosystem. [S18]

## 3. Architecture and ownership

### Initial deployment architecture

```text
Stock Kobo firmware, kernel and drivers (unchanged)
    |
Existing launch/device integration, adapted for Kaia identity and recovery
    |
One reader application process
    |-- Existing KOReader coordinator: input, UI events, display, suspend
    |-- Kaia presentation: redesigned Lua-compatible widgets
    |-- Rust library: new domain logic and narrowly bounded services
    |-- Existing LuaJIT: plugins + compatibility modules + remaining frontend
    `-- Existing native document/text/image engines
```

### Long-term architecture, only after the gates pass

```text
Kaia Rust host and lifecycle coordinator
    |-- Kaia shell and Rust-backed UI components
    |-- Rust application services and owned data
    |-- One embedded, upstream-compatible LuaJIT runtime
    |       `-- Compatibility objects, remaining Lua modules, legacy plugins
    |-- Document adapters -> retained native engines
    `-- Platform adapter -> validated Kobo integration
```

There must be one owner of input dispatch, one owner of display submission and one owner of suspend/resume at any migration stage. Do not run an independent Rust UI loop alongside a competing Lua display/power loop.

Lua plugin execution stays on the designated application thread. Background work uses bounded jobs and cancellation; workers return owned results rather than invoking Lua objects directly. Do not introduce a multithreaded asynchronous runtime merely for architectural fashion.

### Rust workspace, added to the existing fork

| Component | Responsibility |
|---|---|
| `kaia-core` | Typed domain models, commands, feature-specific state and validation. |
| `kaia-storage` | New-feature persistence, migrations, export and backup. |
| `kaia-ffi` | Small reviewed C ABI for the initial Lua-to-Rust integration. |
| `kaia-ui` | UI state/layout and later software-rendered components; not a new EPUB engine. |
| `kaia-compat` | Explicit Lua-facing object/service adapters and compatibility tests. |
| `kaia-platform` | Platform contracts, initially delegating to existing Kobo integration. |
| `kaia-host` | Later host migration experiment; not required for the first useful UI release. |

Keep the original `frontend/`, `plugins/`, `base/` and platform paths initially. Avoid a large directory rename or blanket formatting rewrite, which would make upstream merges harder without improving the product.

### State authority

Assign one authoritative owner to each data class. Existing reading positions, document settings and legacy plugin data remain owned by the existing subsystem until an explicit migration is accepted. The Rust library index is initially a derived cache, not another writer to the same data.

New stickers are owned by Kaia's storage service. When a subsystem later moves to Rust, its Lua-facing interface becomes an adapter to that owner. Do not run two independent copies of document state and synchronize them with best-effort events.

A compatibility adapter is insufficient for a plugin that bypasses it to mutate private fields or files. Classify that case, test it and retain the relevant legacy owner or exclude the plugin explicitly.

## 4. Rust/Lua integration and cross-compilation

### First boundary: small C ABI

Begin with Rust compiled as a loadable native library called from upstream LuaJIT FFI. LuaJIT documents direct calls to C functions; Rust documents C-compatible FFI. [S19–S20]

Use opaque handles, fixed-width scalar types, explicitly sized buffers, versioned structures and error codes. Define who owns and frees every allocation. Never expose Rust `String`, `Vec`, trait objects, borrowed references or unspecified-layout enums across the boundary.

No cross-language callbacks in the first proof. Lua should call Rust, receive a result and then create UI/events on the Lua side. Later callbacks need explicit lifetime and reentrancy tests.

Contain Rust panics at reviewed boundaries and never permit unwinding across a non-unwinding C ABI. Catch or translate C++ exceptions inside the C++ wrapper. Do not allow Lua error handling to jump across active Rust frames in an unreviewed integration path. Treat out-of-memory, native crashes and process aborts as recovery scenarios, not ordinary recoverable errors. [S20]

### Later embedding

`mlua` supports LuaJIT, native-module mode and external Lua libraries. It is a candidate for the Rust-host phase, not a necessary dependency for the first FFI integration. Use the exact approved upstream LuaJIT configuration; do not accidentally enable a second vendored runtime. Do not replace LuaJIT with Lua 5.4, Lua 5.5 or Luau and call that compatible. [S21–S22]

Verify module search paths, native symbol visibility, userdata lifetime, callbacks, garbage collection, errors and the global environment before moving application ownership.

### Build decision

Prefer a GNU-libc-compatible ARM Linux target for in-process integration with the existing application; `armv7-unknown-linux-gnueabihf` is a candidate to validate, not a universal build recipe. Check the actual device and binaries for instruction set, float ABI, dynamic loader, libc symbol requirements and native-library ABI. Rust's platform table specifies target requirements, and KOReader's toolchains distinguish firmware generations. [S10, S23]

Do not load a musl-built shared library into the existing GNU-libc process as a shortcut. A standalone static executable working on Kobo does not establish in-process compatibility.

Record the device firmware, kernel, CPU and memory facts; resolved toolchain image digest; Rust toolchain; linker/sysroot; Cargo.lock; every upstream/submodule revision; and dependency source hashes. Build C/C++ bridges against the same approved sysroot.

Use a pinned Linux build environment for release builds. A Mac can remain the editing/development machine; it does not need to define the shipping binary environment. No production packaging until a fresh environment can repeat the build and the output runs on the device.

## 5. Plugin compatibility contract

The promise is a tested compatibility profile, not “all Lua executes, therefore all plugins work.” KOReader's example plugin, loader and ReaderUI show concrete integration with the application. [S3, S5]

| Class | Policy |
|---|---|
| Applicable bundled plugins | Release-blocking tests at the pinned upstream revision. Explicitly explain disabled/debug-only and platform-inapplicable exceptions. |
| Selected community plugins | Publish repository, exact version/commit, tested workflows and known limitations. |
| Plugins using shared widgets | Preserve API and semantics; adopt Kaia appearance where compatible. |
| Custom-drawing/deeply coupled plugins | Keep legacy presentation/code paths until adapted and tested. |
| Arbitrary user patches or UI replacement plugins | Best effort only; detect known conflicts and do not silently claim support. |

Inventory module imports, global access, UI class extension, menu registration, event ordering, timers, document methods, native FFI and file access. Use static inspection plus runtime instrumentation: dynamic Lua behavior cannot be mapped completely by searching `require()` alone.

Test actual workflows: enable, configure, open a book, trigger commands, show dialogs, change settings, save, suspend, resume, close and reopen. Compare results with stock KOReader. Keep a minimal fixture for each behavior that becomes part of the contract.

Where a plugin depends on custom Lua drawing, retaining its renderer can be safer than translating it. Functional compatibility and visual consistency are separate test dimensions. A custom plugin may work while retaining older-looking UI until an adapter exists.

### New extension interface

Add a versioned Kaia Lua API for declarative menu items, commands, settings, dialogs and annotation overlays. New plugins describe intent so the host can render coherent UI. Keep the legacy interface alongside it; adoption is optional.

Build an extension screen showing compatibility status, origin, version, configuration and failure information. A plugin marketplace is not needed for v1. No silent plugin downloads or automatic execution from untrusted book content.

### Security boundary

Legacy plugins must be treated as trusted code. The existing environment can expose native FFI and operating-system services; running it within a Rust host does not sandbox it. Rust also does not remove vulnerabilities in retained C/C++ engines. [S19, S24]

A permission manifest in the same unrestricted runtime is advisory, not containment. A future restricted plugin mode would require a deliberately separate execution environment and its own threat model. Do not claim that it protects arbitrary legacy plugins.

## 6. UX and E Ink rendering

Redesign the common components first: typography, spacing, buttons, dialogs, lists, keyboard, navigation, settings and annotation controls. Keep compatibility wrappers under existing Lua module names where feasible.

The initial UI implementation should reuse the existing widget/text/display system. Later Rust-backed components can draw into the same composition path. Do not introduce a browser, GPU-dependent framework or independent compositor into the first release.

KOReader's UIManager already handles refresh regions, different refresh modes, scheduling and widget ordering. Preserve these semantics before trying to optimize or replace them. [S25]

Target a small set of product surfaces: Home, Library, Reading, Notes and Settings. Reading controls should remain reachable, with advanced settings grouped consistently rather than removed. Design for touch and page-turn buttons, both handednesses, rotation, large text, high contrast, localization and right-to-left interfaces.

The Libra Colour has a 1264×1680 display with 300-PPI monochrome and 150-PPI color content according to Kobo. Use a grayscale-first UI with color as an accent, never as the only indicator of state. Validate the physical panel, not just desktop screenshots. [S26]

Avoid continuous animation, blinking status indicators and unnecessary clock updates. Redraw only changed regions and let the existing refresh backend choose validated modes initially. Drawings/stickers need explicit image-quality and ghosting tests.

### Two visual gates

1. A redesigned core workflow must be complete and internally consistent before it becomes default.
2. Legacy plugin dialogs must remain usable, correctly layered and dismissible even when they have not yet been restyled.

The first public alpha may expose a compatibility fallback. The stable core workflow must not force users to navigate between multiple competing shells.

## 7. Persistence, stickers and annotations

### Protect the user's data

Use a separate Kaia application/settings identity. During testing, clone the relevant KOReader data rather than sharing a live writable profile. Never modify the Kobo account database. Prevent stock KOReader's updater from overwriting the fork.

Preserve existing document sidecars and their fields; upstream DocSettings is the reference implementation to characterize, not a format to replace casually. Test actual unknown-field round trips before relying on them. [S27]

Use SQLite for new Kaia-owned structured data with explicit transaction and durability policy. Keep the index rebuildable and annotation data backed up. Do not assume a database alone guarantees safety: SQLite documents its filesystem and flush assumptions. Test disk-full, interrupted write, unplug/restart and failed migration scenarios on the real storage. [S28]

Use rollback-journal transactions initially for the single-writer new store unless profiling justifies another mode. On USB mass-storage export, stop jobs, flush, close data handles and coordinate with the existing launcher/device flow. Do not modify a volume simultaneously with a connected computer. Existing Kobo launch code has explicit USB-mass-storage handling; preserve it. [S29]

Export new annotations as versioned, documented JSON plus referenced assets. Back up before schema changes. Test both app rollback and data compatibility: restoring an older executable does not undo a newer schema.

### Anchor the sticker to content, not a screen

For reflowable EPUB, use a document identity, engine locator/range, quoted text with context, attachment policy and relative placement. Preserve the native engine locator rather than assuming it is automatically an EPUB CFI. KOReader already distinguishes engine positions from fixed-page coordinates in its highlight handling. [S30]

For fixed-layout PDF or page images, use document identity, page identity and page-space coordinates plus transformation handling for crop/zoom/rotation. For book-level stickers, attach to the book identity rather than a page.

Text-quote and text-position selectors from the W3C Web Annotation model provide useful fallback concepts. They are not a guarantee that annotations survive an edited edition or an engine migration. [S31]

A changed font must not leave stickers tied to the former page number. A changed book file requires re-anchoring or user confirmation. Preserve unresolved annotations and offer repair; never guess silently or discard them.

Keep original documents unchanged by default. PDF embedding is an explicit export operation. Store asset hashes, licenses, original dimensions and bounded image sizes. Limit archive expansion, decoded pixel counts and asset memory. Touch-based placement should work before optional pen interaction is made a dependency.

### Stylus milestone

Separate basic pen events, coordinate transforms, eraser/button behavior, palm rejection and low-latency refresh into individually tested capabilities. Community stylus code can inform the work, but hardware support and a production annotation workflow are separate achievements. [S16]

## 8. Power, performance and reliability

Establish three separately reported reference conditions: stock Nickel, stock pinned KOReader and Kaia. Use matched documents, light/warmth settings, wireless state, charge range and temperature. Compare Kaia primarily with the KOReader configuration whose functions it retains.

Measure input-to-software completion separately from input-to-visible-page completion. Track cold/warm open, page turn median and p95, peak memory, CPU activity, wakeups, screen refresh count, library scan cost, suspend/resume success and energy per controlled workload.

Short battery-percentage observations and a USB charging meter alone are not proof of sleep-current improvement. Use sufficiently long controlled discharge tests or suitable instrumentation; label uncertainty and repeated-run variability.

### Proposed initial review thresholds, not measured claims

| Area | Gate |
|---|---|
| Reading latency | Investigate any repeatable p95 regression above 5% against the pinned baseline. |
| Memory | Investigate growth above 10% on the agreed corpus; require a hard budget and headroom established from the actual device. |
| Idle | No new periodic application work or unexplained redraws without a feature justification. |
| Suspend | No unexplained drain regression; at least 500 representative suspend/resume cycles before stable certification. |
| Data | No lost committed annotations in the specified fault-injection tests. |
| Soak | At least a week of ordinary reading/sleep use on each certified firmware before release. |

Calibrate statistical thresholds after the baseline exists; do not fail or advertise improvements based on noise. An improvement claim needs repeatable evidence. Never trade meaningful ghosting or annotation loss for a faster synthetic benchmark.

Avoid aggressive prefetching, polling services and always-on sync. Use bounded caching, cancelable indexing, batched work and explicit wireless ownership. Retain the validated power coordinator until a replacement is demonstrably at least as reliable.

## 9. Implementation roadmap and go/no-go gates

Durations are planning estimates for two experienced full-time engineers with part-time design and QA. They are not a quote or measured delivery forecast; phases can overlap only when their gates permit it.

| Phase | Indicative duration | Deliverable | Exit gate |
|---|---:|---|---|
| 0. Reproduce and inventory | 1–3 weeks | Unmodified baseline build, exact device profile, data backup/recovery, feature/plugin inventory, license inventory. | Stock build works on desktop and Kobo; recovery and benchmark methodology demonstrated. |
| 1. Prove Rust integration | 2–4 weeks | A small Rust service called by real KOReader, plus a hybrid-widget experiment and separate embedded-LuaJIT spike. | ABI, callbacks/error behavior, rendering ownership and representative plugins work on hardware. |
| 2. Deliver a coherent UI slice | 4–8 weeks | Library -> open book -> controls -> highlight -> notes -> return workflow. | Daily reading works without data regression; shared plugin dialogs remain usable. |
| 3. Harden compatibility and migrate services | 6–10 weeks | Versioned compatibility profile, tests, selected Rust service ownership and refined UI. | Applicable bundled plugins and selected community workflows meet the contract. |
| 4. Integrated annotations | 4–8 weeks | Persistent stickers, export/backup, reflow handling and optional tested pen functions. | Reopen, reflow, resize, rotation, document-change and interrupted-write tests pass. |
| 5. Public beta and release engineering | 4–8 weeks | Safe updater/rollback, support docs, security review, device soak results. | Signed release, source bundle, reproducible build procedure and recovery certification. |
| Later. Rust host migration | Separately estimated after Phases 1–3 | Move scheduling/UI ownership behind compatibility facades. | Functional, power and data parity proven before removing the old host. |

An integrated alpha is more plausible in months than days. Budget roughly 6–12 months for a polished public beta under the staffing assumption, then revise using measured progress. Broad compatibility and deeper host migration may take substantially longer.

If the FFI or host prototype fails, retain the Lua host and still deliver the redesigned reader with Rust services. If a UI translation breaks important plugins, retain that widget's legacy behavior. If a Rust replacement has no maintainability or measured product benefit, do not merge it merely to reduce Lua line count.

## 10. Test and release system

Keep upstream tests. Add Rust unit/property tests, Lua contract tests, differential document fixtures, deterministic UI images, integration fault tests and hardware tests. Use clearly licensed test books and assets. Include EPUB, PDF, scanned documents, CBZ/DjVu as applicable, large libraries, difficult typography, RTL/CJK content and malformed inputs.

Every migration must identify the old behavior, its test oracle, data owner, fallback, memory/latency impact and plugin-visible changes. Require review of unsafe/FFI code, storage migrations, power changes and all AI-generated changes. Automated code generation is not a substitute for those gates.

Run ordinary untrusted pull requests in an isolated CI environment, not on privileged hardware runners. Use protected, human-approved jobs for device deployment and signing. Pin CI actions/toolchains and keep release secrets out of community PR environments.

The fork needs its own version, package/update identity and issue tracker. Publish a software bill of materials, source and build scripts matching each binary, checksums and signed release metadata. Audit Rust, native-library and plugin dependencies. A hash alone is not publisher authentication.

Use application-level staged updates with current and last-known-good releases, a validated launcher switch, disk-space checks and a recoverable install journal appropriate to the filesystem. Do not assume Unix symlinks or a rename alone provide transactional guarantees on onboard storage. Do not touch boot partitions.

Safe mode should skip third-party plugins and provide a known-good launch or return to Nickel. Recovery must be available from outside the failed UI. Document which failures require USB/manual recovery instead of promising universal automatic repair.

Firmware changes are independent platform changes. Test them in a candidate compatibility lane before advertising support. Do not recommend blind downgrades or indefinite refusal of security updates as a general strategy.

## 11. Open-source operation and licensing

Maintain an identifiable KOReader fork with preserved Git history and clear attribution. Keep changes small and separable, avoid giant generated patches, and upstream broadly useful fixes. A fork inherits code rights, not a guaranteed maintainer community or free compatibility maintenance.

Preserve the AGPLv3 licensing obligations and all original notices. Do not relicense the derived product as MIT just because new code is Rust. Publish the corresponding source/build materials for distributed versions. Review the exact SPDX expression against upstream and per-file notices rather than silently asserting an ambiguous `-only` or `-or-later` interpretation. [S13, S32]

Inventory native libraries, fonts, icons, dictionaries and community plugin code. Do not bundle Kobo firmware, proprietary artwork, commercial dictionaries or reader libraries without permission. Do not assume a dependency package's label replaces inspection of vendored components.

Publish CONTRIBUTING, architecture records, a code of conduct, security reporting, support policy and release process. Use contribution sign-offs and require provenance/license information for imported code. Identify maintainers for platform/power, compatibility, UI and storage/release responsibilities. Keep telemetry off by default and never upload books/notes without explicit consent.

Use an upstream tracking branch plus a tested integration branch. Review changes regularly; merge with compatibility and hardware tests rather than auto-updating to moving `master`. Budget maintenance time explicitly. Keep user bug reports for the fork on the fork's tracker.

## 12. Decisions to approve before implementation

| Decision | Selection |
|---|---|
| Product | Kaia Reader, open source, Kobo-inspired simplicity, offline-first. |
| Source foundation | KOReader fork preserving history, initially investigated at v2026.07.1. |
| Migration method | Incremental application-layer migration, not whole-repository translation. |
| Runtime | Exact approved LuaJIT plus required Lua frontend compatibility. |
| Engines | Existing native engines retained. |
| Rust introduction | Narrow library boundary first; host takeover later and gated. |
| Display | One coordinator, existing E Ink path initially, software rendering. |
| Hardware scope | Libra Colour with explicitly certified firmware; desktop development target. |
| Boot/OS | Stock firmware retained; no Nickel deletion or kernel work. |
| Data | Separate identity; existing data preserved; one owner per data class. |
| Plugins | Versioned tested profile, no universal-compatibility claim. |
| Delivery | Gates tied to user experience, data, performance and recovery—not Rust line count. |

### First concrete milestone

A development Kobo runs a separately identified Kaia build, reads through unchanged KOReader engines, executes a small Rust service, opens an unchanged representative Lua plugin, saves and restores state, suspends/resumes correctly and can return to a known-good reader after an injected failure.

Do not authorize broad rewriting before that milestone. It tests the assumptions that could otherwise invalidate the architecture.

## Source register

Sources were inspected through their public project/documentation pages on October 12, 2026. Tagged sources are distinguished from moving `master` documentation. Assertions above are intentionally narrower than production-validation claims. The exact implementation dependency set must be pinned during Phase 0.

- **S1 — KOReader latest release / baseline:** `https://github.com/koreader/koreader/releases/latest` (resolved to v2026.07.1); `https://github.com/koreader/koreader/releases/tag/v2026.07.1`
- **S2 — Frontend development guide:** `https://github.com/koreader/koreader/blob/master/doc/Development_guide.md`
- **S3 — Tagged example plugin:** `https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/plugins/hello.koplugin/main.lua`
- **S4 — Native engine/runtime architecture:** `https://github.com/koreader/koreader-base`
- **S5 — Plugin loader and application integration:** `https://github.com/koreader/koreader/blob/master/frontend/pluginloader.lua`; `https://github.com/koreader/koreader/blob/master/frontend/apps/reader/readerui.lua`
- **S6 — Tagged Kobo backend:** `https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/frontend/device/kobo/device.lua`
- **S7 — Plato devices:** `https://github.com/baskerville/plato`
- **S8 — Plato Rust build:** `https://github.com/baskerville/plato/blob/master/doc/BUILD.md`
- **S9 — NickelMenu firmware restriction:** `https://github.com/pgaskin/NickelMenu`
- **S10 — KOReader toolchains:** `https://github.com/koreader/koxtoolchain`; `https://github.com/NiLuJe/koxtoolchain`
- **S11 — KOReader release packages:** `https://github.com/koreader/koreader/releases`
- **S12 — Build/development guide:** `https://github.com/koreader/koreader/blob/master/doc/Building.md`
- **S13 — Tagged upstream license:** `https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/COPYING`
- **S14 — ZenOS:** `https://github.com/xZenLabs/zen-os`
- **S15 — kobo-core:** `https://github.com/Nayeem170/kobo-core`
- **S16 — Community stylus plugin:** `https://github.com/mysticknits/pencil.koplugin`
- **S17 — Community sticker plugin:** `https://github.com/toastiez/pagenotes.koplugin`
- **S18 — KOReader quickstart/DRM limitation:** `https://github.com/koreader/koreader/blob/master/frontend/ui/quickstart.lua`
- **S19 — LuaJIT FFI:** `https://luajit.org/ext_ffi.html`
- **S20 — Rust FFI and unwinding:** `https://doc.rust-lang.org/nomicon/ffi.html`
- **S21 — mlua configuration:** `https://github.com/mlua-rs/mlua`
- **S22 — LuaJIT compatibility:** `https://luajit.org/extensions.html`
- **S23 — Rust supported targets:** `https://doc.rust-lang.org/stable/rustc/platform-support.html`
- **S24 — Example native/system access:** `https://github.com/koreader/koreader/blob/master/plugins/SSH.koplugin/main.lua`; S6, S19
- **S25 — UIManager:** `https://github.com/koreader/koreader/blob/master/frontend/ui/uimanager.lua`
- **S26 — Official Libra Colour specifications:** `https://ca.kobobooks.com/products/kobo-libra-colour`
- **S27 — Tagged document settings:** `https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/frontend/docsettings.lua`
- **S28 — SQLite durability assumptions:** `https://www.sqlite.org/atomiccommit.html`
- **S29 — Tagged Kobo launcher:** `https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/platform/kobo/koreader.sh`
- **S30 — Tagged annotation handling:** `https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/frontend/apps/reader/modules/readerhighlight.lua`
- **S31 — Web Annotation selectors:** `https://www.w3.org/TR/annotation-model/`
- **S32 — Upstream license-version discussion:** `https://github.com/koreader/koreader/discussions/11652`
