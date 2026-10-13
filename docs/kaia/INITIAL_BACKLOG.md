# Kaia Reader — Initial Execution Backlog

Version: 0.2 · Naming aligned with ENGINEERING_PLAN.md; task definitions and acceptance criteria unchanged.

Status: Proposed work, not completed. This backlog accompanies ENGINEERING_PLAN.md and uses its source register. Do not interpret acceptance criteria as claims of existing functionality.

## Phase 0: establish a trustworthy baseline

### KR-001 — Preserve upstream history and isolate product identity

Owner: platform/release engineer. Dependencies: none.

Create the fork from the approved upstream revision, record full commit/submodule identities and add the upstream remote. Preserve original source paths. Add a distinct development package, settings directory and issue/update identity. The fork must not replace stock KOReader or use its update feed.

**Acceptance:** both applications can be installed separately; no writable test profile or update channel is shared accidentally; upstream attribution and license are intact.

### KR-002 — Record the actual hardware and firmware

Owner: platform engineer. Dependencies: physical development device.

Record model identifier, firmware, kernel, CPU/float ABI, dynamic loader, libc requirements, total/available memory, display format and safe diagnostic access. Avoid copying identifiers or personal account information into public logs.

**Acceptance:** a versioned, sanitized hardware profile exists; launcher/package selection follows actual firmware rather than assumptions about all Libra Colour devices.

### KR-003 — Demonstrate backup and recovery

Owner: platform engineer + QA. Dependencies: KR-002.

Back up books and relevant settings. Establish an external path to restore the working application after a bad plugin or binary. Keep Nickel intact. Explain failures outside automatic recovery coverage.

**Acceptance:** an injected application startup failure is recovered without deleting books or factory-resetting the device.

### KR-004 — Rebuild unmodified KOReader

Owner: build engineer. Dependencies: KR-001, KR-002.

Resolve v2026.07.1 and submodules, pin build image/tools and compile the unmodified target. Run desktop and device smoke tests. Record compiler and native library configuration.

**Acceptance:** a fresh clean environment builds the pinned baseline; reading, configuration, suspend and exit work on the device.

### KR-005 — Inventory functions and plugins

Owner: compatibility engineer. Dependencies: KR-001.

Inventory user-visible functions and bundled plugins; define platform-inapplicable and debug-only exceptions. Choose a first community plugin set with pinned versions. Identify UI replacement patches that may conflict.

**Acceptance:** every item has an owner, baseline workflow, expected data effects and initial compatibility classification.

### KR-006 — Establish measurements

Owner: QA/performance engineer. Dependencies: KR-002, KR-004.

Create a licensed document corpus and fixed scripts for open/page-turn/search/highlight/suspend. Record Nickel and KOReader separately. Measure software and visible latency, memory, refreshes and controlled discharge uncertainty.

**Acceptance:** repeated baselines and a published method exist; thresholds are calibrated to observed variability.

### KR-007 — License and provenance inventory

Owner: release maintainer. Dependencies: KR-001, KR-004.

Inventory upstream code, native dependencies, fonts/icons/test books and proposed community code. Preserve upstream notices. Resolve exact SPDX declarations against project materials rather than changing license terms by assumption.

**Acceptance:** no unreviewed asset is bundled; a release-source checklist and dependency inventory are committed.

## Phase 1: prove the risky architecture

### KR-008 — First Rust service through LuaJIT FFI

Owner: Rust/platform engineer. Dependencies: KR-003, KR-004.

Implement a small deterministic library service over a reviewed C ABI, called by existing LuaJIT. Match target ABI/sysroot; use owned buffers/opaque handles and explicit errors. Do not add callbacks or a second Lua runtime.

**Acceptance:** repeated calls and allocation/free cycles pass on the device; no leaked allocations or uncontrolled ABI exceptions; disabling the component restores baseline behavior.

### KR-009 — Error and lifetime test suite

Owner: Rust/compatibility engineer. Dependencies: KR-008.

Test bad inputs, large buffers, stale handles, cancellation, panic conversion, shutdown and future callback contracts. Use appropriate host sanitizers/fuzz tests and device stress tests.

**Acceptance:** normal errors preserve the process and user state; unrecoverable failures enter the documented recovery path; reviewers approve all unsafe boundaries.

### KR-010 — Hybrid UI proof

Owner: UI/compatibility engineer. Dependencies: KR-008.

Render one redesigned component through the existing composition/refresh path. Place an unchanged plugin dialog above it. Test close, focus, rotation, buttons, color/grayscale and suspend.

**Acceptance:** there is still one event/display owner; no incorrect layering, lost input or competing redraw loop; performance is measured.

### KR-011 — Embedded LuaJIT host feasibility spike

Owner: Rust/compatibility engineer. Dependencies: KR-004, KR-008.

In an isolated experiment, embed the exact approved LuaJIT with native module paths and symbols. Evaluate mlua against direct integration, preserving configuration. Start selected existing modules/plugins.

**Acceptance:** publish observed results and failure modes. The experiment may conclude that the Lua host should remain longer; no production host replacement is implied.

### KR-012 — Compatibility instrumentation

Owner: compatibility engineer. Dependencies: KR-005, KR-010.

Combine static import scanning with runtime traces of module loads, menu registration, UI lifetimes, events, timers, document calls and storage operations. Avoid logging private book text.

**Acceptance:** representative plugins have repeatable workflow tests; undocumented direct access is visible and classified rather than hidden behind a compatibility claim.

## Phase 2: first complete user experience

### KR-013 — Design tokens and shared components

Owner: UI engineer/designer. Dependencies: KR-010.

Define typography, spacing, icons, contrast, focus and interaction states. Redesign existing shared components with compatible facades. Retain legacy implementations where required.

**Acceptance:** dialogs, buttons, menus and settings use one coherent system; touch/buttons, large text, RTL and localization are considered in tests.

### KR-014 — End-to-end daily-reading slice

Owner: UI/compatibility engineers. Dependencies: KR-012, KR-013.

Implement Library -> Book -> Reading controls -> Highlight -> Notes -> Library using existing engines and data ownership. Do not change the database schema and engine behavior in the same change.

**Acceptance:** the workflow survives restart and suspend, preserves existing data and requires no competing UI launcher.

### KR-015 — New annotation storage contract

Owner: Rust/storage engineer. Dependencies: KR-007, KR-008.

Specify document identity, native EPUB anchors, quoted context, fixed-page coordinates, asset storage, exports, transaction policy and schema migrations. Separate new-feature storage from existing KOReader settings.

**Acceptance:** fixtures cover reflow, rotation, edited documents, orphan repair, power interruption, low disk space and app rollback compatibility.

### KR-016 — Architecture review and scope checkpoint

Owner: maintainers. Dependencies: KR-009 through KR-015 as applicable.

Review real device results, complexity, plugin compatibility and upstream merge burden. Approve or revise each architecture record. Decide whether to continue with Lua hosting, expand Rust services, or advance the host experiment.

**Acceptance:** a recorded go/no-go decision with measured evidence; no broad rewrite authorized only because the prototype compiled.

## First architecture records

- ADR-001: Incremental application migration, preserving upstream history and behavior.
- ADR-002: One coordinator and one LuaJIT runtime; initial Rust C ABI.
- ADR-003: Explicit versioned legacy plugin compatibility profile.
- ADR-004: Existing document engines and Kobo integration retained.
- ADR-005: Single data owner, isolated profile, reversible migrations and durable exports.
- ADR-006: Software-rendered E Ink UI through one refresh path.
- ADR-007: Separate product/update identity, trusted legacy plugins and external recovery.

All ADRs begin as proposed. Accept them only after review; amend them when prototype evidence invalidates assumptions.
