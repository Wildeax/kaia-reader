# Release process

This process follows sections 10 and 11 of [ENGINEERING_PLAN.md](ENGINEERING_PLAN.md). Each release also follows [RELEASE_SOURCE_CHECKLIST.md](RELEASE_SOURCE_CHECKLIST.md).

## Versioning

Release versions use the form `kaia-<major>.<minor>.<patch>`.

Each release record names the upstream base. The first base is KOReader tag v2026.07.1. The record also gives the full upstream commit and the submodule commits.

## Source and build materials

Each published binary comes with:

- The source bundle that matches the binary.
- The build scripts that produced the binary.
- A software bill of materials (SBOM).
- Checksums.
- Signed release metadata.

Release builds run in a pinned Linux environment. The record names the toolchain image digest, the Rust toolchain, the linker and sysroot, `Cargo.lock`, the upstream and submodule revisions and the dependency source hashes. The Mac is for development. It does not define the shipping binary environment.

Kaia audits the Rust, native-library and plugin dependencies before each release.

No production package ships until a fresh environment repeats the build and the output runs on the device.

## Signing and CI

- Ordinary pull requests run in an isolated CI environment, not on privileged hardware runners.
- Signing and device deployment run in protected jobs that a person approves.
- CI actions and toolchains are pinned.
- Release secrets stay out of pull request environments from the community.
- A hash does not prove who published a file. The signature does that.

## Staged updates and recovery

- Updates are applied at the application level. The device keeps the current release and the last known good release.
- The launcher switches to a new release only after validation.
- The updater checks disk space before it installs.
- An install journal records each step. The journal design depends on the filesystem.
- Kaia does not assume that Unix symlinks or a rename give transactional guarantees on the onboard storage.
- Kaia does not touch the boot partitions.
- Safe mode skips third-party plugins.
- Recovery runs outside the failed UI. It returns the device to a known good reader or to Nickel.
- Kaia does not promise automatic repair for every failure. The release notes name the failures that need USB or manual recovery.

## Firmware compatibility

Each firmware change is a platform change. Test it in a candidate compatibility lane before the release advertises support for that firmware.

Kaia does not recommend blind downgrades. Kaia does not refuse security updates indefinitely.

## Soak and certification

- Each certified firmware gets at least one week of ordinary reading and sleep use before release.
- Stable certification requires at least 500 representative suspend and resume cycles.
- Phase 5 exit requires a signed release, a source bundle, a reproducible build procedure and recovery certification.
- The thresholds in ENGINEERING_PLAN.md section 8 are proposed. Calibrate them against the measured baseline before a release gate uses them.
