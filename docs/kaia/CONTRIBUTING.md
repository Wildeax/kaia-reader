# Contributing to Kaia Reader

Kaia Reader is a fork of KOReader. Read [ENGINEERING_PLAN.md](ENGINEERING_PLAN.md) and [INITIAL_BACKLOG.md](INITIAL_BACKLOG.md) before you change the architecture. The architecture decisions are in [adr/](adr/README.md).

## Build and test

Run these commands from the repository root.

```sh
source tools/kaia/env.sh             # PATH for Homebrew GNU tools and cargo on macOS
./kodev build                        # desktop emulator
./kodev run                          # start the emulator
./kodev test front kaia_store        # one Lua spec
cd rust && cargo test --workspace    # Rust tests
cd rust && cargo clippy --workspace --all-targets -- -D warnings
luacheck frontend/kaia plugins/kaia*.koplugin
```

Run the tests and linters for the code you change before you open a pull request. The desktop emulator does not test E Ink display behavior or power use. Those need the device.

## Branch model

- `upstream` tracks the KOReader release tags. Do not put Kaia changes on it.
- `main` is the tested integration branch.
- Feature branches merge into `main` through pull requests.

The project does not update from the moving KOReader `master` branch. Each upstream update merges through the tests and, for changes that touch the device, the hardware tests.

## Sign off commits

Sign off every commit with the Developer Certificate of Origin:

```sh
git commit -s
```

The sign-off line states that you wrote the change, or that you have the right to submit it under the project license. The text of the certificate is at https://developercertificate.org/.

## Provenance and licenses

The upstream KOReader code is licensed under AGPLv3. The exact SPDX expression is still under review. See sources S13 and S32 in [ENGINEERING_PLAN.md](ENGINEERING_PLAN.md). Keep every upstream notice in place. Do not relicense code.

Imported code, fonts, icons, dictionaries, test books and plugin code need a record in `docs/kaia/inventory/LICENSES.md`. The record gives the origin, the license, the copyright notice and the review result. Add the record in the same pull request as the import.

Do not bundle Kobo firmware, proprietary artwork, commercial dictionaries or reader libraries without permission. Telemetry is off by default. Books and notes never leave the device without explicit consent.

## Migration checklist

Every change answers these questions in its pull request description.

1. What is the old behavior?
2. What test shows that the new behavior matches the old behavior?
3. Which component owns the data that the change reads or writes?
4. What is the fallback if the new code fails?
5. How does the change affect memory and latency?
6. Which plugin-visible behavior changes?

## Review requirements

Get a review before you merge these changes.

- Unsafe Rust code and FFI boundaries. The reviewer checks the rules in [specs/ABI.md](specs/ABI.md).
- Storage migrations and schema changes. The reviewer checks the backup, transaction and rollback rules in [specs/STORAGE.md](specs/STORAGE.md).
- Power changes, such as suspend, resume, wireless, frontlight and charging. The review states what was measured, using [device/MEASUREMENT_METHOD.md](device/MEASUREMENT_METHOD.md).
- All AI-generated changes. Generated code gets the same review as hand-written code. Generation does not replace review.

## Writing style

Write prose, comments, commit messages, docs and UI strings in plain language. Use short sentences in the active voice. Use sentence case for headings. Do not use em dashes or en dashes. Separate ideas with periods and commas.

UI strings go through gettext: `local _ = require("gettext")`.

## Bugs and security reports

File bugs in the Kaia tracker at https://github.com/MoodStudios/kaia-reader/issues. Do not file Kaia bugs in KOReader's tracker. [SUPPORT.md](SUPPORT.md) lists what to include in a report.

Report security problems privately. Follow [SECURITY.md](SECURITY.md). Do not open a public issue for a vulnerability.

## Conduct

All contributors follow the [code of conduct](CODE_OF_CONDUCT.md).
