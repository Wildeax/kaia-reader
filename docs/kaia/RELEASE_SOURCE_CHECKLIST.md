# Release source checklist

Use this for every distributed binary: device packages, emulator builds, Android builds and any archive. Attach the completed checklist and its outputs to the release record in `RELEASE_PROCESS.md`. Run the steps in the pinned Linux build environment, not on a development Mac.

Where a step runs a command, paste the output into the release record.

## 1. Source identity

- [ ] Tag the release commit and record the full hash. The corresponding source is that tag.
- [ ] `git status --porcelain` is empty and `git describe --tags --long` names the tag.
- [ ] Record the upstream base (KOReader `v2026.07.1`, commit `9192014d8`) and the upstream commit the fork merged from.

## 2. Submodule commits

- [ ] `git submodule status --recursive` shows no `-` (not initialized), `+` (different commit) or `U` (conflict) lines.
- [ ] Record each submodule commit: `base`, `base/thirdparty/kpvcrlib/crengine`, `l10n`, `platform/android/luajit-launcher`, `resources/fonts`. `test` is not shipped, so its commit goes in the record only when test artifacts are published.

## 3. Native dependency archives

- [ ] Run the inventory: `python3 -I tools/kaia/inventory/license_inventory.py --root . --out docs/kaia/inventory`. Attach `licenses.json` and `LICENSES.md` from that run.
- [ ] For each `DOWNLOAD URL` entry, confirm the archive matches the MD5 in `base/thirdparty/<name>/CMakeLists.txt`. The build checks this too, so a mismatch stops it.
- [ ] Record the SHA-256 of every archive the build downloaded. MD5 is the build's own check; SHA-256 goes in the manifest.
- [ ] For each `DOWNLOAD GIT` entry, record the commit the revision resolved to. A tag or describe string is not a fixed source.
- [ ] Record the SHA-256 of every `.patch` file in `base/thirdparty/<name>/`. Patches not referenced by CMake are listed in `LICENSES.md` and must be explained or removed.

## 4. Rust crates

- [ ] `rust/Cargo.lock` is committed. Build with `cargo build --locked` and `cargo test --locked`.
- [ ] Run `cargo deny check licenses` or `cargo about generate`. Commit the policy file that goes with the tool (`rust/deny.toml` or `rust/about.toml`) and save the output. Any crate with a license outside the approved list blocks the release.
- [ ] Record the Rust toolchain version (`rustc -Vv`) and the target triple.

## 5. Build environment

- [ ] Record the build image digest (`sha256:...`), not a tag.
- [ ] Record the toolchain versions: CMake, Ninja, the host compiler, the cross toolchain archive and its SHA-256, the Rust toolchain.
- [ ] A clean rebuild from the release tag, in a fresh environment, produces the same artifacts (or differences are explained). The Mac is not the release environment.

## 6. SBOM

- [ ] Generate an SBOM (SPDX or CycloneDX) covering the native dependencies, submodules, fonts, icons and Rust crates. Record the tool and its version.
- [ ] Each SBOM component lists name, version, source URL or commit, hash and license as recorded in `licenses.json`. A component with `unknown, needs review` is not a finished entry.

## 7. Checksums and signing

- [ ] Write `SHA256SUMS` for every artifact: device packages, emulator builds, source bundle, SBOM, `licenses.json`, `LICENSES.md`.
- [ ] Sign `SHA256SUMS` and the release metadata with the release key. Record the key fingerprint.
- [ ] Verify the signature after upload, from a clean machine. A hash does not show who published a file; the signature does.

## 8. Corresponding source bundle

- [ ] Build the source bundle from the tag with submodules, for example `git archive --recurse-submodules`. Record its SHA-256.
- [ ] The bundle includes the build scripts (`CMakeLists.txt` files, `tools/`), `Cargo.lock`, `COPYING`, every license file listed as present in `licenses.json`, and the thirdparty patch files.
- [ ] Decide whether the bundle also needs the upstream archives listed in `licenses.json`. The plan requires corresponding source and build materials for distributed versions.

## 9. Upstream notices

- [ ] Every license or notice file recorded in `licenses.json` ships with the package under its original name and path.
- [ ] Every Rust crate in the `Rust crates` section of `docs/kaia/inventory/LICENSES.md` ships its license files, and any NOTICE file, with the package.
- [ ] The Creative Commons BY 3.0 attribution for the icon packs in `resources/icons/` stays with the icons.
- [ ] The Kaia about text (`frontend/ui/elements/common_info_menu_table.lua`, line 134 at the time of writing) matches the decided license expression.

## 10. Material that must not ship

- [ ] No Kobo firmware, `KoboRoot.tgz`, firmware image or Nickel binary. The asset scan in `licenses.json` reports zero flagged paths outside test data.
- [ ] No commercial dictionaries (StarDict `.ifo`, `.idx`, `.dict.dz` or `.syn` files) unless the license permits redistribution. The asset scan lists any dictionary file.
- [ ] No test books (`test/`, `base/spec/`) in the device package.
- [ ] No proprietary artwork, reader library or font without written permission and a license file in the repo.

## 11. Community plugins

- [ ] Each shipped plugin has a provenance record: source URL, commit, author and license file copied into the tree.
- [ ] A plugin without a license file is not shipped.
- [ ] Each shipped plugin is reviewed for network calls and telemetry. Telemetry stays off by default.

## 12. Open items

- [ ] The AGPL-3.0-only versus AGPL-3.0-or-later question is decided, and `COPYING`, the README and the about text state the decision. Discussion: https://github.com/koreader/koreader/discussions/11652
- [ ] Every item in the "Needs review" list of `LICENSES.md` that concerns a shipped component is closed, or the release notes explain why it ships.
