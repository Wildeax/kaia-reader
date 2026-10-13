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
- The `kaia-lua` `emulator` job does not run on device hardware or with signing keys. Device and signing jobs belong in a protected, human-approved environment, not in these workflows.
