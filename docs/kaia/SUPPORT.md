# Support

Kaia Reader has no release yet. This policy applies to the first certified release.

## What is supported

- The Kobo Libra Colour, with the firmware versions that Kaia certifies. No firmware is certified yet.
- DRM-free books in the formats that KOReader reads.
- The desktop emulator for development. The emulator does not test E Ink display behavior or power use.

## What is not supported

- Books with DRM. KOReader does not support DRM-protected books, and Kaia does not either. [S18]
- The Kobo Store and other Kobo account services.
- Cloud sync of any kind, including Kobo cloud annotation sync.
- Other Kobo models, and firmware versions that are not certified.
- Plugins that are not on the supported list in the compatibility profile. Arbitrary patches and UI replacement plugins get best-effort help only.
- Problems that also happen in stock KOReader. Report those to the KOReader project.

## Report a bug

File bugs in the Kaia tracker at https://github.com/Wildeax/kaia-reader/issues. Include these details:

- The Kaia version and the firmware version.
- The steps to reproduce the problem, what you expected and what happened.
- The crash log, if the reader crashed. The crash log is at `.adds/kaia/crash.log`.

Remove personal information and account identifiers from logs before you post them.

Report security problems privately. Follow [SECURITY.md](SECURITY.md).

Sources: S18 in [ENGINEERING_PLAN.md](ENGINEERING_PLAN.md). S18: https://github.com/koreader/koreader/blob/master/frontend/ui/quickstart.lua
