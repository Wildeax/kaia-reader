# ADR-007: Separate product identity, trusted legacy plugins and external recovery

Status: Proposed

Date: 2026-10-13

## Context

Kaia must install beside stock KOReader and must not replace it. The stock updater must not overwrite the fork. Kaia needs its own package name, settings directory, update feed and issue tracker. [ENGINEERING_PLAN.md sections 7, 10 and 11]

Legacy plugins run in the Lua environment. That environment can reach native FFI and operating-system services. [S19, S24] Running the environment inside a Rust host does not sandbox it. A permission manifest in the same runtime is advisory. Rust also does not fix vulnerabilities in the retained C and C++ engines.

Kaia does not replace the kernel, the bootloader or the firmware. It does not delete Nickel. Recovery therefore has to work from outside the failed reader UI. [S18, S29]

## Decision

Kaia has its own product, package, settings and update identity. The fork never reads the stock KOReader update feed. Stock KOReader's updater cannot overwrite Kaia.

Legacy plugins are trusted code. Kaia documents this in `docs/kaia/SECURITY.md`. Kaia makes no sandbox claim for them. A future restricted plugin mode would need its own execution environment and threat model. Until then, the project does not claim that such a mode protects legacy plugins.

Recovery has three parts.

- Safe mode skips third-party plugins.
- A known-good launch returns the device to a working reader, or to Nickel.
- Recovery runs outside the failed UI.

Kaia does not promise automatic repair for every failure. The release notes name the failures that need USB or manual recovery.

## Consequences

- Users install plugins only from sources they trust. The security policy says so.
- Kaia runs a separate update feed, a separate issue tracker and separate release identity. That costs maintenance time.
- Recovery covers some failures. Other failures need manual recovery, and the documentation must say which.
- Kaia does not describe plugin permissions as a security boundary.

## Evidence needed to accept

- KR-001 requires both applications to install separately. No writable test profile or update channel is shared by accident.
- KR-003 requires an injected application startup failure to recover without deleting books and without a factory reset.
- The Phase 5 exit gate requires a safe updater with rollback, a signed release and recovery certification.
- The failure list in the release notes matches the recovery tests that ran.

## Sources

- S18: KOReader quickstart and DRM limitation. https://github.com/koreader/koreader/blob/master/frontend/ui/quickstart.lua
- S19: LuaJIT FFI. https://luajit.org/ext_ffi.html
- S24: Example native and system access. https://github.com/koreader/koreader/blob/master/plugins/SSH.koplugin/main.lua
- S29: Tagged Kobo launcher. https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/platform/kobo/koreader.sh
