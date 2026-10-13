# Kaia Reader documentation

Kaia Reader is a KOReader fork for the Kobo Libra Colour. This directory holds the plan, the contracts, the decisions, the inventories and the device procedures.

## Start here

- [STATUS.md](STATUS.md): what is done, how it was checked, and what still needs a device.
- [INSTALL_KOBO.md](INSTALL_KOBO.md): which package to use and how to install, remove and recover it.

## Plan and contracts

- [ENGINEERING_PLAN.md](ENGINEERING_PLAN.md): the engineering and migration plan, with scope, risks, gates and the source register.
- [INITIAL_BACKLOG.md](INITIAL_BACKLOG.md): the Phase 0 to Phase 5 tasks, KR-001 to KR-016, and the first list of architecture records.
- [specs/ABI.md](specs/ABI.md): version 1 of the C ABI between LuaJIT and `libkaia`.
- [specs/STORAGE.md](specs/STORAGE.md): ownership, file layout, schema and export format for the data Kaia owns.
- [specs/DESIGN.md](specs/DESIGN.md): design tokens and the Home, Library, Reading, Notes and Settings surfaces.
- [specs/KR-010-hybrid-ui.md](specs/KR-010-hybrid-ui.md): results of the hybrid UI proof.
- [spikes/KR-011-embedded-luajit.md](spikes/KR-011-embedded-luajit.md): results of the embedded LuaJIT host spike.

## Decisions

- [adr/README.md](adr/README.md): index of the architecture decision records, with their status.

## Contribution, security and release

- [CONTRIBUTING.md](CONTRIBUTING.md): build commands, branch model, sign-off, provenance, the migration checklist and review rules.
- [SECURITY.md](SECURITY.md): private vulnerability reporting and the trust model for legacy plugins.
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md): the conduct policy, based on the Contributor Covenant 2.1.
- [SUPPORT.md](SUPPORT.md): what is supported, what is not, and how to report a bug with logs.
- [RELEASE_PROCESS.md](RELEASE_PROCESS.md): versioning, staged updates, signing, SBOM, source bundles and certification gates.
- [CI.md](CI.md): the GitHub Actions workflows and what each one gates.
- [RELEASE_SOURCE_CHECKLIST.md](RELEASE_SOURCE_CHECKLIST.md): the checklist for source, license and build materials in each release.

## Inventories

- [inventory/functions.md](inventory/functions.md): user-visible functions, with owner, baseline workflow and compatibility class.
- [inventory/plugins.md](inventory/plugins.md): bundled and community plugins, with their compatibility class.
- [inventory/TRACING.md](inventory/TRACING.md): runtime compatibility tracing and its privacy rule.
- [inventory/LICENSES.md](inventory/LICENSES.md): license and provenance record for upstream code, native libraries, fonts, icons, test books and plugin code.

## Device procedures

- [device/HARDWARE_PROFILE.md](device/HARDWARE_PROFILE.md): sanitized model, firmware, kernel, CPU and memory facts for the development device.
- [device/BACKUP_AND_RECOVERY.md](device/BACKUP_AND_RECOVERY.md): how to back up data and recover the reader from outside the failed UI.
- [device/MEASUREMENT_METHOD.md](device/MEASUREMENT_METHOD.md): the method for latency, memory, refresh and power measurements.
- [device/BENCHMARKS.md](device/BENCHMARKS.md): the scripted workload and the first desktop baseline.
