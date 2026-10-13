# ADR-004: Existing document engines and Kobo integration retained

Status: Proposed

Date: 2026-10-13

## Context

KOReader's native layer uses MuPDF, CREngine, DjVuLibre and libk2pdfopt, with LuaJIT as the runtime. [S4] Kaia keeps these engines and their patches during the application migration.

The tagged Kobo device backend already supports the Libra Colour. It handles the `KoboMonza` model, MTK, keys, frontlight, the color screen and orientation. [S6] Wrapping that backend is safer than replacing it at the start.

Kobo launch code handles USB mass-storage mode explicitly. [S29] Kaia keeps that handling.

Firmware support has to be tested. NickelMenu says firmware 5.x is unsupported. [S9] KOReader publishes separate `kobo` and `kobov5` packages and toolchain targets. [S10, S11] Hardware support therefore does not establish launcher or package support on every firmware.

Plato uses Rust on Libra Colour, and it is a useful reference. [S7, S8] It is not a substitute for KOReader's plugin environment. kobo-core documents the Libra Colour and an ARMv7 musl build. It is a reference, not a drop-in device layer. [S15]

## Decision

Kaia keeps these components:

- MuPDF, CREngine, DjVuLibre and libk2pdfopt, with their patches.
- LuaJIT as the runtime.
- The tagged Kobo device backend.
- The Kobo launcher and its USB mass-storage handling.
- The validated power coordinator, until a replacement is at least as reliable.

Kaia does not rewrite MuPDF, CREngine, text shaping, image codecs or other native libraries. It does not replace the kernel, the bootloader or the firmware. It does not delete Nickel. It does not depend on a Secure Boot bypass.

The `kaia-platform` crate delegates to the existing Kobo integration first. The document adapters call the retained engines.

Kaia names the firmware it certifies. Launcher and package choice follow the firmware that is actually installed.

## Consequences

- Engine bugs stay in Kaia until upstream fixes them. Rust does not fix vulnerabilities in the retained C and C++ engines. See `docs/kaia/SECURITY.md`.
- Kaia picks up engine and device fixes through upstream merges.
- Each adapter adds a layer that needs measurement.
- Kaia supports only the firmware it has tested and certified.

## Evidence needed to accept

- KR-002 requires a sanitized hardware profile. Launcher and package selection follow the installed firmware.
- KR-004 requires the unmodified baseline to build in a clean environment. Reading, configuration, suspend and exit work on the device.
- The Phase 1 exit gate requires representative plugins and rendering ownership to work on hardware.
- A replacement power coordinator needs reliability at least equal to the retained one. The suspend and resume gates in ENGINEERING_PLAN.md section 8 measure it.
- The differential document fixtures from ENGINEERING_PLAN.md section 10 pass.

## Sources

- S4: Native engine and runtime architecture. https://github.com/koreader/koreader-base
- S6: Tagged Kobo backend. https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/frontend/device/kobo/device.lua
- S7: Plato devices. https://github.com/baskerville/plato
- S8: Plato Rust build. https://github.com/baskerville/plato/blob/master/doc/BUILD.md
- S9: NickelMenu firmware restriction. https://github.com/pgaskin/NickelMenu
- S10: KOReader toolchains. https://github.com/koreader/koxtoolchain ; https://github.com/NiLuJe/koxtoolchain
- S11: KOReader release packages. https://github.com/koreader/koreader/releases
- S15: kobo-core. https://github.com/Nayeem170/kobo-core
- S23: Rust supported targets. https://doc.rust-lang.org/stable/rustc/platform-support.html
- S29: Tagged Kobo launcher. https://raw.githubusercontent.com/koreader/koreader/v2026.07.1/platform/kobo/koreader.sh
