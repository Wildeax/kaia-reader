# Decisions needed

These are the items in group C of `BACKLOG.md`. Each one needs a choice from the project owner before it can be built or closed. Each row gives the options and a recommended answer. Reply "accept all", or name the rows you want changed.

Rules that apply to every row: no network access unless the user turns the feature on, no telemetry, and no code or data whose license Kaia cannot ship.

## Features

| Id | Decision | Options | Recommended |
|---|---|---|---|
| B-81 | WebDAV export of notes and highlights (T2-4) | Leave out; one-way export to a WebDAV folder the user sets; two-way sync | One-way export, off by default. One Markdown file and one JSON file per book, written when the user taps Export or closes a book. No sync back. |
| B-82 | Dictionary downloads (T2-6) | Leave out; download from the dictionary list KOReader uses; another source | Use KOReader's list, off by default, behind a switch that says files come from the internet. Pin a SHA-256 for each file in the repository and refuse a file that does not match. |
| B-83 | CBR comics (T3-15) | Leave out; libarchive (BSD license, already in KOReader's build); unrar (license not compatible with AGPL) | libarchive. It reads most RAR files, ships under a compatible license, and KOReader already builds it. Cap the unpacked size per page. |
| B-84 | Factory reset (T3-18) | Kaia settings only; all Kaia data; wider | Two buttons: "Reset settings" and "Erase all Kaia data". Neither touches books, Nickel or KOReader files. Both ask for a typed confirmation and suggest a backup first. |
| B-85 | Japanese, Chinese and Korean input (T3-4) | Leave out; add an input engine | Leave out of the first release. Latin keyboards only, as today. Revisit after release. |
| B-86 | Hyphenation for more languages (T3-7) | One language, as now; the pattern files KOReader ships | Use the patterns KOReader ships with CREngine, under their existing licenses, and pick the language from the book. |
| B-87 | Notebook export to Word, PDF and images (T3-16) | Leave out; add formats | Leave out of the first release. Keep the current notes export. |
| B-88 | Read aloud and screen reader (T3-2) | Leave out; add an offline speech engine | Leave out. The firmware has no offline engine, and adding one is a large separate project. |
| B-89 | Handwriting recognition (T3-1) | Leave out; license an engine | Leave out. Ink stays as drawn strokes. |
| B-90 | Network time (T3-13) | Manual time only; sync from a time server | A switch, off by default, that syncs the clock from pool.ntp.org when Wi-Fi is on. |
| B-91 | Lua app writing Nickel's brightness setting (issue 6.3) | Never write; write only when the user opts in | Never write by default. Add a switch "Match Nickel's brightness" for users who want it. |
| B-92 | Translate (sends text to Google) and Wikipedia lookups (issue 6.1) | Remove; keep behind an opt-in with a warning | Remove Translate. Keep Wikipedia behind a switch, off by default, that says the selected text leaves the device. |
| B-93 | Update channel | Manual install of packages from GitHub Releases; an in-app update feed | Manual install of signed packages from GitHub Releases for the first release. No in-app feed yet. |
| B-94 | Product name | Keep "Kaia Reader" after a trademark check; choose another name | Run a trademark search before the first public release. Keep the name if it is clear. |
| B-95 | Books removed from the device (T1-18) | Keep their reading data for 30 days; remove at once | Keep 30 days, as now. A folder that is not mounted counts as missing, not deleted. |
| B-96 | Ambient light sensor (T3-19) | Leave out; experiment | Leave out. The stock firmware does not use it on the Libra Colour. |
| B-97 | Experimental features list (T3-5) | Keep the list empty; name features | Keep it empty until a feature needs it. |

## Scope of smaller gaps

| Id | Decision | Recommended to build | Recommended to leave out |
|---|---|---|---|
| B-98 | Reading and library gaps from the feature table | In-book search history, per-book statistics, Home controls, double-tap to look up a word | Favorites and tags (collections cover them), zoom modes beyond fit and width, auto-highlight, a separate dyslexic mode (the OpenDyslexic font covers it) |
| B-99 | Device and format gaps from the feature table | Sleep screen with book info, standby timeout, screenshots | Screen lock PIN, LED and water-mode controls, VCOM checks, captive portal login, cloud browsing, audio, offline Wikipedia, image book formats beyond CBZ and CBR, the stock onboarding flow |

## Later, after device testing

| Id | Decision | When |
|---|---|---|
| B-100 | Final memory and speed budgets | After the measurements in group B |
| B-101 | Architecture go or no-go for each remaining ADR | After device, data and plugin results |
| B-102 | Release and maintenance owners, signing approver | Before the first public release |
