# Release process

This process follows sections 10 and 11 of [ENGINEERING_PLAN.md](ENGINEERING_PLAN.md). Each release also follows [RELEASE_SOURCE_CHECKLIST.md](RELEASE_SOURCE_CHECKLIST.md).

## Versioning

Release versions use the form `<major>.<minor>.<patch>`, tagged `kaia-v<major>.<minor>.<patch>`.

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

## Releases and signatures

GitHub releases carry the Kobo packages, `SHA256SUMS` and `SHA256SUMS.sig` (decision B-93). Wildeax, the project owner, is the signing key owner (decision B-102). CI builds the packages and creates a draft release. The signature is made on the owner's Mac. The private key never goes to GitHub or into CI.

`SHA256SUMS` lists each package with its SHA-256 hash. The signature covers `SHA256SUMS`, so it covers every package through its hash.

For each release:

1. Tag the release commit. On `main`, `git status --porcelain` must print nothing. Then run `git tag -a kaia-vX.Y.Z -m "Kaia Reader X.Y.Z"` and `git push origin kaia-vX.Y.Z`. The tag must look like `kaia-v` followed by the version number, such as `kaia-v1.2.3` or `kaia-v0.1.0-rc.1`, so it never matches the upstream KOReader tags.
2. Wait for the draft. The tag starts `.github/workflows/kaia-release.yml`. It runs the `kaia-kobo.yml` build for both targets, writes `SHA256SUMS` and creates a draft release with the two packages, `SHA256SUMS` and `docs/kaia/release/NOTES.md`. Check that the run passed. Its step summary links the draft. A run can also start by hand with `ref` set to an existing tag.
3. Sign on the Mac. Run `sh tools/kaia/release/sign-release.sh kaia-vX.Y.Z` from the repository. The script downloads the draft, checks every file against `SHA256SUMS`, signs `SHA256SUMS` with `~/.kaia-release/kaia-release-ed25519`, verifies the signature with `docs/kaia/release/allowed_signers` and uploads `SHA256SUMS.sig` to the draft. ssh-keygen asks for the passphrase. The script refuses to run when the release is published, when a file does not match, when `SHA256SUMS.sig` is already on the draft, or when the key is missing or has no passphrase.
4. Check the draft. On the GitHub release page, confirm that the draft has the two packages, `SHA256SUMS` and `SHA256SUMS.sig`, and that the notes are correct. To test the download, put the files in a new empty folder and run `sh tools/kaia/release/verify-release.sh DIR` from the checkout.
5. Publish by hand. Choose "Publish release" on GitHub. No workflow in this repository publishes a release.

To sign the draft again, delete its signature with `gh release delete-asset kaia-vX.Y.Z SHA256SUMS.sig -R Wildeax/kaia-reader -y`, then run step 3 again. To change a package, delete the draft with `gh release delete kaia-vX.Y.Z -R Wildeax/kaia-reader --cleanup-tag -y`, fix the code and start again at step 1 with a new version number. Do not reuse a tag after its release is published.

### Create the release key

The owner creates the key once, on the signing Mac. The key has a passphrase and stays on that Mac. Do not put the passphrase or the key into CI, the repository or an environment variable. When the key is missing, `sign-release.sh` prints the first two commands. The last three record the public key and the signer list:

```sh
mkdir -p ~/.kaia-release && chmod 700 ~/.kaia-release
ssh-keygen -t ed25519 -a 100 -C release@kaia-reader -f ~/.kaia-release/kaia-release-ed25519
cp ~/.kaia-release/kaia-release-ed25519.pub docs/kaia/release/kaia-release.pub
printf 'release@kaia-reader namespaces="kaia-release" %s\n' "$(cut -d ' ' -f 1,2 docs/kaia/release/kaia-release.pub)" >> docs/kaia/release/allowed_signers
ssh-keygen -lf docs/kaia/release/kaia-release.pub
```

Commit `docs/kaia/release/kaia-release.pub` and `docs/kaia/release/allowed_signers`, and record the fingerprint that the last command prints in docs/kaia/DECISIONS_NEEDED.md under B-102. Keep a copy of the private key and its passphrase somewhere safe. A lost key means a new key and a new signer list, and every user must take the new list.
