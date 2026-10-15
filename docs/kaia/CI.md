# Kaia CI

This page describes the GitHub Actions workflows that check Kaia's own code. It also says what to do with the upstream KOReader workflows in this repository.

The rules come from `ENGINEERING_PLAN.md` section 10. Ordinary pull requests run in an isolated environment. Actions and toolchains are pinned. No secrets reach pull request jobs. No job touches a device or a signing key.

## Workflows

Every Kaia workflow lives in `.github/workflows/kaia-*.yml`. Each one sets `permissions: contents: read`, runs on `ubuntu-24.04`, and uses `persist-credentials: false` on checkout. Pull requests from forks run with a read-only token and without secrets. None of the workflows uses `pull_request_target`.

Every action is pinned to a full commit SHA with a comment that names the release. The tag resolves to a commit. Downloaded binaries are checked against a SHA-256 value.

### `kaia-rust.yml`

Runs on pushes to `main` and on pull requests that touch `rust/**`.

| Job | Gates | Commands (in `rust/`) |
|---|---|---|
| `test` | Formatting, lints and tests of the `libkaia` workspace | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features --locked` |
| `deny` | Dependency licenses and known advisories | `cargo deny check licenses advisories` |

The toolchain comes from `rust/rust-toolchain.toml` (channel `1.99.0`, components `rustfmt` and `clippy`). The job reads the channel from that file, so the pin lives in one place. `Swatinem/rust-cache` caches the build.

`rust/deny.toml` allows `AGPL-3.0-or-later` for the Kaia crates and the permissive licenses found in the tree: `Apache-2.0`, `MIT`, `Unicode-3.0` and `Zlib`. Add a license only after reading it and recording the crate in `docs/kaia/inventory/LICENSES.md`. The `deny` job downloads the cargo-deny 0.20.2 release and checks its SHA-256 value.

### `kaia-lua.yml`

Runs on pushes to `main` and on pull requests that touch `frontend/kaia/**`, `plugins/kaia*/**`, `spec/unit/kaia_*`, `reader.lua`, `frontend/**`, `.luacheckrc` or the workflow file.

| Job | Gates | Details |
|---|---|---|
| `luacheck` | Lint of Kaia Lua code | Installs the `lua-check` package from Ubuntu 24.04 (luacheck 1.1.2). Lints `frontend/kaia`, `plugins/kaia*.koplugin` and `spec/unit/kaia_*_spec.lua` with the root `.luacheckrc`. |
| `emulator` | Build, Kaia specs and the full front suite | Runs in `koreader/kobase`, the image upstream CircleCI uses, pinned by digest. |

The `emulator` job runs these steps in order:

1. `make fetchthirdparty` fetches the submodules, as upstream CI does.
2. The job restores `base/build` from a cache. The key holds the image digest and the `base` submodule commit. The commit comes from `git rev-parse HEAD:base`.
3. `./kodev build` builds the desktop emulator.
4. rustup 1.29.1 installs Rust 1.99.0 (minimal profile). The rustup binary is checked against its SHA-256 value.
5. `tools/kaia/build-libkaia.sh host` builds `libkaia` and copies it into the emulator. The Kaia specs load it.
6. `./kodev test front` runs every `spec/unit/kaia_*_spec.lua` by name.
7. `./kodev test front` runs the full front suite.

The image is `koreader/kobase@sha256:0751390e1a93b57e4836ceb67b4ffc701b46df675853f7662e3aefb6d6af648f`, the multi-arch index digest of the `1.0.0-22.04` tag. The digest comes from the Docker Hub API. To update the image, look up the new digest, change the `container.image` line and the cache key prefix together.

### CREngine comparison

The `emulator` job in `kaia-lua.yml` runs `tools/kaia/test_cre_compare.py` and `tools/kaia/cre_compare.py` after the emulator build. The comparison step has a 15-minute timeout; its subprocess deadline is 14 minutes, including the Rust example build. This budget excludes the existing emulator build. Changes to the comparison scripts, corpus manifest, native backend, Cargo lockfile, or the `base`, `test` and font submodule pins trigger the workflow.

Run locally from the repository root after building the emulator:

```sh
source tools/kaia/env.sh
export CARGO_TARGET_DIR=$HOME/kaia-wt-cre-target
python3 tools/kaia/test_cre_compare.py
python3 tools/kaia/cre_compare.py --output "$CARGO_TARGET_DIR/cre-comparison"
```

Choose a new or empty output directory for each run. The runner rejects reused output so stale snapshots cannot hide missing output. Pass `--emulator /path/to/koreader` if several emulator builds exist. The runner uses the emulator's LuaJIT and `libs/libkoreader-cre` through `rust/kaia-doc/tests/koreader_layout.lua`. The `cre_snapshot` example uses `CreDocument`, linked to the pinned native build through `kaia-cre-sys`. Both processes read the emulator's exact font files, `epub.css` and hyphenation data. Neither process writes into the emulator tree or downloads books. The runner uses `$HOME/kaia-wt-cre-target` for Cargo outputs on both desktop and CI.

`tools/kaia/cre-corpus/manifest.json` pins the existing `test/juliet.epub` and `test/leaves.epub` bytes by size and SHA-256. It records their source and rights evidence without adding EPUB copies. Missing or changed files fail the run. These English verse books cover cover images and long pagination. They do not complete the broader corpus in `device/MEASUREMENT_METHOD.md`; B-12 and KR-007 still own its selection and license review. The CSS conformance fixtures under `test/epub2-conform` are outside this initial manifest.

Every profile uses a 1264x1680 page, one page per view, no engine header or frontend footer, CSS DPI 96, autohinting mode 2, HarfBuzz kerning mode 3, embedded fonts enabled and no added justification CSS. Settings are engine pixels, with no further UI scaling.

| Profile | Font and size | Margins, left/right and top/bottom | Line spacing | Hyphenation | Embedded styles | Base weight |
|---|---|---|---|---|---|---|
| `reader` | Noto Serif, 47 px | 22 px and 32 px | 100% | English_US.pattern | on | 400 |
| `no-hyphenation` | Noto Serif, 47 px | 22 px and 32 px | 100% | off | on | 400 |
| `sans-large` | Noto Sans, 55 px | 48 px and 40 px | 120% | English_US.pattern | off | 500 |

The reader profile uses `ceil(units * 1264 / 600)` for KOReader's 22-unit font and 10/15-unit margins. The comparison exercises kaia-doc's layout mapping directly; it does not drive the frontend settings UI.

The samples use zero-based pages 0, 1, 2, one quarter, one half, three quarters, and the penultimate page. Duplicates are removed. Each page ends at the next page's start anchor, exclusively. Text uses CREngine's DOM range extraction with newlines between blocks, matching KOReader's `getTextFromXPointers`; it is not the visual-line representation returned by `Document::page_text`. The last page is not sampled because the reference API has no next-page anchor. Manifest books must render at least two pages.

Page count, text and both anchors must match exactly, with no whitespace or Unicode normalization. Images compare raw Gray8 pixels. A run fails if any pixel differs by more than one gray level or the mean absolute error exceeds 0.01 on the 0..255 scale. This permits at most a one-level rounding change over 1% of a page. It does not permit a displaced glyph edge. Both sides use the same engine and dependency build, so a wider cross-platform image tolerance is unnecessary. Each platform compares its own two processes rather than a golden from another platform.

The `cre-comparison` artifact uploads on success or failure and stays for 14 days. It includes `report.json`, `summary.txt`, resource hashes, settings, process logs, both snapshots and page PNGs, raw pixels, and a diff PNG for each shared sample. Diff images show equal pixels in white and amplify differences sixteen times. Text and anchor mismatches also produce unified diffs. Comparator tests inject page, anchor, text and image changes to check the failure paths. A process failure or missing output fails the gate and remains in the report.

Local macOS arm64 result on 2026-10-14: six cases and 42 sampled pages matched exactly, including pixels. Page counts were 236, 236 and 407 for Juliet, and 1085, 1085 and 1780 for Leaves, in profile order. The final comparison took 16.15 seconds with the binary already built and workspace tests running concurrently. No settings mapping discrepancy was found in this corpus, so the existing layout mapping remains unchanged. Linux CI execution and the on-device B-75 comparison remain unverified by this local run.

### `kaia-shell.yml`

Runs on pushes to `main` and on pull requests that touch `tools/kaia/**`, `platform/kobo/kaia/**`, `.shellcheckrc` or the workflow file.

| Job | Gates | Details |
|---|---|---|
| `shellcheck` | Lint of Kaia shell scripts | shellcheck 0.11.0 from the GitHub release. Checks `tools/kaia/**/*.sh` and `platform/kobo/kaia/*.sh` at warning severity. |
| `device-test` | The profile collector test | Runs `sh tools/kaia/device/test-collect-profile.sh`. On Ubuntu, `/bin/sh` is dash, so the test runs under the POSIX shell the device scripts target. The test uses fake Kobo roots and no device. |

### `kaia-docs.yml`

Runs on pushes to `main` and on pull requests that touch the Kaia docs, Kaia Lua and plugin code, `tools/kaia/**`, `rust/**/*.rs`, `CLAUDE.md` or the workflow file.

| Job | Gates | Details |
|---|---|---|
| `style` | No em dash (U+2014) or en dash (U+2013) in Kaia-owned text | Runs `python3 -I tools/kaia/ci/check_style.py`. The script uses only the standard library. |

The check covers `docs/kaia/`, `frontend/kaia/`, `plugins/kaia*`, `tools/kaia/`, `rust/**/*.rs` and `CLAUDE.md`. It skips `docs/kaia/ENGINEERING_PLAN.md` and `docs/kaia/INITIAL_BACKLOG.md`, which the project owner wrote. Run it from the repository root. It prints each dash with its file, line and column.

### `kaia-kobo.yml`

Runs on pushes to `main`, on pull requests that touch `rust/**`, `platform/kobo/**`, `tools/kaia/**` or the workflow file, and on manual runs (`workflow_dispatch`).

| Job | Gates | Details |
|---|---|---|
| `package`, matrix `kobo` and `kobov5` | Both Kobo packages build. The Rust additions pass `check-rust-package.sh` on the unzipped package. The armv7 Rust app runs under qemu and writes its screenshots. | Timeout 180 minutes per target. The two targets run in parallel. `fail-fast: false`, so one failing target does not cancel the other. |

The job runs these steps in order, for each target:

1. Removes four preinstalled directories (`/usr/share/dotnet`, `/usr/local/lib/android`, `/opt/ghc`, `/opt/hostedtoolcache/CodeQL`) to free disk space. `df` prints the space before and after.
2. Checks out the repository with full history and tags. KOReader's `version_spec` reads `git describe`.
3. Runs `make fetchthirdparty`. The `CI` variable makes it fetch the submodules at depth 1, including `test/`, which holds the smoke EPUB.
4. Checks that `docker`, `unzip`, `zip`, `file` and `tee` exist.
5. Restores the cargo caches (see below).
6. Runs `tools/kaia/build-kobo.sh <target>`. This builds the KOReader package in `koreader/kokobo`, pinned by digest in the script. The build compiles from source on every run. It seeds its third-party downloads from the cache described below, and retries network failures.
7. Runs `tools/kaia/build-libkaia.sh <target>`. This builds `libkaia.so` in `kaia/kobo-rust:1`, the image from `tools/kaia/docker/kobo-rust.Dockerfile`.
8. Runs `tools/kaia/build-rust-kobo.sh <target> build -p kaia-app --bin kaia --no-default-features --features kobo`.
9. Runs `tools/kaia/package-kobo.sh` into `dist/packages/<target>/`.
10. Unzips the package into a new directory and runs `tools/kaia/check-rust-package.sh` on that tree. Packaging runs the same check on the staging tree before it zips, so this step checks the bytes that ship.
11. Sets up qemu for `arm` with `docker/setup-qemu-action`.
12. Runs `tools/kaia/rust-app-smoke.sh --skip-binfmt` with its output under `$RUNNER_TEMP/kaia-smoke/<target>`. The `--skip-binfmt` flag stops the script from pulling the unpinned `tonistiigi/binfmt` image, because step 11 registers the handler.
13. Uploads the logs and the packages from every run, so a device test or a release can use a green build. `kaia-release.yml` calls this workflow with `workflow_call`.

Every build step writes its output to `ci-logs/<step>-<target>.log` with `tee`.

Cache keys. `<hash>` is the hash of `rust/Cargo.lock` and `tools/kaia/docker/kobo-rust.Dockerfile`.

- The cargo registry is cached at `$RUNNER_TEMP/kaia-rust-cache/registry` with the key `kaia-kobo-registry-<hash>`. Both targets share it. The restore key `kaia-kobo-registry-` falls back to the newest entry.
- The Rust target directory is cached at `$RUNNER_TEMP/kaia-rust-cache/target-<target>` with the key `kaia-kobo-<target>-target-<hash>`. The restore key `kaia-kobo-<target>-target-` falls back to the newest entry for that target.

`build-rust-kobo.sh` reads these directories from `KAIA_RUST_CACHE_DIR`. Without that variable, it uses Docker volumes, as on a developer machine. `actions/cache` saves an entry only when the job succeeds.

#### Third-party downloads

KOReader's CMake files store each thirdparty project's downloads in `base/thirdparty/<name>/build/downloads`. The build runs in the `kaia-build-<target>` Docker volume, so the workspace copy stays empty. `build-kobo.sh` copies these folders in and out through `KAIA_DOWNLOAD_CACHE_DIR`. The job sets it to `$RUNNER_TEMP/kaia-downloads-cache/<target>`.

- The key is `kaia-kobo-<target>-downloads-<base>`. `<base>` is `git rev-parse HEAD:base`, which pins every download URL and git revision. The restore key `kaia-kobo-<target>-downloads-` falls back to the newest entry for that target.
- The step summary records `hit`, `partial hit` or `miss`.
- The build retries at most twice, 60 seconds apart. It retries only when the log shows a network failure while sources are fetched, such as a name resolution error, a timeout, a connection reset or a git 5xx. The volume keeps the sources from the earlier attempt, so a retry resumes.
- Still fetched on every run: `make fetchthirdparty` (the git submodules, all on github.com, including `base`), the image pulls (`koreader/kokobo` and the image that `docker/setup-qemu-action` uses), the rustup install in `kobo-rust.Dockerfile`, and crates.io on a cargo cache miss. Blocking github.com also blocks `make fetchthirdparty`, so this cache alone does not let a run build without that host.

Artifacts:

- `kaia-kobo-<target>-logs`, kept 14 days, on every run. It holds `ci-logs/` and the smoke output: `qemu.log`, `no-device.log` and the three screenshots.
- `kaia-kobo-<target>-packages`, kept 7 days, on failure only. It holds `dist/packages/<target>/`: the Kaia zip and `SHA256SUMS`. Packages are not uploaded on success because their size is not measured yet. Change the `if:` line to keep them on success.

Not in this job:

- The KOReader build volume is not cached. It is several gigabytes, and each target builds KOReader from scratch on every run. The `libkaia.so` build (`build-libkaia.sh`) also rebuilds its cargo target from scratch, because that script is not cached yet.
- `tools/kaia/arm-smoke.sh`, the Lua side smoke, does not run in this job.
- Changes to `frontend/`, `plugins/` or `resources/` do not start this job, unless the same pull request also touches a listed path. The package includes those directories, so a frontend break shows only in the next run that matches a listed path.
- `debian:bookworm-slim`, the smoke test image, has no digest pin. The image tag comes from `tools/kaia/rust-app-smoke.sh`.

## Upstream workflows

The repository also has upstream KOReader CI. Kaia does not edit those files.

- `.github/workflows/build.yml` (named `macos`) builds upstream KOReader on macOS runners for pushes and pull requests. It does not gate Kaia code. It uses a large share of the macOS runner minutes. Recommendation: keep it disabled in this repository. The repository settings have it disabled (`gh workflow disable`), and its open runs were cancelled.
- `.circleci/config.yml` builds upstream KOReader on CircleCI with the `koreader-vars` context and Docker Hub credentials. The `docs` job runs only on the `master` branch, which this fork does not have. Recommendation: do not connect this repository to CircleCI, or disable the project there. Without the CircleCI project, the build does not run, and no secrets are needed.

## Dependabot

`.github/dependabot.yml` has two entries.

- The `github-actions` ecosystem has `open-pull-requests-limit: 0`. Dependabot does not open PRs for the upstream workflow actions. Kaia's own workflows pin actions by SHA and are updated by hand.
- The `cargo` ecosystem covers `/rust`, runs weekly, and opens at most 5 PRs.

Upstream's Dependabot configuration opened three PRs for upstream actions. The orchestrator closed them.

## Local checks

These commands reproduce the gates from the repository root, unless the table says otherwise.

```sh
python3 -I tools/kaia/ci/check_style.py
sh tools/kaia/device/test-collect-profile.sh
luacheck frontend/kaia plugins/kaia*.koplugin spec/unit/kaia_*_spec.lua
cd rust && cargo fmt --all -- --check
cd rust && cargo clippy --workspace --all-targets --all-features -- -D warnings
cd rust && cargo test --workspace --all-features --locked
cd rust && cargo deny check licenses advisories
```

The `emulator` job compiles the third-party libraries on the first build, so it takes a long time. Run it locally with `./kodev build`, `tools/kaia/build-libkaia.sh host` and `./kodev test front` after `source tools/kaia/env.sh`.

## Limits

- The workflows do not test the E Ink display, power use or device behavior. Those need the device, as `CONTRIBUTING.md` says.
- The desktop emulator job runs on `x86_64` runners. The image index also has an `arm64` variant, which the local checks used.
- The `kaia-kobo` job has not yet run on GitHub. Its 180-minute timeout, its disk use and its cache sizes are estimates until a run records them. The armv7 smoke test runs under qemu, which checks the ABI and startup, not the Libra Colour display.
- The `kaia-lua` `emulator` job does not run on device hardware or with signing keys. Device and signing jobs belong in a protected, human-approved environment, not in these workflows.

### `kaia-release.yml`

Runs on a pushed tag `kaia-v*` or by hand with a ref. It calls `kaia-kobo.yml` to build both packages, writes one `SHA256SUMS` and creates a **draft** GitHub release with the packages, the sums and `docs/kaia/release/NOTES.md`. It never publishes. Only the draft job has `contents: write`. Signing happens afterwards on the maintainer's computer with `tools/kaia/release/sign-release.sh`; no key or secret is stored on GitHub. `tools/kaia/release/test-release.sh` tests the signing and verification scripts locally. See `RELEASE_PROCESS.md`.
